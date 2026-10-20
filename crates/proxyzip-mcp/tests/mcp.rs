use std::sync::{Arc, Mutex};

use bytes::Bytes;
use proxyzip_core::{FlowState, FlowStore, Headers, Request, Response, Scheme};
use proxyzip_mcp::{McpServer, McpState, RulesBackend};
use proxyzip_rules::RuleSet;
use serde_json::{Value, json};

fn add(store: &FlowStore, method: &str, host: &str, path: &str, status: u16, body: &str) -> u64 {
    let id = store.create(
        "127.0.0.1:5000".into(),
        Request {
            method: method.into(),
            scheme: Scheme::Https,
            host: host.into(),
            port: 443,
            path: path.into(),
            http_version: "HTTP/1.1".into(),
            headers: Headers(vec![("accept".into(), "*/*".into())]),
            body: Bytes::new(),
            body_truncated: false,
        },
    );
    store.update(id, |f| {
        f.response = Some(Response {
            status,
            reason: "OK".into(),
            http_version: "HTTP/1.1".into(),
            headers: Headers(vec![("content-type".into(), "application/json".into())]),
            body: Bytes::from(body.to_string()),
            body_truncated: false,
        });
        f.state = FlowState::Completed;
        f.timing.finished = Some(f.timing.started + 12);
    });
    id
}

struct Memory(Mutex<RuleSet>);

impl RulesBackend for Memory {
    fn get(&self) -> RuleSet {
        self.0.lock().unwrap().clone()
    }
    fn set<'a>(
        &'a self,
        rules: RuleSet,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            *self.0.lock().unwrap() = rules;
            Ok(())
        })
    }
}

fn server(allow: bool) -> (McpServer, FlowStore, Arc<Memory>) {
    let store = FlowStore::new(1000);
    add(
        &store,
        "GET",
        "api.example.com",
        "/users/1",
        200,
        r#"{"users":[{"name":"ada","age":36},{"name":"alan","age":41}]}"#,
    );
    add(&store, "POST", "api.example.com", "/login", 500, r#"{"error":"boom"}"#);
    add(&store, "GET", "cdn.example.net", "/logo.png", 200, "not json");
    let rules = Arc::new(Memory(Mutex::new(RuleSet::default())));
    let state = McpState::new(store.clone())
        .with_rules(rules.clone())
        .with_status(|| json!({ "running": true }))
        .allow_changes(allow);
    (McpServer::new(Arc::new(state)), store, rules)
}

async fn rpc(s: &McpServer, method: &str, params: Value) -> Value {
    s.handle(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).await.expect("a response")
}

async fn call(s: &McpServer, name: &str, args: Value) -> (bool, Value) {
    let r = rpc(s, "tools/call", json!({ "name": name, "arguments": args })).await;
    let result = &r["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
    let is_error = result["isError"].as_bool().unwrap_or(false);
    (is_error, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[tokio::test]
async fn initialize_negotiates_the_version_and_lists_tools() {
    let (s, _, _) = server(false);
    let r = rpc(
        &s,
        "initialize",
        json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
    )
    .await;
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(r["result"]["serverInfo"]["name"], "proxyzip");
    let r = rpc(&s, "initialize", json!({ "protocolVersion": "1999-01-01" })).await;
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18", "unknown versions fall back to the newest");

    assert!(s.handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());
    let tools = rpc(&s, "tools/list", json!({})).await;
    let names: Vec<_> =
        tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for expected in ["list_flows", "get_flow", "query_body", "generate_code", "add_rule", "clear_flows"] {
        assert!(names.contains(&expected.to_string()), "{expected} in {names:?}");
    }
    assert_eq!(rpc(&s, "nope", json!({})).await["error"]["code"], -32601);
}

#[tokio::test]
async fn lists_and_filters_flows() {
    let (s, _, _) = server(false);
    let (err, all) = call(&s, "list_flows", json!({})).await;
    assert!(!err);
    assert_eq!(all["total"], 3);
    assert_eq!(all["flows"][0]["url"], "https://cdn.example.net/logo.png", "newest first");

    let (_, errors) = call(&s, "list_flows", json!({ "filter": "status:5xx host:api" })).await;
    assert_eq!(errors["total"], 1);
    assert_eq!(errors["flows"][0]["method"], "POST");

    let (_, page) = call(&s, "list_flows", json!({ "limit": 1, "offset": 1, "order": "oldest" })).await;
    assert_eq!(page["returned"], 1);
    assert_eq!(page["flows"][0]["url"], "https://api.example.com/login");
}

#[tokio::test]
async fn gets_a_flow_with_decoded_bodies() {
    let (s, _, _) = server(false);
    let (err, f) = call(&s, "get_flow", json!({ "id": 1 })).await;
    assert!(!err);
    assert_eq!(f["response"]["status"], 200);
    assert!(f["response"]["body"]["text"].as_str().unwrap().contains("alan"));
    assert_eq!(f["summary"]["duration_ms"], 12);

    let (_, small) = call(&s, "get_flow", json!({ "id": 1, "max_body_bytes": 10 })).await;
    assert_eq!(small["response"]["body"]["text_truncated"], true);
    let (_, none) = call(&s, "get_flow", json!({ "id": 1, "include_bodies": false })).await;
    assert!(none["response"].get("body").is_none());

    let (err, missing) = call(&s, "get_flow", json!({ "id": 99 })).await;
    assert!(err);
    assert!(missing.as_str().unwrap().contains("no request with id 99"));
}

#[tokio::test]
async fn searches_queries_and_generates() {
    let (s, _, _) = server(false);
    let (_, hits) = call(&s, "search_bodies", json!({ "text": "BOOM" })).await;
    assert_eq!(hits["returned"], 1);
    assert_eq!(hits["flows"][0]["matches"][0]["part"], "response");

    let (err, q) =
        call(&s, "query_body", json!({ "id": 1, "part": "response", "expr": ".users[] | select(.age > 40) | .name" }))
            .await;
    assert!(!err);
    assert_eq!(q["results"], json!(["alan"]));
    let (err, _) = call(&s, "query_body", json!({ "id": 1, "part": "response", "expr": ".users[" })).await;
    assert!(err, "a bad jq expression is reported as a tool error");

    let (_, code) = call(&s, "generate_code", json!({ "id": 2, "language": "python" })).await;
    let code = code.as_str().unwrap();
    assert!(code.contains("requests") && code.contains("/login"));
    let (err, bad) = call(&s, "generate_code", json!({ "id": 2, "language": "cobol" })).await;
    assert!(err && bad.as_str().unwrap().contains("unknown language"));

    let (_, har) = call(&s, "export_har", json!({ "filter": "host:cdn" })).await;
    assert!(har.get("log").is_some() && har["log"]["entries"].as_array().unwrap().len() == 1);
}

#[tokio::test]
async fn resources_expose_each_flow() {
    let (s, _, _) = server(false);
    let list = rpc(&s, "resources/list", json!({})).await;
    assert_eq!(list["result"]["resources"].as_array().unwrap().len(), 3);
    let read = rpc(&s, "resources/read", json!({ "uri": "flow://2" })).await;
    assert!(read["result"]["contents"][0]["text"].as_str().unwrap().contains("/login"));
    assert!(rpc(&s, "resources/read", json!({ "uri": "flow://999" })).await.get("error").is_some());
}

#[tokio::test]
async fn changes_need_permission() {
    let (s, store, rules) = server(false);
    let (err, msg) = call(&s, "clear_flows", json!({})).await;
    assert!(err && msg.as_str().unwrap().contains("Allow changes"));
    assert_eq!(store.len(), 3);
    let (err, _) = call(&s, "add_rule", json!({ "kind": "block", "rule": {} })).await;
    assert!(err);
    assert!(rules.get().block.is_empty());
}

#[tokio::test]
async fn adds_and_removes_rules_when_allowed() {
    let (s, store, rules) = server(true);
    let rule = json!({ "name": "no ads", "matcher": { "pattern": "ads.example.com" }, "action": { "type": "status", "code": 403 } });
    let (err, msg) = call(&s, "add_rule", json!({ "kind": "block", "rule": rule })).await;
    assert!(!err, "{msg}");
    assert_eq!(rules.get().block.len(), 1);
    assert_eq!(rules.get().block[0].name, "no ads");

    let (err, msg) =
        call(&s, "add_rule", json!({ "kind": "block", "rule": { "action": { "type": "nonsense" } } })).await;
    assert!(err && msg.as_str().unwrap().contains("invalid rule"));
    assert_eq!(rules.get().block.len(), 1, "an invalid rule changes nothing");
    let (err, _) = call(&s, "add_rule", json!({ "kind": "throttle", "rule": {} })).await;
    assert!(err, "only the listed rule kinds can be added");

    let (err, _) = call(&s, "remove_rule", json!({ "kind": "block", "index": 5 })).await;
    assert!(err);
    let (err, _) = call(&s, "remove_rule", json!({ "kind": "block", "index": 0 })).await;
    assert!(!err);
    assert!(rules.get().block.is_empty());

    let (err, _) = call(&s, "clear_flows", json!({})).await;
    assert!(!err);
    assert!(store.is_empty());
}

#[tokio::test]
async fn stdio_round_trip() {
    let (s, _, _) = server(false);
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        "not json\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
    );
    let mut out = Vec::new();
    proxyzip_mcp::stdio::run_with(Arc::new(s), input.as_bytes(), &mut out).await.unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3, "the notification gets no reply");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert!(lines[2]["result"]["tools"].is_array());
}

#[tokio::test]
async fn http_requires_the_token_and_a_local_origin() {
    use proxyzip_mcp::discovery::Endpoint;
    let (s, _, _) = server(false);
    let token = "secret-token-secret-token-secret-0";
    let handle = proxyzip_mcp::http::serve("127.0.0.1:0".parse().unwrap(), Arc::new(s), token.into()).await.unwrap();
    let url = handle.url();
    let good = Endpoint { url: url.clone(), token: token.into() };
    let bad = Endpoint { url: url.clone(), token: "wrong".into() };
    let body = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get_status","arguments":{}}}"#;

    let (status, text) = proxyzip_mcp::bridge::post(&good, body).await.unwrap();
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["id"], 7);
    assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("\"flows\":3"));

    assert_eq!(proxyzip_mcp::bridge::post(&bad, body).await.unwrap().0, 401);
    let (status, _) =
        proxyzip_mcp::bridge::post(&good, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await.unwrap();
    assert_eq!(status, 202);

    // A request that carries a web page's Origin is refused even with the right token.
    let addr = handle.addr;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://evil.example\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    assert!(raw.starts_with("HTTP/1.1 403"), "{raw}");

    let other = proxyzip_mcp::bridge::post(&Endpoint { url: url.replace("/mcp", "/other"), token: token.into() }, body)
        .await
        .unwrap();
    assert_eq!(other.0, 404);
    assert!(
        proxyzip_mcp::http::serve("0.0.0.0:0".parse().unwrap(), Arc::new(server(false).0), "t".into()).await.is_err(),
        "non-loopback is refused"
    );
    handle.shutdown();
}
