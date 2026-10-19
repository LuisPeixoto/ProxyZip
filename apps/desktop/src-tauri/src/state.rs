use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;
use proxyzip_core::FlowStore;
use proxyzip_proxy::{ProxyConfig, ProxyHandle, ProxyServer, ProxySettings, SslProxyingPolicy};
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

    pub fn proxy_settings(&self, recording: bool) -> ProxySettings {
        ProxySettings {
            ssl: self.ssl.clone(),
            recording,
            max_body_capture: self.max_body_capture_mb.max(1) * 1024 * 1024,
            insecure_upstream: self.insecure_upstream,
        }
    }
}

pub struct AppState {
    pub home: PathBuf,
    pub store: FlowStore,
    pub ca: RwLock<Arc<CertificateAuthority>>,
    pub prefs: RwLock<Preferences>,
    pub proxy: Mutex<Option<ProxyHandle>>,
    pub listen: RwLock<Option<SocketAddr>>,
    pub last_error: RwLock<Option<String>>,
    /// Set when *we* turned the system proxy on, so we restore it on exit.
    pub system_proxy_owned: RwLock<bool>,
}

impl AppState {
    pub async fn init() -> Result<Self> {
        let home = proxyzip_core::data_dir();
        let ca = Arc::new(CertificateAuthority::load_or_create(&home)?);
        let prefs = Preferences::load(&home);
        let state = Self {
            store: FlowStore::new(prefs.max_flows),
            home,
            ca: RwLock::new(ca),
            prefs: RwLock::new(prefs),
            proxy: Mutex::new(None),
            listen: RwLock::new(None),
            last_error: RwLock::new(None),
            system_proxy_owned: RwLock::new(false),
        };
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
            match ProxyServer::start(config, prefs.proxy_settings(recording), ca, self.store.clone()).await {
                Ok(handle) => {
                    let addr = handle.local_addr;
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

    /// Push changed preferences into the running proxy without restarting it.
    pub async fn apply_live_settings(&self) {
        let prefs = self.prefs.read().clone();
        if let Some(h) = self.proxy.lock().await.as_ref() {
            let mut s = h.settings.write();
            let recording = s.recording;
            *s = prefs.proxy_settings(recording);
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
