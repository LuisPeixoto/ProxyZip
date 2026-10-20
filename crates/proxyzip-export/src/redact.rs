//! Remove secrets from flows before they leave your computer.
//!
//! Headers that carry credentials, sensitive query parameters, JSON and form fields with telling names and JSON Web
//! Tokens anywhere in text bodies are replaced with `<redacted>`. The structure stays, so a redacted capture is still
//! useful for debugging.

use std::sync::OnceLock;

use bytes::Bytes;
use proxyzip_core::{Flow, Headers};
use regex::Regex;
use serde_json::Value;

pub const PLACEHOLDER: &str = "<redacted>";

#[derive(Debug, Clone, Copy)]
pub struct RedactOptions {
    pub headers: bool,
    pub query: bool,
    pub bodies: bool,
}

impl Default for RedactOptions {
    fn default() -> Self {
        Self { headers: true, query: true, bodies: true }
    }
}

const HEADER_PARTS: [&str; 8] = ["token", "secret", "key", "session", "auth", "password", "credential", "signature"];
const FIELD_EXACT: [&str; 14] = [
    "key",
    "sig",
    "code",
    "sid",
    "pwd",
    "passwd",
    "auth",
    "otp",
    "pin",
    "jwt",
    "signature",
    "session",
    "credentials",
    "cvv",
];
const FIELD_PARTS: [&str; 6] = ["token", "secret", "password", "apikey", "api_key", "api-key"];

fn jwt() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*").expect("valid regex"))
}

/// Whether a header carries credentials or session data.
pub fn is_sensitive_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "cookie" || n == "set-cookie" || HEADER_PARTS.iter().any(|p| n.contains(p))
}

/// Whether a query parameter or body field is likely to be a secret.
pub fn is_sensitive_field(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    FIELD_EXACT.contains(&n.as_str()) || FIELD_PARTS.iter().any(|p| n.contains(p))
}

fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cookie" => value
            .split(';')
            .map(|pair| match pair.trim().split_once('=') {
                Some((k, _)) => format!("{k}={PLACEHOLDER}"),
                None => pair.trim().to_string(),
            })
            .collect::<Vec<_>>()
            .join("; "),
        "set-cookie" => {
            let mut parts = value.split(';');
            let first = parts.next().unwrap_or("");
            let head = match first.split_once('=') {
                Some((k, _)) => format!("{k}={PLACEHOLDER}"),
                None => first.to_string(),
            };
            std::iter::once(head).chain(parts.map(|p| p.trim().to_string())).collect::<Vec<_>>().join("; ")
        }
        // Keep the scheme (`Bearer`, `Basic`) so the shape of the request is still visible.
        "authorization" | "proxy-authorization" => match value.split_once(' ') {
            Some((scheme, _)) => format!("{scheme} {PLACEHOLDER}"),
            None => PLACEHOLDER.to_string(),
        },
        _ => PLACEHOLDER.to_string(),
    }
}

fn redact_headers(headers: &mut Headers) {
    for (name, value) in headers.0.iter_mut() {
        if is_sensitive_header(name) {
            *value = redact_header_value(name, value);
        }
    }
}

/// Redact the query string of a path such as `/search?q=a&token=b`.
pub fn redact_path(path: &str) -> String {
    let Some((base, query)) = path.split_once('?') else { return path.to_string() };
    let (query, fragment) = match query.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (query, None),
    };
    let redacted = redact_pairs(query);
    match fragment {
        Some(f) => format!("{base}?{redacted}#{f}"),
        None => format!("{base}?{redacted}"),
    }
}

/// Redact `a=1&b=2` pairs (query strings and form bodies).
fn redact_pairs(text: &str) -> String {
    text.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if is_sensitive_field(&percent_decode(k)) => format!("{k}={PLACEHOLDER}"),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("{s}=").as_bytes())
        .next()
        .map(|(k, _)| k.into_owned())
        .unwrap_or_else(|| s.to_string())
}

fn redact_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_sensitive_field(k) && !v.is_object() && !v.is_array() && !v.is_null() {
                    *v = Value::String(PLACEHOLDER.into());
                } else {
                    redact_json(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_json),
        Value::String(s) if jwt().is_match(s) => *s = jwt().replace_all(s, PLACEHOLDER).into_owned(),
        _ => {}
    }
}

/// Redact a body. The result is plain (not compressed), and the length and encoding headers are fixed up.
fn redact_body(headers: &mut Headers, body: &mut Bytes) {
    if body.is_empty() {
        return;
    }
    let (decoded, _, error) = proxyzip_codec::decode_content(headers, body);
    if error.is_some() {
        return;
    }
    let Ok(text) = std::str::from_utf8(&decoded) else { return };
    let content_type = headers.get("content-type").unwrap_or("").to_ascii_lowercase();
    let redacted = if content_type.contains("json") || text.trim_start().starts_with(['{', '[']) {
        match serde_json::from_str::<Value>(text) {
            Ok(mut v) => {
                redact_json(&mut v);
                serde_json::to_string(&v).unwrap_or_else(|_| text.to_string())
            }
            Err(_) => jwt().replace_all(text, PLACEHOLDER).into_owned(),
        }
    } else if content_type.contains("x-www-form-urlencoded") {
        jwt().replace_all(&redact_pairs(text), PLACEHOLDER).into_owned()
    } else if content_type.starts_with("text/") || content_type.contains("xml") || content_type.contains("javascript") {
        jwt().replace_all(text, PLACEHOLDER).into_owned()
    } else {
        return;
    };
    if redacted != text || headers.get("content-encoding").is_some() {
        headers
            .0
            .retain(|(k, _)| !k.eq_ignore_ascii_case("content-encoding") && !k.eq_ignore_ascii_case("content-length"));
        headers.push("content-length", redacted.len().to_string());
        *body = Bytes::from(redacted);
    }
}

/// Redact one flow in place.
pub fn redact_flow(flow: &mut Flow, options: &RedactOptions) {
    if options.query {
        flow.request.path = redact_path(&flow.request.path);
    }
    if options.headers {
        redact_headers(&mut flow.request.headers);
        if let Some(r) = flow.response.as_mut() {
            redact_headers(&mut r.headers);
        }
    }
    if options.bodies {
        redact_body(&mut flow.request.headers, &mut flow.request.body);
        if let Some(r) = flow.response.as_mut() {
            redact_body(&mut r.headers, &mut r.body);
        }
        for m in flow.ws_messages.iter_mut() {
            if let Ok(text) = std::str::from_utf8(&m.payload)
                && jwt().is_match(text)
            {
                m.payload = Bytes::from(jwt().replace_all(text, PLACEHOLDER).into_owned());
            }
        }
    }
}

/// Redact copies of `flows`.
pub fn redacted(flows: &[Flow], options: &RedactOptions) -> Vec<Flow> {
    flows
        .iter()
        .cloned()
        .map(|mut f| {
            redact_flow(&mut f, options);
            f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxyzip_core::{FlowState, Request, Response, Scheme};
    use std::io::Write;

    const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhZGEifQ.c2lnbmF0dXJl";

    fn flow(
        req_headers: &[(&str, &str)],
        path: &str,
        req_body: &[u8],
        res_headers: &[(&str, &str)],
        res_body: &[u8],
    ) -> Flow {
        let hdr = |h: &[(&str, &str)]| Headers(h.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let mut f = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Https,
                host: "api.test".into(),
                port: 443,
                path: path.into(),
                http_version: "HTTP/1.1".into(),
                headers: hdr(req_headers),
                body: Bytes::copy_from_slice(req_body),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status: 200,
            reason: "OK".into(),
            http_version: "HTTP/1.1".into(),
            headers: hdr(res_headers),
            body: Bytes::copy_from_slice(res_body),
            body_truncated: false,
        });
        f.state = FlowState::Completed;
        f
    }

    #[test]
    fn headers_keep_their_shape() {
        let mut f = flow(
            &[
                ("Authorization", "Bearer abc.def"),
                ("Cookie", "sid=123; theme=dark"),
                ("X-Api-Key", "k-1"),
                ("Accept", "*/*"),
                ("X-Request-Id", "r-1"),
            ],
            "/x",
            b"",
            &[("Set-Cookie", "sid=999; Path=/; HttpOnly"), ("Content-Type", "text/plain")],
            b"",
        );
        redact_flow(&mut f, &RedactOptions::default());
        let h = &f.request.headers;
        assert_eq!(h.get("authorization"), Some("Bearer <redacted>"));
        assert_eq!(h.get("cookie"), Some("sid=<redacted>; theme=<redacted>"));
        assert_eq!(h.get("x-api-key"), Some("<redacted>"));
        assert_eq!(h.get("accept"), Some("*/*"));
        assert_eq!(h.get("x-request-id"), Some("r-1"));
        assert_eq!(f.response.unwrap().headers.get("set-cookie"), Some("sid=<redacted>; Path=/; HttpOnly"));
    }

    #[test]
    fn query_parameters_are_redacted_by_name() {
        assert_eq!(redact_path("/search?q=rust&token=abc&page=2"), "/search?q=rust&token=<redacted>&page=2");
        assert_eq!(redact_path("/cb?code=1&state=ok#frag"), "/cb?code=<redacted>&state=ok#frag");
        assert_eq!(redact_path("/plain"), "/plain");
        assert_eq!(redact_path("/x?API%5FKey=1"), "/x?API%5FKey=<redacted>");
    }

    #[test]
    fn json_fields_and_jwts_are_redacted_at_any_depth() {
        let body = format!(
            r#"{{"user":"ada","password":"hunter2","nested":{{"access_token":"t","items":[{{"secret":"s","id":1}}]}},"note":"see {TOKEN}"}}"#
        );
        let mut f = flow(&[("content-type", "application/json")], "/", body.as_bytes(), &[], b"");
        redact_flow(&mut f, &RedactOptions::default());
        let out: Value = serde_json::from_slice(&f.request.body).unwrap();
        assert_eq!(out["user"], "ada");
        assert_eq!(out["password"], PLACEHOLDER);
        assert_eq!(out["nested"]["access_token"], PLACEHOLDER);
        assert_eq!(out["nested"]["items"][0]["secret"], PLACEHOLDER);
        assert_eq!(out["nested"]["items"][0]["id"], 1);
        assert_eq!(out["note"], "see <redacted>");
        assert_eq!(f.request.headers.get("content-length"), Some(f.request.body.len().to_string().as_str()));
    }

    #[test]
    fn form_bodies_and_plain_text() {
        let mut f = flow(
            &[("content-type", "application/x-www-form-urlencoded")],
            "/",
            b"user=ada&password=pw&grant_type=password",
            &[("content-type", "text/plain")],
            format!("your token is {TOKEN}.").as_bytes(),
        );
        redact_flow(&mut f, &RedactOptions::default());
        assert_eq!(&f.request.body[..], b"user=ada&password=<redacted>&grant_type=password");
        assert_eq!(&f.response.unwrap().body[..], b"your token is <redacted>.");
    }

    #[test]
    fn compressed_bodies_are_decoded_first() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(br#"{"token":"abc"}"#).unwrap();
        let gz = enc.finish().unwrap();
        let mut f = flow(&[], "/", b"", &[("content-type", "application/json"), ("content-encoding", "gzip")], &gz);
        redact_flow(&mut f, &RedactOptions::default());
        let r = f.response.unwrap();
        assert!(r.headers.get("content-encoding").is_none(), "the stored body is plain now");
        assert_eq!(&r.body[..], br#"{"token":"<redacted>"}"#);
    }

    #[test]
    fn binary_bodies_and_options_are_respected() {
        let mut f = flow(
            &[("authorization", "Bearer x")],
            "/x?token=1",
            &[0xff, 0xfe, 0x00],
            &[("content-type", "image/png")],
            &[1, 2, 3],
        );
        redact_flow(&mut f, &RedactOptions { headers: false, query: false, bodies: true });
        assert_eq!(f.request.headers.get("authorization"), Some("Bearer x"));
        assert_eq!(f.request.path, "/x?token=1");
        assert_eq!(&f.request.body[..], &[0xff, 0xfe, 0x00]);
        assert_eq!(&f.response.unwrap().body[..], &[1, 2, 3]);
    }

    #[test]
    fn sensitive_name_rules() {
        for name in ["token", "access_token", "clientSecret", "password", "apiKey", "X-API-Key", "Authorization"] {
            assert!(is_sensitive_field(name) || is_sensitive_header(name), "{name}");
        }
        for name in ["user", "page", "keep-alive", "content-type"] {
            assert!(!is_sensitive_header(name), "{name}");
        }
    }
}
