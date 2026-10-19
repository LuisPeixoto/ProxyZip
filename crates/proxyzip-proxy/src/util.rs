use bytes::Bytes;
use http::HeaderMap;
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
