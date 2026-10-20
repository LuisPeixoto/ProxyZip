//! Import a Postman collection (v2.1) as a suite.

use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;

use crate::model::{Assertion, Suite, SuiteRequest};

fn variable() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").expect("valid regex"))
}

/// Postman's `{{name}}` becomes `{{vars.name}}`, and its dynamic variables map to the ones ProxyZip has.
fn convert(text: &str) -> String {
    variable()
        .replace_all(text, |caps: &regex::Captures| {
            let name = &caps[1];
            match name {
                "$guid" | "$randomUUID" => "{{uuid}}".to_string(),
                "$timestamp" => "{{now_unix}}".to_string(),
                "$isoTimestamp" => "{{now}}".to_string(),
                "$randomInt" => "{{random_int 0 1000}}".to_string(),
                n if n.starts_with('$') => String::new(),
                n if n.contains('.') || n.contains(' ') => caps[0].to_string(),
                n => format!("{{{{vars.{n}}}}}"),
            }
        })
        .into_owned()
}

fn text(v: &Value) -> String {
    v.as_str().map(convert).unwrap_or_default()
}

fn walk(items: &[Value], prefix: &str, out: &mut Vec<SuiteRequest>, collection_auth: Option<&Value>) {
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("request");
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            walk(children, &format!("{prefix}{name} / "), out, collection_auth);
            continue;
        }
        let Some(request) = item.get("request") else { continue };
        // A request can be a plain URL string in older exports.
        if let Some(url) = request.as_str() {
            out.push(SuiteRequest {
                name: format!("{prefix}{name}"),
                enabled: true,
                method: "GET".into(),
                url: convert(url),
                headers: vec![],
                body: None,
                assertions: default_assertions(),
                extract: vec![],
            });
            continue;
        }
        let url = match request.get("url") {
            Some(Value::String(s)) => convert(s),
            Some(u) => u.get("raw").map(text).unwrap_or_default(),
            None => String::new(),
        };
        let mut headers: Vec<(String, String)> = request
            .get("header")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|h| h.get("disabled").and_then(Value::as_bool) != Some(true))
            .filter_map(|h| Some((h.get("key")?.as_str()?.to_string(), text(h.get("value")?))))
            .collect();
        let mut note = String::new();
        let mut body = None;
        if let Some(b) = request.get("body") {
            match b.get("mode").and_then(Value::as_str) {
                Some("raw") => body = b.get("raw").map(text),
                Some("urlencoded") => {
                    let pairs: Vec<String> = b
                        .get("urlencoded")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|p| p.get("disabled").and_then(Value::as_bool) != Some(true))
                        .filter_map(|p| {
                            Some(format!(
                                "{}={}",
                                p.get("key")?.as_str()?,
                                p.get("value").map(text).unwrap_or_default()
                            ))
                        })
                        .collect();
                    body = Some(pairs.join("&"));
                    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                        headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
                    }
                }
                Some("formdata") | Some("file") | Some("graphql") => note = " (body not imported)".into(),
                _ => {}
            }
        }
        if let Some(auth) = request.get("auth").or(collection_auth) {
            apply_auth(auth, &mut headers);
        }
        out.push(SuiteRequest {
            name: format!("{prefix}{name}{note}"),
            enabled: true,
            method: request.get("method").and_then(Value::as_str).unwrap_or("GET").to_ascii_uppercase(),
            url,
            headers,
            body,
            assertions: default_assertions(),
            extract: vec![],
        });
    }
}

fn default_assertions() -> Vec<Assertion> {
    vec![Assertion::Status { equals: None, class: Some("2xx".into()) }]
}

fn auth_value<'a>(auth: &'a Value, kind: &str, key: &str) -> Option<&'a str> {
    auth.get(kind)?
        .as_array()?
        .iter()
        .find(|e| e.get("key").and_then(Value::as_str) == Some(key))?
        .get("value")?
        .as_str()
}

fn apply_auth(auth: &Value, headers: &mut Vec<(String, String)>) {
    if headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")) {
        return;
    }
    match auth.get("type").and_then(Value::as_str) {
        Some("bearer") => {
            if let Some(token) = auth_value(auth, "bearer", "token") {
                headers.push(("Authorization".into(), format!("Bearer {}", convert(token))));
            }
        }
        Some("basic") => {
            // The credentials are encoded now, so only literal ones can be imported.
            if let (Some(user), Some(pass)) =
                (auth_value(auth, "basic", "username"), auth_value(auth, "basic", "password"))
                && !user.contains("{{")
                && !pass.contains("{{")
            {
                use std::io::Write;
                let mut encoded = Vec::new();
                let _ = write!(encoded, "{user}:{pass}");
                headers.push(("Authorization".into(), format!("Basic {}", base64(&encoded))));
            }
        }
        _ => {}
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Turn a Postman collection into a suite. Folders become name prefixes; requests expect a 2xx answer.
pub fn import_postman(json: &str) -> Result<Suite> {
    let doc: Value = serde_json::from_str(json).context("this is not a Postman collection (invalid JSON)")?;
    let items = doc.get("item").and_then(Value::as_array);
    if doc.get("info").is_none() || items.is_none() {
        bail!("this is not a Postman collection (v2.1)");
    }
    let name = doc.pointer("/info/name").and_then(Value::as_str).unwrap_or("Postman collection");
    let mut suite = Suite::new(name);
    for v in doc.get("variable").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(k), Some(val)) = (v.get("key").and_then(Value::as_str), v.get("value")) {
            suite.variables.insert(k.to_string(), val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string()));
        }
    }
    walk(items.expect("checked"), "", &mut suite.requests, doc.get("auth"));
    Ok(suite)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"{
      "info": { "name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "variable": [{ "key": "base", "value": "https://api.test" }, { "key": "n", "value": 3 }],
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}" }] },
      "item": [
        { "name": "Auth", "item": [
          { "name": "Login", "request": { "method": "post", "header": [{ "key": "Content-Type", "value": "application/json" }, { "key": "X-Off", "value": "1", "disabled": true }],
            "url": { "raw": "{{base}}/login" }, "body": { "mode": "raw", "raw": "{\"id\":\"{{$guid}}\",\"user\":\"{{user}}\"}" } } }
        ] },
        { "name": "Form", "request": { "method": "POST", "url": "{{base}}/form", "body": { "mode": "urlencoded", "urlencoded": [{ "key": "a", "value": "{{n}}" }, { "key": "off", "value": "x", "disabled": true }] } } },
        { "name": "Upload", "request": { "method": "POST", "url": "{{base}}/up", "body": { "mode": "formdata" } } },
        { "name": "Basic", "request": { "method": "GET", "url": "{{base}}/b", "auth": { "type": "basic", "basic": [{ "key": "username", "value": "ada" }, { "key": "password", "value": "pw" }] } } }
      ]
    }"#;

    #[test]
    fn converts_variables_folders_bodies_and_auth() {
        let suite = import_postman(COLLECTION).unwrap();
        assert_eq!(suite.name, "Shop");
        assert_eq!(suite.variables["base"], "https://api.test");
        assert_eq!(suite.variables["n"], "3");
        assert_eq!(suite.requests.len(), 4);

        let login = &suite.requests[0];
        assert_eq!(login.name, "Auth / Login");
        assert_eq!(login.method, "POST");
        assert_eq!(login.url, "{{vars.base}}/login");
        assert_eq!(login.body.as_deref(), Some(r#"{"id":"{{uuid}}","user":"{{vars.user}}"}"#));
        assert!(login.headers.contains(&("Content-Type".into(), "application/json".into())));
        assert!(!login.headers.iter().any(|(k, _)| k == "X-Off"), "disabled headers are dropped");
        assert!(
            login.headers.contains(&("Authorization".into(), "Bearer {{vars.token}}".into())),
            "the collection's auth applies"
        );

        let form = &suite.requests[1];
        assert_eq!(form.body.as_deref(), Some("a={{vars.n}}"));
        assert!(form.headers.iter().any(|(k, v)| k == "Content-Type" && v.contains("urlencoded")));
        assert!(suite.requests[2].name.ends_with("(body not imported)"));
        assert!(suite.requests[3].headers.contains(&("Authorization".into(), "Basic YWRhOnB3".into())));
        assert_eq!(login.assertions, vec![Assertion::Status { equals: None, class: Some("2xx".into()) }]);
    }

    #[test]
    fn rejects_other_documents() {
        assert!(import_postman("{}").is_err());
        assert!(import_postman("nope").is_err());
        assert!(import_postman(r#"{"info":{"name":"x"},"item":[]}"#).unwrap().requests.is_empty());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"ada:pw"), "YWRhOnB3");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
    }
}
