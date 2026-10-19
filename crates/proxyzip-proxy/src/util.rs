use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use proxyzip_core::Headers;

pub type ProxyBody = BoxBody<Bytes, hyper::Error>;

pub fn full(bytes: impl Into<Bytes>) -> ProxyBody {
    Full::new(bytes.into()).map_err(|never| match never {}).boxed()
}

pub fn to_headers(map: &HeaderMap) -> Headers {
    Headers(
        map.iter().map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect(),
    )
}

/// Rebuild a header map from the editable header list. Entries that are not valid HTTP headers are skipped.
pub fn to_header_map(headers: &Headers) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in headers.iter() {
        if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            map.append(n, v);
        }
    }
    map
}

/// Returned from a service to make hyper close the client connection without answering.
#[derive(Debug)]
pub struct ConnectionDropped;

impl std::fmt::Display for ConnectionDropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection dropped by a ProxyZip rule")
    }
}

impl std::error::Error for ConnectionDropped {}

/// `host` or `host:port`, leaving out the scheme's default port.
pub fn authority_for(scheme: &str, host: &str, port: u16) -> String {
    let default = matches!((scheme, port), ("http" | "ws", 80) | ("https" | "wss", 443));
    if default {
        if host.contains(':') { format!("[{host}]") } else { host.to_string() }
    } else {
        format_authority(host, port)
    }
}

pub fn version_str(v: http::Version) -> String {
    match v {
        http::Version::HTTP_09 => "HTTP/0.9",
        http::Version::HTTP_10 => "HTTP/1.0",
        http::Version::HTTP_11 => "HTTP/1.1",
        http::Version::HTTP_2 => "HTTP/2",
        http::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/?",
    }
    .to_string()
}

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Remove hop-by-hop headers (RFC 9110 §7.6.1), including those named by `Connection`.
pub fn strip_hop_by_hop(map: &mut HeaderMap) {
    let listed: Vec<String> = map
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    for name in HOP_BY_HOP.iter().copied().map(str::to_string).chain(listed) {
        map.remove(name.as_str());
    }
}

pub fn is_websocket_upgrade(map: &HeaderMap) -> bool {
    map.get(http::header::UPGRADE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

/// Split `host:port` / `[v6]:port` with a default port.
pub fn split_authority(authority: &str, default_port: u16) -> (String, u16) {
    if let Some(rest) = authority.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        let port = tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default_port);
        return (host.to_string(), port);
    }
    match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse().unwrap_or(default_port)),
        _ => (authority.to_string(), default_port),
    }
}

pub fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority() {
        assert_eq!(split_authority("a.com:8443", 443), ("a.com".into(), 8443));
        assert_eq!(split_authority("a.com", 443), ("a.com".into(), 443));
        assert_eq!(split_authority("[::1]:80", 443), ("::1".into(), 80));
        assert_eq!(format_authority("::1", 80), "[::1]:80");
    }

    #[test]
    fn header_round_trip_and_authority() {
        let headers =
            Headers(vec![("X-A".into(), "1".into()), ("x-a".into(), "2".into()), ("bad name".into(), "x".into())]);
        let map = to_header_map(&headers);
        assert_eq!(map.get_all("x-a").iter().count(), 2);
        assert_eq!(map.len(), 2);
        assert_eq!(authority_for("https", "a.com", 443), "a.com");
        assert_eq!(authority_for("http", "a.com", 8080), "a.com:8080");
        assert_eq!(authority_for("http", "::1", 80), "[::1]");
    }

    #[test]
    fn hop_by_hop() {
        let mut m = HeaderMap::new();
        m.insert("connection", "close, x-foo".parse().unwrap());
        m.insert("x-foo", "1".parse().unwrap());
        m.insert("keep-alive", "1".parse().unwrap());
        m.insert("accept", "*/*".parse().unwrap());
        strip_hop_by_hop(&mut m);
        assert_eq!(m.len(), 1);
    }
}
