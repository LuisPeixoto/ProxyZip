//! HAR 1.2 — <http://www.softwareishard.com/blog/har-12-spec/>.
//! WebSocket messages use Chrome DevTools' `_webSocketMessages` extension.

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use proxyzip_core::{Flow, FlowState, Headers, Request, Response, Scheme, Timing, WsDirection, WsKind, WsMessage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Har {
    pub log: Log,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Log {
    pub version: String,
    pub creator: Creator,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Creator {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub started_date_time: DateTime<Utc>,
    #[serde(default)]
    pub time: f64,
    pub request: HarRequest,
    pub response: HarResponse,
    #[serde(default)]
    pub cache: serde_json::Value,
    #[serde(default)]
    pub timings: Timings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_ip_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(rename = "_webSocketMessages", default, skip_serializing_if = "Vec::is_empty")]
    pub web_socket_messages: Vec<HarWsMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarRequest {
    pub method: String,
    pub url: String,
    pub http_version: String,
    #[serde(default)]
    pub cookies: Vec<serde_json::Value>,
    #[serde(default)]
    pub headers: Vec<NameValue>,
    #[serde(default)]
    pub query_string: Vec<NameValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_data: Option<PostData>,
    #[serde(default = "minus_one")]
    pub headers_size: i64,
    #[serde(default = "minus_one")]
    pub body_size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostData {
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarResponse {
    pub status: i64,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub http_version: String,
    #[serde(default)]
    pub cookies: Vec<serde_json::Value>,
    #[serde(default)]
    pub headers: Vec<NameValue>,
    pub content: Content,
    #[serde(default, rename = "redirectURL")]
    pub redirect_url: String,
    #[serde(default = "minus_one")]
    pub headers_size: i64,
    #[serde(default = "minus_one")]
    pub body_size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Timings {
    #[serde(default)]
    pub send: f64,
    #[serde(default)]
    pub wait: f64,
    #[serde(default)]
    pub receive: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HarWsMessage {
    #[serde(rename = "type")]
    pub kind: String,
    pub time: f64,
    pub opcode: u8,
    pub data: String,
}

fn minus_one() -> i64 {
    -1
}

fn name_values(h: &Headers) -> Vec<NameValue> {
    h.iter().map(|(k, v)| NameValue { name: k.clone(), value: v.clone() }).collect()
}

/// Text if valid UTF-8, otherwise base64 with `encoding = "base64"`.
fn encode_body(bytes: &[u8]) -> (String, Option<String>) {
    match std::str::from_utf8(bytes) {
        Ok(t) => (t.to_string(), None),
        Err(_) => (B64.encode(bytes), Some("base64".into())),
    }
}

fn entry(flow: &Flow) -> Entry {
    let req = &flow.request;
    let query_string = req
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| NameValue { name: k.into(), value: v.into() })
                .collect()
        })
        .unwrap_or_default();
    let post_data = (!req.body.is_empty()).then(|| {
        let (decoded, _, _) = proxyzip_codec::decode_content(&req.headers, &req.body);
        let (text, encoding) = encode_body(&decoded);
        PostData { mime_type: req.headers.get("content-type").unwrap_or("").to_string(), text, encoding }
    });
    let response = match &flow.response {
        Some(r) => {
            let (decoded, _, _) = proxyzip_codec::decode_content(&r.headers, &r.body);
            let (text, encoding) = encode_body(&decoded);
            HarResponse {
                status: r.status as i64,
                status_text: r.reason.clone(),
                http_version: r.http_version.clone(),
                cookies: vec![],
                headers: name_values(&r.headers),
                content: Content {
                    size: decoded.len() as i64,
                    mime_type: r.headers.get("content-type").unwrap_or("").to_string(),
                    text: Some(text),
                    encoding,
                },
                redirect_url: r.headers.get("location").unwrap_or("").to_string(),
                headers_size: -1,
                body_size: r.body.len() as i64,
            }
        }
        // HAR requires a response; status 0 marks "no response" (as browsers do).
        None => HarResponse {
            status: 0,
            status_text: flow.error.clone().unwrap_or_default(),
            http_version: String::new(),
            cookies: vec![],
            headers: vec![],
            content: Content { size: 0, mime_type: String::new(), text: None, encoding: None },
            redirect_url: String::new(),
            headers_size: -1,
            body_size: -1,
        },
    };
    let t = &flow.timing;
    let ms = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => (b - a).max(0) as f64,
        _ => 0.0,
    };
    let timings = Timings {
        send: ms(Some(t.started), t.request_sent),
        wait: ms(t.request_sent, t.response_started),
        receive: ms(t.response_started, t.finished),
    };
    Entry {
        started_date_time: flow.created_at,
        time: t.duration_ms().unwrap_or(0) as f64,
        request: HarRequest {
            method: req.method.clone(),
            url: req.url(),
            http_version: req.http_version.clone(),
            cookies: vec![],
            headers: name_values(&req.headers),
            query_string,
            post_data,
            headers_size: -1,
            body_size: req.body.len() as i64,
        },
        response,
        cache: serde_json::json!({}),
        timings,
        server_ip_address: None,
        comment: flow.comment.clone(),
        web_socket_messages: flow
            .ws_messages
            .iter()
            .map(|m| {
                let (opcode, data) = match m.kind {
                    WsKind::Text => (1, String::from_utf8_lossy(&m.payload).into_owned()),
                    WsKind::Binary => (2, B64.encode(&m.payload)),
                    WsKind::Close => (8, String::from_utf8_lossy(&m.payload).into_owned()),
                    WsKind::Ping => (9, B64.encode(&m.payload)),
                    WsKind::Pong => (10, B64.encode(&m.payload)),
                };
                HarWsMessage {
                    kind: if m.direction == WsDirection::Outgoing { "send" } else { "receive" }.into(),
                    time: m.timestamp as f64 / 1000.0,
                    opcode,
                    data,
                }
            })
            .collect(),
    }
}

pub fn export(flows: &[Flow]) -> Har {
    Har {
        log: Log {
            version: "1.2".into(),
            creator: Creator { name: "ProxyZip".into(), version: env!("CARGO_PKG_VERSION").into() },
            entries: flows.iter().filter(|f| f.request.method != "CONNECT").map(entry).collect(),
        },
    }
}

pub fn export_json(flows: &[Flow]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&export(flows))?)
}

fn decode_body(text: &str, encoding: Option<&str>) -> Bytes {
    if encoding == Some("base64")
        && let Ok(b) = B64.decode(text)
    {
        return Bytes::from(b);
    }
    Bytes::from(text.to_string())
}

fn headers_from(list: &[NameValue], drop_encoding: bool) -> Headers {
    Headers(
        list.iter()
            .filter(|nv| !nv.name.starts_with(':'))
            .filter(|nv| !(drop_encoding && nv.name.eq_ignore_ascii_case("content-encoding")))
            .map(|nv| (nv.name.clone(), nv.value.clone()))
            .collect(),
    )
}

/// Parse a HAR document into flows (ids are assigned when imported into a store).
pub fn import(json: &str) -> Result<Vec<Flow>> {
    let har: Har = serde_json::from_str(json).context("not a valid HAR file")?;
    let mut flows = Vec::with_capacity(har.log.entries.len());
    for e in har.log.entries {
        let url = url::Url::parse(&e.request.url).with_context(|| format!("bad URL {}", e.request.url))?;
        let scheme = match url.scheme() {
            "https" => Scheme::Https,
            "wss" => Scheme::Wss,
            "ws" => Scheme::Ws,
            _ => Scheme::Http,
        };
        let mut path = url.path().to_string();
        if let Some(q) = url.query() {
            path.push('?');
            path.push_str(q);
        }
        // HAR bodies are already decoded, so content-encoding no longer applies.
        let request = Request {
            method: e.request.method,
            scheme,
            host: url.host_str().unwrap_or_default().to_string(),
            port: url.port_or_known_default().unwrap_or(80),
            path,
            http_version: e.request.http_version,
            headers: headers_from(&e.request.headers, true),
            body: e.request.post_data.as_ref().map(|p| decode_body(&p.text, p.encoding.as_deref())).unwrap_or_default(),
            body_truncated: false,
        };
        let started = e.started_date_time.timestamp_millis();
        let response = (e.response.status > 0).then(|| Response {
            status: e.response.status as u16,
            reason: e.response.status_text.clone(),
            http_version: e.response.http_version.clone(),
            headers: headers_from(&e.response.headers, true),
            body: e
                .response
                .content
                .text
                .as_deref()
                .map(|t| decode_body(t, e.response.content.encoding.as_deref()))
                .unwrap_or_default(),
            body_truncated: false,
        });
        let send = e.timings.send.max(0.0) as i64;
        let wait = e.timings.wait.max(0.0) as i64;
        let timing = Timing {
            started,
            request_sent: Some(started + send),
            response_started: Some(started + send + wait),
            finished: Some(started + e.time.max(0.0) as i64),
        };
        let ws_messages = e
            .web_socket_messages
            .iter()
            .map(|m| WsMessage {
                direction: if m.kind == "send" { WsDirection::Outgoing } else { WsDirection::Incoming },
                kind: match m.opcode {
                    1 => WsKind::Text,
                    8 => WsKind::Close,
                    9 => WsKind::Ping,
                    10 => WsKind::Pong,
                    _ => WsKind::Binary,
                },
                timestamp: (m.time * 1000.0) as i64,
                payload: if m.opcode == 1 || m.opcode == 8 {
                    Bytes::from(m.data.clone())
                } else {
                    B64.decode(&m.data).map(Bytes::from).unwrap_or_else(|_| Bytes::from(m.data.clone()))
                },
            })
            .collect();
        let failed = response.is_none();
        let mut flow = Flow::new(0, "imported".into(), request);
        flow.created_at = e.started_date_time;
        flow.response = response;
        flow.timing = timing;
        flow.state = if failed { FlowState::Failed } else { FlowState::Completed };
        flow.error = failed.then(|| e.response.status_text.clone()).filter(|s| !s.is_empty());
        flow.comment = e.comment;
        flow.ws_messages = ws_messages;
        flows.push(flow);
    }
    Ok(flows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Flow {
        let mut f = Flow::new(
            7,
            "127.0.0.1:5".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Https,
                host: "example.com".into(),
                port: 8443,
                path: "/a?b=1&c=two".into(),
                http_version: "HTTP/2".into(),
                headers: Headers(vec![("content-type".into(), "application/json".into())]),
                body: Bytes::from_static(br#"{"x":1}"#),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status: 201,
            reason: "Created".into(),
            http_version: "HTTP/2".into(),
            headers: Headers(vec![("content-type".into(), "image/png".into())]),
            body: Bytes::from_static(&[0x89, b'P', b'N', b'G', 0xff, 0x00]),
            body_truncated: false,
        });
        f.timing.request_sent = Some(f.timing.started + 3);
        f.timing.response_started = Some(f.timing.started + 10);
        f.timing.finished = Some(f.timing.started + 15);
        f.state = FlowState::Completed;
        f.ws_messages.push(WsMessage {
            direction: WsDirection::Incoming,
            kind: WsKind::Text,
            timestamp: 1_700_000_000_000,
            payload: Bytes::from_static(b"hey"),
        });
        f
    }

    #[test]
    fn roundtrip() {
        let original = sample();
        let json = export_json(std::slice::from_ref(&original)).unwrap();
        assert!(json.contains("\"encoding\": \"base64\""));
        let back = import(&json).unwrap();
        assert_eq!(back.len(), 1);
        let f = &back[0];
        assert_eq!(f.request.url(), original.request.url());
        assert_eq!(f.request.body, original.request.body);
        let (r, o) = (f.response.as_ref().unwrap(), original.response.as_ref().unwrap());
        assert_eq!(r.status, 201);
        assert_eq!(r.body, o.body);
        assert_eq!(f.timing.duration_ms(), Some(15));
        assert_eq!(f.ws_messages.len(), 1);
        assert_eq!(&f.ws_messages[0].payload[..], b"hey");
    }
}
