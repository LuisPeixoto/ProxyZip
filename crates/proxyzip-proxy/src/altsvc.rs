//! `Alt-Svc` headers: servers use them to say "this site is also reachable over HTTP/3 on this UDP port".
//!
//! Two things matter to a debugging proxy. A client that learns about HTTP/3 may try QUIC directly next time and
//! bypass the proxy, so [`strip_h3`] removes those entries from responses. And ProxyZip itself can use the
//! advertisement to talk HTTP/3 to the server, which [`AltSvcCache`] remembers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use http::{HeaderMap, HeaderValue};
use parking_lot::Mutex;

/// One `h3` alternative: where to connect (host defaults to the origin) and how long it is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H3Alt {
    pub host: Option<String>,
    pub port: u16,
    pub max_age: Duration,
}

fn is_h3(entry: &str) -> bool {
    let proto = entry.trim().split('=').next().unwrap_or("");
    proto == "h3"
}

/// Parse the `h3` entries of one header value, e.g. `h3=":443"; ma=86400, h3-29=":443"`.
pub fn parse_h3(value: &str) -> Vec<H3Alt> {
    value
        .split(',')
        .filter(|e| is_h3(e))
        .filter_map(|entry| {
            let mut parts = entry.split(';');
            let (_, authority) = parts.next()?.split_once('=')?;
            let authority = authority.trim().trim_matches('"');
            let (host, port) = authority.rsplit_once(':')?;
            let port: u16 = port.parse().ok()?;
            let max_age = parts
                .filter_map(|p| p.trim().strip_prefix("ma="))
                .find_map(|s| s.trim().trim_matches('"').parse::<u64>().ok())
                .unwrap_or(86_400);
            Some(H3Alt {
                host: (!host.is_empty()).then(|| host.to_string()),
                port,
                max_age: Duration::from_secs(max_age),
            })
        })
        .collect()
}

/// Remove HTTP/3 alternatives from `alt-svc` headers. Returns whether anything was removed.
pub fn strip_h3(headers: &mut HeaderMap) -> bool {
    let values: Vec<String> =
        headers.get_all(http::header::ALT_SVC).iter().filter_map(|v| v.to_str().ok()).map(str::to_string).collect();
    if values.is_empty() || !values.iter().any(|v| v.split(',').any(is_h3)) {
        return false;
    }
    headers.remove(http::header::ALT_SVC);
    for value in values {
        let kept: Vec<&str> = value.split(',').map(str::trim).filter(|e| !e.is_empty() && !is_h3(e)).collect();
        if kept.is_empty() {
            continue;
        }
        if let Ok(v) = HeaderValue::from_str(&kept.join(", ")) {
            headers.append(http::header::ALT_SVC, v);
        }
    }
    true
}

type Origin = (String, u16);

/// What the proxy has learned about HTTP/3 support, per origin.
#[derive(Default)]
pub struct AltSvcCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Origin → (UDP target, expiry).
    known: HashMap<Origin, (Origin, Instant)>,
    /// Origins where HTTP/3 failed recently; skipped until the instant passes.
    broken: HashMap<Origin, Instant>,
}

/// After a failed attempt, use TCP for this long before trying HTTP/3 again.
const BROKEN_FOR: Duration = Duration::from_secs(5 * 60);

impl AltSvcCache {
    /// Remember the `h3` alternative (if any) a response advertised for `origin`.
    pub fn learn(&self, origin_host: &str, origin_port: u16, headers: &HeaderMap) {
        let alts: Vec<H3Alt> =
            headers.get_all(http::header::ALT_SVC).iter().filter_map(|v| v.to_str().ok()).flat_map(parse_h3).collect();
        let key = (origin_host.to_ascii_lowercase(), origin_port);
        let mut inner = self.inner.lock();
        // `Alt-Svc: clear` withdraws everything.
        if headers
            .get_all(http::header::ALT_SVC)
            .iter()
            .any(|v| v.to_str().is_ok_and(|s| s.trim().eq_ignore_ascii_case("clear")))
        {
            inner.known.remove(&key);
            return;
        }
        if let Some(alt) = alts.into_iter().find(|a| !a.max_age.is_zero()) {
            let target = (alt.host.unwrap_or_else(|| origin_host.to_string()), alt.port);
            inner.known.insert(key, (target, Instant::now() + alt.max_age));
        }
    }

    /// The HTTP/3 endpoint to try for `origin`, if one was advertised and has not expired or failed.
    pub fn lookup(&self, origin_host: &str, origin_port: u16) -> Option<(String, u16)> {
        let key = (origin_host.to_ascii_lowercase(), origin_port);
        let mut inner = self.inner.lock();
        if inner.broken.get(&key).is_some_and(|until| *until > Instant::now()) {
            return None;
        }
        match inner.known.get(&key) {
            Some((target, expiry)) if *expiry > Instant::now() => Some(target.clone()),
            Some(_) => {
                inner.known.remove(&key);
                None
            }
            None => None,
        }
    }

    pub fn mark_broken(&self, origin_host: &str, origin_port: u16) {
        self.inner.lock().broken.insert((origin_host.to_ascii_lowercase(), origin_port), Instant::now() + BROKEN_FOR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append(http::header::ALT_SVC, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn parses_h3_entries() {
        let alts = parse_h3(r#"h3=":443"; ma=3600, h3-29=":443", h2=":443", h3="alt.example.com:8443""#);
        assert_eq!(alts.len(), 2);
        assert_eq!(alts[0], H3Alt { host: None, port: 443, max_age: Duration::from_secs(3600) });
        assert_eq!(alts[1].host.as_deref(), Some("alt.example.com"));
        assert_eq!(alts[1].port, 8443);
        assert_eq!(alts[1].max_age, Duration::from_secs(86_400), "ma defaults to 24 hours");
        assert!(parse_h3("clear").is_empty());
        assert!(parse_h3("h3=garbage").is_empty());
    }

    #[test]
    fn strips_only_h3_and_keeps_the_rest() {
        let mut h = headers(&[r#"h3=":443"; ma=86400, h3-29=":443"; ma=86400"#, r#"h2=":8443"; ma=60"#]);
        assert!(strip_h3(&mut h));
        let left: Vec<_> = h.get_all("alt-svc").iter().map(|v| v.to_str().unwrap().to_string()).collect();
        assert_eq!(left, vec![r#"h3-29=":443"; ma=86400"#, r#"h2=":8443"; ma=60"#]);
    }

    #[test]
    fn removes_the_header_when_nothing_is_left() {
        let mut h = headers(&[r#"h3=":443"; ma=86400"#]);
        assert!(strip_h3(&mut h));
        assert!(h.get("alt-svc").is_none());
        let mut none = headers(&[r#"h2=":443""#]);
        assert!(!strip_h3(&mut none));
        assert!(none.get("alt-svc").is_some());
        assert!(!strip_h3(&mut HeaderMap::new()));
    }

    #[test]
    fn cache_learns_expires_and_avoids_broken_origins() {
        let cache = AltSvcCache::default();
        assert_eq!(cache.lookup("Example.com", 443), None);
        cache.learn("example.com", 443, &headers(&[r#"h3=":4443"; ma=60"#]));
        assert_eq!(cache.lookup("EXAMPLE.com", 443), Some(("example.com".into(), 4443)));
        assert_eq!(cache.lookup("example.com", 8443), None, "per origin");

        cache.mark_broken("example.com", 443);
        assert_eq!(cache.lookup("example.com", 443), None);

        let other = AltSvcCache::default();
        other.learn("a.test", 443, &headers(&[r#"h3="b.test:443"; ma=60"#]));
        assert_eq!(other.lookup("a.test", 443), Some(("b.test".into(), 443)));
        other.learn("a.test", 443, &headers(&["clear"]));
        assert_eq!(other.lookup("a.test", 443), None);
        other.learn("a.test", 443, &headers(&[r#"h3=":443"; ma=0"#]));
        assert_eq!(other.lookup("a.test", 443), None, "ma=0 means do not remember");
    }
}
