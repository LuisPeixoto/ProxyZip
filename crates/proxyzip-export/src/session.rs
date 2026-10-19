//! The `.pzsession` file: a whole capture session in one file.
//!
//! The format is zstd-compressed JSON with a small header, so it stays compact and any tool can read it
//! (`zstd -d session.pzsession | jq .`). Bodies are base64 strings.

use std::io::Read;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use proxyzip_core::{
    Flow, FlowState, Headers, LogLine, Request, Response, Scheme, Timing, TlsInfo, WsDirection, WsKind, WsMessage,
};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "proxyzip-session";
pub const VERSION: u32 = 1;
/// Refuse to inflate more than this, so a hostile file cannot exhaust memory.
const MAX_UNCOMPRESSED: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct SessionFile {
    format: String,
    version: u32,
    created_at: DateTime<Utc>,
    app_version: String,
    flows: Vec<FlowDto>,
}

#[derive(Serialize, Deserialize)]
struct FlowDto {
    created_at: DateTime<Utc>,
    client_addr: String,
    request: RequestDto,
    response: Option<ResponseDto>,
    timing: Timing,
    tls: Option<TlsInfo>,
    state: FlowState,
    error: Option<String>,
    #[serde(default)]
    ws_messages: Vec<WsDto>,
    comment: Option<String>,
    pinned: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    graphql: Option<String>,
    #[serde(default)]
    logs: Vec<LogLine>,
}

#[derive(Serialize, Deserialize)]
struct RequestDto {
    method: String,
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
    http_version: String,
    headers: Headers,
    body: String,
    body_truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct ResponseDto {
    status: u16,
    reason: String,
    http_version: String,
    headers: Headers,
    body: String,
    body_truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct WsDto {
    direction: WsDirection,
    kind: WsKind,
    timestamp: i64,
    payload: String,
}

fn decode(b64: &str) -> Result<Bytes> {
    Ok(Bytes::from(B64.decode(b64).context("a body in the session file is not valid base64")?))
}

impl From<&Flow> for FlowDto {
    fn from(f: &Flow) -> Self {
        Self {
            created_at: f.created_at,
            client_addr: f.client_addr.clone(),
            request: RequestDto {
                method: f.request.method.clone(),
                scheme: f.request.scheme,
                host: f.request.host.clone(),
                port: f.request.port,
                path: f.request.path.clone(),
                http_version: f.request.http_version.clone(),
                headers: f.request.headers.clone(),
                body: B64.encode(&f.request.body),
                body_truncated: f.request.body_truncated,
            },
            response: f.response.as_ref().map(|r| ResponseDto {
                status: r.status,
                reason: r.reason.clone(),
                http_version: r.http_version.clone(),
                headers: r.headers.clone(),
                body: B64.encode(&r.body),
                body_truncated: r.body_truncated,
            }),
            timing: f.timing.clone(),
            tls: f.tls.clone(),
            state: f.state,
            error: f.error.clone(),
            ws_messages: f
                .ws_messages
                .iter()
                .map(|m| WsDto {
                    direction: m.direction,
                    kind: m.kind,
                    timestamp: m.timestamp,
                    payload: B64.encode(&m.payload),
                })
                .collect(),
            comment: f.comment.clone(),
            pinned: f.pinned,
            tags: f.tags.clone(),
            graphql: f.graphql.clone(),
            logs: f.logs.clone(),
        }
    }
}

impl FlowDto {
    fn into_flow(self) -> Result<Flow> {
        let request = Request {
            method: self.request.method,
            scheme: self.request.scheme,
            host: self.request.host,
            port: self.request.port,
            path: self.request.path,
            http_version: self.request.http_version,
            headers: self.request.headers,
            body: decode(&self.request.body)?,
            body_truncated: self.request.body_truncated,
        };
        let mut flow = Flow::new(0, self.client_addr, request);
        flow.created_at = self.created_at;
        flow.response = match self.response {
            Some(r) => Some(Response {
                status: r.status,
                reason: r.reason,
                http_version: r.http_version,
                headers: r.headers,
                body: decode(&r.body)?,
                body_truncated: r.body_truncated,
            }),
            None => None,
        };
        flow.timing = self.timing;
        flow.tls = self.tls;
        // A session is a recording: nothing in it is still in flight.
        flow.state = match self.state {
            FlowState::Pending | FlowState::Receiving | FlowState::Paused | FlowState::WebSocketOpen => {
                FlowState::Failed
            }
            other => other,
        };
        flow.error = self.error.or_else(|| {
            matches!(
                self.state,
                FlowState::Pending | FlowState::Receiving | FlowState::Paused | FlowState::WebSocketOpen
            )
            .then(|| "The session was saved while this request was still in progress.".into())
        });
        flow.ws_messages = self
            .ws_messages
            .into_iter()
            .map(|m| {
                Ok(WsMessage {
                    direction: m.direction,
                    kind: m.kind,
                    timestamp: m.timestamp,
                    payload: decode(&m.payload)?,
                })
            })
            .collect::<Result<_>>()?;
        flow.comment = self.comment;
        flow.pinned = self.pinned;
        flow.tags = self.tags;
        flow.graphql = self.graphql;
        flow.logs = self.logs;
        Ok(flow)
    }
}

/// Serialize flows into the `.pzsession` format.
pub fn write(flows: &[Flow]) -> Result<Vec<u8>> {
    let file = SessionFile {
        format: FORMAT.into(),
        version: VERSION,
        created_at: Utc::now(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        flows: flows.iter().map(FlowDto::from).collect(),
    };
    let json = serde_json::to_vec(&file)?;
    Ok(zstd::encode_all(&json[..], 3)?)
}

/// Read a `.pzsession` file. Flow ids are not preserved: the store assigns new ones on import.
pub fn read(bytes: &[u8]) -> Result<Vec<Flow>> {
    let decoder = zstd::stream::read::Decoder::new(bytes).context("this is not a ProxyZip session file")?;
    let mut json = Vec::new();
    decoder
        .take(MAX_UNCOMPRESSED)
        .read_to_end(&mut json)
        .context("this is not a ProxyZip session file (it could not be decompressed)")?;
    let file: SessionFile = serde_json::from_slice(&json).context("the session file is damaged")?;
    if file.format != FORMAT {
        bail!("this is not a ProxyZip session file");
    }
    if file.version > VERSION {
        bail!(
            "this session was saved by a newer ProxyZip (format version {}); update the app to open it",
            file.version
        );
    }
    file.flows.into_iter().map(FlowDto::into_flow).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Flow {
        let mut f = Flow::new(
            9,
            "10.0.0.2:5555".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Wss,
                host: "example.com".into(),
                port: 443,
                path: "/chat?room=1".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![("a".into(), "1".into()), ("a".into(), "2".into())]),
                body: Bytes::from_static(&[0, 1, 2, 0xff, 0xfe]),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status: 101,
            reason: "Switching Protocols".into(),
            http_version: "HTTP/1.1".into(),
            headers: Headers(vec![("upgrade".into(), "websocket".into())]),
            body: Bytes::new(),
            body_truncated: false,
        });
        f.state = FlowState::Completed;
        f.tags = vec!["rewrite".into()];
        f.graphql = Some("query X".into());
        f.comment = Some("note".into());
        f.pinned = true;
        f.logs.push(LogLine { level: "log".into(), message: "hi".into(), timestamp: 5 });
        f.ws_messages.push(WsMessage {
            direction: WsDirection::Incoming,
            kind: WsKind::Binary,
            timestamp: 7,
            payload: Bytes::from_static(&[9, 8, 7]),
        });
        f
    }

    #[test]
    fn round_trip_keeps_everything_including_binary_bodies() {
        let original = flow();
        let bytes = write(std::slice::from_ref(&original)).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.len(), 1);
        let f = &back[0];
        assert_eq!(f.request.body, original.request.body);
        assert_eq!(f.request.headers, original.request.headers);
        assert_eq!(f.request.url(), original.request.url());
        assert_eq!(f.response.as_ref().unwrap().status, 101);
        assert_eq!(f.state, FlowState::Completed);
        assert_eq!(
            (f.tags.clone(), f.graphql.clone(), f.comment.clone(), f.pinned),
            (original.tags, original.graphql, original.comment, true)
        );
        assert_eq!(f.logs, original.logs);
        assert_eq!(f.ws_messages[0].payload, original.ws_messages[0].payload);
        assert_eq!(f.created_at, original.created_at);
    }

    #[test]
    fn files_are_compressed_zstd_json() {
        let many: Vec<Flow> = (0..200).map(|_| flow()).collect();
        let bytes = write(&many).unwrap();
        assert_eq!(&bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd], "zstd magic");
        let json = zstd::decode_all(&bytes[..]).unwrap();
        assert!(bytes.len() * 5 < json.len(), "repetitive sessions compress well: {} vs {}", bytes.len(), json.len());
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!((value["format"].as_str(), value["version"].as_u64()), (Some(FORMAT), Some(1)));
    }

    #[test]
    fn in_flight_flows_are_marked_failed() {
        let mut f = flow();
        f.state = FlowState::Receiving;
        let back = read(&write(&[f]).unwrap()).unwrap();
        assert_eq!(back[0].state, FlowState::Failed);
        assert!(back[0].error.as_ref().unwrap().contains("in progress"));
    }

    #[test]
    fn bad_files_are_rejected_with_clear_messages() {
        assert!(read(b"hello").unwrap_err().to_string().contains("not a ProxyZip session"));
        let wrong_format = zstd::encode_all(
            &br#"{"format":"other","version":1,"created_at":"2026-01-01T00:00:00Z","app_version":"x","flows":[]}"#[..],
            1,
        )
        .unwrap();
        assert!(read(&wrong_format).unwrap_err().to_string().contains("not a ProxyZip session"));
        let newer = zstd::encode_all(&br#"{"format":"proxyzip-session","version":99,"created_at":"2026-01-01T00:00:00Z","app_version":"x","flows":[]}"#[..], 1).unwrap();
        assert!(read(&newer).unwrap_err().to_string().contains("newer ProxyZip"));
        let damaged = zstd::encode_all(&b"{not json"[..], 1).unwrap();
        assert!(read(&damaged).unwrap_err().to_string().contains("damaged"));
        assert!(read(&write(&[]).unwrap()).unwrap().is_empty());
    }
}
