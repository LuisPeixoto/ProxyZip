//! Turns raw captured bodies into something a human can read: removes
//! content-encoding, classifies the payload and prepares text/pretty views.

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use proxyzip_core::Headers;
use serde::Serialize;

/// Decoded bodies beyond this size are not pretty-printed or sent as text.
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
/// Hard cap on decompressed output, protects against decompression bombs.
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    Empty,
    Json,
    Xml,
    Html,
    Javascript,
    Css,
    Form,
    Multipart,
    Text,
    Image,
    Protobuf,
    Binary,
}

#[derive(Debug, Clone, Serialize)]
pub struct BodyView {
    pub kind: BodyKind,
    pub mime: Option<String>,
    /// Content-encoding chain that was removed, e.g. `["gzip"]`.
    pub encodings: Vec<String>,
    pub decode_error: Option<String>,
    pub raw_size: usize,
    pub size: usize,
    /// UTF-8 text of the decoded body, if it is text and not too large.
    pub text: Option<String>,
    /// Pretty-printed variant (JSON / form) when applicable.
    pub pretty: Option<String>,
    /// Decoded bytes, base64 (for hex/image viewers). Omitted when huge.
    pub base64: Option<String>,
    pub truncated: bool,
}

/// Remove `Content-Encoding` layers (applied in reverse order of listing).
pub fn decode_content(headers: &Headers, body: &[u8]) -> (Bytes, Vec<String>, Option<String>) {
    let encodings: Vec<String> = headers
        .get_all("content-encoding")
        .flat_map(|v| v.split(','))
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty() && e != "identity")
        .collect();
    let mut data = body.to_vec();
    for enc in encodings.iter().rev() {
        match decode_one(enc, &data) {
            Ok(d) => data = d,
            Err(e) => return (Bytes::from(data), encodings.clone(), Some(format!("{enc}: {e}"))),
        }
    }
    (Bytes::from(data), encodings, None)
}

fn read_limited(mut r: impl Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    r.by_ref().take(MAX_DECODED_BYTES as u64).read_to_end(&mut out)?;
    Ok(out)
}

pub fn decode_one(encoding: &str, data: &[u8]) -> std::io::Result<Vec<u8>> {
    match encoding {
        "gzip" | "x-gzip" => read_limited(flate2::read::MultiGzDecoder::new(data)),
        "deflate" => {
            // "deflate" is zlib-wrapped per spec, but many servers send raw deflate.
            read_limited(flate2::read::ZlibDecoder::new(data))
                .or_else(|_| read_limited(flate2::read::DeflateDecoder::new(data)))
        }
        "br" => read_limited(brotli::Decompressor::new(data, 64 * 1024)),
        "zstd" => read_limited(zstd::stream::read::Decoder::new(data)?),
        other => Err(std::io::Error::new(std::io::ErrorKind::Unsupported, format!("unsupported encoding {other}"))),
    }
}

pub fn classify(mime: Option<&str>, body: &[u8]) -> BodyKind {
    if body.is_empty() {
        return BodyKind::Empty;
    }
    let mime = mime.unwrap_or("").to_ascii_lowercase();
    let essence = mime.split(';').next().unwrap_or("").trim();
    match essence {
        m if m.contains("json") => return BodyKind::Json,
        m if m.contains("html") => return BodyKind::Html,
        m if m.ends_with("xml") || m.contains("+xml") => return BodyKind::Xml,
        m if m.contains("javascript") || m.contains("ecmascript") => return BodyKind::Javascript,
        "text/css" => return BodyKind::Css,
        "application/x-www-form-urlencoded" => return BodyKind::Form,
        m if m.starts_with("multipart/") => return BodyKind::Multipart,
        m if m.starts_with("image/") => return BodyKind::Image,
        m if m.contains("protobuf") || m.contains("grpc") => return BodyKind::Protobuf,
        m if m.starts_with("text/") => return BodyKind::Text,
        _ => {}
    }
    // Sniff.
    let trimmed = body.iter().position(|b| !b.is_ascii_whitespace()).map(|i| &body[i..]).unwrap_or(body);
    if matches!(trimmed.first(), Some(b'{') | Some(b'[')) && serde_json::from_slice::<serde_json::Value>(body).is_ok() {
        return BodyKind::Json;
    }
    if trimmed.starts_with(b"<?xml") {
        return BodyKind::Xml;
    }
    if trimmed.len() > 5 && (trimmed[..5].eq_ignore_ascii_case(b"<!doc") || trimmed[..5].eq_ignore_ascii_case(b"<html"))
    {
        return BodyKind::Html;
    }
    if is_image_magic(body) {
        return BodyKind::Image;
    }
    if std::str::from_utf8(body).is_ok() {
        return BodyKind::Text;
    }
    BodyKind::Binary
}

fn is_image_magic(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG")
        || b.starts_with(b"\xff\xd8\xff")
        || b.starts_with(b"GIF8")
        || (b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

pub fn pretty_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Decode an `application/x-www-form-urlencoded` string / URL query into pairs.
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(q.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

pub fn pretty_form(text: &str) -> String {
    url::form_urlencoded::parse(text.as_bytes()).map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join("\n")
}

/// Build the full presentation of a body.
pub fn view(headers: &Headers, body: &[u8], truncated: bool) -> BodyView {
    let (decoded, encodings, decode_error) = decode_content(headers, body);
    let mime = headers.get("content-type").map(str::to_string);
    let kind = classify(mime.as_deref(), &decoded);
    let textual = !matches!(kind, BodyKind::Image | BodyKind::Binary | BodyKind::Protobuf | BodyKind::Empty);
    let text = (textual && decoded.len() <= MAX_TEXT_BYTES).then(|| String::from_utf8_lossy(&decoded).into_owned());
    let pretty = match (&kind, &text) {
        (BodyKind::Json, Some(t)) => pretty_json(t),
        (BodyKind::Form, Some(t)) => Some(pretty_form(t)),
        _ => None,
    };
    let base64 = (decoded.len() <= MAX_TEXT_BYTES).then(|| B64.encode(&decoded));
    BodyView {
        kind,
        mime,
        encodings,
        decode_error,
        raw_size: body.len(),
        size: decoded.len(),
        text,
        pretty,
        base64,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn headers(pairs: &[(&str, &str)]) -> Headers {
        Headers(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn gzip_json_roundtrip() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(br#"{"a":1,"b":[true]}"#).unwrap();
        let gz = enc.finish().unwrap();
        let h = headers(&[("Content-Encoding", "gzip"), ("Content-Type", "application/json; charset=utf-8")]);
        let v = view(&h, &gz, false);
        assert_eq!(v.kind, BodyKind::Json);
        assert_eq!(v.encodings, vec!["gzip"]);
        assert!(v.pretty.unwrap().contains("\"a\": 1"));
    }

    #[test]
    fn brotli_and_zstd() {
        let mut br = Vec::new();
        brotli::CompressorWriter::new(&mut br, 4096, 5, 22).write_all(b"hello").unwrap();
        assert_eq!(decode_one("br", &br).unwrap(), b"hello");
        let z = zstd::encode_all(&b"world"[..], 3).unwrap();
        assert_eq!(decode_one("zstd", &z).unwrap(), b"world");
    }

    #[test]
    fn sniffing() {
        assert_eq!(classify(None, b"  {\"x\":2}"), BodyKind::Json);
        assert_eq!(classify(None, b"\x89PNG\r\n"), BodyKind::Image);
        assert_eq!(classify(None, &[0xff, 0x00, 0xfe]), BodyKind::Binary);
        assert_eq!(classify(Some("application/x-www-form-urlencoded"), b"a=1"), BodyKind::Form);
    }
}
