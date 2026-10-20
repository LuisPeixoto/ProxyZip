use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;
use proxyzip_codec::ProtoRegistry;
use proxyzip_core::FlowStore;
use proxyzip_proxy::{ProxyConfig, ProxyHandle, ProxyServer, ProxySettings, SslProxyingPolicy};
use proxyzip_rules::RuleSet;
use proxyzip_tls::CertificateAuthority;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Persisted user preferences (`settings.json` in the data directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub port: u16,
    /// Listen on all interfaces so phones/emulators on the LAN can connect.
    pub allow_lan: bool,
    pub ssl: SslProxyingPolicy,
    pub max_body_capture_mb: usize,
    pub insecure_upstream: bool,
    pub max_flows: usize,
    /// Save the session when the app quits and load it again on the next launch.
    pub restore_last_session: bool,
    /// Serve the captured traffic to AI assistants over MCP on `127.0.0.1:mcp_port`.
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    /// Let assistants change rules and clear traffic.
    pub mcp_allow_changes: bool,
    /// Look for a newer version on GitHub when the app starts.
    pub check_updates: bool,
    /// Remove HTTP/3 advertisements from responses so clients keep going through the proxy.
    pub strip_alt_svc: bool,
    /// Use HTTP/3 towards servers that advertise it (experimental).
    pub http3_upstream: bool,
    /// Replace secrets (tokens, cookies, passwords) when exporting HAR, CSV or Postman.
    pub redact_exports: bool,
}

/// A session file the user saved or opened recently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentSession {
    pub path: String,
    pub name: String,
    /// Milliseconds since the Unix epoch.
    pub saved_at: i64,
    pub flows: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            port: 9393,
            allow_lan: false,
            ssl: SslProxyingPolicy::default(),
            max_body_capture_mb: 32,
            insecure_upstream: false,
            max_flows: 50_000,
            restore_last_session: false,
            mcp_enabled: false,
            mcp_port: 9395,
            mcp_allow_changes: false,
            check_updates: true,
            strip_alt_svc: true,
            http3_upstream: false,
            redact_exports: true,
        }
    }
}

impl Preferences {
    fn path(home: &std::path::Path) -> PathBuf {
        home.join("settings.json")
    }

    pub fn load(home: &std::path::Path) -> Self {
        std::fs::read_to_string(Self::path(home)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, home: &std::path::Path) -> Result<()> {
        std::fs::create_dir_all(home)?;
        std::fs::write(Self::path(home), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn proxy_settings(&self, recording: bool, rules: Arc<RuleSet>) -> ProxySettings {
        ProxySettings {
            ssl: self.ssl.clone(),
            recording,
            max_body_capture: self.max_body_capture_mb.max(1) * 1024 * 1024,
            insecure_upstream: self.insecure_upstream,
            strip_alt_svc: self.strip_alt_svc,
            http3_upstream: self.http3_upstream,
            rules,
        }
    }
}

const MAX_RECENT: usize = 10;

pub fn protos_dir(home: &std::path::Path) -> PathBuf {
    home.join("protos")
}

/// Read every descriptor set in the protos folder. Files that cannot be read are skipped with a warning.
fn load_protos(home: &std::path::Path) -> ProtoRegistry {
    let mut registry = ProtoRegistry::new();
    let Ok(entries) = std::fs::read_dir(protos_dir(home)) else { return registry };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    paths.sort();
    for path in paths {
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| registry.add_descriptor_set(&bytes)) {
            Ok(()) => {}
            Err(e) => tracing::warn!(file = %path.display(), error = %e, "skipping an invalid descriptor set"),
        }
    }
    registry
}

fn recent_path(home: &std::path::Path) -> PathBuf {
    home.join("recent.json")
}

fn last_session_path(home: &std::path::Path) -> PathBuf {
    home.join("last.pzsession")
}

pub fn plugins_dir(home: &std::path::Path) -> PathBuf {
    home.join("plugins")
}

fn rules_path(home: &std::path::Path) -> PathBuf {
    home.join("rules.json")
}

fn load_rules(home: &std::path::Path) -> RuleSet {
    match std::fs::read_to_string(rules_path(home)) {
        Ok(json) => RuleSet::from_json(&json).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "rules.json is invalid; starting without rules");
            RuleSet::default()
        }),
        Err(_) => RuleSet::default(),
    }
}

pub struct AppState {
    pub home: PathBuf,
    pub store: FlowStore,
    pub ca: RwLock<Arc<CertificateAuthority>>,
    pub prefs: RwLock<Preferences>,
    pub rules: RwLock<RuleSet>,
    /// Protobuf schemas imported by the user, for decoding bodies and gRPC.
    pub protos: RwLock<ProtoRegistry>,
    /// Reverse proxy and SOCKS5 listeners that failed to start (for example, a port in use).
    pub listener_errors: RwLock<Vec<String>>,
    pub proxy: Mutex<Option<ProxyHandle>>,
    pub listen: RwLock<Option<SocketAddr>>,
    pub last_error: RwLock<Option<String>>,
    /// Set when *we* turned the system proxy on, so we restore it on exit.
    pub system_proxy_owned: RwLock<bool>,
    /// The AI-assistants server, when it is on.
    pub mcp: Mutex<Option<proxyzip_mcp::http::HttpHandle>>,
    pub mcp_state: RwLock<Option<Arc<proxyzip_mcp::McpState>>>,
    pub mcp_error: RwLock<Option<String>>,
    /// Installed plugins (`<data>/plugins`).
    pub plugins: RwLock<proxyzip_script::plugin::Registry>,
}

impl AppState {
    pub async fn init() -> Result<Self> {
        let home = proxyzip_core::data_dir();
        let ca = Arc::new(CertificateAuthority::load_or_create(&home)?);
        let prefs = Preferences::load(&home);
        let home_for_rules = home.clone();
        let state = Self {
            store: FlowStore::new(prefs.max_flows),
            home,
            ca: RwLock::new(ca),
            prefs: RwLock::new(prefs),
            rules: RwLock::new(load_rules(&home_for_rules)),
            protos: RwLock::new(load_protos(&home_for_rules)),
            listener_errors: RwLock::new(Vec::new()),
            proxy: Mutex::new(None),
            listen: RwLock::new(None),
            last_error: RwLock::new(None),
            system_proxy_owned: RwLock::new(false),
            mcp: Mutex::new(None),
            mcp_state: RwLock::new(None),
            mcp_error: RwLock::new(None),
            plugins: RwLock::new(proxyzip_script::plugin::Registry::load(&plugins_dir(&home_for_rules))),
        };
        state.restore_last_session();
        if let Err(e) = state.start_proxy(true).await {
            tracing::error!(error = %e, "could not start proxy");
            *state.last_error.write() = Some(format!("{e:#}"));
        }
        Ok(state)
    }

    /// (Re)start the proxy with current preferences. With `probe`, walk up to
    /// ten ports past the configured one when it is taken.
    pub async fn start_proxy(&self, probe: bool) -> Result<SocketAddr> {
        let mut guard = self.proxy.lock().await;
        let recording = guard.as_ref().map(|h| h.settings.read().recording).unwrap_or(true);
        if let Some(old) = guard.take() {
            old.shutdown().await;
        }
        let prefs = self.prefs.read().clone();
        let ip = if prefs.allow_lan { [0, 0, 0, 0] } else { [127, 0, 0, 1] };
        let attempts = if probe { 10 } else { 1 };
        let mut last = None;
        for offset in 0..attempts {
            let port = prefs.port.saturating_add(offset);
            let config = ProxyConfig { listen: SocketAddr::from((ip, port)), ..Default::default() };
            let ca = self.ca.read().clone();
            let settings = prefs.proxy_settings(recording, Arc::new(self.effective_rules(self.rules.read().clone())));
            match ProxyServer::start(config, settings, ca, self.store.clone()).await {
                Ok(handle) => {
                    let addr = handle.local_addr;
                    *self.listener_errors.write() = handle.listener_errors.clone();
                    *guard = Some(handle);
                    *self.listen.write() = Some(addr);
                    *self.last_error.write() = None;
                    return Ok(addr);
                }
                Err(e) => last = Some(e),
            }
        }
        *self.listen.write() = None;
        Err(last.unwrap())
    }

    /// The user's rules plus the traffic hooks of enabled plugins. Plugin hooks are never written to `rules.json`.
    pub fn effective_rules(&self, mut rules: RuleSet) -> RuleSet {
        use proxyzip_rules::{Matcher, ScriptRule};
        for hook in self.plugins.read().hook_specs() {
            let mut matcher = Matcher::new(hook.hooks.pattern.clone());
            matcher.regex = hook.hooks.regex;
            matcher.methods = hook.hooks.methods.clone();
            rules.scripts.push(ScriptRule {
                name: format!("plugin: {}", hook.name),
                enabled: true,
                matcher,
                source: hook.source,
                on_request: hook.hooks.request,
                on_response: hook.hooks.response,
            });
        }
        rules
    }

    /// Re-apply the rules after a plugin was installed, removed or switched.
    pub async fn refresh_plugin_hooks(&self) {
        let rules = self.rules.read().clone();
        if let Some(handle) = self.proxy.lock().await.as_ref() {
            let _ = handle.apply_rules(self.effective_rules(rules)).await;
        }
    }

    /// Push changed preferences into the running proxy without restarting it.
    pub async fn apply_live_settings(&self) {
        let prefs = self.prefs.read().clone();
        if let Some(h) = self.proxy.lock().await.as_ref() {
            let mut s = h.settings.write();
            let recording = s.recording;
            *s = prefs.proxy_settings(recording, Arc::new(self.effective_rules(self.rules.read().clone())));
        }
    }

    /// Save new rules and apply them to the running proxy. Returns messages for listeners that could not start.
    pub async fn apply_rules(&self, rules: RuleSet) -> Result<Vec<String>> {
        std::fs::create_dir_all(&self.home)?;
        std::fs::write(rules_path(&self.home), rules.to_json()?)?;
        *self.rules.write() = rules.clone();
        let errors = match self.proxy.lock().await.as_ref() {
            Some(handle) => handle.apply_rules(self.effective_rules(rules)).await,
            None => Vec::new(),
        };
        *self.listener_errors.write() = errors.clone();
        Ok(errors)
    }

    pub fn reload_protos(&self) {
        *self.protos.write() = load_protos(&self.home);
    }

    pub fn recent_sessions(&self) -> Vec<RecentSession> {
        std::fs::read_to_string(recent_path(&self.home))
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<RecentSession>>(&s).ok())
            .map(|list| list.into_iter().filter(|r| std::path::Path::new(&r.path).exists()).collect())
            .unwrap_or_default()
    }

    /// Put `path` at the top of the recent sessions list.
    pub fn remember_session(&self, path: &std::path::Path, flows: usize) {
        let mut list = self.recent_sessions();
        let path_text = path.display().to_string();
        list.retain(|r| r.path != path_text);
        list.insert(
            0,
            RecentSession {
                name: path.file_stem().and_then(|s| s.to_str()).unwrap_or("Session").to_string(),
                path: path_text,
                saved_at: proxyzip_core::now_ms(),
                flows,
            },
        );
        list.truncate(MAX_RECENT);
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::create_dir_all(&self.home);
            let _ = std::fs::write(recent_path(&self.home), json);
        }
    }

    pub fn forget_session(&self, path: &str) {
        let mut list = self.recent_sessions();
        list.retain(|r| r.path != path);
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(recent_path(&self.home), json);
        }
    }

    /// On quit: keep the session for the next launch if the user asked for that.
    pub fn save_last_session(&self) {
        if !self.prefs.read().restore_last_session {
            return;
        }
        let path = last_session_path(&self.home);
        let flows = self.store.all();
        if flows.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        match proxyzip_export::session::write(&flows).map_err(|e| e.to_string()).and_then(|b| {
            let _ = std::fs::create_dir_all(&self.home);
            std::fs::write(&path, b).map_err(|e| e.to_string())
        }) {
            Ok(()) => {}
            Err(e) => tracing::warn!(error = %e, "could not save the session for the next launch"),
        }
    }

    fn restore_last_session(&self) {
        if !self.prefs.read().restore_last_session {
            return;
        }
        let path = last_session_path(&self.home);
        let Ok(bytes) = std::fs::read(&path) else { return };
        match proxyzip_export::session::read(&bytes) {
            Ok(flows) => {
                for flow in flows {
                    self.store.import(flow);
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not restore the last session"),
        }
    }

    /// Address clients should use (loopback even when listening on all interfaces).
    pub fn client_host_port(&self) -> Option<(String, u16)> {
        let addr = (*self.listen.read())?;
        let host = if addr.ip().is_unspecified() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
        Some((host, addr.port()))
    }

    pub fn ca_path(&self) -> PathBuf {
        self.ca.read().cert_path().unwrap_or_else(|| self.home.join(proxyzip_tls::CA_CERT_FILE))
    }

    pub fn restore_system_proxy(&self) {
        if *self.system_proxy_owned.read() {
            if let Err(e) = proxyzip_sysproxy::disable() {
                tracing::warn!(error = %e, "could not restore system proxy");
            }
            *self.system_proxy_owned.write() = false;
        }
    }
}
