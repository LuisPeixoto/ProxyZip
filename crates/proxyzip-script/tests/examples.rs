//! The example plugins in `examples/plugins` must keep working.

use std::path::PathBuf;

use proxyzip_script::plugin::Registry;
use proxyzip_script::{PluginEntry, ScriptEngine, ScriptRequest, Verdict};
use serde_json::json;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins")
}

fn installed() -> (tempfile::TempDir, Registry) {
    let tmp = tempfile::tempdir().unwrap();
    let mut reg = Registry::load(&tmp.path().join("plugins"));
    for name in ["jwt-viewer", "request-id"] {
        reg.install(&examples().join(name)).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    }
    (tmp, reg)
}

#[tokio::test]
async fn jwt_viewer_decodes_a_token_and_formats_markdown() {
    let (_tmp, reg) = installed();
    let engine = ScriptEngine::default();
    let (plugin, _) = reg.viewers_for("application/jwt").next().expect("the jwt viewer matches");
    // header {"alg":"HS256","typ":"JWT"}, payload {"sub":"ada","exp":1700000000}
    let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhZGEiLCJleHAiOjE3MDAwMDAwMDB9.sig";
    let out = engine.call(&plugin.source, PluginEntry::Decode, json!({ "text": token, "base64": "" }), json!({})).await;
    let Ok(Verdict::Changed(v)) = out.verdict else { panic!("{:?}", out.verdict) };
    assert_eq!(v["format"], "jwt");
    assert_eq!(v["json"]["payload"]["sub"], "ada");
    assert_eq!(v["json"]["expires"], "2023-11-14T22:13:20.000Z");

    let request = ScriptRequest {
        method: "GET".into(),
        url: "http://a/b".into(),
        headers: vec![("accept".into(), "*/*".into())],
        body: vec![],
    };
    let out = engine.call(&plugin.source, PluginEntry::Generate, request.to_json(), json!({})).await;
    let Ok(Verdict::Changed(text)) = out.verdict else { panic!() };
    assert!(text.as_str().unwrap().contains("GET http://a/b\naccept: */*"));
}

#[tokio::test]
async fn request_id_hook_adds_the_header_once() {
    let (_tmp, reg) = installed();
    let hook = reg.hook_specs().into_iter().find(|h| h.plugin_id == "request-id").expect("hook");
    let engine = ScriptEngine::default();
    let request = ScriptRequest { method: "GET".into(), url: "http://a/".into(), headers: vec![], body: vec![] };
    let Ok(Verdict::Changed(edited)) = engine.run_request(&hook.source, &request).await.verdict else { panic!() };
    let id = edited.headers.iter().find(|(k, _)| k == "x-request-id").expect("header added").1.clone();
    assert_eq!(id.len(), 36);

    let again = ScriptRequest { headers: vec![("X-Request-Id".into(), "keep".into())], ..request };
    assert!(matches!(engine.run_request(&hook.source, &again).await.verdict, Ok(Verdict::Unchanged)));
}
