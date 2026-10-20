use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use proxyzip_rules::{RuleSet, wildcard_match};
use rustls::pki_types::CertificateDer;
use serde::{Deserialize, Serialize};

/// Static configuration fixed when the proxy starts.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub listen: SocketAddr,
    /// Extra roots trusted when connecting upstream (e.g. a corporate or test CA).
    pub extra_upstream_roots: Vec<CertificateDer<'static>>,
    /// Write TLS secrets here (NSS key log format) for Wireshark.
    pub keylog_path: Option<PathBuf>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self { listen: SocketAddr::from(([127, 0, 0, 1], 9393)), extra_upstream_roots: Vec::new(), keylog_path: None }
    }
}

/// Which CONNECT targets get decrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SslProxyingPolicy {
    /// Decrypt every host unless excluded. When false only `include` hosts are decrypted.
    pub intercept_all: bool,
    /// Wildcard host patterns (`*.example.com`, `api.*`).
    pub include: Vec<String>,
    /// Hosts that are always tunnelled untouched (pinned apps, banking, ...).
    pub exclude: Vec<String>,
}

impl Default for SslProxyingPolicy {
    fn default() -> Self {
        Self { intercept_all: true, include: Vec::new(), exclude: Vec::new() }
    }
}

impl SslProxyingPolicy {
    pub fn should_intercept(&self, host: &str) -> bool {
        if self.exclude.iter().any(|p| wildcard_match(p, host)) {
            return false;
        }
        self.intercept_all || self.include.iter().any(|p| wildcard_match(p, host))
    }
}

/// Settings that can be changed while the proxy is running.
#[derive(Debug, Clone)]
pub struct ProxySettings {
    pub ssl: SslProxyingPolicy,
    /// When false traffic is still proxied but not stored.
    pub recording: bool,
    /// Bodies larger than this are forwarded intact but captured truncated.
    pub max_body_capture: usize,
    /// Skip upstream certificate verification (debugging self-signed servers).
    pub insecure_upstream: bool,
    /// Remove HTTP/3 advertisements (`Alt-Svc: h3`) from responses, so clients keep using the proxy instead of QUIC.
    pub strip_alt_svc: bool,
    /// Talk HTTP/3 to servers that advertise it (experimental).
    pub http3_upstream: bool,
    /// Block, map, rewrite, breakpoint, throttle and network rules. Cheap to clone.
    pub rules: Arc<RuleSet>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            ssl: SslProxyingPolicy::default(),
            recording: true,
            max_body_capture: 32 * 1024 * 1024,
            insecure_upstream: false,
            strip_alt_svc: true,
            http3_upstream: false,
            rules: Arc::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        let mut p = SslProxyingPolicy::default();
        p.exclude.push("*.apple.com".into());
        assert!(p.should_intercept("example.com"));
        assert!(!p.should_intercept("gs.apple.com"));
        p.intercept_all = false;
        p.include.push("api.example.com".into());
        assert!(p.should_intercept("api.example.com"));
        assert!(!p.should_intercept("example.com"));
    }
}
