use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proxyzip_core::LogLine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use crate::convert::{ScriptRequest, ScriptResponse};
use crate::worker::{Report, RunSpec, Worker};

/// Resource limits for script execution.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Worker threads, each with its own JavaScript runtime.
    pub threads: usize,
    /// Wall-clock time one `onRequest`/`onResponse` call may take.
    pub timeout: Duration,
    /// Memory one runtime may use.
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { threads: 2, timeout: Duration::from_secs(2), memory_bytes: 64 * 1024 * 1024 }
    }
}

/// What a script decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict<T> {
    /// No function for this phase, or it left the message as it was.
    Unchanged,
    Changed(T),
    /// `onRequest` returned `null`: do not send the request.
    Block,
}

/// Functions a plugin can export besides the traffic hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEntry {
    /// `decode(body, info)`: turn a body into `{ format, json }`.
    Decode,
    /// `generate(request)`: turn a request into code.
    Generate,
}

/// The outcome of running a script, plus whatever it printed.
#[derive(Debug)]
pub struct RunOutput<T> {
    /// `Err` carries a message for the user; traffic should continue without the script's edit.
    pub verdict: Result<Verdict<T>, String>,
    pub logs: Vec<LogLine>,
}

struct Job {
    spec: RunSpec,
    reply: oneshot::Sender<Report>,
}

/// Runs scripts on a small pool of worker threads. Cheap to share behind an `Arc`.
pub struct ScriptEngine {
    sender: mpsc::Sender<Job>,
    /// `ctx.shared` per script, kept across runs. The last run to finish wins when scripts run concurrently.
    shared: Mutex<HashMap<String, String>>,
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl ScriptEngine {
    pub fn new(limits: Limits) -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..limits.threads.max(1) {
            let receiver = receiver.clone();
            let spawned = std::thread::Builder::new().name(format!("proxyzip-script-{index}")).spawn(move || {
                let mut worker: Option<Worker> = None;
                loop {
                    let job = match receiver.lock().map(|r| r.recv()) {
                        Ok(Ok(job)) => job,
                        // The engine was dropped.
                        _ => return,
                    };
                    if worker.is_none() {
                        match Worker::new(limits.memory_bytes) {
                            Ok(w) => worker = Some(w),
                            Err(e) => {
                                let report = Report {
                                    result: Err(format!("could not start the script runtime: {e}")),
                                    logs: Vec::new(),
                                    poisoned: false,
                                };
                                let _ = job.reply.send(report);
                                continue;
                            }
                        }
                    }
                    let Some(active) = worker.as_mut() else { continue };
                    let report = active.run(&job.spec, limits.timeout);
                    if report.poisoned {
                        // Start from a clean runtime after a timeout or an out-of-memory error.
                        worker = None;
                    }
                    let _ = job.reply.send(report);
                }
            });
            if let Err(e) = spawned {
                tracing::error!(error = %e, "could not start a script worker");
            }
        }
        Self { sender, shared: Mutex::new(HashMap::new()) }
    }

    fn script_id(source: &str) -> String {
        Sha256::digest(source.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    async fn run(
        &self,
        source: &str,
        phase: &'static str,
        request: Value,
        response: Option<Value>,
    ) -> (Result<Value, String>, Vec<LogLine>) {
        let id = Self::script_id(source);
        let shared_json = self.shared.lock().ok().and_then(|m| m.get(&id).cloned()).unwrap_or_else(|| "{}".into());
        let (reply, answer) = oneshot::channel();
        let spec = RunSpec {
            id: id.clone(),
            source: source.to_string(),
            phase,
            shared_json,
            request_json: request.to_string(),
            response_json: response.map(|r| r.to_string()).unwrap_or_default(),
        };
        let job = Job { spec, reply };
        if self.sender.send(job).is_err() {
            return (Err("the script engine is not running".into()), Vec::new());
        }
        let Ok(report) = answer.await else {
            return (Err("the script worker stopped unexpectedly".into()), Vec::new());
        };
        let value = report
            .result
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| format!("invalid script result: {e}")));
        if let Ok(v) = &value
            && let Some(shared) = v.get("shared")
            && let Ok(mut map) = self.shared.lock()
        {
            map.insert(id, shared.to_string());
        }
        (value, report.logs)
    }

    /// Call a plugin entry point: `decode(body, info)` or `generate(request)`.
    ///
    /// The result is `None` when the script has no such function or returned `null`.
    pub async fn call(&self, source: &str, entry: PluginEntry, first: Value, second: Value) -> RunOutput<Value> {
        let phase = match entry {
            PluginEntry::Decode => "decode",
            PluginEntry::Generate => "generate",
        };
        let (value, logs) = self.run(source, phase, first, Some(second)).await;
        let verdict = value.map(|v| {
            if v.get("skipped").and_then(Value::as_bool) == Some(true) {
                return Verdict::Unchanged;
            }
            match v.get("result") {
                Some(r) if !r.is_null() => Verdict::Changed(r.clone()),
                _ => Verdict::Unchanged,
            }
        });
        RunOutput { verdict, logs }
    }

    /// Run the script's `onRequest(ctx, request)`.
    pub async fn run_request(&self, source: &str, request: &ScriptRequest) -> RunOutput<ScriptRequest> {
        let (value, logs) = self.run(source, "request", request.to_json(), None).await;
        let verdict = value.and_then(|v| {
            if v.get("skipped").and_then(Value::as_bool) == Some(true) {
                return Ok(Verdict::Unchanged);
            }
            if v.get("blocked").and_then(Value::as_bool) == Some(true) {
                return Ok(Verdict::Block);
            }
            let result = v
                .get("result")
                .filter(|r| r.is_object())
                .ok_or("onRequest must return the request, null to block it, or nothing")?;
            let edited = ScriptRequest::from_json(result, request)?;
            Ok(if &edited == request { Verdict::Unchanged } else { Verdict::Changed(edited) })
        });
        RunOutput { verdict, logs }
    }

    /// Run the script's `onResponse(ctx, request, response)`.
    pub async fn run_response(
        &self,
        source: &str,
        request: &ScriptRequest,
        response: &ScriptResponse,
    ) -> RunOutput<ScriptResponse> {
        let (value, logs) = self.run(source, "response", request.to_json(), Some(response.to_json())).await;
        let verdict = value.and_then(|v| {
            if v.get("skipped").and_then(Value::as_bool) == Some(true) {
                return Ok(Verdict::Unchanged);
            }
            // Returning null (or a non-object) from onResponse leaves the response alone.
            let Some(result) = v.get("result").filter(|r| r.is_object()) else {
                return Ok(Verdict::Unchanged);
            };
            let edited = ScriptResponse::from_json(result, response)?;
            Ok(if &edited == response { Verdict::Unchanged } else { Verdict::Changed(edited) })
        });
        RunOutput { verdict, logs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ScriptRequest {
        ScriptRequest {
            method: "POST".into(),
            url: "https://api.example.com/v1/items?id=2".into(),
            headers: vec![("content-type".into(), "application/json".into()), ("accept".into(), "*/*".into())],
            body: br#"{"name":"ada"}"#.to_vec(),
        }
    }

    fn response() -> ScriptResponse {
        ScriptResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: br#"{"ok":true,"n":1}"#.to_vec(),
        }
    }

    fn fast() -> ScriptEngine {
        ScriptEngine::new(Limits { threads: 2, timeout: Duration::from_millis(400), memory_bytes: 16 * 1024 * 1024 })
    }

    #[tokio::test]
    async fn on_request_can_edit_headers_url_method_and_body() {
        let src = r#"
            function onRequest(ctx, request) {
              request.headers["x-env"] = "dev";
              delete request.headers.accept;
              request.method = "put";
              request.url = request.url.replace("v1", "v2");
              request.body = JSON.stringify({ ...JSON.parse(request.body), added: true });
              return request;
            }"#;
        let out = fast().run_request(src, &request()).await;
        let Ok(Verdict::Changed(r)) = out.verdict else { panic!("{:?}", out.verdict) };
        assert_eq!(r.method, "PUT");
        assert_eq!(r.url, "https://api.example.com/v2/items?id=2");
        assert!(r.headers.contains(&("x-env".into(), "dev".into())));
        assert!(!r.headers.iter().any(|(k, _)| k == "accept"));
        assert_eq!(r.body, br#"{"name":"ada","added":true}"#);
    }

    #[tokio::test]
    async fn returning_nothing_uses_the_mutated_object_and_untouched_means_unchanged() {
        let engine = fast();
        let touched = engine.run_request("function onRequest(ctx, r) { r.headers.a = '1'; }", &request()).await;
        assert!(matches!(touched.verdict, Ok(Verdict::Changed(_))));
        let untouched = engine.run_request("function onRequest(ctx, r) { return r; }", &request()).await;
        assert_eq!(untouched.verdict.unwrap(), Verdict::Unchanged);
        let no_function = engine.run_request("var x = 1;", &request()).await;
        assert_eq!(no_function.verdict.unwrap(), Verdict::Unchanged);
        let wrong_phase = engine.run_request("function onResponse() {}", &request()).await;
        assert_eq!(wrong_phase.verdict.unwrap(), Verdict::Unchanged);
    }

    #[tokio::test]
    async fn null_blocks_the_request() {
        let out = fast()
            .run_request("function onRequest(ctx, r) { return r.url.includes('items') ? null : r; }", &request())
            .await;
        assert_eq!(out.verdict.unwrap(), Verdict::Block);
    }

    #[tokio::test]
    async fn on_response_rewrites_status_headers_and_body() {
        let src = r#"
            function onResponse(ctx, request, response) {
              const data = JSON.parse(response.body);
              data.ok = false;
              response.body = JSON.stringify(data);
              response.status = 503;
              response.headers["x-seen-url"] = request.url;
              return response;
            }"#;
        let out = fast().run_response(src, &request(), &response()).await;
        let Ok(Verdict::Changed(r)) = out.verdict else { panic!("{:?}", out.verdict) };
        assert_eq!(r.status, 503);
        assert_eq!(r.body, br#"{"ok":false,"n":1}"#);
        assert!(r.headers.iter().any(|(k, v)| k == "x-seen-url" && v.contains("/v1/items")));
    }

    #[tokio::test]
    async fn null_from_on_response_leaves_the_response_alone() {
        let out = fast().run_response("function onResponse() { return null; }", &request(), &response()).await;
        assert_eq!(out.verdict.unwrap(), Verdict::Unchanged);
    }

    #[tokio::test]
    async fn binary_bodies_can_be_edited_through_base64() {
        let binary = ScriptResponse { status: 200, headers: vec![], body: vec![0xff, 0xfe, 0x00] };
        let src = "function onResponse(ctx, req, res) { if (res.body !== null) throw new Error('expected null text'); res.bodyBase64 = $.base64.encode('AB'); }";
        let out = fast().run_response(src, &request(), &binary).await;
        let Ok(Verdict::Changed(r)) = out.verdict else { panic!("{:?}", out.verdict) };
        assert_eq!(r.body, b"AB".to_vec());
    }

    #[tokio::test]
    async fn console_output_is_captured() {
        let src = "function onRequest(ctx, r) { console.log('hello', {a: 1}, [2]); console.warn('careful'); console.error(new Error('boom').message); }";
        let out = fast().run_request(src, &request()).await;
        assert_eq!(out.verdict.unwrap(), Verdict::Unchanged);
        let lines: Vec<_> = out.logs.iter().map(|l| (l.level.as_str(), l.message.as_str())).collect();
        assert_eq!(lines, vec![("log", r#"hello {"a":1} [2]"#), ("warn", "careful"), ("error", "boom")]);
    }

    #[tokio::test]
    async fn shared_state_persists_between_runs() {
        let engine = fast();
        let src = "function onRequest(ctx, r) { ctx.shared.count = (ctx.shared.count || 0) + 1; r.headers['x-count'] = String(ctx.shared.count); }";
        for expected in ["1", "2", "3"] {
            let out = engine.run_request(src, &request()).await;
            let Ok(Verdict::Changed(r)) = out.verdict else { panic!() };
            assert!(r.headers.contains(&("x-count".into(), expected.into())), "run {expected}");
        }
        // Another script has its own state.
        let other = engine.run_request(&format!("{src} // different"), &request()).await;
        let Ok(Verdict::Changed(r)) = other.verdict else { panic!() };
        assert!(r.headers.contains(&("x-count".into(), "1".into())));
    }

    #[tokio::test]
    async fn helpers_match_known_values() {
        let src = r#"
            function onRequest(ctx, r) {
              r.headers.md5 = $.md5("abc");
              r.headers.sha1 = $.sha1("abc");
              r.headers.sha256 = $.sha256("abc");
              r.headers.hmac = $.hmacSha256("key", "The quick brown fox jumps over the lazy dog");
              r.headers.b64 = $.base64.encode("héllo");
              r.headers.unb64 = $.base64.decode("aMOpbGxv");
              r.headers.uuid = $.uuid();
              r.headers.jwt = JSON.stringify($.jwt.decode("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMiLCJuYW1lIjoiQWRhIn0.c2ln"));
              r.headers.json = $.json.stringify($.json.parse('{"a":[1,2]}'));
            }"#;
        let out = fast().run_request(src, &request()).await;
        let Ok(Verdict::Changed(r)) = out.verdict else { panic!("{:?}", out.verdict) };
        let get = |name: &str| r.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("md5"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(get("sha1"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(get("sha256"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(get("hmac"), "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
        assert_eq!(get("b64"), "aMOpbGxv");
        assert_eq!(get("unb64"), "héllo");
        assert_eq!(get("uuid").len(), 36);
        assert_eq!(get("jwt"), r#"{"header":{"alg":"HS256"},"payload":{"sub":"123","name":"Ada"},"signature":"c2ln"}"#);
        assert_eq!(get("json"), r#"{"a":[1,2]}"#);
    }

    #[tokio::test]
    async fn errors_are_reported_without_panicking() {
        let engine = fast();
        let syntax = engine.run_request("function onRequest( {", &request()).await;
        assert!(syntax.verdict.is_err(), "{:?}", syntax.verdict);
        let thrown = engine.run_request("function onRequest() { throw new Error('nope'); }", &request()).await;
        assert!(thrown.verdict.unwrap_err().contains("nope"));
        let reference = engine.run_request("function onRequest() { return missing.thing; }", &request()).await;
        assert!(reference.verdict.unwrap_err().contains("missing"));
        let wrong = engine.run_request("function onRequest() { return 42; }", &request()).await;
        assert!(wrong.verdict.unwrap_err().contains("must return"));
        let bad_status =
            engine.run_response("function onResponse(c, q, r) { r.status = 42; }", &request(), &response()).await;
        assert!(bad_status.verdict.unwrap_err().contains("status"));
        // A broken script does not poison the engine.
        let ok = engine.run_request("function onRequest(c, r) { r.headers.a = '1'; }", &request()).await;
        assert!(matches!(ok.verdict, Ok(Verdict::Changed(_))));
    }

    #[tokio::test]
    async fn async_functions_are_rejected_clearly() {
        let out = fast().run_request("async function onRequest(ctx, r) { return r; }", &request()).await;
        assert!(out.verdict.unwrap_err().contains("async"));
    }

    #[tokio::test]
    async fn infinite_loops_are_stopped_and_the_worker_recovers() {
        let engine = ScriptEngine::new(Limits {
            threads: 1,
            timeout: Duration::from_millis(250),
            memory_bytes: 16 * 1024 * 1024,
        });
        let started = std::time::Instant::now();
        let out = engine.run_request("function onRequest() { while (true) {} }", &request()).await;
        assert!(out.verdict.unwrap_err().contains("stopped"));
        assert!(started.elapsed() < Duration::from_secs(5));
        // The same single worker keeps working afterwards.
        let ok = engine.run_request("function onRequest(c, r) { r.headers.a = '1'; }", &request()).await;
        assert!(matches!(ok.verdict, Ok(Verdict::Changed(_))));
    }

    #[tokio::test]
    async fn runaway_memory_is_contained() {
        let engine =
            ScriptEngine::new(Limits { threads: 1, timeout: Duration::from_secs(5), memory_bytes: 8 * 1024 * 1024 });
        let out = engine
            .run_request("function onRequest() { const a = []; while (true) a.push('x'.repeat(100000)); }", &request())
            .await;
        assert!(out.verdict.is_err(), "{:?}", out.verdict);
        let ok = engine.run_request("function onRequest(c, r) { r.headers.a = '1'; }", &request()).await;
        assert!(matches!(ok.verdict, Ok(Verdict::Changed(_))), "{:?}", ok.verdict);
    }

    #[tokio::test]
    async fn scripts_cannot_reach_the_file_system_or_network() {
        let engine = fast();
        for probe in
            ["require('fs')", "process.env", "fetch('http://127.0.0.1')", "XMLHttpRequest", "std.open", "os.readdir"]
        {
            let src = format!("function onRequest() {{ {probe}; }}");
            assert!(engine.run_request(&src, &request()).await.verdict.is_err(), "{probe} should not be available");
        }
    }

    #[tokio::test]
    async fn concurrent_runs_do_not_interfere() {
        let engine = Arc::new(fast());
        let mut tasks = Vec::new();
        for i in 0..16 {
            let engine = engine.clone();
            tasks.push(tokio::spawn(async move {
                let src = format!("function onRequest(c, r) {{ r.headers.n = '{i}'; }}");
                let Ok(Verdict::Changed(r)) = engine.run_request(&src, &request()).await.verdict else { panic!() };
                assert!(r.headers.contains(&("n".into(), i.to_string())));
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
    }

    #[tokio::test]
    async fn plugin_entry_points_decode_and_generate() {
        use serde_json::json;
        let engine = fast();
        let src = "function decode(body, info) { return { format: 'demo', json: { len: body.text.length, type: info.contentType } }; }\n\
                   function generate(request) { return 'GET ' + request.url; }";
        let out = engine
            .call(
                src,
                PluginEntry::Decode,
                json!({ "text": "hello", "base64": "aGVsbG8=" }),
                json!({ "contentType": "x/y" }),
            )
            .await;
        let Ok(Verdict::Changed(v)) = out.verdict else { panic!("{:?}", out.verdict) };
        assert_eq!(v["json"]["len"], 5);
        assert_eq!(v["json"]["type"], "x/y");

        let out = engine.call(src, PluginEntry::Generate, json!({ "url": "http://a/b" }), json!({})).await;
        assert!(matches!(out.verdict, Ok(Verdict::Changed(v)) if v == json!("GET http://a/b")));
    }

    #[tokio::test]
    async fn plugin_entry_points_that_are_missing_or_null_change_nothing() {
        use serde_json::json;
        let engine = fast();
        let none = engine.call("function onRequest() {}", PluginEntry::Decode, json!({}), json!({})).await;
        assert!(matches!(none.verdict, Ok(Verdict::Unchanged)));
        let null = engine.call("function decode() { return null; }", PluginEntry::Decode, json!({}), json!({})).await;
        assert!(matches!(null.verdict, Ok(Verdict::Unchanged)));
        let boom = engine
            .call("function generate() { throw new Error('nope'); }", PluginEntry::Generate, json!({}), json!({}))
            .await;
        assert!(boom.verdict.unwrap_err().contains("nope"));
    }
}
