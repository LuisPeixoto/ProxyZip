use std::net::SocketAddr;
use std::path::PathBuf;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySettings {
    pub ssl: SslProxyingPolicy,
    /// When false traffic is still proxied but not stored.
    pub recording: bool,
    /// Bodies larger than this are forwarded intact but captured truncated.
    pub max_body_capture: usize,
    /// Skip upstream certificate verification (debugging self-signed servers).
    pub insecure_upstream: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            ssl: SslProxyingPolicy::default(),
            recording: true,
            max_body_capture: 32 * 1024 * 1024,
            insecure_upstream: false,
        }
    }
}

/// Case-insensitive glob match supporting `*` (any run) and `?` (one char).
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard() {
        assert!(wildcard_match("*.example.com", "api.example.com"));
        assert!(!wildcard_match("*.example.com", "example.com"));
        assert!(wildcard_match("api.*", "API.github.com"));
        assert!(wildcard_match("h?st", "host"));
        assert!(wildcard_match("*", "anything"));
    }

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
