//! A tiny template language for mock responses and test suites.
//!
//! `{{expr}}` is replaced by the value of `expr`; anything unknown renders as an empty string, so a template never
//! fails. Write `\{{` for a literal `{{`.
//!
//! | Expression | Value |
//! |------------|-------|
//! | `params.id` | A `:id` segment of the matched route |
//! | `query.page` | A query parameter |
//! | `headers.x-token` | A request header (case-insensitive) |
//! | `body.user.name`, `body.items.0.id` | A field of the JSON request body |
//! | `env.NAME`, `vars.NAME` | A variable (suites) |
//! | `uuid` | A random UUID |
//! | `now`, `now_unix` | The current time, as RFC 3339 or Unix seconds |
//! | `random_int 1 100` | A random integer in the range |

use std::collections::HashMap;

use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    pub params: HashMap<String, String>,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
    /// Variables, available as both `env.X` and `vars.X`.
    pub vars: HashMap<String, String>,
}

fn json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn show(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn evaluate(expr: &str, ctx: &TemplateContext) -> String {
    let mut words = expr.split_whitespace();
    let Some(head) = words.next() else { return String::new() };
    match head {
        "uuid" => uuid::Uuid::new_v4().to_string(),
        "now" => chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "now_unix" => chrono::Utc::now().timestamp().to_string(),
        "random_int" => {
            let a: i64 = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
            let b: i64 = words.next().and_then(|w| w.parse().ok()).unwrap_or(100);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            fastrand::i64(lo..=hi).to_string()
        }
        other => {
            let (scope, rest) = other.split_once('.').unwrap_or((other, ""));
            match scope {
                "params" => ctx.params.get(rest).cloned().unwrap_or_default(),
                "query" => ctx.query.iter().find(|(k, _)| k == rest).map(|(_, v)| v.clone()).unwrap_or_default(),
                "headers" => ctx
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(rest))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
                "body" => ctx.body.as_ref().and_then(|b| json_path(b, rest)).map(show).unwrap_or_default(),
                "env" | "vars" => ctx.vars.get(rest).cloned().unwrap_or_default(),
                _ => String::new(),
            }
        }
    }
}

/// Fill in `template`.
pub fn render(template: &str, ctx: &TemplateContext) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find("{{") {
        let escaped = rest[..at].ends_with('\\');
        if escaped {
            out.push_str(&rest[..at - 1]);
            out.push_str("{{");
            rest = &rest[at + 2..];
            continue;
        }
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        match after.find("}}") {
            Some(end) => {
                out.push_str(&evaluate(after[..end].trim(), ctx));
                rest = &after[end + 2..];
            }
            None => {
                // An unclosed `{{` is plain text.
                out.push_str(&rest[at..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> TemplateContext {
        TemplateContext {
            params: HashMap::from([("id".to_string(), "42".to_string())]),
            query: vec![("page".into(), "3".into())],
            headers: vec![("X-Token".into(), "abc".into())],
            body: Some(json!({ "user": { "name": "ada", "tags": ["a", "b"] }, "age": 36, "ok": true })),
            vars: HashMap::from([("base".to_string(), "http://x".to_string())]),
        }
    }

    #[test]
    fn fills_every_scope() {
        let c = ctx();
        assert_eq!(render("id={{params.id}} page={{ query.page }} t={{headers.x-token}}", &c), "id=42 page=3 t=abc");
        assert_eq!(render("{{body.user.name}} {{body.user.tags.1}} {{body.age}} {{body.ok}}", &c), "ada b 36 true");
        assert_eq!(render("{{env.base}}/{{vars.base}}", &c), "http://x/http://x");
        assert_eq!(render("{{body.user}}", &c), r#"{"name":"ada","tags":["a","b"]}"#);
    }

    #[test]
    fn unknown_values_are_empty_and_never_fail() {
        let c = ctx();
        assert_eq!(render("[{{params.nope}}][{{query.x}}][{{body.a.b.c}}][{{whatever}}][{{}}]", &c), "[][][][][]");
        assert_eq!(render("a {{ unclosed", &c), "a {{ unclosed");
        assert_eq!(render("plain", &TemplateContext::default()), "plain");
    }

    #[test]
    fn generated_values() {
        let c = TemplateContext::default();
        assert_eq!(render("{{uuid}}", &c).len(), 36);
        assert!(render("{{now}}", &c).ends_with('Z'));
        assert!(render("{{now_unix}}", &c).parse::<i64>().unwrap() > 1_700_000_000);
        for _ in 0..50 {
            let n: i64 = render("{{random_int 5 7}}", &c).parse().unwrap();
            assert!((5..=7).contains(&n));
        }
        let swapped: i64 = render("{{random_int 9 3}}", &c).parse().unwrap();
        assert!((3..=9).contains(&swapped));
    }

    #[test]
    fn escapes() {
        assert_eq!(render(r"\{{params.id}} and {{params.id}}", &ctx()), "{{params.id}} and 42");
    }
}
