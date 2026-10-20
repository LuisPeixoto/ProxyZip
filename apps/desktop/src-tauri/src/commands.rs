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
    /// Tabs that enabled plugins add to the inspector.
    plugin_tabs: Vec<PluginTab>,
}

#[derive(Serialize)]
pub struct PluginTab {
    plugin: String,
    plugin_name: String,
    id: String,
    name: String,
}

#[tauri::command]
pub async fn get_flow_detail(state: State<'_, AppState>, id: FlowId) -> CmdResult<FlowDetail> {
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
    let mut request = RequestView {
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
    let mut response = flow.response.as_ref().map(|r| ResponseView {
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
    let url = flow.request.url();
    plugin_decode(&state, &mut request.body, &url).await;
    if let Some(r) = response.as_mut() {
        plugin_decode(&state, &mut r.body, &url).await;
    }
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
        plugin_tabs: state
            .plugins
            .read()
            .tabs()
            .into_iter()
            .map(|(p, t)| PluginTab {
                plugin: p.manifest.id.clone(),
                plugin_name: p.manifest.name.clone(),
                id: t.id.clone(),
                name: t.name.clone(),
            })
            .collect(),
    })
}

/// Let plugin viewers decode a body that no built-in viewer recognised.
async fn plugin_decode(state: &AppState, body: &mut proxyzip_codec::BodyView, url: &str) {
    use proxyzip_script::{PluginEntry, Verdict};
    if body.structured.is_some() || body.size == 0 {
        return;
    }
    let content_type = body.mime.clone().unwrap_or_default();
    let candidates: Vec<(String, String, String, String)> = state
        .plugins
        .read()
        .viewers_for(&content_type)
        .map(|(p, v)| (p.manifest.id.clone(), p.manifest.name.clone(), v.name.clone(), p.runtime_source()))
        .collect();
    if candidates.is_empty() {
        return;
    }
    let Some(engine) = state.proxy.lock().await.as_ref().map(|h| h.scripts.clone()) else { return };
    for (_id, plugin_name, viewer_name, source) in candidates {
        let first = serde_json::json!({ "text": body.text, "base64": body.base64 });
        let second = serde_json::json!({ "contentType": content_type, "url": url });
        match engine.call(&source, PluginEntry::Decode, first, second).await.verdict {
            Ok(Verdict::Changed(v)) => {
                let json = v.get("json").cloned().unwrap_or_else(|| v.clone());
                let format = v.get("format").and_then(|f| f.as_str()).unwrap_or(&viewer_name).to_string();
                body.structured = Some(proxyzip_codec::Structured {
                    format,
                    json,
                    note: Some(format!("Decoded by the {plugin_name} plugin")),
                });
                return;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(plugin = %plugin_name, error = %e, "plugin viewer failed"),
        }
    }
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
    let flows = export_flows(&state, ids);
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
pub fn terminal_env(state: State<'_, AppState>, shell: Option<String>) -> CmdResult<String> {
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    let cert = state.ca_path();
    Ok(match shell.as_deref().and_then(proxyzip_certinstall::runtime::Shell::from_name) {
        Some(sh) => proxyzip_certinstall::terminal_env_for(sh, &h, p, &cert),
        None => proxyzip_certinstall::terminal_env(&h, p, &cert),
    })
}

#[derive(Serialize)]
pub struct McpInfo {
    pub enabled: bool,
    pub port: u16,
    pub running: bool,
    pub url: String,
    /// Shown so the user can copy it into an assistant's configuration. Anyone with it can read the captured traffic.
    pub token: String,
    pub allow_changes: bool,
    pub error: Option<String>,
}

fn mcp_info(state: &AppState) -> CmdResult<McpInfo> {
    let (enabled, port, allow) = {
        let p = state.prefs.read();
        (p.mcp_enabled, p.mcp_port, p.mcp_allow_changes)
    };
    Ok(McpInfo {
        enabled,
        port,
        running: state.mcp_state.read().is_some(),
        url: format!("http://127.0.0.1:{port}/mcp"),
        token: proxyzip_mcp::discovery::load_or_create_token(&state.home).map_err(err)?,
        allow_changes: allow,
        error: state.mcp_error.read().clone(),
    })
}

#[tauri::command]
pub fn get_mcp_info(state: State<'_, AppState>) -> CmdResult<McpInfo> {
    mcp_info(&state)
}

#[tauri::command]
pub async fn set_mcp(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
    port: u16,
    allow_changes: bool,
) -> CmdResult<McpInfo> {
    let prefs = {
        let mut p = state.prefs.write();
        p.mcp_enabled = enabled;
        p.mcp_port = port;
        p.mcp_allow_changes = allow_changes;
        p.clone()
    };
    prefs.save(&state.home).map_err(err)?;
    crate::mcp::restart(&app).await;
    mcp_info(&state)
}

#[tauri::command]
pub async fn regenerate_mcp_token(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<McpInfo> {
    proxyzip_mcp::discovery::regenerate_token(&state.home).map_err(err)?;
    crate::mcp::restart(&app).await;
    mcp_info(&state)
}

#[tauri::command]
pub async fn open_terminal(state: State<'_, AppState>) -> CmdResult<()> {
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    let cert = state.ca_path();
    tokio::task::spawn_blocking(move || {
        proxyzip_certinstall::terminal::open(&h, p, &cert).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct JavaInfo {
    pub found: bool,
    pub truststore: Option<String>,
    pub store_type: Option<&'static str>,
    pub password: &'static str,
    pub options: Option<String>,
}

#[tauri::command]
pub async fn java_setup(state: State<'_, AppState>) -> CmdResult<JavaInfo> {
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    let cert = state.ca_path();
    tokio::task::spawn_blocking(move || {
        let files = proxyzip_certinstall::runtime::prepare(&cert).map_err(|e| format!("{e:#}"))?;
        let snippet =
            proxyzip_certinstall::runtime::render(proxyzip_certinstall::runtime::Shell::Posix, &h, p, &cert, &files);
        let options = snippet.lines().find(|l| l.starts_with("export JAVA_TOOL_OPTIONS=")).map(|l| {
            l.trim_start_matches("export JAVA_TOOL_OPTIONS=\"${JAVA_TOOL_OPTIONS:+$JAVA_TOOL_OPTIONS }")
                .trim_end_matches('"')
                .to_string()
        });
        Ok(JavaInfo {
            found: files.java_truststore.is_some(),
            truststore: files.java_truststore.as_ref().map(|(p, _)| p.display().to_string()),
            store_type: files.java_truststore.as_ref().map(|(_, t)| *t),
            password: proxyzip_certinstall::runtime::JAVA_STORE_PASSWORD,
            options,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn install_firefox(state: State<'_, AppState>) -> CmdResult<proxyzip_certinstall::firefox::FirefoxReport> {
    let cert = state.ca_path();
    tokio::task::spawn_blocking(move || proxyzip_certinstall::firefox::install(&cert).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[derive(Serialize)]
pub struct IosInfo {
    /// Address the phone should use as its Wi-Fi proxy, and the page to open on it.
    pub proxy: Option<String>,
    pub page: Option<String>,
    pub qr_svg: Option<String>,
}

#[tauri::command]
pub fn ios_setup(state: State<'_, AppState>) -> CmdResult<IosInfo> {
    let port = state.client_host_port().map(|(_, p)| p).ok_or("proxy is not running")?;
    let Some(ip) = lan_addresses().into_iter().next() else {
        return Ok(IosInfo { proxy: None, page: None, qr_svg: None });
    };
    let page = format!("http://{ip}:{port}/");
    let qr_svg = qrcode::QrCode::new(page.as_bytes())
        .map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(180, 180).quiet_zone(true).build())
        .ok();
    Ok(IosInfo { proxy: Some(format!("{ip}:{port}")), page: Some(page), qr_svg })
}

#[tauri::command]
pub async fn android_devices() -> CmdResult<Vec<proxyzip_certinstall::android::Device>> {
    tokio::task::spawn_blocking(|| proxyzip_certinstall::android::list_devices().map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
pub async fn android_set_proxy(state: State<'_, AppState>, serial: String, enable: bool) -> CmdResult<()> {
    let port = state.client_host_port().map(|(_, p)| p).ok_or("proxy is not running")?;
    tokio::task::spawn_blocking(move || {
        if enable {
            proxyzip_certinstall::android::set_proxy(&serial, port)
        } else {
            proxyzip_certinstall::android::clear_proxy(&serial)
        }
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)?
}

/// Install the CA on an Android device, as a system certificate or through the user-certificate installer.
#[tauri::command]
pub async fn android_install_cert(state: State<'_, AppState>, serial: String, system: bool) -> CmdResult<String> {
    let pem_path = state.ca_path();
    let der = state.ca.read().cert_der().to_vec();
    tokio::task::spawn_blocking(move || {
        use proxyzip_certinstall::android;
        if system {
            android::install_system_cert(&serial, &pem_path, &der).map_err(|e| format!("{e:#}"))
        } else {
            let file = pem_path.with_file_name("proxyzip-ca.crt");
            std::fs::write(&file, &der).map_err(|e| e.to_string())?;
            android::install_user_cert(&serial, &file).map_err(|e| format!("{e:#}"))?;
            Ok("Opened the certificate installer on the device. Confirm it there.".to_string())
        }
    })
    .await
    .map_err(err)?
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

// ---- Plugins ----------------------------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PluginList {
    plugins: Vec<proxyzip_script::plugin::Plugin>,
    errors: Vec<proxyzip_script::plugin::LoadError>,
    dir: String,
}

fn plugin_list(state: &AppState) -> PluginList {
    let reg = state.plugins.read();
    PluginList {
        plugins: reg.plugins.clone(),
        errors: reg.errors.clone(),
        dir: crate::state::plugins_dir(&state.home).display().to_string(),
    }
}

#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>) -> PluginList {
    plugin_list(&state)
}

#[tauri::command]
pub async fn install_plugin(state: State<'_, AppState>, path: String) -> CmdResult<PluginList> {
    state.plugins.write().install(std::path::Path::new(&path)).map_err(|e| format!("{e:#}"))?;
    state.refresh_plugin_hooks().await;
    Ok(plugin_list(&state))
}

#[tauri::command]
pub async fn set_plugin_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> CmdResult<PluginList> {
    state.plugins.write().set_enabled(&id, enabled).map_err(|e| format!("{e:#}"))?;
    state.refresh_plugin_hooks().await;
    Ok(plugin_list(&state))
}

#[tauri::command]
pub async fn remove_plugin(state: State<'_, AppState>, id: String) -> CmdResult<PluginList> {
    state.plugins.write().remove(&id).map_err(|e| format!("{e:#}"))?;
    state.refresh_plugin_hooks().await;
    Ok(plugin_list(&state))
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
    id: String,
    label: String,
    syntax: String,
}

#[tauri::command]
pub fn code_languages(state: State<'_, AppState>) -> Vec<LanguageInfo> {
    let mut out: Vec<LanguageInfo> = proxyzip_export::codegen::Language::ALL
        .iter()
        .map(|l| LanguageInfo { id: l.id().into(), label: l.label().into(), syntax: l.syntax().into() })
        .collect();
    for p in state.plugins.read().enabled() {
        for c in &p.manifest.contributes.copy_as {
            out.push(LanguageInfo {
                id: format!("plugin:{}:{}", p.manifest.id, c.id),
                label: format!("{} ({})", c.name, p.manifest.name),
                syntax: c.syntax.clone(),
            });
        }
    }
    out
}

#[tauri::command]
pub async fn generate_code(state: State<'_, AppState>, id: FlowId, language: String) -> CmdResult<String> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    if let Some(rest) = language.strip_prefix("plugin:") {
        let (plugin, entry) = rest.split_once(':').ok_or("unknown language")?;
        let source = state
            .plugins
            .read()
            .find_copy_as(plugin, entry)
            .map(|(p, _)| p.runtime_source())
            .ok_or("that plugin is not available")?;
        let engine = state.proxy.lock().await.as_ref().map(|h| h.scripts.clone()).ok_or("proxy is not running")?;
        let (decoded, _, _) = proxyzip_codec::decode_content(&flow.request.headers, &flow.request.body);
        let request = proxyzip_script::ScriptRequest {
            method: flow.request.method.clone(),
            url: flow.request.url(),
            headers: flow.request.headers.0.clone(),
            body: decoded.to_vec(),
        };
        let out = engine
            .call(&source, proxyzip_script::PluginEntry::Generate, request.to_json(), serde_json::json!({}))
            .await;
        return match out.verdict {
            Ok(proxyzip_script::Verdict::Changed(v)) => Ok(match v {
                serde_json::Value::String(s) => s,
                other => serde_json::to_string_pretty(&other).unwrap_or_default(),
            }),
            Ok(_) => Err("the plugin returned nothing for this request".into()),
            Err(e) => Err(e),
        };
    }
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

/// Flows for an export: redacted when the preference says so.
fn export_flows(state: &AppState, ids: Option<Vec<FlowId>>) -> Vec<proxyzip_core::Flow> {
    let flows = select_flows(state, ids);
    if state.prefs.read().redact_exports {
        proxyzip_export::redact::redacted(&flows, &Default::default())
    } else {
        flows
    }
}

/// Save flows (all of them, or the selected ones) to a `.pzsession` file. `redacted` removes secrets first.
#[tauri::command]
pub fn save_session(
    state: State<'_, AppState>,
    path: PathBuf,
    ids: Option<Vec<FlowId>>,
    redacted: Option<bool>,
) -> CmdResult<usize> {
    let mut flows = select_flows(&state, ids);
    if redacted == Some(true) {
        flows = proxyzip_export::redact::redacted(&flows, &Default::default());
    }
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
    let flows = export_flows(&state, ids);
    std::fs::write(&path, proxyzip_export::csv::export(&flows)).map_err(err)?;
    Ok(flows.len())
}

#[tauri::command]
pub fn export_postman(state: State<'_, AppState>, path: PathBuf, ids: Option<Vec<FlowId>>) -> CmdResult<usize> {
    let flows = export_flows(&state, ids);
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("ProxyZip").to_string();
    let json = proxyzip_export::postman::export_json(&name, &flows).map_err(err)?;
    std::fs::write(&path, json).map_err(err)?;
    Ok(flows.len())
}

// ---- Workspaces -------------------------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct WorkspaceSummary {
    rules: usize,
    protos: usize,
    plugins: usize,
}

/// Save the rules, imported descriptors, SSL lists and enabled plugins to a `.pzworkspace` file.
#[tauri::command]
pub fn export_workspace(state: State<'_, AppState>, path: PathBuf, redact: bool) -> CmdResult<WorkspaceSummary> {
    use proxyzip_export::workspace::{SslSettings, Workspace};
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("ProxyZip").to_string();
    let mut workspace = Workspace::new(name, state.rules.read().clone());
    if let Ok(entries) = std::fs::read_dir(crate::state::protos_dir(&state.home)) {
        let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for file in files {
            if let (Some(n), Ok(bytes)) = (file.file_name().and_then(|n| n.to_str()), std::fs::read(&file)) {
                workspace.add_proto(n, &bytes);
            }
        }
    }
    workspace.plugins = state.plugins.read().enabled().map(|p| p.manifest.id.clone()).collect();
    let ssl = state.prefs.read().ssl.clone();
    workspace.ssl = Some(SslSettings { intercept_all: ssl.intercept_all, include: ssl.include, exclude: ssl.exclude });
    let text = proxyzip_export::workspace::write(&workspace, redact).map_err(|e| format!("{e:#}"))?;
    std::fs::write(&path, text).map_err(err)?;
    let r = &workspace.rules;
    Ok(WorkspaceSummary {
        rules: r.block.len()
            + r.map_local.len()
            + r.map_remote.len()
            + r.rewrite.len()
            + r.breakpoints.len()
            + r.scripts.len()
            + r.dns.len()
            + r.reverse.len(),
        protos: workspace.protos.len(),
        plugins: workspace.plugins.len(),
    })
}

/// Load a `.pzworkspace` file, replacing the current setup or adding to it.
#[tauri::command]
pub async fn import_workspace(state: State<'_, AppState>, path: PathBuf, merge: bool) -> CmdResult<WorkspaceSummary> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let workspace = proxyzip_export::workspace::read(&text).map_err(|e| format!("{e:#}"))?;
    let protos = workspace.proto_bytes().map_err(|e| format!("{e:#}"))?;
    let dir = crate::state::protos_dir(&state.home);
    std::fs::create_dir_all(&dir).map_err(err)?;
    for (name, bytes) in &protos {
        // Only the file name is used, so a crafted workspace cannot write outside the folder.
        let Some(safe) = std::path::Path::new(name).file_name() else { continue };
        std::fs::write(dir.join(safe), bytes).map_err(err)?;
    }
    state.reload_protos();
    if let Some(ssl) = workspace.ssl.clone() {
        let prefs = {
            let mut p = state.prefs.write();
            if merge {
                for h in ssl.include {
                    if !p.ssl.include.contains(&h) {
                        p.ssl.include.push(h);
                    }
                }
                for h in ssl.exclude {
                    if !p.ssl.exclude.contains(&h) {
                        p.ssl.exclude.push(h);
                    }
                }
            } else {
                p.ssl.intercept_all = ssl.intercept_all;
                p.ssl.include = ssl.include;
                p.ssl.exclude = ssl.exclude;
            }
            p.clone()
        };
        prefs.save(&state.home).map_err(err)?;
    }
    for id in &workspace.plugins {
        let _ = state.plugins.write().set_enabled(id, true);
    }
    let summary = WorkspaceSummary { rules: 0, protos: protos.len(), plugins: workspace.plugins.len() };
    let mut rules = workspace.rules;
    let count = rules.block.len()
        + rules.map_local.len()
        + rules.map_remote.len()
        + rules.rewrite.len()
        + rules.breakpoints.len()
        + rules.scripts.len()
        + rules.dns.len()
        + rules.reverse.len();
    if merge {
        let mut base = state.rules.read().clone();
        proxyzip_export::workspace::merge_rules(&mut base, rules);
        rules = base;
    }
    state.apply_rules(rules).await.map_err(|e| format!("{e:#}"))?;
    state.apply_live_settings().await;
    Ok(WorkspaceSummary { rules: count, ..summary })
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
