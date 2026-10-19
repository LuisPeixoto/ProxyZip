use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::header::HeaderValue;
use http_body::{Body, Frame, SizeHint};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use proxyzip_core::{FlowId, FlowState, FlowStore, Scheme, now_ms};

use crate::server::{Conn, Ctx};
use crate::util::{
    ProxyBody, format_authority, full, is_websocket_upgrade, split_authority, strip_hop_by_hop, to_headers, version_str,
};
use crate::{local, websocket};

struct Target {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

fn resolve_target(req: &Request<Incoming>, conn: &Conn) -> Option<Target> {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
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
    let default_port = if scheme == Scheme::Https { 443 } else { 80 };
    let host = req.uri().host()?.trim_start_matches('[').trim_end_matches(']').to_string();
    let port = req.uri().port_u16().unwrap_or(default_port);
    Some(Target { scheme, host, port, path })
}

pub(crate) async fn forward(mut req: Request<Incoming>, ctx: Arc<Ctx>, conn: Conn) -> Response<ProxyBody> {
    let Some(target) = resolve_target(&req, &conn) else {
        return error_response(StatusCode::BAD_REQUEST, "ProxyZip: request has no target host");
    };
    if conn.tunnel_target.is_none() && local::is_local(&target.host, target.port, &ctx) {
        return local::respond(&target.path, &ctx);
    }
    if target.host.eq_ignore_ascii_case(local::CERT_HOST) {
        return local::respond(&target.path, &ctx);
    }

    let websocket = is_websocket_upgrade(req.headers()) && req.version() <= http::Version::HTTP_11;
    let client_upgrade = websocket.then(|| hyper::upgrade::on(&mut req));
    let settings = ctx.settings.read().clone();

    let (parts, body) = req.into_parts();
    let body = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("ProxyZip: reading request body failed: {e}"));
        }
    };

    let scheme = match (websocket, target.scheme.is_secure()) {
        (true, true) => Scheme::Wss,
        (true, false) => Scheme::Ws,
        (false, true) => Scheme::Https,
        (false, false) => Scheme::Http,
    };
    let flow_id = settings.recording.then(|| {
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
        if conn.tls.is_some() {
            let tls = conn.tls.clone();
            ctx.store.update(id, |f| f.tls = tls);
        }
        id
    });

    let wire_scheme = if target.scheme.is_secure() { "https" } else { "http" };
    let uri = format!("{wire_scheme}://{}{}", format_authority(&target.host, target.port), target.path);
    let mut headers = parts.headers.clone();
    if websocket {
        strip_hop_by_hop(&mut headers);
        headers.insert(http::header::CONNECTION, HeaderValue::from_static("Upgrade"));
        headers.insert(http::header::UPGRADE, HeaderValue::from_static("websocket"));
        // tungstenite cannot relay compressed frames; keep the session uncompressed.
        headers.remove(http::header::SEC_WEBSOCKET_EXTENSIONS);
    } else {
        strip_hop_by_hop(&mut headers);
    }
    let mut builder = Request::builder().method(parts.method.clone()).uri(&uri).version(http::Version::HTTP_11);
    *builder.headers_mut().unwrap() = headers;
    let upstream_req = match builder.body(Full::new(body)) {
        Ok(r) => r,
        Err(e) => {
            return fail(&ctx.store, flow_id, StatusCode::BAD_REQUEST, format!("invalid upstream request {uri}: {e}"));
        }
    };

    if let Some(id) = flow_id {
        ctx.store.update(id, |f| f.timing.request_sent = Some(now_ms()));
    }
    // hyper drops this future when the client hangs up; the guard records that.
    let mut guard = PendingGuard { store: ctx.store.clone(), id: flow_id };
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
            return fail(&ctx.store, flow_id, StatusCode::BAD_GATEWAY, msg);
        }
    };
    guard.id = None;

    let status = response.status();
    if let Some(id) = flow_id {
        let headers = to_headers(response.headers());
        let version = version_str(response.version());
        ctx.store.update(id, |f| {
            f.timing.response_started = Some(now_ms());
            f.state = FlowState::Receiving;
            f.response = Some(proxyzip_core::Response {
                status: status.as_u16(),
                reason: status.canonical_reason().unwrap_or("").to_string(),
                http_version: version,
                headers,
                body: Bytes::new(),
                body_truncated: false,
            });
        });
    }

    if status == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_upgrade) = client_upgrade
    {
        let server_upgrade = hyper::upgrade::on(&mut response);
        let mut out = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
        *out.headers_mut().unwrap() = response.headers().clone();
        if let Some(id) = flow_id {
            ctx.store.update(id, |f| f.state = FlowState::WebSocketOpen);
        }
        tokio::spawn(websocket::relay(client_upgrade, server_upgrade, ctx.store.clone(), flow_id));
        return out.body(full("")).unwrap();
    }

    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop(&mut parts.headers);
    let body = TapBody {
        inner: body,
        buf: Vec::new(),
        cap: settings.max_body_capture,
        truncated: false,
        store: ctx.store.clone(),
        id: flow_id,
        done: false,
    };
    Response::from_parts(parts, body.boxed())
}

fn fail(store: &FlowStore, id: Option<FlowId>, status: StatusCode, msg: String) -> Response<ProxyBody> {
    tracing::debug!(%msg, "request failed");
    if let Some(id) = id {
        let m = msg.clone();
        store.update(id, |f| {
            f.state = FlowState::Failed;
            f.error = Some(m);
            f.timing.finished = Some(now_ms());
        });
    }
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
}

impl TapBody {
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
