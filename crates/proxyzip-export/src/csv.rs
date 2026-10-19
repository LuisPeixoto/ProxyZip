//! CSV export of the request list.

use proxyzip_core::Flow;

/// Column headers, in order.
pub const COLUMNS: [&str; 20] = [
    "id",
    "started",
    "method",
    "scheme",
    "host",
    "port",
    "path",
    "url",
    "status",
    "reason",
    "content_type",
    "request_bytes",
    "response_bytes",
    "duration_ms",
    "http_version",
    "client",
    "state",
    "tags",
    "graphql",
    "error",
];

/// Quote a field when needed. Text that a spreadsheet could read as a formula gets a leading apostrophe, so opening
/// an export of hostile traffic cannot run anything.
fn field(value: &str, text: bool) -> String {
    let mut v = value.to_string();
    if text && v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        v.insert(0, '\'');
    }
    if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v }
}

fn row(flow: &Flow) -> Vec<String> {
    let s = flow.summary();
    let state = serde_json::to_value(s.state).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    vec![
        field(&flow.id.to_string(), false),
        field(&flow.created_at.to_rfc3339(), false),
        field(&s.method, true),
        field(s.scheme.as_str(), false),
        field(&s.host, true),
        field(&s.port.to_string(), false),
        field(&s.path, true),
        field(&s.url, true),
        field(&s.status.map(|v| v.to_string()).unwrap_or_default(), false),
        field(flow.response.as_ref().map_or("", |r| r.reason.as_str()), true),
        field(s.content_type.as_deref().unwrap_or(""), true),
        field(&s.request_size.to_string(), false),
        field(&s.response_size.map(|v| v.to_string()).unwrap_or_default(), false),
        field(&s.duration_ms.map(|v| v.to_string()).unwrap_or_default(), false),
        field(&s.http_version, true),
        field(&s.client_addr, true),
        field(&state, false),
        field(&s.tags.join(" "), true),
        field(s.graphql.as_deref().unwrap_or(""), true),
        field(s.error.as_deref().unwrap_or(""), true),
    ]
}

/// One header line and one line per flow, CRLF-terminated as RFC 4180 asks.
pub fn export(flows: &[Flow]) -> String {
    let mut out = COLUMNS.join(",");
    out.push_str("\r\n");
    for flow in flows.iter().filter(|f| f.request.method != "CONNECT") {
        out.push_str(&row(flow).join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use proxyzip_core::{FlowState, Headers, Request, Response, Scheme};

    fn flow(id: u64, path: &str) -> Flow {
        let mut f = Flow::new(
            id,
            "127.0.0.1:5000".into(),
            Request {
                method: "GET".into(),
                scheme: Scheme::Https,
                host: "api.example.com".into(),
                port: 443,
                path: path.into(),
                http_version: "HTTP/2".into(),
                headers: Headers::default(),
                body: Bytes::new(),
                body_truncated: false,
            },
        );
        f.created_at = "2026-01-02T03:04:05Z".parse().unwrap();
        f.response = Some(Response {
            status: 200,
            reason: "OK".into(),
            http_version: "HTTP/2".into(),
            headers: Headers(vec![("content-type".into(), "application/json; charset=utf-8".into())]),
            body: Bytes::from_static(b"{}"),
            body_truncated: false,
        });
        f.timing.finished = Some(f.timing.started + 42);
        f.state = FlowState::Completed;
        f.tags = vec!["rewrite".into(), "throttle".into()];
        f
    }

    #[test]
    fn golden_row() {
        let csv = export(&[flow(7, "/v1/items?id=2")]);
        let mut lines = csv.split("\r\n");
        assert_eq!(lines.next().unwrap(), COLUMNS.join(","));
        assert_eq!(
            lines.next().unwrap(),
            "7,2026-01-02T03:04:05+00:00,GET,https,api.example.com,443,/v1/items?id=2,https://api.example.com/v1/items?id=2,200,OK,application/json,0,2,42,HTTP/2,127.0.0.1:5000,completed,rewrite throttle,,"
        );
        assert_eq!(lines.next().unwrap(), "");
    }

    #[test]
    fn quoting_and_formula_protection() {
        let mut tricky = flow(1, "/say,\"hi\"\nthere");
        tricky.request.host = "=cmd|' /C calc'!A0".into();
        let csv = export(&[tricky]);
        assert!(csv.contains("\"/say,\"\"hi\"\"\nthere\""), "{csv}");
        assert!(csv.contains("'=cmd"), "formulas are defused: {csv}");
        assert!(!csv.contains(",=cmd"));
    }

    #[test]
    fn tunnels_are_left_out() {
        let mut connect = flow(2, "");
        connect.request.method = "CONNECT".into();
        assert_eq!(export(&[connect]).lines().count(), 1, "only the header");
    }
}
