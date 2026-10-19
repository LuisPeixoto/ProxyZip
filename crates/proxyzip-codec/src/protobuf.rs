//! Protocol Buffers and gRPC.
//!
//! Without a schema a message is decoded from the wire format alone (field numbers and best-guess values). With a
//! descriptor set (`protoc --descriptor_set_out`), field names and types come from the schema.

use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor};
use serde_json::{Map, Value, json};

/// Schemas the user imported.
#[derive(Clone, Default)]
pub struct ProtoRegistry {
    pool: DescriptorPool,
}

impl std::fmt::Debug for ProtoRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtoRegistry").field("messages", &self.pool.all_messages().len()).finish()
    }
}

impl ProtoRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a serialized `FileDescriptorSet`.
    pub fn add_descriptor_set(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut candidate = self.pool.clone();
        candidate.decode_file_descriptor_set(bytes).map_err(|e| format!("not a valid descriptor set: {e}"))?;
        self.pool = candidate;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.pool.all_messages().len() == 0
    }

    /// Fully qualified names of every message, for pickers.
    pub fn message_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.pool.all_messages().map(|m| m.full_name().to_string()).collect();
        names.sort();
        names
    }

    /// `["pkg.Service/Method", …]`
    pub fn method_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .pool
            .services()
            .flat_map(|s| s.methods().map(|m| format!("{}/{}", s.full_name(), m.name())).collect::<Vec<_>>())
            .collect();
        names.sort();
        names
    }

    pub fn message(&self, full_name: &str) -> Option<MessageDescriptor> {
        self.pool.get_message_by_name(full_name.trim_start_matches('.'))
    }

    /// Look a method up by its gRPC path, `/pkg.Service/Method`.
    pub fn method(&self, path: &str) -> Option<MethodDescriptor> {
        let (service, method) = path.trim_start_matches('/').split_once('/')?;
        self.pool.get_service_by_name(service)?.methods().find(|m| m.name() == method)
    }
}

/// Decode `data` as `descriptor`.
pub fn decode_with(data: &[u8], descriptor: &MessageDescriptor) -> Result<Value, String> {
    let message = DynamicMessage::decode(descriptor.clone(), data).map_err(|e| e.to_string())?;
    serde_json::to_value(&message).map_err(|e| e.to_string())
}

/// Decode `data` without a schema. Keys are field numbers; repeated fields become arrays.
pub fn decode_raw(data: &[u8]) -> Result<Value, String> {
    parse_message(data, 0)
}

const MAX_DEPTH: usize = 12;

fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *data.get(*pos).ok_or("truncated varint")?;
        *pos += 1;
        if shift == 63 && byte > 1 {
            return Err("varint overflows 64 bits".into());
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("varint is too long".into())
}

fn parse_message(data: &[u8], depth: usize) -> Result<Value, String> {
    let mut order: Vec<u32> = Vec::new();
    let mut fields: std::collections::HashMap<u32, Vec<Value>> = std::collections::HashMap::new();
    let mut pos = 0;
    while pos < data.len() {
        let key = read_varint(data, &mut pos)?;
        let number = u32::try_from(key >> 3).map_err(|_| "field number out of range")?;
        if number == 0 {
            return Err("field number 0 is not allowed".into());
        }
        let value = match key & 7 {
            0 => Value::from(read_varint(data, &mut pos)?),
            1 => {
                let bytes: [u8; 8] = data.get(pos..pos + 8).ok_or("truncated fixed64")?.try_into().unwrap();
                pos += 8;
                Value::from(u64::from_le_bytes(bytes))
            }
            2 => {
                let len = usize::try_from(read_varint(data, &mut pos)?).map_err(|_| "length out of range")?;
                let end =
                    pos.checked_add(len).filter(|e| *e <= data.len()).ok_or("truncated length-delimited field")?;
                let slice = &data[pos..end];
                pos = end;
                classify(slice, depth)
            }
            5 => {
                let bytes: [u8; 4] = data.get(pos..pos + 4).ok_or("truncated fixed32")?.try_into().unwrap();
                pos += 4;
                Value::from(u32::from_le_bytes(bytes))
            }
            3 | 4 => return Err("groups are not supported".into()),
            other => return Err(format!("invalid wire type {other}")),
        };
        if !fields.contains_key(&number) {
            order.push(number);
        }
        fields.entry(number).or_default().push(value);
    }
    let mut map = Map::new();
    for number in order {
        let mut values = fields.remove(&number).unwrap_or_default();
        map.insert(number.to_string(), if values.len() == 1 { values.remove(0) } else { Value::Array(values) });
    }
    Ok(Value::Object(map))
}

/// A length-delimited field is a string, a nested message or raw bytes. Text wins; then a clean parse as a message.
fn classify(bytes: &[u8], depth: usize) -> Value {
    if bytes.is_empty() {
        return json!("");
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
    {
        return json!(text);
    }
    if depth < MAX_DEPTH
        && let Ok(nested) = parse_message(bytes, depth + 1)
    {
        return nested;
    }
    json!(format!("hex:{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// One frame of a gRPC (or gRPC-Web) stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub compressed: bool,
    /// gRPC-Web frames with the high bit set carry trailers instead of a message.
    pub trailers: bool,
    pub payload: Vec<u8>,
}

/// Split a gRPC body into its length-prefixed frames.
pub fn split_frames(body: &[u8]) -> Result<Vec<Frame>, String> {
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos < body.len() {
        let header = body.get(pos..pos + 5).ok_or("truncated gRPC frame header")?;
        let flags = header[0];
        let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
        let end = (pos + 5).checked_add(len).filter(|e| *e <= body.len()).ok_or("truncated gRPC frame")?;
        frames.push(Frame {
            compressed: flags & 1 == 1,
            trailers: flags & 0x80 != 0,
            payload: body[pos + 5..end].to_vec(),
        });
        pos = end;
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use prost::Message;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet, MethodDescriptorProto,
        ServiceDescriptorProto,
        field_descriptor_proto::{Label, Type},
    };

    use super::*;

    fn field(name: &str, number: i32, ty: Type, repeated: bool) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.into()),
            number: Some(number),
            r#type: Some(ty as i32),
            label: Some(if repeated { Label::Repeated } else { Label::Optional } as i32),
            json_name: Some(name.into()),
            ..Default::default()
        }
    }

    fn descriptor_set() -> Vec<u8> {
        use prost::Message;
        let file = FileDescriptorProto {
            name: Some("greeter.proto".into()),
            package: Some("demo".into()),
            syntax: Some("proto3".into()),
            message_type: vec![
                DescriptorProto {
                    name: Some("HelloRequest".into()),
                    field: vec![field("name", 1, Type::String, false), field("tags", 2, Type::String, true)],
                    ..Default::default()
                },
                DescriptorProto {
                    name: Some("HelloReply".into()),
                    field: vec![field("message", 1, Type::String, false), field("count", 2, Type::Int32, false)],
                    ..Default::default()
                },
            ],
            service: vec![ServiceDescriptorProto {
                name: Some("Greeter".into()),
                method: vec![MethodDescriptorProto {
                    name: Some("SayHello".into()),
                    input_type: Some(".demo.HelloRequest".into()),
                    output_type: Some(".demo.HelloReply".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        FileDescriptorSet { file: vec![file] }.encode_to_vec()
    }

    fn registry() -> ProtoRegistry {
        let mut r = ProtoRegistry::new();
        r.add_descriptor_set(&descriptor_set()).unwrap();
        r
    }

    #[test]
    fn raw_decoding_of_known_bytes() {
        // field 1 = varint 150, field 2 = "testing"  (the example from the protobuf encoding guide)
        let bytes = [0x08, 0x96, 0x01, 0x12, 0x07, b't', b'e', b's', b't', b'i', b'n', b'g'];
        assert_eq!(decode_raw(&bytes).unwrap(), json!({"1": 150, "2": "testing"}));
    }

    #[test]
    fn raw_decoding_handles_nesting_repeats_fixed_and_binary() {
        // field 3 = nested { 1: 1 }, field 4 repeated varints, field 5 fixed32, field 6 fixed64, field 7 binary
        let mut b =
            vec![0x1a, 0x02, 0x08, 0x01, 0x20, 0x01, 0x20, 0x02, 0x2d, 0x07, 0, 0, 0, 0x31, 0x09, 0, 0, 0, 0, 0, 0, 0];
        b.extend_from_slice(&[0x3a, 0x03, 0xff, 0xfe, 0xfd]);
        let v = decode_raw(&b).unwrap();
        assert_eq!(v["3"], json!({"1": 1}));
        assert_eq!(v["4"], json!([1, 2]));
        assert_eq!(v["5"], 7);
        assert_eq!(v["6"], 9);
        assert_eq!(v["7"], "hex:fffefd");
    }

    #[test]
    fn raw_decoding_rejects_malformed_input() {
        assert!(decode_raw(&[0x08]).is_err(), "truncated varint value");
        assert!(decode_raw(&[0x12, 0x05, 0x01]).is_err(), "length beyond the data");
        assert!(decode_raw(&[0x00, 0x01]).is_err(), "field number zero");
        assert!(decode_raw(&[0x0b]).is_err(), "groups");
        assert_eq!(decode_raw(&[]).unwrap(), json!({}));
    }

    #[test]
    fn schema_decoding_uses_field_names() {
        let r = registry();
        let desc = r.message("demo.HelloRequest").unwrap();
        let mut msg = DynamicMessage::new(desc.clone());
        msg.set_field_by_name("name", prost_reflect::Value::String("ada".into()));
        msg.set_field_by_name(
            "tags",
            prost_reflect::Value::List(vec![
                prost_reflect::Value::String("a".into()),
                prost_reflect::Value::String("b".into()),
            ]),
        );
        let bytes = msg.encode_to_vec();
        assert_eq!(decode_with(&bytes, &desc).unwrap(), json!({"name": "ada", "tags": ["a", "b"]}));
    }

    #[test]
    fn registry_lookups_and_bad_descriptor_sets() {
        let r = registry();
        assert!(!r.is_empty());
        assert_eq!(r.method_names(), vec!["demo.Greeter/SayHello"]);
        assert_eq!(r.message_names(), vec!["demo.HelloReply", "demo.HelloRequest"]);
        let m = r.method("/demo.Greeter/SayHello").unwrap();
        assert_eq!(m.input().full_name(), "demo.HelloRequest");
        assert_eq!(m.output().full_name(), "demo.HelloReply");
        assert!(r.method("/demo.Greeter/Nope").is_none());
        assert!(r.method("garbage").is_none());
        let mut r2 = ProtoRegistry::new();
        assert!(r2.add_descriptor_set(b"definitely not protobuf").is_err());
        assert!(r2.is_empty());
    }

    #[test]
    fn grpc_frames() {
        let mut body = Vec::new();
        for payload in [&b"abc"[..], &b""[..], &b"de"[..]] {
            body.push(0);
            body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            body.extend_from_slice(payload);
        }
        // a gRPC-Web trailers frame
        body.extend_from_slice(&[0x80, 0, 0, 0, 2, b'o', b'k']);
        let frames = split_frames(&body).unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].payload, b"abc");
        assert!(frames[1].payload.is_empty());
        assert!(frames[3].trailers && !frames[0].trailers);
        assert!(split_frames(&[0, 0, 0, 0, 9, 1]).is_err());
        assert!(split_frames(&[0, 0]).is_err());
        assert!(split_frames(&[]).unwrap().is_empty());
    }
}
