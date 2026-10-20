//! The plugins in `plugins/` (the catalog) must keep working and the catalog must describe them exactly.

use std::path::PathBuf;

use proxyzip_script::plugin::Registry;
use proxyzip_script::{PluginEntry, ScriptEngine, ScriptRequest, Verdict};
use serde_json::json;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
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

#[tokio::test]
async fn base64_viewer_honours_its_setting() {
    let (_tmp, mut reg) = installed_all();
    let (plugin, _) = reg.viewers_for("application/x-base64").next().expect("the viewer matches");
    let engine = ScriptEngine::default();
    let encoded = "eyJvayI6dHJ1ZX0="; // {"ok":true}
    let out = engine
        .call(&plugin.runtime_source(), PluginEntry::Decode, json!({ "text": encoded, "base64": "" }), json!({}))
        .await;
    let Ok(Verdict::Changed(v)) = out.verdict else { panic!("{:?}", out.verdict) };
    assert_eq!(v["json"], json!({ "ok": true }));

    let mut values = serde_json::Map::new();
    values.insert("parse_json".into(), json!(false));
    reg.set_settings("base64-viewer", &values).unwrap();
    let (plugin, _) = reg.viewers_for("application/x-base64").next().unwrap();
    let out = engine
        .call(&plugin.runtime_source(), PluginEntry::Decode, json!({ "text": encoded, "base64": "" }), json!({}))
        .await;
    let Ok(Verdict::Changed(v)) = out.verdict else { panic!() };
    assert_eq!(v["json"]["text"], "{\"ok\":true}");
}

#[tokio::test]
async fn add_header_hook_follows_its_settings() {
    let (_tmp, mut reg) = installed_all();
    let engine = ScriptEngine::default();
    let request =
        ScriptRequest { method: "GET".into(), url: "https://api.example.com/x".into(), headers: vec![], body: vec![] };
    let spec = |reg: &Registry| reg.hook_specs().into_iter().find(|h| h.plugin_id == "add-header").unwrap();
    let Ok(Verdict::Changed(edited)) = engine.run_request(&spec(&reg).source, &request).await.verdict else { panic!() };
    assert!(edited.headers.contains(&("x-debug".to_string(), "proxyzip".to_string())), "the defaults apply");

    let mut values = serde_json::Map::new();
    values.insert("host".into(), json!("other.test"));
    reg.set_settings("add-header", &values).unwrap();
    assert!(
        matches!(engine.run_request(&spec(&reg).source, &request).await.verdict, Ok(Verdict::Unchanged)),
        "other hosts are left alone"
    );

    values.insert("host".into(), json!("example.com"));
    values.insert("name".into(), json!("X-Flag"));
    values.insert("value".into(), json!("on"));
    reg.set_settings("add-header", &values).unwrap();
    let Ok(Verdict::Changed(edited)) = engine.run_request(&spec(&reg).source, &request).await.verdict else { panic!() };
    assert!(edited.headers.contains(&("X-Flag".to_string(), "on".to_string())));
}

#[tokio::test]
async fn timeline_tab_renders_a_table() {
    let (_tmp, reg) = installed_all();
    let (plugin, tab) = reg.tabs().into_iter().next().expect("a tab");
    assert_eq!(tab.id, "timeline");
    let engine = ScriptEngine::default();
    let flow =
        json!({ "timing": { "started": 1000, "request_sent": 1010, "response_started": 1060, "finished": 1100 } });
    let out = engine.call(&plugin.runtime_source(), PluginEntry::Render, flow, json!({ "tab": "timeline" })).await;
    let Ok(Verdict::Changed(v)) = out.verdict else { panic!("{:?}", out.verdict) };
    assert_eq!(v["type"], "table");
    assert_eq!(v["content"]["rows"][3], json!(["Total", "100"]));
    let pending = engine
        .call(&plugin.runtime_source(), PluginEntry::Render, json!({ "timing": { "started": 1 } }), json!({}))
        .await;
    assert!(matches!(pending.verdict, Ok(Verdict::Changed(v)) if v["type"] == "text"));
}

#[test]
fn settings_are_validated_and_survive_a_restart() {
    let (tmp, mut reg) = installed_all();
    let mut values = serde_json::Map::new();
    values.insert("name".into(), json!(42));
    assert!(reg.set_settings("add-header", &values).is_err(), "a number is not a string");
    values.insert("name".into(), json!("X-One"));
    values.insert("unknown".into(), json!(true));
    reg.set_settings("add-header", &values).unwrap();
    let reloaded = Registry::load(&tmp.path().join("plugins"));
    let p = reloaded.plugins.iter().find(|p| p.manifest.id == "add-header").unwrap();
    assert_eq!(p.values["name"], "X-One");
    assert_eq!(p.values["value"], "proxyzip", "defaults fill the rest");
    assert!(!p.values.contains_key("unknown"));
}

fn installed_all() -> (tempfile::TempDir, Registry) {
    let tmp = tempfile::tempdir().unwrap();
    let mut reg = Registry::load(&tmp.path().join("plugins"));
    for entry in std::fs::read_dir(examples()).unwrap().flatten() {
        if entry.path().join("plugin.json").exists() {
            reg.install(&entry.path()).unwrap_or_else(|e| panic!("{}: {e:#}", entry.path().display()));
        }
    }
    (tmp, reg)
}

/// `plugins/catalog.json` must list exactly the files in the plugin folders, with their hashes. Rebuild it with
/// `python3 plugins/build_catalog.py` after changing a plugin.
#[test]
fn the_catalog_matches_the_plugin_folders() {
    use proxyzip_script::catalog::{Catalog, sha256_hex};
    let catalog = Catalog::parse(&std::fs::read_to_string(examples().join("catalog.json")).unwrap()).unwrap();
    let mut folders: Vec<String> = std::fs::read_dir(examples())
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("plugin.json").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    folders.sort();
    let mut ids: Vec<String> = catalog.plugins.iter().map(|p| p.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, folders, "every plugin folder is in the catalog and the other way around");
    for entry in &catalog.plugins {
        let dir = examples().join(&entry.id);
        let manifest =
            proxyzip_script::plugin::Manifest::parse(&std::fs::read_to_string(dir.join("plugin.json")).unwrap())
                .unwrap();
        assert_eq!(
            (manifest.version.as_str(), manifest.name.as_str()),
            (entry.version.as_str(), entry.name.as_str()),
            "{}",
            entry.id
        );
        for file in &entry.files {
            let bytes = std::fs::read(dir.join(&file.path))
                .unwrap_or_else(|_| panic!("{}/{} is listed but missing", entry.id, file.path));
            assert_eq!(
                sha256_hex(&bytes),
                file.sha256,
                "{}/{} changed: run python3 plugins/build_catalog.py",
                entry.id,
                file.path
            );
        }
        let on_disk = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().is_file()).count();
        assert_eq!(on_disk, entry.files.len(), "{} has files the catalog does not list", entry.id);
    }
}
