//! Making up a value that satisfies a schema.

use serde_json::{Map, Value, json};

use crate::schema::lookup;

const MAX_DEPTH: usize = 8;

/// Generate an example for `schema` (which may contain `$ref`s into `doc`). Explicit `example`, `default`, `const` and
/// `enum` values win; everything else is the simplest value of the type.
pub fn generate_example(doc: &Value, schema: &Value) -> Value {
    build(doc, schema, 0, &mut Vec::new())
}

fn build(doc: &Value, schema: &Value, depth: usize, refs: &mut Vec<String>) -> Value {
    if depth > MAX_DEPTH {
        return Value::Null;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if refs.iter().any(|r| r == reference) {
            return Value::Null;
        }
        let Some(target) = lookup(doc, reference) else { return Value::Null };
        refs.push(reference.to_string());
        let out = build(doc, target, depth + 1, refs);
        refs.pop();
        return out;
    }
    if let Some(v) = schema.get("example").or_else(|| schema.get("default")).or_else(|| schema.get("const")) {
        return v.clone();
    }
    if let Some(first) = schema.get("enum").and_then(Value::as_array).and_then(|e| e.first()) {
        return first.clone();
    }
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        let mut merged = Map::new();
        for part in all {
            if let Value::Object(o) = build(doc, part, depth + 1, refs) {
                merged.extend(o);
            }
        }
        return Value::Object(merged);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(first) = schema.get(key).and_then(Value::as_array).and_then(|a| a.first()) {
            return build(doc, first, depth + 1, refs);
        }
    }
    let kind = match schema.get("type") {
        Some(Value::String(t)) => t.clone(),
        // 3.1 allows a list; take the first type that is not null.
        Some(Value::Array(types)) => {
            types.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("null").to_string()
        }
        _ if schema.get("properties").is_some() => "object".into(),
        _ if schema.get("items").is_some() => "array".into(),
        _ => return Value::Null,
    };
    match kind.as_str() {
        "object" => {
            let mut out = Map::new();
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (name, prop) in props {
                    out.insert(name.clone(), build(doc, prop, depth + 1, refs));
                }
            }
            Value::Object(out)
        }
        "array" => {
            let item = schema.get("items").map(|i| build(doc, i, depth + 1, refs)).unwrap_or(Value::Null);
            let count = schema.get("minItems").and_then(Value::as_u64).unwrap_or(1).clamp(1, 3) as usize;
            Value::Array(vec![item; count])
        }
        "string" => string_example(schema),
        "integer" => {
            let min = schema.get("minimum").and_then(Value::as_i64);
            let max = schema.get("maximum").and_then(Value::as_i64);
            json!(min.unwrap_or(1).min(max.unwrap_or(i64::MAX)))
        }
        "number" => {
            let min = schema.get("minimum").and_then(Value::as_f64).unwrap_or(1.5);
            json!(min)
        }
        "boolean" => json!(true),
        _ => Value::Null,
    }
}

fn string_example(schema: &Value) -> Value {
    let by_format = match schema.get("format").and_then(Value::as_str) {
        Some("date-time") => Some("2024-01-01T00:00:00Z"),
        Some("date") => Some("2024-01-01"),
        Some("time") => Some("12:00:00Z"),
        Some("uuid") => Some("3fa85f64-5717-4562-b3fc-2c963f66afa6"),
        Some("email") => Some("user@example.com"),
        Some("uri") | Some("url") => Some("https://example.com"),
        Some("hostname") => Some("example.com"),
        Some("ipv4") => Some("192.0.2.1"),
        Some("ipv6") => Some("2001:db8::1"),
        Some("byte") => Some("c3RyaW5n"),
        _ => None,
    };
    if let Some(s) = by_format {
        return json!(s);
    }
    let min = schema.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut s = "string".to_string();
    while s.len() < min {
        s.push('x');
    }
    if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
        s.truncate(max as usize);
    }
    json!(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        json!({ "components": { "schemas": {
            "Pet": { "type": "object", "required": ["id"], "properties": { "id": { "type": "integer" }, "name": { "type": "string" }, "owner": { "$ref": "#/components/schemas/Owner" } } },
            "Owner": { "type": "object", "properties": { "pet": { "$ref": "#/components/schemas/Pet" }, "email": { "type": "string", "format": "email" } } }
        } } })
    }

    #[test]
    fn explicit_values_win() {
        let d = json!({});
        assert_eq!(generate_example(&d, &json!({ "type": "string", "example": "hi", "enum": ["a"] })), json!("hi"));
        assert_eq!(generate_example(&d, &json!({ "type": "string", "enum": ["a", "b"] })), json!("a"));
        assert_eq!(generate_example(&d, &json!({ "type": "integer", "default": 5 })), json!(5));
        assert_eq!(generate_example(&d, &json!({ "const": "k" })), json!("k"));
    }

    #[test]
    fn scalars_follow_their_constraints() {
        let d = json!({});
        assert_eq!(generate_example(&d, &json!({ "type": "integer", "minimum": 10, "maximum": 20 })), json!(10));
        assert_eq!(generate_example(&d, &json!({ "type": "integer", "maximum": -3 })), json!(-3));
        assert_eq!(
            generate_example(&d, &json!({ "type": "string", "format": "date-time" })),
            json!("2024-01-01T00:00:00Z")
        );
        assert_eq!(generate_example(&d, &json!({ "type": "string", "minLength": 8 })), json!("stringxx"));
        assert_eq!(generate_example(&d, &json!({ "type": "string", "maxLength": 3 })), json!("str"));
        assert_eq!(generate_example(&d, &json!({ "type": ["null", "boolean"] })), json!(true));
        assert_eq!(
            generate_example(&d, &json!({ "type": "array", "items": { "type": "string" }, "minItems": 2 })),
            json!(["string", "string"])
        );
        assert_eq!(generate_example(&d, &json!({})), Value::Null);
    }

    #[test]
    fn objects_compose_and_cycles_stop() {
        let d = doc();
        let pet = generate_example(&d, &json!({ "$ref": "#/components/schemas/Pet" }));
        assert_eq!(pet["id"], 1);
        assert_eq!(pet["name"], "string");
        assert_eq!(pet["owner"]["email"], "user@example.com");
        assert_eq!(pet["owner"]["pet"], Value::Null, "the cycle ends");
        let merged = generate_example(
            &d,
            &json!({ "allOf": [{ "type": "object", "properties": { "a": { "type": "integer" } } }, { "type": "object", "properties": { "b": { "type": "boolean" } } }] }),
        );
        assert_eq!(merged, json!({ "a": 1, "b": true }));
        assert_eq!(
            generate_example(&d, &json!({ "oneOf": [{ "type": "string" }, { "type": "integer" }] })),
            json!("string")
        );
    }
}
