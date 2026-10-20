//! Commands for the API tools: mocks and OpenAPI, replay suites and plugin tabs, settings and the catalog.

use std::path::PathBuf;

use proxyzip_core::{Flow, FlowId};
use proxyzip_openapi::Spec;
use proxyzip_rules::{MockBody, MockRoute, MockServer};
use proxyzip_script::catalog::{CATALOG_URL, Catalog, CatalogEntry};
use proxyzip_script::{PluginEntry, ScriptRequest, ScriptResponse, Verdict};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tauri::State;

use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---- Mocks and OpenAPI ------------------------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ImportedSpec {
    title: String,
    routes: usize,
}

/// Add a mock server built from an OpenAPI document (`.yaml`, `.yml` or `.json`).
#[tauri::command]
pub async fn import_openapi(
    state: State<'_, AppState>,
    path: PathBuf,
    host: Option<String>,
) -> CmdResult<ImportedSpec> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let spec = Spec::parse(&text).map_err(|e| format!("{e:#}"))?;
    let host = host
        .filter(|h| !h.trim().is_empty())
        .or_else(|| spec.servers().first().map(|(h, _)| h.clone()))
        .unwrap_or_default();
    let mut server = spec.to_mock_server(&spec.title(), &host, None);
    // Requests to the real server keep their base path (`/v1/pets`), so the routes get it too.
    if let Some((_, base)) = spec.servers().first().filter(|(h, _)| *h == host) {
        for route in server.routes.iter_mut() {
            route.path = format!("{base}{}", route.path);
        }
    }
    let routes = server.routes.len();
    let title = server.name.clone();
    let mut rules = state.rules.read().clone();
    rules.mocks.retain(|m| m.name != server.name);
    rules.mocks.push(server);
    rules.tools.mocks = true;
    state.apply_rules(rules).await.map_err(|e| format!("{e:#}"))?;
    Ok(ImportedSpec { title, routes })
}

#[derive(Serialize)]
pub struct ContractReport {
    title: String,
    #[serde(flatten)]
    report: proxyzip_openapi::Report,
}

/// Check captured flows (all of them, or the given ones) against an OpenAPI document and tag the ones that do not conform.
#[tauri::command]
pub fn validate_openapi(
    state: State<'_, AppState>,
    path: PathBuf,
    ids: Option<Vec<FlowId>>,
) -> CmdResult<ContractReport> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let spec = Spec::parse(&text).map_err(|e| format!("{e:#}"))?;
    let flows: Vec<Flow> = match ids {
        Some(ids) if !ids.is_empty() => state.store.get_many(&ids),
        _ => state.store.all(),
    };
    let report = proxyzip_openapi::validate_flows(&spec, &flows);
    for id in report.flows_with_violations() {
        state.store.update(id, |f| f.add_tag("contract-violation"));
    }
    Ok(ContractReport { title: spec.title(), report })
}

/// Turn a captured response into a mock route (on a server called "Captured" for that host).
#[tauri::command]
pub async fn add_mock_from_flow(state: State<'_, AppState>, id: FlowId) -> CmdResult<usize> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let response = flow.response.as_ref().ok_or("this request has no response yet")?;
    let (decoded, _, _) = proxyzip_codec::decode_content(&response.headers, &response.body);
    let text = String::from_utf8(decoded.to_vec())
        .map_err(|_| "binary bodies cannot be turned into a mock yet".to_string())?;
    let path = flow.request.path.split('?').next().unwrap_or("/").to_string();
    let keep = ["content-type", "cache-control", "etag", "location", "content-language"];
    let headers =
        response.headers.iter().filter(|(k, _)| keep.contains(&k.to_ascii_lowercase().as_str())).cloned().collect();
    let route = MockRoute {
        name: format!("{} {path}", flow.request.method),
        method: flow.request.method.clone(),
        path,
        status: response.status,
        headers,
        body: MockBody::Inline { text },
        ..Default::default()
    };
    let mut rules = state.rules.read().clone();
    let name = "Captured".to_string();
    let host = flow.request.host.clone();
    let server = match rules.mocks.iter_mut().position(|m| m.name == name && m.host == host) {
        Some(i) => &mut rules.mocks[i],
        None => {
            rules.mocks.push(MockServer { name: name.clone(), host, passthrough: true, ..Default::default() });
            rules.mocks.last_mut().expect("just pushed")
        }
    };
    server.routes.retain(|r| !(r.method == route.method && r.path == route.path));
    server.routes.push(route);
    let count = server.routes.len();
    rules.tools.mocks = true;
    state.apply_rules(rules).await.map_err(|e| format!("{e:#}"))?;
    Ok(count)
}

// ---- Replay suites ----------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn suite_from_flows(
    state: State<'_, AppState>,
    ids: Vec<FlowId>,
    name: Option<String>,
) -> CmdResult<proxyzip_suite::Suite> {
    let flows = state.store.get_many(&ids);
    if flows.is_empty() {
        return Err("select at least one request".into());
    }
    let mut suite = proxyzip_suite::Suite::new(name.unwrap_or_else(|| "New suite".into()));
    suite.requests =
        flows.iter().filter(|f| f.request.method != "CONNECT").map(proxyzip_suite::SuiteRequest::from_flow).collect();
    Ok(suite)
}

#[tauri::command]
pub fn open_suite(path: PathBuf) -> CmdResult<proxyzip_suite::Suite> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let is_postman = text.contains("\"info\"") && text.contains("\"item\"") && !text.contains("proxyzip-suite");
    if is_postman {
        proxyzip_suite::import_postman(&text).map_err(|e| format!("{e:#}"))
    } else {
        proxyzip_suite::Suite::from_json(&text).map_err(|e| format!("{e:#}"))
    }
}

#[tauri::command]
pub fn save_suite(path: PathBuf, suite: proxyzip_suite::Suite) -> CmdResult<()> {
    std::fs::write(&path, suite.to_json().map_err(err)?).map_err(err)
}

#[tauri::command]
pub fn export_suite_junit(path: PathBuf, report: Value) -> CmdResult<()> {
    // The report is the JSON the app got from `run_suite`; rebuild just enough of it for the XML.
    let report: SuiteReportIn = serde_json::from_value(report).map_err(err)?;
    std::fs::write(&path, report.to_junit()).map_err(err)
}

#[derive(serde::Deserialize)]
struct SuiteReportIn {
    suite: String,
    duration_ms: u64,
    failed: usize,
    skipped: usize,
    results: Vec<SuiteResultIn>,
}

#[derive(serde::Deserialize)]
struct SuiteResultIn {
    name: String,
    duration_ms: u64,
    passed: bool,
    skipped: bool,
    error: Option<String>,
    assertions: Vec<SuiteAssertionIn>,
}

#[derive(serde::Deserialize)]
struct SuiteAssertionIn {
    description: String,
    passed: bool,
    message: Option<String>,
}

impl SuiteReportIn {
    fn to_junit(&self) -> String {
        let to_report = proxyzip_suite::Report {
            suite: self.suite.clone(),
            environment: None,
            duration_ms: self.duration_ms,
            passed: self.results.iter().filter(|r| r.passed).count(),
            failed: self.failed,
            skipped: self.skipped,
            results: self
                .results
                .iter()
                .map(|r| proxyzip_suite::RequestResult {
                    name: r.name.clone(),
                    method: String::new(),
                    url: String::new(),
                    status: None,
                    duration_ms: r.duration_ms,
                    passed: r.passed,
                    error: r.error.clone(),
                    assertions: r
                        .assertions
                        .iter()
                        .map(|a| proxyzip_suite::AssertionResult {
                            description: a.description.clone(),
                            passed: a.passed,
                            message: a.message.clone(),
                        })
                        .collect(),
                    extracted: Default::default(),
                    skipped: r.skipped,
                })
                .collect(),
        };
        proxyzip_suite::to_junit(&to_report)
    }
}

/// Run a suite. With `via_proxy`, the requests go through ProxyZip itself and show up in the list tagged `suite`.
#[tauri::command]
pub async fn run_suite(
    state: State<'_, AppState>,
    suite: proxyzip_suite::Suite,
    environment: Option<String>,
    via_proxy: bool,
    base_dir: Option<PathBuf>,
) -> CmdResult<proxyzip_suite::Report> {
    let mut options =
        proxyzip_suite::RunOptions { environment, base_dir: base_dir.unwrap_or_default(), ..Default::default() };
    if via_proxy {
        let (host, port) = state.client_host_port().ok_or("the proxy is not running")?;
        options.proxy = Some(format!("http://{host}:{port}"));
        options.extra_root_pem = Some(state.ca.read().cert_pem().to_string());
    }
    Ok(proxyzip_suite::run(&suite, &options).await)
}

// ---- Plugin tabs, settings and the catalog ----------------------------------------------------------------------------

/// The flow as a plugin's `render(flow)` sees it.
fn flow_for_plugin(flow: &Flow) -> Value {
    let (decoded, _, _) = proxyzip_codec::decode_content(&flow.request.headers, &flow.request.body);
    let request = ScriptRequest {
        method: flow.request.method.clone(),
        url: flow.request.url(),
        headers: flow.request.headers.0.clone(),
        body: decoded.to_vec(),
    }
    .to_json();
    let response = flow.response.as_ref().map(|r| {
        let (decoded, _, _) = proxyzip_codec::decode_content(&r.headers, &r.body);
        ScriptResponse { status: r.status, headers: r.headers.0.clone(), body: decoded.to_vec() }.to_json()
    });
    json!({
        "id": flow.id,
        "request": request,
        "response": response,
        "timing": flow.timing,
        "tags": flow.tags,
        "client": flow.client_addr,
        "tls": flow.tls,
        "error": flow.error,
    })
}

/// Ask a plugin to draw one of its inspector tabs.
#[tauri::command]
pub async fn plugin_tab_content(
    state: State<'_, AppState>,
    id: FlowId,
    plugin: String,
    tab: String,
) -> CmdResult<Value> {
    let flow = state.store.get(id).ok_or("flow not found")?;
    let source = state
        .plugins
        .read()
        .find_tab(&plugin, &tab)
        .map(|p| p.runtime_source())
        .ok_or("that plugin is not available")?;
    let engine = state.proxy.lock().await.as_ref().map(|h| h.scripts.clone()).ok_or("the proxy is not running")?;
    let out = engine.call(&source, PluginEntry::Render, flow_for_plugin(&flow), json!({ "tab": tab })).await;
    match out.verdict {
        Ok(Verdict::Changed(v)) => Ok(v),
        Ok(_) => Ok(json!({ "type": "text", "content": "This plugin has nothing to show for this request." })),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn set_plugin_settings(state: State<'_, AppState>, id: String, values: Map<String, Value>) -> CmdResult<()> {
    state.plugins.write().set_settings(&id, &values).map_err(|e| format!("{e:#}"))?;
    // Hooks carry the settings inside their source, so the running proxy needs the new version.
    state.refresh_plugin_hooks().await;
    Ok(())
}

#[derive(Serialize)]
pub struct CatalogItem {
    #[serde(flatten)]
    entry: CatalogEntry,
    installed: Option<String>,
    update: bool,
}

#[derive(Serialize)]
pub struct CatalogView {
    plugins: Vec<CatalogItem>,
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("ProxyZip/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(err)?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    response.text().await.map_err(err)
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(err)?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    Ok(response.bytes().await.map_err(err)?.to_vec())
}

#[tauri::command]
pub async fn plugin_catalog(state: State<'_, AppState>) -> CmdResult<CatalogView> {
    let catalog = Catalog::parse(&fetch_text(CATALOG_URL).await?).map_err(|e| format!("{e:#}"))?;
    let reg = state.plugins.read();
    let plugins = catalog
        .plugins
        .into_iter()
        .map(|entry| {
            let installed = reg.plugins.iter().find(|p| p.manifest.id == entry.id).map(|p| p.manifest.version.clone());
            let update = installed.as_ref().is_some_and(|v| *v != entry.version);
            CatalogItem { entry, installed, update }
        })
        .collect();
    Ok(CatalogView { plugins })
}

/// Download a plugin from the catalog, check every file against its SHA-256, and install it.
#[tauri::command]
pub async fn install_catalog_plugin(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let catalog = Catalog::parse(&fetch_text(CATALOG_URL).await?).map_err(|e| format!("{e:#}"))?;
    let entry = catalog.find(&id).ok_or("that plugin is not in the catalog")?.clone();
    let mut downloaded = Vec::new();
    for file in &entry.files {
        downloaded.push((file.path.clone(), fetch_bytes(&entry.url_of(file)).await?));
    }
    let staging = std::env::temp_dir().join(format!("proxyzip-plugin-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(err)?;
    let result = entry.stage(&downloaded, &staging).and_then(|()| state.plugins.write().install(&staging).map(|_| ()));
    let _ = std::fs::remove_dir_all(&staging);
    result.map_err(|e| format!("{e:#}"))?;
    state.refresh_plugin_hooks().await;
    Ok(())
}
