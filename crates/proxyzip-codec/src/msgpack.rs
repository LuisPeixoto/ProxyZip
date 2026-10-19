//! MessagePack → JSON.

use serde_json::{Map, Value};

/// Decode a MessagePack document. Binary values become `"base64:..."`, extension types `{"$ext": type, "data": "base64:..."}`.
pub fn decode(data: &[u8]) -> Result<Value, String> {
    let mut cursor = data;
    let value = rmpv::decode::read_value(&mut cursor).map_err(|e| e.to_string())?;
    if !cursor.is_empty() {
        return Err(format!("{} unexpected bytes after the MessagePack value", cursor.len()));
    }
    Ok(to_json(&value))
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    format!("base64:{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn to_json(value: &rmpv::Value) -> Value {
    use rmpv::Value as M;
    match value {
        M::Nil => Value::Null,
        M::Boolean(b) => Value::Bool(*b),
        M::Integer(i) => {
            if let Some(n) = i.as_i64() {
                Value::from(n)
            } else if let Some(n) = i.as_u64() {
                Value::from(n)
            } else {
                Value::Null
            }
        }
        M::F32(f) => serde_json::Number::from_f64(f64::from(*f)).map_or(Value::Null, Value::Number),
        M::F64(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
        M::String(s) => Value::String(s.as_str().map_or_else(|| b64(s.as_bytes()), str::to_string)),
        M::Binary(b) => Value::String(b64(b)),
        M::Array(items) => Value::Array(items.iter().map(to_json).collect()),
        M::Map(entries) => {
            let mut map = Map::new();
            for (k, v) in entries {
                let key = match k {
                    M::String(s) => s.as_str().unwrap_or("?").to_string(),
                    other => to_json(other).to_string(),
                };
                map.insert(key, to_json(v));
            }
            Value::Object(map)
        }
        M::Ext(kind, data) => serde_json::json!({ "$ext": kind, "data": b64(data) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_maps_arrays_and_scalars() {
        let doc = rmpv::Value::Map(vec![
            ("name".into(), "proxyzip".into()),
            ("tags".into(), rmpv::Value::Array(vec![1.into(), 2.5.into(), true.into(), rmpv::Value::Nil])),
            ("blob".into(), rmpv::Value::Binary(vec![1, 2, 3])),
            ("ext".into(), rmpv::Value::Ext(5, vec![9])),
        ]);
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &doc).unwrap();
        let json = decode(&bytes).unwrap();
        assert_eq!(json["name"], "proxyzip");
        assert_eq!(json["tags"], serde_json::json!([1, 2.5, true, null]));
        assert_eq!(json["blob"], "base64:AQID");
        assert_eq!(json["ext"]["$ext"], 5);
    }

    #[test]
    fn rejects_garbage_and_trailing_bytes() {
        assert!(decode(&[0x92, 0x01]).is_err(), "an array that promises two items but holds one");
        assert!(decode(&[0x01, 0x02]).unwrap_err().contains("unexpected"));
        assert!(decode(&[]).is_err());
    }
}
