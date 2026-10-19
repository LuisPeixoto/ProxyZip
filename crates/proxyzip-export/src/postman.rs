//! Postman collection v2.1 export. The collection groups requests by host.

use proxyzip_core::Flow;
use serde_json::{Value, json};

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";
/// Headers a client sets itself.
const SKIPPED: &[&str] = &["host", "content-length", "connection", "proxy-connection", "transfer-encoding"];
const MAX_EXAMPLE_BODY: usize = 1024 * 1024;

fn url_object(flow: &Flow) -> Value {
    let req = &flow.request;
    let (path, query) = match req.path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (req.path.as_str(), None),
    };
    let query: Vec<Value> = query
        .map(|q| url::form_urlencoded::parse(q.as_bytes()).map(|(k, v)| json!({ "key": k, "value": v })).collect())
        .unwrap_or_default();
    let mut url = json!({
        "raw": req.url(),
        "protocol": match req.scheme { proxyzip_core::Scheme::Https | proxyzip_core::Scheme::Wss => "https", _ => "http" },
        "host": req.host.split('.').collect::<Vec<_>>(),
        "path": path.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
    });
    if !matches!((req.scheme.is_secure(), req.port), (true, 443) | (false, 80)) {
        url["port"] = json!(req.port.to_string());
    }
    if !query.is_empty() {
        url["query"] = Value::Array(query);
    }
    url
}

fn headers(list: &proxyzip_core::Headers, drop_encoding: bool) -> Vec<Value> {
    list.iter()
        .filter(|(k, _)| !k.starts_with(':') && !SKIPPED.contains(&k.to_ascii_lowercase().as_str()))
        .filter(|(k, _)| !(drop_encoding && k.eq_ignore_ascii_case("content-encoding")))
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect()
}

fn item(flow: &Flow) -> Value {
    let req = &flow.request;
    let (decoded, encodings, error) = proxyzip_codec::decode_content(&req.headers, &req.body);
    let was_decoded = !encodings.is_empty() && error.is_none();
    let mut request = json!({
        "method": req.method,
        "header": headers(&req.headers, was_decoded),
        "url": url_object(flow),
    });
    if !decoded.is_empty() {
        match std::str::from_utf8(&decoded) {
            Ok(text) => {
                let language = match req.headers.get("content-type") {
                    Some(ct) if ct.contains("json") => "json",
                    Some(ct) if ct.contains("xml") => "xml",
                    Some(ct) if ct.contains("html") => "html",
                    _ => "text",
                };
                request["body"] = json!({ "mode": "raw", "raw": text, "options": { "raw": { "language": language } } });
            }
            Err(_) => request["description"] = json!("The original request had a binary body, which is not included."),
        }
    }
    let path = req.path.split('?').next().unwrap_or("/");
    let mut out =
        json!({ "name": format!("{} {}", req.method, if path.is_empty() { "/" } else { path }), "request": request });
    if let Some(res) = &flow.response {
        let (body, encodings, error) = proxyzip_codec::decode_content(&res.headers, &res.body);
        let was_decoded = !encodings.is_empty() && error.is_none();
        let text = std::str::from_utf8(&body).ok().filter(|t| t.len() <= MAX_EXAMPLE_BODY);
        out["response"] = json!([{
            "name": format!("{} {}", res.status, res.reason).trim().to_string(),
            "originalRequest": out["request"].clone(),
            "status": res.reason,
            "code": res.status,
            "header": headers(&res.headers, was_decoded),
            "body": text.unwrap_or(""),
        }]);
    } else {
        out["response"] = json!([]);
    }
    out
}

/// Build a collection named `name` from `flows`.
pub fn export(name: &str, flows: &[Flow]) -> Value {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for flow in flows.iter().filter(|f| f.request.method != "CONNECT") {
        let host = flow.request.host.clone();
        match groups.iter_mut().find(|(h, _)| *h == host) {
            Some((_, items)) => items.push(item(flow)),
            None => groups.push((host, vec![item(flow)])),
        }
    }
    json!({
        "info": { "name": name, "schema": SCHEMA },
        "item": groups.into_iter().map(|(host, items)| json!({ "name": host, "item": items })).collect::<Vec<_>>(),
    })
}

pub fn export_json(name: &str, flows: &[Flow]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&export(name, flows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use proxyzip_core::{Headers, Request, Response, Scheme};

    fn flow(host: &str, method: &str, path: &str, body: &[u8]) -> Flow {
        let mut f = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: method.into(),
                scheme: Scheme::Https,
                host: host.into(),
                port: 8443,
                path: path.into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("Host".into(), host.into()),
                ]),
                body: Bytes::copy_from_slice(body),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status: 201,
            reason: "Created".into(),
            http_version: "HTTP/1.1".into(),
            headers: Headers(vec![("content-type".into(), "application/json".into())]),
            body: Bytes::from_static(br#"{"id":1}"#),
            body_truncated: false,
        });
        f
    }

    #[test]
    fn collection_structure() {
        let flows = [
            flow("api.example.com", "POST", "/v1/items?x=1&y=two", br#"{"a":1}"#),
            flow("cdn.example.com", "GET", "/logo.png", b""),
            flow("api.example.com", "GET", "/v1/items", b""),
        ];
        let c = export("Session", &flows);
        assert_eq!(c["info"]["schema"], SCHEMA);
        assert_eq!(c["item"].as_array().unwrap().len(), 2, "grouped by host");
        let api = &c["item"][0];
        assert_eq!(api["name"], "api.example.com");
        assert_eq!(api["item"].as_array().unwrap().len(), 2);
        let post = &api["item"][0];
        assert_eq!(post["name"], "POST /v1/items");
        assert_eq!(post["request"]["method"], "POST");
        assert_eq!(post["request"]["body"]["raw"], r#"{"a":1}"#);
        assert_eq!(post["request"]["body"]["options"]["raw"]["language"], "json");
        let url = &post["request"]["url"];
        assert_eq!(url["raw"], "https://api.example.com:8443/v1/items?x=1&y=two");
        assert_eq!(url["host"], json!(["api", "example", "com"]));
        assert_eq!(url["path"], json!(["v1", "items"]));
        assert_eq!(url["port"], "8443");
        assert_eq!(url["query"], json!([{"key": "x", "value": "1"}, {"key": "y", "value": "two"}]));
        // Host is set by Postman itself.
        assert_eq!(post["request"]["header"], json!([{"key": "Content-Type", "value": "application/json"}]));
        let example = &post["response"][0];
        assert_eq!((example["code"].as_u64(), example["body"].as_str()), (Some(201), Some(r#"{"id":1}"#)));
        assert!(api["item"][1]["request"].get("body").is_none());
    }

    #[test]
    fn binary_bodies_are_noted_and_tunnels_skipped() {
        let c = export("S", &[flow("a.dev", "POST", "/", &[0xff, 0xfe]), flow("a.dev", "CONNECT", "", b"")]);
        let item = &c["item"][0]["item"];
        assert_eq!(item.as_array().unwrap().len(), 1);
        assert!(item[0]["request"].get("body").is_none());
        assert!(item[0]["request"]["description"].as_str().unwrap().contains("binary"));
        assert!(export_json("S", &[]).unwrap().contains("\"item\": []"));
    }
}
