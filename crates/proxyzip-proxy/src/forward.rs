use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use http::header::HeaderValue;
use http_body::{Body, Frame, SizeHint};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use proxyzip_core::{BreakpointPhase, FlowId, FlowState, FlowStore, Headers, LogLine, Scheme, now_ms};
use proxyzip_rules::{
    Action, BlockAction, LocalFile, Plan, Remap, RequestInfo, RequestParts, ResponseParts, apply_request_ops,
    apply_response_ops, content_type_for, decode_request_body, decode_response_body, set_header,
};
use proxyzip_script::{ScriptRequest, ScriptResponse, Verdict};

use crate::breakpoint::{Decision, RequestEdit, ResponseEdit};
use crate::server::{Conn, Ctx};
use crate::throttle::ThrottledBody;
use crate::util::{
    ConnectionDropped, ProxyBody, authority_for, format_authority, full, is_websocket_upgrade, split_authority,
    strip_hop_by_hop, to_header_map, to_headers, version_str,
};
use crate::{local, websocket};

type Reply = Result<Response<ProxyBody>, ConnectionDropped>;

struct Target {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

fn default_port(scheme: Scheme) -> u16 {
    if scheme.is_secure() { 443 } else { 80 }
}

fn resolve_target(req: &Request<Incoming>, conn: &Conn) -> Option<Target> {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
    if let Some(reverse) = &conn.reverse {
        let request_path = req.uri().path_and_query().map_or("/", |p| p.as_str());
        let path = format!("{}{request_path}", reverse.base_path);
        return Some(Target { scheme: reverse.scheme, host: reverse.host.clone(), port: reverse.port, path });
    }
    let host_header = req.headers().get(http::header::HOST).and_then(|h| h.to_str().ok());
    if let Some((tunnel_host, tunnel_port)) = &conn.tunnel_target {
        // Inside a tunnel requests are origin-form; HTTP/2 carries :authority in the URI.
        let host = req
            .uri()
            .host()
            .map(str::to_string)
            .or_else(|| host_header.map(|h| split_authority(h, *tunnel_port).0))
            .unwrap_or_else(|| tunnel_host.clone());
        return Some(Target { scheme: conn.scheme, host, port: *tunnel_port, path });
    }
    // Main listener: absolute-form `GET http://host/path`.
    let scheme = match req.uri().scheme_str() {
        Some("https") => Scheme::Https,
        Some(_) => Scheme::Http,
        None => {
            // Origin-form request aimed at the proxy itself.
            let (host, port) = split_authority(host_header?, 80);
            return Some(Target { scheme: Scheme::Http, host, port, path });
        }
    };
    let host = req.uri().host()?.trim_start_matches('[').trim_end_matches(']').to_string();
    let port = req.uri().port_u16().unwrap_or(default_port(scheme));
    Some(Target { scheme, host, port, path })
}

/// `query GetUser`-style label when a JSON request body is a GraphQL operation.
fn graphql_label(headers: &http::HeaderMap, body: &Bytes) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let content_type = headers.get(http::header::CONTENT_TYPE)?.to_str().ok()?.to_ascii_lowercase();
    if !content_type.contains("json") && !content_type.contains("graphql") {
        return None;
    }
    let (decoded, _, error) = proxyzip_codec::decode_content(&to_headers(headers), body);
    if error.is_some() {
        return None;
    }
    proxyzip_codec::graphql::label(&decoded)
}

fn flow_scheme(websocket: bool, secure: bool) -> Scheme {
    match (websocket, secure) {
        (true, true) => Scheme::Wss,
        (true, false) => Scheme::Ws,
        (false, true) => Scheme::Https,
        (false, false) => Scheme::Http,
    }
}

pub(crate) async fn forward(mut req: Request<Incoming>, ctx: Arc<Ctx>, conn: Conn) -> Reply {
    let Some(mut target) = resolve_target(&req, &conn) else {
        return Ok(error_response(StatusCode::BAD_REQUEST, "ProxyZip: request has no target host"));
    };
    let host_header = req.headers().get(http::header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string);
    if conn.reverse.is_none() && conn.tunnel_target.is_none() && local::is_local(&target.host, target.port, &ctx) {
        return Ok(local::respond(&target.path, &ctx, host_header.as_deref()));
    }
    if conn.reverse.is_none() && target.host.eq_ignore_ascii_case(local::CERT_HOST) {
        return Ok(local::respond(&target.path, &ctx, host_header.as_deref()));
    }

    let websocket = is_websocket_upgrade(req.headers()) && req.version() <= http::Version::HTTP_11;
    let client_upgrade = websocket.then(|| hyper::upgrade::on(&mut req));
    let settings = ctx.settings.read().clone();

    let (parts, body) = req.into_parts();
    let body = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            return Ok(error_response(StatusCode::BAD_REQUEST, &format!("ProxyZip: reading request body failed: {e}")));
        }
    };

    let scheme = flow_scheme(websocket, target.scheme.is_secure());
    let rules = settings.rules.clone();
    let info = RequestInfo {
        method: parts.method.as_str(),
        scheme: scheme.as_str(),
        host: &target.host,
        port: target.port,
        path: &target.path,
    };
    let plan = rules.plan(&info);
    let record = settings.recording && rules.should_record(&info);

    let flow_id = record.then(|| {
        let captured = body.slice(..body.len().min(settings.max_body_capture));
        let request = proxyzip_core::Request {
            method: parts.method.to_string(),
            scheme,
            host: target.host.clone(),
            port: target.port,
            path: target.path.clone(),
            http_version: version_str(parts.version),
            headers: to_headers(&parts.headers),
            body: captured,
            body_truncated: body.len() > settings.max_body_capture,
        };
        let id = ctx.store.create(conn.peer.to_string(), request);
        let tls = conn.tls.clone();
        let tags = plan.tags.clone();
        let graphql = graphql_label(&parts.headers, &body);
        ctx.store.update(id, |f| {
            f.tls = tls;
            f.graphql = graphql;
            for tag in &tags {
                f.add_tag(tag);
            }
        });
        id
    });

    match plan.action.clone() {
        Action::Forward => {}
        Action::Block(BlockAction::Drop) => {
            fail_flow(&ctx.store, flow_id, "Connection dropped by a block rule");
            return Err(ConnectionDropped);
        }
        Action::Block(BlockAction::Status { code }) => {
            let headers = vec![
                ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
                ("x-proxyzip-rule".to_string(), "block".to_string()),
            ];
            let text = format!("Blocked by a ProxyZip rule: {} {}{}", parts.method, target.host, target.path);
            return Ok(synthetic(&ctx.store, flow_id, code, headers, Bytes::from(text)));
        }
        Action::MapLocal(file) => return Ok(map_local(&ctx.store, flow_id, &file).await),
    }

    // From here on the flow is "in flight"; the guard fails it if the client hangs up.
    let mut guard = PendingGuard { store: ctx.store.clone(), id: flow_id };

    let mut req_parts = RequestParts {
        method: parts.method.to_string(),
        path: target.path.clone(),
        headers: to_headers(&parts.headers),
        body,
    };
    let mut modified = false;
    if conn.reverse.is_some() {
        // The client addressed the proxy; the server must see its own name.
        let authority = authority_for(target.scheme.as_str(), &target.host, target.port);
        set_header(&mut req_parts.headers, "host", &authority);
        modified = true;
    }
    if let Some(remap) = &plan.remap {
        apply_remap(&mut target, remap, &mut req_parts);
        modified = true;
    }
    modified |= apply_request_ops(&plan.request_ops, &mut req_parts);
    target.path = req_parts.path.clone();

    if !plan.request_scripts.is_empty() {
        match run_request_scripts(&ctx, flow_id, &plan, &mut target, &mut req_parts).await {
            ScriptFlow::Continue { changed } => modified |= changed,
            ScriptFlow::Block => {
                guard.id = None;
                if let Some(id) = flow_id {
                    ctx.store.update(id, |f| f.add_tag("blocked"));
                }
                let headers = vec![
                    ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
                    ("x-proxyzip-rule".to_string(), "script".to_string()),
                ];
                let text = format!("Blocked by a ProxyZip script: {} {}{}", req_parts.method, target.host, target.path);
                return Ok(synthetic(&ctx.store, flow_id, 403, headers, Bytes::from(text)));
            }
        }
    }

    let mut edited_by_user = false;
    if plan.break_on_request
        && let Some(id) = flow_id
    {
        sync_request(&ctx.store, id, flow_scheme(websocket, target.scheme.is_secure()), &target, &req_parts, &settings);
        ctx.store.update(id, |f| f.state = FlowState::Paused);
        ctx.store.notify_breakpoint(id, BreakpointPhase::Request);
        let decision = ctx.breakpoints.pause(id).await;
        ctx.store.update(id, |f| f.state = FlowState::Pending);
        match decision {
            Decision::Abort => {
                guard.id = None;
                fail_flow(&ctx.store, flow_id, "Aborted at a breakpoint");
                return Err(ConnectionDropped);
            }
            Decision::EditRequest(edit) => {
                apply_request_edit(&mut target, &mut req_parts, edit);
                edited_by_user = true;
                modified = true;
            }
            Decision::Continue | Decision::EditResponse(_) => {}
        }
    }
    if modified && let Some(id) = flow_id {
        sync_request(&ctx.store, id, flow_scheme(websocket, target.scheme.is_secure()), &target, &req_parts, &settings);
        if edited_by_user {
            ctx.store.update(id, |f| f.add_tag("edited"));
        }
    }

    let wire_scheme = if target.scheme.is_secure() { "https" } else { "http" };
    let uri = format!("{wire_scheme}://{}{}", format_authority(&target.host, target.port), target.path);
    // Untouched requests keep the exact header values the client sent.
    let mut headers = if modified { to_header_map(&req_parts.headers) } else { parts.headers.clone() };
    strip_hop_by_hop(&mut headers);
    if websocket {
        headers.insert(http::header::CONNECTION, HeaderValue::from_static("Upgrade"));
        headers.insert(http::header::UPGRADE, HeaderValue::from_static("websocket"));
        // tungstenite cannot relay compressed frames; keep the session uncompressed.
        headers.remove(http::header::SEC_WEBSOCKET_EXTENSIONS);
    }
    let method = match http::Method::from_bytes(req_parts.method.as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            guard.id = None;
            return Ok(fail(
                &ctx.store,
                flow_id,
                StatusCode::BAD_REQUEST,
                format!("invalid method {}", req_parts.method),
            ));
        }
    };
    let mut builder = Request::builder().method(method).uri(&uri).version(http::Version::HTTP_11);
    *builder.headers_mut().unwrap() = headers;
    let upstream_req = match builder.body(Full::new(req_parts.body.clone())) {
        Ok(r) => r,
        Err(e) => {
            guard.id = None;
            return Ok(fail(
                &ctx.store,
                flow_id,
                StatusCode::BAD_REQUEST,
                format!("invalid upstream request {uri}: {e}"),
            ));
        }
    };

    if let Some(id) = flow_id {
        ctx.store.update(id, |f| f.timing.request_sent = Some(now_ms()));
    }
    if let Some(profile) = &plan.throttle {
        let mut delay = Duration::from_millis(profile.latency_ms);
        if let Some(rate) = profile.upload_bytes_per_sec() {
            delay += Duration::from_secs_f64(req_parts.body.len() as f64 / rate as f64);
        }
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
    let client = ctx.upstream.pick(settings.insecure_upstream, websocket);
    let mut response = match client.request(upstream_req).await {
        Ok(r) => r,
        Err(e) => {
            let mut msg = format!("upstream request to {uri} failed: {e}");
            let mut source = std::error::Error::source(&e);
            while let Some(s) = source {
                msg.push_str(&format!(": {s}"));
                source = s.source();
            }
            guard.id = None;
            return Ok(fail(&ctx.store, flow_id, StatusCode::BAD_GATEWAY, msg));
        }
    };
    guard.id = None;

    let status = response.status();
    if status == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_upgrade) = client_upgrade
    {
        if let Some(id) = flow_id {
            record_head(
                &ctx.store,
                id,
                status.as_u16(),
                &to_headers(response.headers()),
                &version_str(response.version()),
            );
            ctx.store.update(id, |f| f.state = FlowState::WebSocketOpen);
        }
        let server_upgrade = hyper::upgrade::on(&mut response);
        let mut out = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
        *out.headers_mut().unwrap() = response.headers().clone();
        tokio::spawn(websocket::relay(client_upgrade, server_upgrade, ctx.store.clone(), flow_id));
        return Ok(out.body(full("")).unwrap());
    }

    let version = version_str(response.version());
    let (mut rparts, rbody) = response.into_parts();
    if plan.needs_response_body() {
        let script_request = (!plan.response_scripts.is_empty()).then(|| ScriptRequest {
            method: req_parts.method.clone(),
            url: current_url(&target, &req_parts),
            headers: req_parts.headers.0.clone(),
            body: req_parts.body.to_vec(),
        });
        let context = ResponseContext {
            flow_id,
            plan: &plan,
            settings: &settings,
            version: &version,
            script_request: script_request.as_ref(),
        };
        return buffered_response(&ctx, &context, rparts, rbody).await;
    }

    // Header-only rewrites still stream the body.
    if !plan.response_ops.is_empty() {
        let mut res =
            ResponseParts { status: rparts.status.as_u16(), headers: to_headers(&rparts.headers), body: Bytes::new() };
        if apply_response_ops(&plan.response_ops, &mut res) {
            rparts.status = StatusCode::from_u16(res.status).unwrap_or(rparts.status);
            rparts.headers = to_header_map(&res.headers);
        }
    }
    if let Some(id) = flow_id {
        record_head(&ctx.store, id, rparts.status.as_u16(), &to_headers(&rparts.headers), &version);
    }
    strip_hop_by_hop(&mut rparts.headers);
    let live = rparts
        .headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/event-stream"));
    let body = TapBody {
        inner: rbody,
        buf: Vec::new(),
        cap: settings.max_body_capture,
        truncated: false,
        store: ctx.store.clone(),
        id: flow_id,
        done: false,
        live,
        published: 0,
        last_publish: Instant::now(),
    };
    let body = match plan.throttle.as_ref().and_then(|t| t.download_bytes_per_sec()) {
        Some(rate) => ThrottledBody::new(body, rate).boxed(),
        None => body.boxed(),
    };
    Ok(Response::from_parts(rparts, body))
}

/// Hold the whole response so it can be rewritten or edited at a breakpoint before it reaches the client.
/// What the buffered response path needs besides the connection itself.
struct ResponseContext<'a> {
    flow_id: Option<FlowId>,
    plan: &'a Plan,
    settings: &'a crate::config::ProxySettings,
    version: &'a str,
    script_request: Option<&'a ScriptRequest>,
}

async fn buffered_response(
    ctx: &Arc<Ctx>,
    context: &ResponseContext<'_>,
    rparts: http::response::Parts,
    rbody: Incoming,
) -> Reply {
    let ResponseContext { flow_id, plan, settings, version, script_request } = *context;
    let body = match rbody.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            return Ok(fail(&ctx.store, flow_id, StatusCode::BAD_GATEWAY, format!("reading the response failed: {e}")));
        }
    };
    let mut res = ResponseParts { status: rparts.status.as_u16(), headers: to_headers(&rparts.headers), body };
    // Edits and previews work on readable bytes, so remove the content encoding up front.
    decode_response_body(&mut res);
    apply_response_ops(&plan.response_ops, &mut res);
    if let Some(request) = script_request {
        for rule in &plan.response_scripts {
            let input = ScriptResponse { status: res.status, headers: res.headers.0.clone(), body: res.body.to_vec() };
            let out = ctx.scripts.run_response(&rule.source, request, &input).await;
            record_script_output(&ctx.store, flow_id, &rule.name, out.logs, out.verdict.as_ref().err());
            if let Ok(Verdict::Changed(edited)) = out.verdict {
                res.status = edited.status;
                res.headers = Headers(edited.headers);
                res.body = Bytes::from(edited.body);
                proxyzip_rules::remove_header(&mut res.headers, "content-length");
            }
        }
    }

    if let Some(id) = flow_id {
        record_buffered(&ctx.store, id, &res, version, settings.max_body_capture, FlowState::Receiving);
    }
    if plan.break_on_response
        && let Some(id) = flow_id
    {
        ctx.store.update(id, |f| f.state = FlowState::Paused);
        ctx.store.notify_breakpoint(id, BreakpointPhase::Response);
        let decision = ctx.breakpoints.pause(id).await;
        match decision {
            Decision::Abort => {
                fail_flow(&ctx.store, flow_id, "Aborted at a breakpoint");
                return Err(ConnectionDropped);
            }
            Decision::EditResponse(edit) => {
                apply_response_edit(&mut res, edit);
                ctx.store.update(id, |f| f.add_tag("edited"));
            }
            Decision::Continue | Decision::EditRequest(_) => {}
        }
    }
    if let Some(id) = flow_id {
        record_buffered(&ctx.store, id, &res, version, settings.max_body_capture, FlowState::Completed);
        ctx.store.update(id, |f| f.timing.finished = Some(now_ms()));
    }

    let mut headers = to_header_map(&res.headers);
    strip_hop_by_hop(&mut headers);
    headers.remove(http::header::CONTENT_LENGTH);
    let body = match plan.throttle.as_ref().and_then(|t| t.download_bytes_per_sec()) {
        Some(rate) => ThrottledBody::new(full(res.body), rate).boxed(),
        None => full(res.body),
    };
    let mut out = Response::new(body);
    *out.status_mut() = StatusCode::from_u16(res.status).unwrap_or(StatusCode::BAD_GATEWAY);
    *out.headers_mut() = headers;
    Ok(out)
}

fn apply_remap(target: &mut Target, remap: &Remap, req: &mut RequestParts) {
    if let Some(scheme) = &remap.scheme {
        let old_default = default_port(target.scheme);
        target.scheme = if matches!(scheme.as_str(), "https" | "wss") { Scheme::Https } else { Scheme::Http };
        if remap.port.is_none() && target.port == old_default {
            target.port = default_port(target.scheme);
        }
    }
    if let Some(host) = &remap.host {
        target.host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    }
    if let Some(port) = remap.port {
        target.port = port;
    }
    if let Some(path) = &remap.path {
        let query = req.path.split_once('?').map(|(_, q)| q.to_string());
        req.path = match query {
            Some(q) if !path.contains('?') => format!("{path}?{q}"),
            _ => path.clone(),
        };
    }
    if !remap.keep_host_header {
        let authority = authority_for(target.scheme.as_str(), &target.host, target.port);
        set_header(&mut req.headers, "host", &authority);
    }
}

fn apply_request_edit(target: &mut Target, req: &mut RequestParts, edit: RequestEdit) {
    let method = edit.method.trim();
    if !method.is_empty() {
        req.method = method.to_ascii_uppercase();
    }
    req.headers = Headers(edit.headers);
    if let Ok(bytes) = B64.decode(edit.body_base64.as_bytes()) {
        req.body = Bytes::from(bytes);
        proxyzip_rules::remove_header(&mut req.headers, "content-length");
    }
    apply_url(target, req, &edit.url);
}

/// Point the request at a new URL (absolute, or just a path). When the host changes and the `Host` header still named
/// the old one, it follows.
fn apply_url(target: &mut Target, req: &mut RequestParts, url: &str) {
    let old_authority = authority_for(target.scheme.as_str(), &target.host, target.port);
    let url = url.trim();
    if url.starts_with('/') {
        req.path = url.to_string();
    } else if let Ok(uri) = url.parse::<http::Uri>()
        && let (Some(scheme), Some(host)) = (uri.scheme_str(), uri.host())
    {
        target.scheme = if matches!(scheme, "https" | "wss") { Scheme::Https } else { Scheme::Http };
        target.host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        target.port = uri.port_u16().unwrap_or(default_port(target.scheme));
        req.path = uri.path_and_query().map_or_else(|| "/".to_string(), |p| p.as_str().to_string());
    } else {
        tracing::debug!(url, "ignoring a URL that could not be parsed");
    }
    target.path = req.path.clone();
    let new_authority = authority_for(target.scheme.as_str(), &target.host, target.port);
    let host_matches_old = req.headers.get("host").is_none_or(|h| h.eq_ignore_ascii_case(&old_authority));
    if new_authority != old_authority && host_matches_old {
        set_header(&mut req.headers, "host", &new_authority);
    }
}

fn current_url(target: &Target, req: &RequestParts) -> String {
    let scheme = if target.scheme.is_secure() { "https" } else { "http" };
    format!("{scheme}://{}{}", authority_for(scheme, &target.host, target.port), req.path)
}

enum ScriptFlow {
    Continue {
        changed: bool,
    },
    /// A script returned `null` from `onRequest`.
    Block,
}

/// Run the matching request scripts in order. A script that fails is skipped and reported on the flow.
async fn run_request_scripts(
    ctx: &Arc<Ctx>,
    flow_id: Option<FlowId>,
    plan: &Plan,
    target: &mut Target,
    req: &mut RequestParts,
) -> ScriptFlow {
    // Scripts read and edit the body as text, so remove any content encoding first.
    let mut changed = decode_request_body(req);
    for rule in &plan.request_scripts {
        let input = ScriptRequest {
            method: req.method.clone(),
            url: current_url(target, req),
            headers: req.headers.0.clone(),
            body: req.body.to_vec(),
        };
        let out = ctx.scripts.run_request(&rule.source, &input).await;
        record_script_output(&ctx.store, flow_id, &rule.name, out.logs, out.verdict.as_ref().err());
        match out.verdict {
            Ok(Verdict::Unchanged) | Err(_) => {}
            Ok(Verdict::Block) => return ScriptFlow::Block,
            Ok(Verdict::Changed(edited)) => {
                req.method = edited.method;
                req.headers = Headers(edited.headers);
                req.body = Bytes::from(edited.body);
                proxyzip_rules::remove_header(&mut req.headers, "content-length");
                apply_url(target, req, &edited.url);
                changed = true;
            }
        }
    }
    ScriptFlow::Continue { changed }
}

/// Put what a script printed (and why it failed, if it did) on the flow.
fn record_script_output(
    store: &FlowStore,
    id: Option<FlowId>,
    name: &str,
    mut logs: Vec<LogLine>,
    error: Option<&String>,
) {
    let Some(id) = id else { return };
    if let Some(error) = error {
        let label = if name.is_empty() { "script".to_string() } else { format!("script “{name}”") };
        logs.push(LogLine { level: "error".into(), message: format!("{label}: {error}"), timestamp: now_ms() });
    }
    if logs.is_empty() {
        return;
    }
    let failed = error.is_some();
    store.update(id, |f| {
        f.logs.extend(logs);
        if failed {
            f.add_tag("script-error");
        }
    });
}

fn apply_response_edit(res: &mut ResponseParts, edit: ResponseEdit) {
    if (100..=999).contains(&edit.status) {
        res.status = edit.status;
    }
    res.headers = Headers(edit.headers);
    if let Ok(bytes) = B64.decode(edit.body_base64.as_bytes()) {
        res.body = Bytes::from(bytes);
    }
    proxyzip_rules::remove_header(&mut res.headers, "content-length");
}

/// Make the stored request match what is actually sent upstream.
fn sync_request(
    store: &FlowStore,
    id: FlowId,
    scheme: Scheme,
    target: &Target,
    req: &RequestParts,
    settings: &crate::config::ProxySettings,
) {
    let cap = settings.max_body_capture;
    store.update(id, |f| {
        f.request.method = req.method.clone();
        f.request.scheme = scheme;
        f.request.host = target.host.clone();
        f.request.port = target.port;
        f.request.path = req.path.clone();
        f.request.headers = req.headers.clone();
        f.request.body = req.body.slice(..req.body.len().min(cap));
        f.request.body_truncated = req.body.len() > cap;
    });
}

fn reason(status: u16) -> String {
    StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("").to_string()
}

fn record_head(store: &FlowStore, id: FlowId, status: u16, headers: &Headers, version: &str) {
    let headers = headers.clone();
    let version = version.to_string();
    store.update(id, |f| {
        f.timing.response_started = Some(now_ms());
        f.state = FlowState::Receiving;
        f.response = Some(proxyzip_core::Response {
            status,
            reason: reason(status),
            http_version: version,
            headers,
            body: Bytes::new(),
            body_truncated: false,
        });
    });
}

fn record_buffered(store: &FlowStore, id: FlowId, res: &ResponseParts, version: &str, cap: usize, state: FlowState) {
    store.update(id, |f| {
        f.timing.response_started.get_or_insert_with(now_ms);
        f.state = state;
        f.response = Some(proxyzip_core::Response {
            status: res.status,
            reason: reason(res.status),
            http_version: version.to_string(),
            headers: res.headers.clone(),
            body: res.body.slice(..res.body.len().min(cap)),
            body_truncated: res.body.len() > cap,
        });
    });
}

/// Answer without contacting any server, recording the answer on the flow.
fn synthetic(
    store: &FlowStore,
    id: Option<FlowId>,
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
) -> Response<ProxyBody> {
    if let Some(id) = id {
        let recorded = Headers(headers.clone());
        let recorded_body = body.clone();
        store.update(id, |f| {
            let now = now_ms();
            f.timing.request_sent = Some(now);
            f.timing.response_started = Some(now);
            f.timing.finished = Some(now);
            f.state = FlowState::Completed;
            f.response = Some(proxyzip_core::Response {
                status,
                reason: reason(status),
                http_version: "HTTP/1.1".into(),
                headers: recorded,
                body: recorded_body,
                body_truncated: false,
            });
        });
    }
    let mut out = Response::new(full(body));
    *out.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN);
    *out.headers_mut() = to_header_map(&Headers(headers));
    out
}

async fn map_local(store: &FlowStore, id: Option<FlowId>, file: &LocalFile) -> Response<ProxyBody> {
    match tokio::fs::read(&file.path).await {
        Ok(bytes) => {
            let content_type = file.content_type.clone().unwrap_or_else(|| content_type_for(&file.path).to_string());
            let mut headers = vec![
                ("content-type".to_string(), content_type),
                ("x-proxyzip-rule".to_string(), "map-local".to_string()),
            ];
            for (k, v) in &file.headers {
                headers.retain(|(name, _)| !name.eq_ignore_ascii_case(k));
                headers.push((k.clone(), v.clone()));
            }
            synthetic(store, id, file.status, headers, Bytes::from(bytes))
        }
        Err(e) => {
            let text = format!("ProxyZip Map Local: cannot read {}: {e}", file.path.display());
            let headers = vec![
                ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
                ("x-proxyzip-rule".to_string(), "map-local-error".to_string()),
            ];
            synthetic(store, id, 404, headers, Bytes::from(text))
        }
    }
}

fn fail_flow(store: &FlowStore, id: Option<FlowId>, msg: &str) {
    if let Some(id) = id {
        let msg = msg.to_string();
        store.update(id, |f| {
            f.state = FlowState::Failed;
            f.error = Some(msg);
            f.timing.finished = Some(now_ms());
        });
    }
}

fn fail(store: &FlowStore, id: Option<FlowId>, status: StatusCode, msg: String) -> Response<ProxyBody> {
    tracing::debug!(%msg, "request failed");
    fail_flow(store, id, &msg);
    error_response(status, &format!("ProxyZip: {msg}"))
}

fn error_response(status: StatusCode, msg: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .header("x-proxyzip-error", "1")
        .body(full(msg.to_string()))
        .unwrap()
}

/// Marks the flow failed if the request future is dropped before a response arrives.
struct PendingGuard {
    store: FlowStore,
    id: Option<FlowId>,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.store.update(id, |f| {
                f.state = FlowState::Failed;
                f.timing.finished = Some(now_ms());
                f.error = Some("client closed the connection before the server responded".into());
            });
        }
    }
}

/// Streams the upstream body to the client while keeping a (bounded) copy.
struct TapBody {
    inner: Incoming,
    buf: Vec<u8>,
    cap: usize,
    truncated: bool,
    store: FlowStore,
    id: Option<FlowId>,
    done: bool,
    /// Event streams publish what they have so far, so events show up while the stream is still open.
    live: bool,
    published: usize,
    last_publish: Instant,
}

impl TapBody {
    fn publish_partial(&mut self) {
        let Some(id) = self.id else { return };
        if self.buf.len() == self.published || self.last_publish.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.published = self.buf.len();
        self.last_publish = Instant::now();
        let partial = Bytes::copy_from_slice(&self.buf);
        self.store.update(id, |f| {
            if let Some(r) = f.response.as_mut() {
                r.body = partial;
            }
        });
    }

    fn finish(&mut self, error: Option<String>) {
        if self.done {
            return;
        }
        self.done = true;
        let Some(id) = self.id else { return };
        let body = Bytes::from(std::mem::take(&mut self.buf));
        let truncated = self.truncated;
        self.store.update(id, |f| {
            if let Some(r) = f.response.as_mut() {
                r.body = body;
                r.body_truncated = truncated;
            }
            f.timing.finished = Some(now_ms());
            match error {
                Some(e) => {
                    f.state = FlowState::Failed;
                    f.error = Some(e);
                }
                None => f.state = FlowState::Completed,
            }
        });
    }
}

impl Body for TapBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, hyper::Error>>> {
        match Pin::new(&mut self.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let room = self.cap.saturating_sub(self.buf.len());
                    if data.len() > room {
                        self.truncated = true;
                    }
                    let take = data.len().min(room);
                    let chunk = data.slice(..take);
                    self.buf.extend_from_slice(&chunk);
                    if self.live {
                        self.publish_partial();
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.finish(Some(format!("response body error: {e}")));
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.finish(None);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for TapBody {
    fn drop(&mut self) {
        if !self.done {
            if self.inner.is_end_stream() {
                self.finish(None);
            } else {
                self.finish(Some("client closed the connection before the response finished".into()));
            }
        }
    }
}
