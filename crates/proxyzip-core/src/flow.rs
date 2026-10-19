use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type FlowId = u64;

/// An ordered, case-preserving header list (duplicates allowed, like on the wire).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headers(pub Vec<(String, String)>);

impl Headers {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().filter(move |(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
    Ws,
    Wss,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
            Scheme::Ws => "ws",
            Scheme::Wss => "wss",
        }
    }

    pub fn is_secure(self) -> bool {
        matches!(self, Scheme::Https | Scheme::Wss)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    /// Path including the query string, e.g. `/search?q=rust`.
    pub path: String,
    pub http_version: String,
    pub headers: Headers,
    /// Body exactly as sent on the wire (still content-encoded).
    pub body: Bytes,
    /// True when the body exceeded the capture limit and was cut.
    pub body_truncated: bool,
}

impl Request {
    pub fn url(&self) -> String {
        let default_port = match self.scheme {
            Scheme::Http | Scheme::Ws => 80,
            Scheme::Https | Scheme::Wss => 443,
        };
        if self.port == default_port {
            format!("{}://{}{}", self.scheme.as_str(), self.host, self.path)
        } else {
            format!("{}://{}:{}{}", self.scheme.as_str(), self.host, self.port, self.path)
        }
    }

    pub fn query(&self) -> Option<&str> {
        self.path.split_once('?').map(|(_, q)| q)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub headers: Headers,
    pub body: Bytes,
    pub body_truncated: bool,
}

/// Milestones of a flow, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Timing {
    pub started: i64,
    pub request_sent: Option<i64>,
    pub response_started: Option<i64>,
    pub finished: Option<i64>,
}

impl Timing {
    pub fn duration_ms(&self) -> Option<i64> {
        self.finished.map(|f| f - self.started)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsInfo {
    pub sni: Option<String>,
    pub alpn: Option<String>,
    pub version: Option<String>,
    pub cipher: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WsDirection {
    /// Client → server.
    Outgoing,
    /// Server → client.
    Incoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WsKind {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub direction: WsDirection,
    pub kind: WsKind,
    pub timestamp: i64,
    pub payload: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowState {
    /// Request is being received / sent upstream.
    Pending,
    /// Response headers arrived, body is still streaming.
    Receiving,
    /// Upgraded WebSocket connection still open.
    WebSocketOpen,
    Completed,
    Failed,
    /// CONNECT tunnel forwarded without decryption (SSL proxying disabled for the host).
    Tunnel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: FlowId,
    pub created_at: DateTime<Utc>,
    pub client_addr: String,
    pub request: Request,
    pub response: Option<Response>,
    pub timing: Timing,
    pub tls: Option<TlsInfo>,
    pub state: FlowState,
    pub error: Option<String>,
    pub ws_messages: Vec<WsMessage>,
    /// Free-form user annotations (comment, color, pin) set from the UI.
    pub comment: Option<String>,
    pub pinned: bool,
}

impl Flow {
    pub fn new(id: FlowId, client_addr: String, request: Request) -> Self {
        let now = Utc::now();
        Self {
            id,
            created_at: now,
            client_addr,
            request,
            response: None,
            timing: Timing { started: now.timestamp_millis(), ..Default::default() },
            tls: None,
            state: FlowState::Pending,
            error: None,
            ws_messages: Vec::new(),
            comment: None,
            pinned: false,
        }
    }

    pub fn summary(&self) -> FlowSummary {
        let content_type = self
            .response
            .as_ref()
            .and_then(|r| r.headers.get("content-type"))
            .or_else(|| self.request.headers.get("content-type"))
            .map(|ct| ct.split(';').next().unwrap_or(ct).trim().to_string());
        FlowSummary {
            id: self.id,
            created_at: self.created_at.timestamp_millis(),
            method: self.request.method.clone(),
            scheme: self.request.scheme,
            host: self.request.host.clone(),
            port: self.request.port,
            path: self.request.path.clone(),
            url: self.request.url(),
            http_version: self
                .response
                .as_ref()
                .map(|r| r.http_version.clone())
                .unwrap_or_else(|| self.request.http_version.clone()),
            status: self.response.as_ref().map(|r| r.status),
            content_type,
            request_size: self.request.body.len(),
            response_size: self.response.as_ref().map(|r| r.body.len()),
            duration_ms: self.timing.duration_ms(),
            state: self.state,
            error: self.error.clone(),
            client_addr: self.client_addr.clone(),
            ws_message_count: self.ws_messages.len(),
            pinned: self.pinned,
            has_comment: self.comment.is_some(),
        }
    }
}

/// Lightweight projection of a [`Flow`] used for list views and events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSummary {
    pub id: FlowId,
    pub created_at: i64,
    pub method: String,
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub url: String,
    pub http_version: String,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub request_size: usize,
    pub response_size: Option<usize>,
    pub duration_ms: Option<i64>,
    pub state: FlowState,
    pub error: Option<String>,
    pub client_addr: String,
    pub ws_message_count: usize,
    pub pinned: bool,
    pub has_comment: bool,
}

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}
