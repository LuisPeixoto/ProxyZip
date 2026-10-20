use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use proxyzip_core::FlowStore;
use proxyzip_rules::RuleSet;
use serde_json::{Value, json};

use crate::tools;

/// Protocol revisions this server can speak, newest first.
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Reads and replaces the rule set. The desktop app persists and applies it; a read-only server has none.
pub trait RulesBackend: Send + Sync {
    fn get(&self) -> RuleSet;
    fn set<'a>(&'a self, rules: RuleSet) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

type StatusFn = Box<dyn Fn() -> Value + Send + Sync>;

/// Everything the tools need.
pub struct McpState {
    pub store: FlowStore,
    pub rules: Option<Arc<dyn RulesBackend>>,
    /// When false, tools that change state are refused.
    pub allow_changes: AtomicBool,
    status: StatusFn,
}

impl McpState {
    pub fn new(store: FlowStore) -> Self {
        Self { store, rules: None, allow_changes: AtomicBool::new(false), status: Box::new(|| json!({})) }
    }

    pub fn with_rules(mut self, rules: Arc<dyn RulesBackend>) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn with_status(mut self, status: impl Fn() -> Value + Send + Sync + 'static) -> Self {
        self.status = Box::new(status);
        self
    }

    pub fn allow_changes(self, allow: bool) -> Self {
        self.allow_changes.store(allow, Ordering::Relaxed);
        self
    }

    pub(crate) fn status(&self) -> Value {
        (self.status)()
    }

    pub(crate) fn changes_allowed(&self) -> bool {
        self.allow_changes.load(Ordering::Relaxed)
    }
}

pub struct McpServer {
    pub state: Arc<McpState>,
}

fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn result(id: &Value, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

impl McpServer {
    pub fn new(state: Arc<McpState>) -> Self {
        Self { state }
    }

    /// Handle a JSON-RPC message or batch. Notifications produce no response.
    pub async fn handle(&self, message: Value) -> Option<Value> {
        match message {
            Value::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    if let Some(r) = self.handle_one(item).await {
                        out.push(r);
                    }
                }
                (!out.is_empty()).then(|| Value::Array(out))
            }
            other => self.handle_one(other).await,
        }
    }

    async fn handle_one(&self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response from the client (we send no requests) or garbage.
            return id.map(|id| error(&id, -32600, "invalid request"));
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            return None; // notification, such as notifications/initialized
        };
        Some(match method {
            "initialize" => result(&id, self.initialize(&params)),
            "ping" => result(&id, json!({})),
            "tools/list" => result(&id, json!({ "tools": tools::definitions(&self.state) })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                result(&id, tools::call(&self.state, name, &args).await)
            }
            "resources/list" => result(&id, tools::list_resources(&self.state)),
            "resources/templates/list" => result(
                &id,
                json!({ "resourceTemplates": [{ "uriTemplate": "flow://{id}", "name": "flow", "description": "A captured request and its response", "mimeType": "application/json" }] }),
            ),
            "resources/read" => {
                match tools::read_resource(&self.state, params.get("uri").and_then(Value::as_str).unwrap_or_default()) {
                    Ok(v) => result(&id, v),
                    Err(e) => error(&id, -32002, e),
                }
            }
            "prompts/list" => result(&id, json!({ "prompts": [] })),
            other => error(&id, -32601, format!("method not found: {other}")),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let version = SUPPORTED_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(SUPPORTED_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false, "subscribe": false } },
            "serverInfo": { "name": "proxyzip", "title": "ProxyZip", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Inspect HTTP(S) traffic captured by ProxyZip. Start with list_flows (use the same filter syntax as the app: host:api status:5xx method:post), then get_flow for details. Bodies are decoded and truncated; use query_body with a jq expression to extract fields from large JSON."
        })
    }
}
