//! Conversion between the proxy's request/response types and the JSON handed to scripts.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Map, Value, json};

/// A request as scripts see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    pub method: String,
    /// Full URL, such as `https://api.example.com/v1/items?id=2`.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A response as scripts see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Headers become an object. A name that appears more than once holds an array.
fn headers_to_json(headers: &[(String, String)]) -> Value {
    let mut map = Map::new();
    for (name, value) in headers {
        match map.get_mut(name) {
            None => {
                map.insert(name.clone(), json!(value));
            }
            Some(Value::Array(items)) => items.push(json!(value)),
            Some(existing) => {
                let first = existing.take();
                *existing = Value::Array(vec![first, json!(value)]);
            }
        }
    }
    Value::Object(map)
}

fn headers_from_json(value: &Value) -> Result<Vec<(String, String)>, String> {
    let object = match value {
        Value::Object(o) => o,
        Value::Null => return Ok(Vec::new()),
        _ => return Err("headers must be an object".into()),
    };
    let mut headers = Vec::new();
    let scalar = |v: &Value| -> Option<String> {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    };
    for (name, v) in object {
        match v {
            Value::Array(items) => headers.extend(items.iter().filter_map(scalar).map(|s| (name.clone(), s))),
            Value::Null => {}
            other => headers.extend(scalar(other).map(|s| (name.clone(), s))),
        }
    }
    Ok(headers)
}

fn body_to_json(body: &[u8]) -> (Value, Value) {
    (std::str::from_utf8(body).map_or(Value::Null, |t| json!(t)), json!(B64.encode(body)))
}

/// Text edits win, then base64 edits; an untouched body keeps its exact bytes.
fn body_from_json(value: &Value, original: &[u8]) -> Result<Vec<u8>, String> {
    let original_text = std::str::from_utf8(original).ok();
    if let Some(text) = value.get("body").and_then(Value::as_str)
        && Some(text) != original_text
    {
        return Ok(text.as_bytes().to_vec());
    }
    if let Some(b64) = value.get("bodyBase64").and_then(Value::as_str)
        && b64 != B64.encode(original)
    {
        return B64.decode(b64).map_err(|e| format!("bodyBase64 is not valid base64: {e}"));
    }
    Ok(original.to_vec())
}

impl ScriptRequest {
    pub fn to_json(&self) -> Value {
        let (body, body_base64) = body_to_json(&self.body);
        json!({
            "method": self.method,
            "url": self.url,
            "headers": headers_to_json(&self.headers),
            "body": body,
            "bodyBase64": body_base64,
        })
    }

    pub(crate) fn from_json(value: &Value, original: &Self) -> Result<Self, String> {
        Ok(Self {
            method: value.get("method").and_then(Value::as_str).unwrap_or(&original.method).trim().to_ascii_uppercase(),
            url: value.get("url").and_then(Value::as_str).unwrap_or(&original.url).to_string(),
            headers: headers_from_json(value.get("headers").unwrap_or(&Value::Null))?,
            body: body_from_json(value, &original.body)?,
        })
    }
}

impl ScriptResponse {
    pub fn to_json(&self) -> Value {
        let (body, body_base64) = body_to_json(&self.body);
        json!({
            "status": self.status,
            "headers": headers_to_json(&self.headers),
            "body": body,
            "bodyBase64": body_base64,
        })
    }

    pub(crate) fn from_json(value: &Value, original: &Self) -> Result<Self, String> {
        let status = match value.get("status") {
            None | Some(Value::Null) => original.status,
            Some(v) => v
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| (100..=999).contains(n))
                .ok_or("status must be a number between 100 and 999")?,
        };
        Ok(Self {
            status,
            headers: headers_from_json(value.get("headers").unwrap_or(&Value::Null))?,
            body: body_from_json(value, &original.body)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ScriptRequest {
        ScriptRequest {
            method: "POST".into(),
            url: "https://a.dev/x?y=1".into(),
            headers: vec![
                ("set-cookie".into(), "a=1".into()),
                ("Set-Cookie".into(), "b=2".into()),
                ("x-one".into(), "1".into()),
            ],
            body: b"hello".to_vec(),
        }
    }

    #[test]
    fn repeated_headers_become_arrays_and_back() {
        let json = request().to_json();
        assert_eq!(json["headers"]["x-one"], "1");
        // Different spellings stay separate keys.
        assert_eq!(json["headers"]["set-cookie"], "a=1");
        let two = ScriptRequest { headers: vec![("a".into(), "1".into()), ("a".into(), "2".into())], ..request() };
        let j = two.to_json();
        assert_eq!(j["headers"]["a"], json!(["1", "2"]));
        let back = ScriptRequest::from_json(&j, &two).unwrap();
        assert_eq!(back.headers, vec![("a".to_string(), "1".to_string()), ("a".to_string(), "2".to_string())]);
    }

    #[test]
    fn untouched_round_trip_is_identical() {
        let original = ScriptRequest { headers: vec![("a".into(), "1".into())], ..request() };
        assert_eq!(ScriptRequest::from_json(&original.to_json(), &original).unwrap(), original);
        let response = ScriptResponse { status: 201, headers: vec![("a".into(), "1".into())], body: vec![0xff, 0x00] };
        assert_eq!(ScriptResponse::from_json(&response.to_json(), &response).unwrap(), response);
    }

    #[test]
    fn body_edits_by_text_or_base64() {
        let original = request();
        let mut j = original.to_json();
        j["body"] = json!("changed");
        assert_eq!(ScriptRequest::from_json(&j, &original).unwrap().body, b"changed");
        let mut j = original.to_json();
        j["bodyBase64"] = json!(B64.encode([1u8, 2, 3]));
        assert_eq!(ScriptRequest::from_json(&j, &original).unwrap().body, vec![1, 2, 3]);
        let mut j = original.to_json();
        j["bodyBase64"] = json!("***");
        assert!(ScriptRequest::from_json(&j, &original).unwrap_err().contains("base64"));
    }

    #[test]
    fn binary_bodies_expose_null_text() {
        let response = ScriptResponse { status: 200, headers: vec![], body: vec![0xff, 0xfe] };
        let j = response.to_json();
        assert!(j["body"].is_null());
        let mut edited = j.clone();
        edited["body"] = json!("now text");
        assert_eq!(ScriptResponse::from_json(&edited, &response).unwrap().body, b"now text");
    }

    #[test]
    fn validation() {
        let original = ScriptResponse { status: 200, headers: vec![], body: vec![] };
        assert!(ScriptResponse::from_json(&json!({"status": 42}), &original).is_err());
        assert!(ScriptResponse::from_json(&json!({"status": "ok"}), &original).is_err());
        assert_eq!(ScriptResponse::from_json(&json!({"status": 404}), &original).unwrap().status, 404);
        assert!(ScriptResponse::from_json(&json!({"headers": 5}), &original).is_err());
        let removed = ScriptResponse::from_json(&json!({"headers": {"a": null, "b": 7}}), &original).unwrap();
        assert_eq!(removed.headers, vec![("b".to_string(), "7".to_string())]);
        let m = ScriptRequest::from_json(&json!({"method": " put "}), &request()).unwrap();
        assert_eq!(m.method, "PUT");
    }
}
