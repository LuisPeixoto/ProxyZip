use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

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

/// Selects requests by method and URL.
///
/// Wildcard patterns (the default) understand `*` and `?`:
///
/// - `https://api.example.com/v1/*` matches the whole URL, scheme included.
/// - `api.example.com/v1/*` ignores the scheme.
/// - `example.com` (no wildcard, no path) matches every request to that host.
///
/// With `regex` set, the pattern is a case-insensitive regular expression tested against the full URL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Matcher {
    /// Empty means every method.
    pub methods: Vec<String>,
    pub pattern: String,
    pub regex: bool,
    #[serde(skip)]
    compiled: OnceLock<Option<Regex>>,
}

impl Matcher {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self { pattern: pattern.into(), ..Self::default() }
    }

    pub fn with_methods(mut self, methods: &[&str]) -> Self {
        self.methods = methods.iter().map(|m| m.to_ascii_uppercase()).collect();
        self
    }

    pub fn regex(pattern: impl Into<String>) -> Self {
        Self { pattern: pattern.into(), regex: true, ..Self::default() }
    }

    /// `path` includes the query string.
    pub fn matches(&self, method: &str, scheme: &str, host: &str, port: u16, path: &str) -> bool {
        if !self.methods.is_empty() && !self.methods.iter().any(|m| m.eq_ignore_ascii_case(method)) {
            return false;
        }
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return true;
        }
        let default_port = matches!((scheme, port), ("http" | "ws", 80) | ("https" | "wss", 443));
        let host_part = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
        let authority = if default_port { host_part } else { format!("{host_part}:{port}") };
        let full = format!("{scheme}://{authority}{path}");
        if self.regex {
            let re = self.compiled.get_or_init(|| {
                regex::RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| tracing::warn!(pattern, error = %e, "invalid regular expression in rule"))
                    .ok()
            });
            return re.as_ref().is_some_and(|re| re.is_match(&full));
        }
        wildcard_url(pattern, scheme, host, &authority, &full)
    }
}

fn wildcard_url(pattern: &str, scheme: &str, host: &str, authority: &str, full: &str) -> bool {
    let has_wildcard = pattern.contains(['*', '?']);
    let (pattern_scheme, rest) = match pattern.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, pattern),
    };
    if !has_wildcard && !rest.contains('/') {
        // A bare host (optionally with a port) matches every path on it.
        if pattern_scheme.is_some_and(|s| !s.eq_ignore_ascii_case(scheme)) {
            return false;
        }
        let candidate = if rest.contains(':') && !rest.starts_with('[') { authority } else { host };
        return rest.eq_ignore_ascii_case(candidate);
    }
    let target = if pattern_scheme.is_some() { full } else { full.split_once("://").map_or(full, |(_, r)| r) };
    wildcard_match(pattern, target)
        || (!has_wildcard && target.split_once('?').is_some_and(|(before, _)| wildcard_match(pattern, before)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str) -> Matcher {
        Matcher::new(p)
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.example.com", "api.example.com"));
        assert!(!wildcard_match("*.example.com", "example.com"));
        assert!(wildcard_match("h?st", "HOST"));
        assert!(wildcard_match("*", "anything"));
    }

    #[test]
    fn bare_host_matches_any_path_and_port() {
        let rule = m("example.com");
        assert!(rule.matches("GET", "https", "example.com", 443, "/a/b?c=1"));
        assert!(rule.matches("GET", "http", "example.com", 8080, "/"));
        assert!(!rule.matches("GET", "https", "api.example.com", 443, "/"));
    }

    #[test]
    fn bare_host_with_port_and_scheme() {
        assert!(m("example.com:8080").matches("GET", "http", "example.com", 8080, "/x"));
        assert!(!m("example.com:8080").matches("GET", "http", "example.com", 80, "/x"));
        assert!(m("https://example.com").matches("GET", "https", "example.com", 443, "/x"));
        assert!(!m("https://example.com").matches("GET", "http", "example.com", 80, "/x"));
    }

    #[test]
    fn path_patterns() {
        let rule = m("api.example.com/v1/*");
        assert!(rule.matches("GET", "https", "api.example.com", 443, "/v1/users?id=2"));
        assert!(rule.matches("GET", "http", "api.example.com", 80, "/v1/"));
        assert!(!rule.matches("GET", "https", "api.example.com", 443, "/v2/users"));
        let with_scheme = m("https://api.example.com/*");
        assert!(with_scheme.matches("POST", "https", "api.example.com", 443, "/x"));
        assert!(!with_scheme.matches("POST", "http", "api.example.com", 80, "/x"));
    }

    #[test]
    fn exact_path_ignores_the_query() {
        let rule = m("example.com/login");
        assert!(rule.matches("GET", "https", "example.com", 443, "/login"));
        assert!(rule.matches("GET", "https", "example.com", 443, "/login?next=/"));
        assert!(!rule.matches("GET", "https", "example.com", 443, "/login/extra"));
    }

    #[test]
    fn methods_are_filtered() {
        let rule = m("example.com").with_methods(&["post", "put"]);
        assert!(rule.matches("POST", "https", "example.com", 443, "/"));
        assert!(!rule.matches("GET", "https", "example.com", 443, "/"));
    }

    #[test]
    fn regex_matches_the_full_url() {
        let rule = Matcher::regex(r"/users/\d+$");
        assert!(rule.matches("GET", "https", "example.com", 443, "/users/42"));
        assert!(!rule.matches("GET", "https", "example.com", 443, "/users/abc"));
        // An invalid expression never matches instead of panicking.
        assert!(!Matcher::regex("(").matches("GET", "https", "example.com", 443, "/"));
    }

    #[test]
    fn empty_pattern_matches_everything() {
        assert!(m("").matches("GET", "https", "x.dev", 443, "/"));
    }

    #[test]
    fn deserializes_without_optional_fields() {
        let rule: Matcher = serde_json::from_str(r#"{"pattern":"a.dev"}"#).unwrap();
        assert!(rule.matches("GET", "https", "a.dev", 443, "/"));
    }
}
