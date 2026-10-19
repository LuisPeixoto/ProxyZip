//! GraphQL over HTTP: recognise requests and describe their operations.

use graphql_parser::query::{Definition, OperationDefinition, parse_query};
use serde_json::{Value, json};

/// Operations found in a GraphQL request body (a single request or a batch).
pub fn describe(body: &[u8]) -> Option<Value> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let requests: Vec<&Value> = match &parsed {
        Value::Array(items) if !items.is_empty() => items.iter().collect(),
        Value::Object(_) => vec![&parsed],
        _ => return None,
    };
    let mut described = Vec::new();
    for request in requests {
        let query = request.get("query")?.as_str()?;
        described.push(describe_one(query, request));
    }
    Some(if described.len() == 1 { described.remove(0) } else { Value::Array(described) })
}

fn describe_one(query: &str, request: &Value) -> Value {
    let named = request.get("operationName").and_then(Value::as_str).map(str::to_string);
    let mut out = json!({ "query": query });
    match parse_query::<String>(query) {
        Ok(doc) => {
            let ops: Vec<(String, Option<String>)> = doc
                .definitions
                .iter()
                .filter_map(|d| match d {
                    Definition::Operation(op) => Some(operation_label(op)),
                    Definition::Fragment(_) => None,
                })
                .collect();
            let chosen = named
                .as_ref()
                .and_then(|n| ops.iter().find(|(_, name)| name.as_ref() == Some(n)))
                .or_else(|| ops.first());
            if let Some((kind, name)) = chosen {
                out["operation"] = json!(kind);
                if let Some(name) = name.as_ref().or(named.as_ref()) {
                    out["name"] = json!(name);
                }
            }
        }
        Err(e) => out["parseError"] = json!(e.to_string()),
    }
    if let Some(vars) = request.get("variables").filter(|v| !v.is_null()) {
        out["variables"] = vars.clone();
    }
    for key in ["operationName", "extensions"] {
        if let Some(v) = request.get(key).filter(|v| !v.is_null()) {
            out[key] = v.clone();
        }
    }
    out
}

fn operation_label(op: &OperationDefinition<'_, String>) -> (String, Option<String>) {
    match op {
        OperationDefinition::SelectionSet(_) => ("query".into(), None),
        OperationDefinition::Query(q) => ("query".into(), q.name.clone()),
        OperationDefinition::Mutation(m) => ("mutation".into(), m.name.clone()),
        OperationDefinition::Subscription(s) => ("subscription".into(), s.name.clone()),
    }
}

/// Short label for the request list, such as `query GetUser` or `mutation (2 ops)`.
pub fn label(body: &[u8]) -> Option<String> {
    let described = describe(body)?;
    let one = |v: &Value| -> String {
        let kind = v.get("operation").and_then(Value::as_str).unwrap_or("query");
        match v.get("name").and_then(Value::as_str) {
            Some(name) => format!("{kind} {name}"),
            None => kind.to_string(),
        }
    };
    Some(match &described {
        Value::Array(items) => format!("batch ({} ops)", items.len()),
        single => one(single),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_query_with_variables() {
        let body = br#"{"query":"query GetUser($id: ID!) { user(id: $id) { name } }","variables":{"id":"7"}}"#;
        let d = describe(body).unwrap();
        assert_eq!((d["operation"].as_str(), d["name"].as_str()), (Some("query"), Some("GetUser")));
        assert_eq!(d["variables"]["id"], "7");
        assert_eq!(label(body).as_deref(), Some("query GetUser"));
    }

    #[test]
    fn mutations_shorthand_queries_and_operation_name_selection() {
        assert_eq!(label(br#"{"query":"mutation Save { save }"}"#).as_deref(), Some("mutation Save"));
        assert_eq!(label(br#"{"query":"{ viewer { id } }"}"#).as_deref(), Some("query"));
        let two = br#"{"query":"query A { a } query B { b }","operationName":"B"}"#;
        assert_eq!(label(two).as_deref(), Some("query B"));
    }

    #[test]
    fn batches_and_non_graphql_bodies() {
        let batch = br#"[{"query":"{ a }"},{"query":"query X { x }"}]"#;
        assert_eq!(label(batch).as_deref(), Some("batch (2 ops)"));
        assert!(describe(br#"{"hello":"world"}"#).is_none());
        assert!(describe(b"not json").is_none());
        assert!(describe(br#"{"query":42}"#).is_none());
    }

    #[test]
    fn syntax_errors_are_reported_not_fatal() {
        let d = describe(br#"{"query":"query {"}"#).unwrap();
        assert!(d["parseError"].is_string());
    }
}
