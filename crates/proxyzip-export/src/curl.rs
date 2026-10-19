use proxyzip_core::Flow;

/// Quote for POSIX shells: wrap in single quotes, escaping embedded ones.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

const SKIP: &[&str] = &["host", "content-length", "connection", "proxy-connection", "accept-encoding"];

/// Render the flow's request as a reproducible `curl` command.
pub fn to_curl(flow: &Flow) -> String {
    let req = &flow.request;
    let mut parts = vec!["curl".to_string()];
    // curl infers GET (no body) and POST (with body); spell out everything else.
    let implied = (req.method == "GET" && req.body.is_empty()) || (req.method == "POST" && !req.body.is_empty());
    if !implied {
        parts.push(format!("-X {}", req.method));
    }
    parts.push(sh_quote(&req.url()));
    let mut compressed = false;
    for (k, v) in req.headers.iter() {
        let lk = k.to_ascii_lowercase();
        if lk == "accept-encoding" {
            compressed = true;
        }
        if SKIP.contains(&lk.as_str()) || lk.starts_with(':') {
            continue;
        }
        parts.push(format!("-H {}", sh_quote(&format!("{k}: {v}"))));
    }
    if compressed {
        parts.push("--compressed".into());
    }
    if !req.body.is_empty() {
        match std::str::from_utf8(&req.body) {
            Ok(text) => parts.push(format!("--data-binary {}", sh_quote(text))),
            Err(_) => parts.push("--data-binary @request-body.bin".into()),
        }
    }
    if req.http_version == "HTTP/2" {
        parts.push("--http2".into());
    }
    parts.join(" \\\n  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxyzip_core::{Headers, Request, Scheme};

    #[test]
    fn golden() {
        let flow = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Https,
                host: "api.example.com".into(),
                port: 443,
                path: "/v1/items?x=1".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![
                    ("Host".into(), "api.example.com".into()),
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept-Encoding".into(), "gzip".into()),
                    ("X-Note".into(), "it's".into()),
                ]),
                body: bytes::Bytes::from_static(br#"{"a":1}"#),
                body_truncated: false,
            },
        );
        assert_eq!(
            to_curl(&flow),
            "curl \\\n  'https://api.example.com/v1/items?x=1' \\\n  -H 'Content-Type: application/json' \\\n  -H 'X-Note: it'\\''s' \\\n  --compressed \\\n  --data-binary '{\"a\":1}'"
        );
    }
}
