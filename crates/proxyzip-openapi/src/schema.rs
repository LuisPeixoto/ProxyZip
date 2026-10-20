//! `$ref` resolution and the 3.0 → JSON Schema differences.

use serde_json::{Map, Value};

const MAX_DEPTH: usize = 24;

/// Look up a local reference such as `#/components/schemas/Pet`.
pub fn lookup<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    doc.pointer(&pointer.replace("%7B", "{").replace("%7D", "}"))
}

/// Replace a top-level `$ref` (and chains of them) with its target. Other keys are left alone.
pub fn resolve(doc: &Value, value: &Value) -> Value {
    let mut current = value;
    for _ in 0..MAX_DEPTH {
        match current.get("$ref").and_then(Value::as_str).and_then(|r| lookup(doc, r)) {
            Some(next) => current = next,
            None => break,
        }
    }
    current.clone()
}

/// Expand every local `$ref` in a schema (cycles are cut off with an empty schema) and convert OpenAPI 3.0 keywords to
/// plain JSON Schema: `nullable`, boolean `exclusiveMinimum`/`exclusiveMaximum`, and annotations a validator ignores.
pub fn to_json_schema(doc: &Value, schema: &Value) -> Value {
    expand(doc, schema, &mut Vec::new())
}

fn expand(doc: &Value, schema: &Value, seen: &mut Vec<String>) -> Value {
    match schema {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                if seen.iter().any(|r| r == reference) || seen.len() >= MAX_DEPTH {
                    return Value::Object(Map::new());
                }
                let Some(target) = lookup(doc, reference) else { return Value::Object(Map::new()) };
                seen.push(reference.to_string());
                let out = expand(doc, target, seen);
                seen.pop();
                return out;
            }
            let mut out = Map::new();
            for (k, v) in map {
                match k.as_str() {
                    // Not part of validation.
                    "example" | "xml" | "externalDocs" | "discriminator" | "deprecated" | "readOnly" | "writeOnly" => {}
                    // `properties` values and `$defs`-like maps hold schemas by name; recurse into the values.
                    "properties" | "patternProperties" | "dependentSchemas" => {
                        let inner: Map<String, Value> = v
                            .as_object()
                            .map(|m| m.iter().map(|(n, s)| (n.clone(), expand(doc, s, seen))).collect())
                            .unwrap_or_default();
                        out.insert(k.clone(), Value::Object(inner));
                    }
                    "items"
                    | "additionalProperties"
                    | "not"
                    | "if"
                    | "then"
                    | "else"
                    | "contains"
                    | "propertyNames" => {
                        out.insert(k.clone(), if v.is_boolean() { v.clone() } else { expand(doc, v, seen) });
                    }
                    "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                        let list: Vec<Value> =
                            v.as_array().map(|a| a.iter().map(|s| expand(doc, s, seen)).collect()).unwrap_or_default();
                        out.insert(k.clone(), Value::Array(list));
                    }
                    "exclusiveMinimum" if v.is_boolean() => {
                        if v.as_bool() == Some(true)
                            && let Some(min) = map.get("minimum")
                        {
                            out.insert("exclusiveMinimum".into(), min.clone());
                        }
                    }
                    "exclusiveMaximum" if v.is_boolean() => {
                        if v.as_bool() == Some(true)
                            && let Some(max) = map.get("maximum")
                        {
                            out.insert("exclusiveMaximum".into(), max.clone());
                        }
                    }
                    "minimum" if map.get("exclusiveMinimum").and_then(Value::as_bool) == Some(true) => {}
                    "maximum" if map.get("exclusiveMaximum").and_then(Value::as_bool) == Some(true) => {}
                    "nullable" => {}
                    _ => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            if map.get("nullable").and_then(Value::as_bool) == Some(true) {
                allow_null(&mut out);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// `nullable: true` accepts `null` in addition to the declared type.
fn allow_null(schema: &mut Map<String, Value>) {
    match schema.get("type").cloned() {
        Some(Value::String(t)) => {
            schema.insert("type".into(), serde_json::json!([t, "null"]));
        }
        Some(Value::Array(mut types)) => {
            if !types.iter().any(|t| t == "null") {
                types.push("null".into());
                schema.insert("type".into(), Value::Array(types));
            }
        }
        _ => {
            if let Some(Value::Array(list)) = schema.get_mut("enum")
                && !list.iter().any(Value::is_null)
            {
                list.push(Value::Null);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expands_refs_and_cuts_cycles() {
        let doc = json!({ "components": { "schemas": {
            "Node": { "type": "object", "properties": { "child": { "$ref": "#/components/schemas/Node" }, "n": { "type": "integer" } } }
        } } });
        let out = to_json_schema(&doc, &json!({ "$ref": "#/components/schemas/Node" }));
        assert_eq!(out["properties"]["n"]["type"], "integer");
        assert_eq!(out["properties"]["child"], json!({}), "the cycle is cut");
        assert_eq!(to_json_schema(&doc, &json!({ "$ref": "#/nope" })), json!({}));
    }

    #[test]
    fn converts_openapi_30_keywords() {
        let doc = json!({});
        let s = to_json_schema(&doc, &json!({ "type": "string", "nullable": true, "example": "x" }));
        assert_eq!(s, json!({ "type": ["string", "null"] }));
        let s = to_json_schema(&doc, &json!({ "type": "number", "minimum": 0, "exclusiveMinimum": true }));
        assert_eq!(s, json!({ "type": "number", "exclusiveMinimum": 0 }));
        let s = to_json_schema(&doc, &json!({ "type": "number", "minimum": 0, "exclusiveMinimum": false }));
        assert_eq!(s, json!({ "type": "number", "minimum": 0 }));
    }
}
