use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use proxyzip_codec::BodyView;
use proxyzip_core::{FlowId, FlowSummary, Headers, Timing, TlsInfo, WsDirection, WsKind};
use proxyzip_tls::CertificateAuthority;
use serde::Serialize;
use tauri::State;

use crate::state::{AppState, Preferences};

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
    let request = RequestView {
        headers: req.headers.clone(),
        query,
        cookies,
        body: proxyzip_codec::view(&req.headers, &req.body, req.body_truncated),
    };
    let response = flow.response.as_ref().map(|r| ResponseView {
        status: r.status,
        reason: r.reason.clone(),
        http_version: r.http_version.clone(),
        headers: r.headers.clone(),
        set_cookies: r.headers.get_all("set-cookie").map(str::to_string).collect(),
        body: proxyzip_codec::view(&r.headers, &r.body, r.body_truncated),
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

/// Re-send a captured request through ProxyZip itself, so the replay is captured too.
#[tauri::command]
pub async fn repeat_flow(state: State<'_, AppState>, id: FlowId) -> CmdResult<()> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let (h, p) = state.client_host_port().ok_or("proxy is not running")?;
    let ca_pem = state.ca.read().cert_pem().to_string();
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{h}:{p}")).map_err(err)?)
        .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).map_err(err)?)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(err)?;
    let req = &flow.request;
    let method = reqwest::Method::from_bytes(req.method.as_bytes()).map_err(err)?;
    let mut builder = client.request(method, req.url().replace("ws://", "http://").replace("wss://", "https://"));
    for (k, v) in req.headers.iter() {
        let lk = k.to_ascii_lowercase();
        if matches!(lk.as_str(), "host" | "content-length" | "connection" | "proxy-connection" | "transfer-encoding") {
            continue;
        }
        builder = builder.header(k, v);
    }
    builder.body(req.body.clone()).send().await.map_err(err)?.bytes().await.map_err(err)?;
    Ok(())
}
