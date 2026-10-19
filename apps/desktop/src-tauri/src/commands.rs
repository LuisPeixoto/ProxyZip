use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use proxyzip_codec::{BodyView, ViewContext};
use proxyzip_core::{FlowId, FlowSummary, Headers, LogLine, Timing, TlsInfo, WsDirection, WsKind};
use proxyzip_export::curl::ComposedRequest;
use proxyzip_proxy::Decision;
use proxyzip_rules::{RequestInfo, RuleSet, ThrottleProfile};
use proxyzip_script::{ScriptEngine, ScriptRequest, ScriptResponse, Verdict};
use proxyzip_tls::CertificateAuthority;
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, Preferences, RecentSession, protos_dir};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_flows(state: State<'_, AppState>) -> Vec<FlowSummary> {
    state.store.summaries()
}

#[derive(Serialize)]
pub struct RequestView {
    headers: Headers,
    query: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
    body: BodyView,
}

#[derive(Serialize)]
pub struct ResponseView {
    status: u16,
    reason: String,
    http_version: String,
    headers: Headers,
    set_cookies: Vec<String>,
    body: BodyView,
}

#[derive(Serialize)]
pub struct WsView {
    direction: WsDirection,
    kind: WsKind,
    timestamp: i64,
    size: usize,
    text: Option<String>,
    base64: String,
}

#[derive(Serialize)]
pub struct FlowDetail {
    summary: FlowSummary,
    request: RequestView,
    response: Option<ResponseView>,
    timing: Timing,
    tls: Option<TlsInfo>,
    comment: Option<String>,
    ws_messages: Vec<WsView>,
    logs: Vec<LogLine>,
}

#[tauri::command]
pub fn get_flow_detail(state: State<'_, AppState>, id: FlowId) -> CmdResult<FlowDetail> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let req = &flow.request;
    let query = req.query().map(proxyzip_codec::parse_query).unwrap_or_default();
    let cookies = req
        .headers
        .get_all("cookie")
        .flat_map(|c| c.split(';'))
        .filter_map(|kv| kv.trim().split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();

    // Schemas and the user's message-type mappings let Protobuf and gRPC bodies show field names.
    let protos = state.protos.read().clone();
    let info = RequestInfo {
        method: &req.method,
        scheme: req.scheme.as_str(),
        host: &req.host,
        port: req.port,
        path: &req.path,
    };
    let (request_type, response_type) = state.rules.read().proto_types(&info).unwrap_or((None, None));
    let request = RequestView {
        headers: req.headers.clone(),
        query,
        cookies,
        body: proxyzip_codec::view_with(ViewContext {
            headers: &req.headers,
            body: &req.body,
            truncated: req.body_truncated,
            path: &req.path,
            is_request: true,
            protos: Some(&protos),
            message_type: request_type.as_deref(),
        }),
    };
    let response = flow.response.as_ref().map(|r| ResponseView {
        status: r.status,
        reason: r.reason.clone(),
        http_version: r.http_version.clone(),
        headers: r.headers.clone(),
        set_cookies: r.headers.get_all("set-cookie").map(str::to_string).collect(),
        body: proxyzip_codec::view_with(ViewContext {
            headers: &r.headers,
            body: &r.body,
            truncated: r.body_truncated,
            path: &req.path,
            is_request: false,
            protos: Some(&protos),
            message_type: response_type.as_deref(),
        }),
    });
    let ws_messages = flow
        .ws_messages
        .iter()
        .map(|m| WsView {
            direction: m.direction,
            kind: m.kind,
            timestamp: m.timestamp,
            size: m.payload.len(),
            text: matches!(m.kind, WsKind::Text | WsKind::Close)
                .then(|| String::from_utf8_lossy(&m.payload).into_owned()),
            base64: B64.encode(&m.payload),
        })
        .collect();
    Ok(FlowDetail {
        summary: flow.summary(),
        request,
        response,
        timing: flow.timing.clone(),
        tls: flow.tls.clone(),
        comment: flow.comment.clone(),
        ws_messages,
        logs: flow.logs.clone(),
    })
}

#[tauri::command]
pub fn clear_flows(state: State<'_, AppState>) {
    state.store.clear();
}

#[tauri::command]
pub fn remove_flows(state: State<'_, AppState>, ids: Vec<FlowId>) {
    state.store.remove(&ids);
}

#[tauri::command]
pub fn set_pinned(state: State<'_, AppState>, ids: Vec<FlowId>, pinned: bool) {
    for id in ids {
        state.store.update(id, |f| f.pinned = pinned);
    }
}

#[tauri::command]
pub fn set_comment(state: State<'_, AppState>, id: FlowId, comment: Option<String>) {
    state.store.update(id, |f| f.comment = comment.filter(|c| !c.trim().is_empty()));
}

#[derive(Serialize)]
pub struct Status {
    running: bool,
    host: Option<String>,
    port: Option<u16>,
    listen: Option<String>,
    recording: bool,
    error: Option<String>,
    system_proxy: bool,
    ca_installed: bool,
    ca_detail: String,
    ca_path: String,
    lan_addresses: Vec<String>,
    platform: &'static str,
    listener_errors: Vec<String>,
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> CmdResult<Status> {
    let recording = state.proxy.lock().await.as_ref().map(|h| h.settings.read().recording).unwrap_or(false);
    let hp = state.client_host_port();
    let system_proxy = match &hp {
        Some((h, p)) => proxyzip_sysproxy::status().map(|s| s.points_to(h, *p)).unwrap_or(false),
        None => false,
    };
    let ca_path = state.ca_path();
    let trust = tokio::task::spawn_blocking({
        let p = ca_path.clone();
        move || proxyzip_certinstall::status(&p)
    })
    .await
    .map_err(err)?;
    Ok(Status {
        running: hp.is_some(),
        host: hp.as_ref().map(|(h, _)| h.clone()),
        port: hp.as_ref().map(|(_, p)| *p),
        listen: state.listen.read().map(|a| a.to_string()),
        recording,
        error: state.last_error.read().clone(),
        system_proxy,
        ca_installed: trust.installed,
        ca_detail: trust.detail,
        ca_path: ca_path.display().to_string(),
        lan_addresses: lan_addresses(),
        platform: std::env::consts::OS,
        listener_errors: state.listener_errors.read().clone(),
    })
}

/// Best-effort local IPv4 addresses (for configuring phones on the same network).
fn lan_addresses() -> Vec<String> {
    // Connecting a UDP socket sends nothing but reveals the outbound interface.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| vec![a.ip().to_string()])
        .unwrap_or_default()
}

#[tauri::command]
pub async fn set_recording(state: State<'_, AppState>, recording: bool) -> CmdResult<()> {
    if let Some(h) = state.proxy.lock().await.as_ref() {
        h.settings.write().recording = recording;
    }
    Ok(())
}

#[tauri::command]
pub async fn restart_proxy(state: State<'_, AppState>) -> CmdResult<String> {
    let owned = *state.system_proxy_owned.read();
    match state.start_proxy(false).await {
        Ok(addr) => {
            if owned && let Some((h, p)) = state.client_host_port() {
                proxyzip_sysproxy::enable(&h, p).map_err(err)?;
            }
            Ok(addr.to_string())
        }
        Err(e) => {
            let msg = format!("{e:#}");
            *state.last_error.write() = Some(msg.clone());
            Err(msg)
        }
    }
}

#[tauri::command]
pub async fn set_system_proxy(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    let hp = state.client_host_port();
    tokio::task::spawn_blocking(move || {
        if enabled {
            let (h, p) = hp.ok_or("proxy is not running")?;
            proxyzip_sysproxy::enable(&h, p).map_err(|e| format!("{e:#}"))
        } else {
            proxyzip_sysproxy::disable().map_err(|e| format!("{e:#}"))
        }
    })
    .await
    .map_err(err)??;
    *state.system_proxy_owned.write() = enabled;
    Ok(())
}

#[tauri::command]
pub async fn install_ca(state: State<'_, AppState>) -> CmdResult<()> {
    let p = state.ca_path();
    tokio::task::spawn_blocking(move || proxyzip_certinstall::install(&p).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn uninstall_ca(state: State<'_, AppState>) -> CmdResult<()> {
    let p = state.ca_path();
    tokio::task::spawn_blocking(move || proxyzip_certinstall::uninstall(&p).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn install_ca_ios_simulator(state: State<'_, AppState>) -> CmdResult<()> {
    let p = state.ca_path();
    tokio::task::spawn_blocking(move || proxyzip_certinstall::install_ios_simulator(&p).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn regenerate_ca(state: State<'_, AppState>) -> CmdResult<()> {
    let ca = CertificateAuthority::regenerate(&state.home).map_err(err)?;
    *state.ca.write() = Arc::new(ca);
    state.start_proxy(false).await.map_err(|e| format!("{e:#}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Preferences {
    state.prefs.read().clone()
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, prefs: Preferences) -> CmdResult<()> {
    let needs_restart = {
        let old = state.prefs.read();
        old.port != prefs.port || old.allow_lan != prefs.allow_lan
    };
    prefs.save(&state.home).map_err(err)?;
    *state.prefs.write() = prefs;
    if needs_restart {
        restart_proxy(state).await?;
    } else {
        state.apply_live_settings().await;
    }
    Ok(())
}

/// Add/remove a host from the SSL proxying exclude list (context-menu action).
#[tauri::command]
pub async fn ssl_proxying_toggle_host(state: State<'_, AppState>, host: String, exclude: bool) -> CmdResult<()> {
    let prefs = {
        let mut p = state.prefs.write();
        p.ssl.exclude.retain(|h| !h.eq_ignore_ascii_case(&host));
        if exclude {
            p.ssl.exclude.push(host);
        }
        p.clone()
    };
    prefs.save(&state.home).map_err(err)?;
    state.apply_live_settings().await;
    Ok(())
}

#[tauri::command]
pub fn copy_as_curl(state: State<'_, AppState>, id: FlowId) -> CmdResult<String> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    Ok(proxyzip_export::curl::to_curl(&flow))
}

#[tauri::command]
pub fn export_har(state: State<'_, AppState>, path: PathBuf, ids: Option<Vec<FlowId>>) -> CmdResult<usize> {
    let flows = match ids {
        Some(ids) => state.store.get_many(&ids),
        None => state.store.all(),
    };
    let json = proxyzip_export::har::export_json(&flows).map_err(err)?;
    std::fs::write(&path, json).map_err(err)?;
    Ok(flows.len())
}

#[tauri::command]
pub fn import_har(state: State<'_, AppState>, path: PathBuf) -> CmdResult<usize> {
    let json = std::fs::read_to_string(&path).map_err(err)?;
    let flows = proxyzip_export::har::import(&json).map_err(|e| format!("{e:#}"))?;
    let n = flows.len();
    for f in flows {
        state.store.import(f);
    }
    Ok(n)
}

#[tauri::command]
pub fn terminal_env(state: State<'_, AppState>) -> CmdResult<String> {
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    Ok(proxyzip_certinstall::terminal_env(&h, p, &state.ca_path()))
}

/// Send a request through ProxyZip itself, so it shows up in the list like any other traffic.
async fn send_through_proxy(
    state: &AppState,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> CmdResult<u16> {
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    let ca_pem = state.ca.read().cert_pem().to_string();
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{h}:{p}")).map_err(err)?)
        .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).map_err(err)?)
        .redirect(reqwest::redirect::Policy::none())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(err)?;
    let method = reqwest::Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes()).map_err(err)?;
    let url = url.replace("ws://", "http://").replace("wss://", "https://");
    let mut builder = client.request(method, url);
    for (k, v) in headers {
        let lk = k.to_ascii_lowercase();
        if matches!(lk.as_str(), "host" | "content-length" | "connection" | "proxy-connection" | "transfer-encoding") {
            continue;
        }
        builder = builder.header(k, v);
    }
    let response = builder.body(body).send().await.map_err(err)?;
    let status = response.status().as_u16();
    response.bytes().await.map_err(err)?;
    Ok(status)
}

/// Re-send a captured request exactly as it was captured.
#[tauri::command]
pub async fn repeat_flow(state: State<'_, AppState>, id: FlowId) -> CmdResult<()> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let req = &flow.request;
    send_through_proxy(&state, &req.method, &req.url(), &req.headers.0, req.body.to_vec()).await?;
    Ok(())
}

/// A request composed by hand in the interface.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ComposeInput {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body_base64: String,
}

#[tauri::command]
pub async fn send_request(state: State<'_, AppState>, request: ComposeInput) -> CmdResult<u16> {
    let body = B64.decode(request.body_base64.as_bytes()).map_err(|e| format!("the body is not valid base64: {e}"))?;
    send_through_proxy(&state, &request.method, &request.url, &request.headers, body).await
}

#[tauri::command]
pub fn parse_curl(command: String) -> CmdResult<ComposeInput> {
    let ComposedRequest { method, url, headers, body } = proxyzip_export::curl::parse(&command)?;
    Ok(ComposeInput { method, url, headers, body_base64: B64.encode(body) })
}

#[tauri::command]
pub fn get_rules(state: State<'_, AppState>) -> RuleSet {
    state.rules.read().clone()
}

/// Save and apply rules. Returns messages for listeners that could not start.
#[tauri::command]
pub async fn save_rules(state: State<'_, AppState>, rules: RuleSet) -> CmdResult<Vec<String>> {
    state.apply_rules(rules).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn throttle_presets() -> Vec<ThrottleProfile> {
    ThrottleProfile::PRESETS.iter().filter_map(|p| ThrottleProfile::preset(p)).collect()
}

/// Answer a flow that is paused at a breakpoint.
#[tauri::command]
pub async fn resolve_breakpoint(state: State<'_, AppState>, id: FlowId, mut decision: Decision) -> CmdResult<bool> {
    if let Decision::EditRequest(edit) = &mut decision {
        // The editor shows the decoded body, so the encoding header no longer describes it.
        edit.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-encoding"));
    }
    let hub = state.proxy.lock().await.as_ref().map(|h| h.breakpoints.clone()).ok_or("proxy is not running")?;
    Ok(hub.resolve(id, decision))
}

/// Let every paused flow continue unchanged.
#[tauri::command]
pub async fn release_breakpoints(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(h) = state.proxy.lock().await.as_ref() {
        h.breakpoints.release_all();
    }
    Ok(())
}

// ---- Protobuf schemas -------------------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProtoInfo {
    files: Vec<String>,
    messages: Vec<String>,
    methods: Vec<String>,
}

fn proto_info(state: &AppState) -> ProtoInfo {
    let mut files: Vec<String> = std::fs::read_dir(protos_dir(&state.home))
        .map(|d| d.flatten().filter(|e| e.path().is_file()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    files.sort();
    let registry = state.protos.read();
    ProtoInfo { files, messages: registry.message_names(), methods: registry.method_names() }
}

#[tauri::command]
pub fn list_protos(state: State<'_, AppState>) -> ProtoInfo {
    proto_info(&state)
}

/// Import a descriptor set (`protoc --descriptor_set_out=schema.desc --include_imports …`).
#[tauri::command]
pub fn import_proto(state: State<'_, AppState>, path: PathBuf) -> CmdResult<ProtoInfo> {
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    // Validate before keeping a copy, so a bad file never ends up in the schemas folder.
    proxyzip_codec::ProtoRegistry::new().add_descriptor_set(&bytes).map_err(err)?;
    let dir = protos_dir(&state.home);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("the file has no name")?;
    std::fs::write(dir.join(name), bytes).map_err(err)?;
    state.reload_protos();
    Ok(proto_info(&state))
}

#[tauri::command]
pub fn remove_proto(state: State<'_, AppState>, name: String) -> CmdResult<ProtoInfo> {
    // Only plain file names inside the schemas folder.
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err("invalid file name".into());
    }
    std::fs::remove_file(protos_dir(&state.home).join(&name)).map_err(err)?;
    state.reload_protos();
    Ok(proto_info(&state))
}

// ---- Code generation and jq --------------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LanguageInfo {
    id: &'static str,
    label: &'static str,
    syntax: &'static str,
}

#[tauri::command]
pub fn code_languages() -> Vec<LanguageInfo> {
    proxyzip_export::codegen::Language::ALL
        .iter()
        .map(|l| LanguageInfo { id: l.id(), label: l.label(), syntax: l.syntax() })
        .collect()
}

#[tauri::command]
pub fn generate_code(state: State<'_, AppState>, id: FlowId, language: String) -> CmdResult<String> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let language = proxyzip_export::codegen::Language::from_id(&language).ok_or("unknown language")?;
    Ok(proxyzip_export::codegen::generate(&flow, language))
}

/// Run a jq filter over a flow's request or response body.
#[tauri::command]
pub fn query_body(
    state: State<'_, AppState>,
    id: FlowId,
    part: String,
    expr: String,
) -> CmdResult<Vec<serde_json::Value>> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let (headers, body) = match part.as_str() {
        "request" => (&flow.request.headers, &flow.request.body),
        "response" => {
            let r = flow.response.as_ref().ok_or("this flow has no response")?;
            (&r.headers, &r.body)
        }
        _ => return Err("part must be request or response".into()),
    };
    let (decoded, _, error) = proxyzip_codec::decode_content(headers, body);
    if let Some(e) = error {
        return Err(format!("could not decode the body: {e}"));
    }
    proxyzip_codec::query::run(&decoded, &expr)
}

// ---- Sessions and exports ----------------------------------------------------------------------------------------------

fn select_flows(state: &AppState, ids: Option<Vec<FlowId>>) -> Vec<proxyzip_core::Flow> {
    match ids {
        Some(ids) if !ids.is_empty() => state.store.get_many(&ids),
        _ => state.store.all(),
    }
}

/// Save flows (all of them, or the selected ones) to a `.pzsession` file.
#[tauri::command]
pub fn save_session(state: State<'_, AppState>, path: PathBuf, ids: Option<Vec<FlowId>>) -> CmdResult<usize> {
    let flows = select_flows(&state, ids);
    let bytes = proxyzip_export::session::write(&flows).map_err(|e| format!("{e:#}"))?;
    std::fs::write(&path, bytes).map_err(err)?;
    state.remember_session(&path, flows.len());
    Ok(flows.len())
}

/// Open a `.pzsession` file, replacing the current list or adding to it.
#[tauri::command]
pub fn open_session(state: State<'_, AppState>, path: PathBuf, append: bool) -> CmdResult<usize> {
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let flows = proxyzip_export::session::read(&bytes).map_err(|e| format!("{e:#}"))?;
    if !append {
        state.store.clear();
    }
    let count = flows.len();
    for flow in flows {
        state.store.import(flow);
    }
    state.remember_session(&path, count);
    Ok(count)
}

#[tauri::command]
pub fn recent_sessions(state: State<'_, AppState>) -> Vec<RecentSession> {
    state.recent_sessions()
}

#[tauri::command]
pub fn forget_recent_session(state: State<'_, AppState>, path: String) {
    state.forget_session(&path);
}

#[tauri::command]
pub fn export_csv(state: State<'_, AppState>, path: PathBuf, ids: Option<Vec<FlowId>>) -> CmdResult<usize> {
    let flows = select_flows(&state, ids);
    std::fs::write(&path, proxyzip_export::csv::export(&flows)).map_err(err)?;
    Ok(flows.len())
}

#[tauri::command]
pub fn export_postman(state: State<'_, AppState>, path: PathBuf, ids: Option<Vec<FlowId>>) -> CmdResult<usize> {
    let flows = select_flows(&state, ids);
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("ProxyZip").to_string();
    let json = proxyzip_export::postman::export_json(&name, &flows).map_err(err)?;
    std::fs::write(&path, json).map_err(err)?;
    Ok(flows.len())
}

// ---- Script testing ---------------------------------------------------------------------------------------------------

fn verdict_json<T>(
    verdict: Result<Verdict<T>, String>,
    to_json: impl Fn(&T) -> serde_json::Value,
    unchanged: serde_json::Value,
) -> serde_json::Value {
    match verdict {
        Ok(Verdict::Unchanged) => serde_json::json!({ "outcome": "unchanged", "value": unchanged }),
        Ok(Verdict::Changed(v)) => serde_json::json!({ "outcome": "changed", "value": to_json(&v) }),
        Ok(Verdict::Block) => serde_json::json!({ "outcome": "block", "value": null }),
        Err(e) => serde_json::json!({ "outcome": "error", "error": e, "value": unchanged }),
    }
}

/// Run a script against a captured flow without sending anything, and show what it would do.
#[tauri::command]
pub async fn test_script(state: State<'_, AppState>, source: String, id: FlowId) -> CmdResult<serde_json::Value> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let engine = match state.proxy.lock().await.as_ref() {
        Some(handle) => handle.scripts.clone(),
        None => Arc::new(ScriptEngine::default()),
    };
    let readable = |headers: &Headers, body: &[u8]| {
        let (decoded, encodings, error) = proxyzip_codec::decode_content(headers, body);
        let mut headers = headers.0.clone();
        if !encodings.is_empty() && error.is_none() {
            headers.retain(|(k, _)| {
                !k.eq_ignore_ascii_case("content-encoding") && !k.eq_ignore_ascii_case("content-length")
            });
        }
        (headers, decoded.to_vec())
    };
    let (headers, body) = readable(&flow.request.headers, &flow.request.body);
    let request = ScriptRequest { method: flow.request.method.clone(), url: flow.request.url(), headers, body };

    let request_run = engine.run_request(&source, &request).await;
    let mut logs = request_run.logs;
    let effective = match &request_run.verdict {
        Ok(Verdict::Changed(r)) => r.clone(),
        _ => request.clone(),
    };
    let request_json = verdict_json(request_run.verdict, ScriptRequest::to_json, request.to_json());

    let response_json = match &flow.response {
        Some(r) => {
            let (headers, body) = readable(&r.headers, &r.body);
            let response = ScriptResponse { status: r.status, headers, body };
            let run = engine.run_response(&source, &effective, &response).await;
            logs.extend(run.logs);
            Some(verdict_json(run.verdict, ScriptResponse::to_json, response.to_json()))
        }
        None => None,
    };
    Ok(serde_json::json!({ "request": request_json, "response": response_json, "logs": logs }))
}
