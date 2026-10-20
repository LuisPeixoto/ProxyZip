use proxyzip_codec::{BodyKind, BodyView};
use proxyzip_core::{Filter, Flow, FlowId, FlowSummary, Headers};
use proxyzip_rules::RuleSet;
use serde_json::{Value, json};

use crate::McpState;

const DEFAULT_LIST: usize = 50;
const MAX_LIST: usize = 500;
const DEFAULT_BODY_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 512 * 1024;
const CHANGES_DISABLED: &str = "Changes are turned off. Enable \"Allow changes\" under Settings > AI assistants in ProxyZip (or start the CLI with --mcp-allow-changes) to let assistants edit rules or clear traffic.";
/// Rule lists an assistant may add to, with the toggle that enables each.
const RULE_KINDS: [&str; 6] = ["block", "map_local", "map_remote", "rewrite", "scripts", "dns"];

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": !read_only, "openWorldHint": false },
    })
}

pub fn definitions(state: &McpState) -> Vec<Value> {
    let filter = json!({ "type": "string", "description": "Filter in the app's syntax, e.g. `host:api status:5xx method:post -analytics`, `type:json`, `gql:GetUser`, `/regex/`" });
    let id = json!({ "type": "integer", "description": "Flow id from list_flows" });
    let mut tools = vec![
        tool(
            "get_status",
            "Whether ProxyZip is capturing, its address and how many requests it holds.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "list_flows",
            "List captured requests, newest first. Returns id, method, url, status, type, size, duration, tags and GraphQL operation.",
            json!({
                "filter": filter,
                "limit": { "type": "integer", "description": "Default 50, maximum 500" },
                "offset": { "type": "integer", "description": "Skip this many matches (for paging)" },
                "order": { "type": "string", "enum": ["newest", "oldest"] },
            }),
            &[],
            true,
        ),
        tool(
            "get_flow",
            "Everything about one request: headers, decoded bodies, timing, TLS, tags and script logs.",
            json!({
                "id": id,
                "include_bodies": { "type": "boolean", "description": "Default true" },
                "max_body_bytes": { "type": "integer", "description": "Truncate each body text to this many bytes (default 16384)" },
                "redact": { "type": "boolean", "description": "Replace tokens, cookies and passwords with <redacted>. Default true; set false only when the user asks to see them" },
            }),
            &["id"],
            true,
        ),
        tool(
            "search_bodies",
            "Find requests whose request or response body contains some text (case-insensitive).",
            json!({
                "text": { "type": "string" },
                "in": { "type": "string", "enum": ["request", "response", "both"], "description": "Default both" },
                "redact": { "type": "boolean", "description": "Search the redacted text (default true)" },
                "filter": filter,
                "limit": { "type": "integer", "description": "Default 20" },
            }),
            &["text"],
            true,
        ),
        tool(
            "query_body",
            "Run a jq expression on a JSON request or response body and return the results.",
            json!({
                "id": id,
                "part": { "type": "string", "enum": ["request", "response"] },
                "expr": { "type": "string", "description": "jq filter, e.g. `.items[] | {id, name}`" },
            }),
            &["id", "part", "expr"],
            true,
        ),
        tool(
            "generate_code",
            "Generate code that sends a captured request. Languages: curl, httpie, powershell, python, javascript, node-axios, go, rust, java, kotlin, swift, csharp, php, ruby.",
            json!({ "id": id, "language": { "type": "string" } }),
            &["id", "language"],
            true,
        ),
        tool(
            "export_har",
            "Export requests as HAR 1.2 JSON (at most 200).",
            json!({ "filter": filter, "ids": { "type": "array", "items": { "type": "integer" } }, "redact": { "type": "boolean", "description": "Default true" } }),
            &[],
            true,
        ),
        tool(
            "list_rules",
            "The current rules: block, map local/remote, rewrite, breakpoints, scripts, DNS and more.",
            json!({}),
            &[],
            true,
        ),
    ];
    if state.rules.is_some() {
        tools.push(tool(
            "add_rule",
            "Add a rule (needs Allow changes). `kind` is one of block, map_local, map_remote, rewrite, scripts, dns. `rule` has the shape of an entry in rules.json; unspecified fields use defaults. The matcher is {\"pattern\": \"host or URL text\", \"regex\": false, \"methods\": []}. Examples: block {\"name\":\"no ads\",\"matcher\":{\"pattern\":\"ads.example.com\"},\"action\":{\"type\":\"status\",\"code\":403}}; map_remote {\"matcher\":{\"pattern\":\"api.example.com\"},\"scheme\":\"http\",\"host\":\"localhost\",\"port\":3000}; scripts {\"name\":\"tag\",\"matcher\":{\"pattern\":\"example.com\"},\"on_request\":true,\"source\":\"function onRequest(ctx, request){ request.headers['x-test']='1'; return request; }\"}.",
            json!({ "kind": { "type": "string", "enum": RULE_KINDS }, "rule": { "type": "object" } }),
            &["kind", "rule"],
            false,
        ));
        tools.push(tool(
            "remove_rule",
            "Remove a rule by its index in list_rules (needs Allow changes).",
            json!({ "kind": { "type": "string", "enum": RULE_KINDS }, "index": { "type": "integer" } }),
            &["kind", "index"],
            false,
        ));
    }
    tools.push(tool("clear_flows", "Delete all captured requests (needs Allow changes).", json!({}), &[], false));
    tools
}

fn ok(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

fn fail(text: impl Into<String>) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text.into() }] })
}

fn json_text(v: &Value) -> Value {
    ok(serde_json::to_string(v).unwrap_or_default())
}

fn usize_arg(args: &Value, key: &str, default: usize, max: usize) -> usize {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize).unwrap_or(default).min(max)
}

fn flow_id(args: &Value) -> Result<FlowId, String> {
    args.get("id").and_then(Value::as_u64).ok_or_else(|| "missing required integer argument `id`".to_string())
}

pub async fn call(state: &McpState, name: &str, args: &Value) -> Value {
    match call_inner(state, name, args).await {
        Ok(v) => v,
        Err(e) => fail(e),
    }
}

async fn call_inner(state: &McpState, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "get_status" => {
            let mut s = state.status();
            if let Some(o) = s.as_object_mut() {
                o.insert("flows".into(), json!(state.store.len()));
                o.insert("changes_allowed".into(), json!(state.changes_allowed()));
            }
            Ok(json_text(&s))
        }
        "list_flows" => {
            let matched = matching(state, args.get("filter").and_then(Value::as_str).unwrap_or(""));
            let total = matched.len();
            let limit = usize_arg(args, "limit", DEFAULT_LIST, MAX_LIST);
            let offset = usize_arg(args, "offset", 0, usize::MAX);
            let oldest = args.get("order").and_then(Value::as_str) == Some("oldest");
            let page: Vec<Value> = if oldest {
                matched.iter().skip(offset).take(limit).map(compact).collect()
            } else {
                matched.iter().rev().skip(offset).take(limit).map(compact).collect()
            };
            Ok(json_text(&json!({ "total": total, "offset": offset, "returned": page.len(), "flows": page })))
        }
        "get_flow" => {
            let id = flow_id(args)?;
            let mut flow = state.store.get(id).ok_or_else(|| format!("no request with id {id}"))?;
            if redact_requested(args) {
                proxyzip_export::redact::redact_flow(&mut flow, &Default::default());
            }
            let bodies = args.get("include_bodies").and_then(Value::as_bool).unwrap_or(true);
            let max = usize_arg(args, "max_body_bytes", DEFAULT_BODY_BYTES, MAX_BODY_BYTES);
            Ok(json_text(&flow_detail(&flow, bodies, max)))
        }
        "search_bodies" => {
            let needle = args
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .ok_or("missing `text`")?
                .to_lowercase();
            let scope = args.get("in").and_then(Value::as_str).unwrap_or("both");
            let limit = usize_arg(args, "limit", 20, 200);
            let mut hits = Vec::new();
            for summary in
                matching(state, args.get("filter").and_then(Value::as_str).unwrap_or("")).iter().rev().take(5000)
            {
                let Some(mut flow) = state.store.get(summary.id) else { continue };
                if redact_requested(args) {
                    proxyzip_export::redact::redact_flow(&mut flow, &Default::default());
                }
                let mut found = Vec::new();
                if scope != "response"
                    && let Some(s) = snippet(&flow.request.headers, &flow.request.body, &needle)
                {
                    found.push(json!({ "part": "request", "snippet": s }));
                }
                if scope != "request"
                    && let Some(r) = &flow.response
                    && let Some(s) = snippet(&r.headers, &r.body, &needle)
                {
                    found.push(json!({ "part": "response", "snippet": s }));
                }
                if !found.is_empty() {
                    let mut c = compact(summary);
                    c["matches"] = Value::Array(found);
                    hits.push(c);
                    if hits.len() >= limit {
                        break;
                    }
                }
            }
            Ok(json_text(&json!({ "returned": hits.len(), "flows": hits })))
        }
        "query_body" => {
            let id = flow_id(args)?;
            let mut flow = state.store.get(id).ok_or_else(|| format!("no request with id {id}"))?;
            if redact_requested(args) {
                proxyzip_export::redact::redact_flow(&mut flow, &Default::default());
            }
            let expr = args.get("expr").and_then(Value::as_str).ok_or("missing `expr`")?;
            let (headers, body) = match args.get("part").and_then(Value::as_str) {
                Some("request") => (&flow.request.headers, &flow.request.body),
                Some("response") => {
                    let r = flow.response.as_ref().ok_or("this request has no response yet")?;
                    (&r.headers, &r.body)
                }
                _ => return Err("`part` must be \"request\" or \"response\"".into()),
            };
            let (decoded, _, _) = proxyzip_codec::decode_content(headers, body);
            let values = proxyzip_codec::query::run(&decoded, expr)?;
            Ok(json_text(&json!({ "count": values.len(), "results": values })))
        }
        "generate_code" => {
            let id = flow_id(args)?;
            let flow = state.store.get(id).ok_or_else(|| format!("no request with id {id}"))?;
            let lang = args.get("language").and_then(Value::as_str).unwrap_or("curl");
            let language = proxyzip_export::codegen::Language::from_id(lang).ok_or_else(|| {
                let ids: Vec<_> = proxyzip_export::codegen::Language::ALL.iter().map(|l| l.id()).collect();
                format!("unknown language `{lang}`; use one of: {}", ids.join(", "))
            })?;
            Ok(ok(proxyzip_export::codegen::generate(&flow, language)))
        }
        "export_har" => {
            let flows: Vec<Flow> = match args.get("ids").and_then(Value::as_array) {
                Some(ids) => {
                    let ids: Vec<FlowId> = ids.iter().filter_map(Value::as_u64).take(200).collect();
                    state.store.get_many(&ids)
                }
                None => {
                    let ids: Vec<FlowId> = matching(state, args.get("filter").and_then(Value::as_str).unwrap_or(""))
                        .iter()
                        .rev()
                        .take(200)
                        .map(|s| s.id)
                        .collect();
                    state.store.get_many(&ids)
                }
            };
            let flows = if redact_requested(args) {
                proxyzip_export::redact::redacted(&flows, &Default::default())
            } else {
                flows
            };
            Ok(ok(proxyzip_export::har::export_json(&flows).map_err(|e| e.to_string())?))
        }
        "list_rules" => {
            let rules = state.rules.as_ref().map(|r| r.get()).ok_or("this server has no rules (read-only session)")?;
            Ok(json_text(&serde_json::to_value(&rules).map_err(|e| e.to_string())?))
        }
        "add_rule" => {
            require_changes(state)?;
            let backend = state.rules.as_ref().ok_or("this server has no rules")?;
            let kind = rule_kind(args)?;
            let rule = args.get("rule").filter(|r| r.is_object()).ok_or("`rule` must be an object")?.clone();
            let mut doc = serde_json::to_value(backend.get()).map_err(|e| e.to_string())?;
            doc.get_mut(kind).and_then(Value::as_array_mut).ok_or("unknown rule list")?.push(rule);
            if let Some(toggle) = doc.get_mut("tools").and_then(|t| t.get_mut(kind)) {
                *toggle = Value::Bool(true);
            }
            let next: RuleSet = serde_json::from_value(doc).map_err(|e| format!("invalid rule: {e}"))?;
            let index = match kind {
                "block" => next.block.len(),
                "map_local" => next.map_local.len(),
                "map_remote" => next.map_remote.len(),
                "rewrite" => next.rewrite.len(),
                "scripts" => next.scripts.len(),
                _ => next.dns.len(),
            } - 1;
            backend.set(next).await?;
            Ok(ok(format!("Added {kind} rule #{index}. It applies to new requests.")))
        }
        "remove_rule" => {
            require_changes(state)?;
            let backend = state.rules.as_ref().ok_or("this server has no rules")?;
            let kind = rule_kind(args)?;
            let index = args.get("index").and_then(Value::as_u64).ok_or("missing `index`")? as usize;
            let mut doc = serde_json::to_value(backend.get()).map_err(|e| e.to_string())?;
            let list = doc.get_mut(kind).and_then(Value::as_array_mut).ok_or("unknown rule list")?;
            if index >= list.len() {
                return Err(format!("there is no {kind} rule #{index} (the list has {})", list.len()));
            }
            list.remove(index);
            let next: RuleSet = serde_json::from_value(doc).map_err(|e| e.to_string())?;
            backend.set(next).await?;
            Ok(ok(format!("Removed {kind} rule #{index}.")))
        }
        "clear_flows" => {
            require_changes(state)?;
            let n = state.store.len();
            state.store.clear();
            Ok(ok(format!("Deleted {n} requests.")))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// Assistants get redacted data unless they ask otherwise.
fn redact_requested(args: &Value) -> bool {
    args.get("redact").and_then(Value::as_bool).unwrap_or(true)
}

fn require_changes(state: &McpState) -> Result<(), String> {
    state.changes_allowed().then_some(()).ok_or_else(|| CHANGES_DISABLED.to_string())
}

fn rule_kind(args: &Value) -> Result<&'static str, String> {
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
    RULE_KINDS
        .iter()
        .find(|k| **k == kind)
        .copied()
        .ok_or_else(|| format!("`kind` must be one of: {}", RULE_KINDS.join(", ")))
}

fn matching(state: &McpState, filter: &str) -> Vec<FlowSummary> {
    let f = Filter::parse(filter);
    state.store.summaries().into_iter().filter(|s| f.matches(s)).collect()
}

fn compact(s: &FlowSummary) -> Value {
    let mut v = json!({
        "id": s.id,
        "method": s.method,
        "url": s.url,
        "status": s.status,
        "type": s.content_type,
        "state": s.state,
        "request_bytes": s.request_size,
        "response_bytes": s.response_size,
        "duration_ms": s.duration_ms,
        "time": s.created_at,
    });
    let o = v.as_object_mut().expect("object");
    if !s.tags.is_empty() {
        o.insert("tags".into(), json!(s.tags));
    }
    if let Some(g) = &s.graphql {
        o.insert("graphql".into(), json!(g));
    }
    if let Some(e) = &s.error {
        o.insert("error".into(), json!(e));
    }
    v
}

fn headers_json(h: &Headers) -> Value {
    Value::Array(h.iter().map(|(k, v)| json!([k, v])).collect())
}

fn body_json(headers: &Headers, body: &[u8], truncated: bool, max: usize) -> Value {
    let view = proxyzip_codec::view(headers, body, truncated);
    describe_view(&view, max)
}

fn describe_view(view: &BodyView, max: usize) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("kind".into(), json!(view.kind));
    o.insert("size".into(), json!(view.size));
    if let Some(m) = &view.mime {
        o.insert("mime".into(), json!(m));
    }
    if !view.encodings.is_empty() {
        o.insert("decoded_from".into(), json!(view.encodings));
    }
    if view.truncated {
        o.insert("capture_truncated".into(), json!(true));
    }
    if let Some(e) = &view.decode_error {
        o.insert("decode_error".into(), json!(e));
    }
    if let Some(s) = &view.structured {
        let text = serde_json::to_string(&s.json).unwrap_or_default();
        o.insert("format".into(), json!(s.format));
        let (text, cut) = cut(&text, max);
        o.insert("decoded".into(), json!(text));
        if cut {
            o.insert("text_truncated".into(), json!(true));
        }
    } else if let Some(t) = view.pretty.as_ref().or(view.text.as_ref()) {
        let (text, was_cut) = cut(t, max);
        o.insert("text".into(), json!(text));
        if was_cut {
            o.insert("text_truncated".into(), json!(true));
        }
    } else if !matches!(view.kind, BodyKind::Empty) {
        o.insert("binary".into(), json!(true));
        if let Some(b64) = &view.base64 {
            let head: String = b64.chars().take(256).collect();
            o.insert("base64_prefix".into(), json!(head));
        }
    }
    Value::Object(o)
}

/// Cut `s` to at most `max` bytes on a character boundary.
fn cut(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

pub fn flow_detail(flow: &Flow, bodies: bool, max: usize) -> Value {
    let mut o = serde_json::Map::new();
    let summary = flow.summary();
    o.insert("summary".into(), compact(&summary));
    o.insert("client".into(), json!(flow.client_addr));
    let mut req = json!({
        "http_version": flow.request.http_version,
        "headers": headers_json(&flow.request.headers),
    });
    if bodies {
        req["body"] = body_json(&flow.request.headers, &flow.request.body, flow.request.body_truncated, max);
    }
    o.insert("request".into(), req);
    if let Some(r) = &flow.response {
        let mut res = json!({
            "status": r.status,
            "reason": r.reason,
            "http_version": r.http_version,
            "headers": headers_json(&r.headers),
        });
        if bodies {
            res["body"] = body_json(&r.headers, &r.body, r.body_truncated, max);
        }
        o.insert("response".into(), res);
    }
    o.insert("timing".into(), serde_json::to_value(&flow.timing).unwrap_or_default());
    if let Some(t) = &flow.tls {
        o.insert("tls".into(), serde_json::to_value(t).unwrap_or_default());
    }
    if !flow.ws_messages.is_empty() {
        o.insert("websocket_messages".into(), json!(flow.ws_messages.len()));
    }
    if let Some(c) = &flow.comment {
        o.insert("comment".into(), json!(c));
    }
    if !flow.logs.is_empty() {
        o.insert("script_logs".into(), serde_json::to_value(&flow.logs).unwrap_or_default());
    }
    Value::Object(o)
}

/// A short excerpt around the first occurrence of `needle` (lowercase) in the decoded body.
fn snippet(headers: &Headers, body: &[u8], needle: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let (decoded, _, _) = proxyzip_codec::decode_content(headers, body);
    let text = String::from_utf8_lossy(&decoded);
    let lower = text.to_lowercase();
    let at = lower.find(needle)?;
    // `lower` can differ in length from `text` for exotic characters; fall back to a char-safe window.
    let start = text.char_indices().map(|(i, _)| i).rfind(|i| *i <= at.saturating_sub(60)).unwrap_or(0);
    let end_target = (at + needle.len() + 60).min(text.len());
    let end = text.char_indices().map(|(i, _)| i).chain([text.len()]).find(|i| *i >= end_target).unwrap_or(text.len());
    Some(text[start..end].replace(['\n', '\r'], " "))
}

pub fn list_resources(state: &McpState) -> Value {
    let resources: Vec<Value> = state
        .store
        .summaries()
        .iter()
        .rev()
        .take(100)
        .map(|s| json!({ "uri": format!("flow://{}", s.id), "name": format!("{} {}", s.method, s.url), "mimeType": "application/json" }))
        .collect();
    json!({ "resources": resources })
}

pub fn read_resource(state: &McpState, uri: &str) -> Result<Value, String> {
    let id: FlowId =
        uri.strip_prefix("flow://").and_then(|s| s.parse().ok()).ok_or_else(|| format!("unknown resource {uri}"))?;
    let mut flow = state.store.get(id).ok_or_else(|| format!("no request with id {id}"))?;
    proxyzip_export::redact::redact_flow(&mut flow, &Default::default());
    let text = serde_json::to_string_pretty(&flow_detail(&flow, true, DEFAULT_BODY_BYTES)).unwrap_or_default();
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }] }))
}
