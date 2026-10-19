//! Pure functions that apply rewrite operations to a request or response.

use bytes::Bytes;
use proxyzip_core::Headers;

use crate::model::RewriteOp;

#[derive(Debug, Clone)]
pub struct RequestParts {
    pub method: String,
    /// Path including the query string.
    pub path: String,
    pub headers: Headers,
    pub body: Bytes,
}

#[derive(Debug, Clone)]
pub struct ResponseParts {
    pub status: u16,
    pub headers: Headers,
    pub body: Bytes,
}

/// Remove every header called `name` (case-insensitive).
pub fn remove_header(headers: &mut Headers, name: &str) {
    headers.0.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
}

/// Replace every header called `name` with a single one, keeping the position of the first.
pub fn set_header(headers: &mut Headers, name: &str, value: &str) {
    let first = headers.0.iter().position(|(k, _)| k.eq_ignore_ascii_case(name));
    remove_header(headers, name);
    match first {
        Some(i) if i <= headers.0.len() => headers.0.insert(i, (name.to_string(), value.to_string())),
        _ => headers.push(name, value),
    }
}

/// Remove the content encoding so the body can be read and edited. Returns whether anything changed.
fn decode_in_place(headers: &mut Headers, body: &mut Bytes) -> bool {
    if headers.get("content-encoding").is_none() {
        return false;
    }
    let (decoded, _, error) = proxyzip_codec::decode_content(headers, body);
    if let Some(e) = error {
        tracing::debug!(error = %e, "leaving the body encoded");
        return false;
    }
    *body = decoded;
    remove_header(headers, "content-encoding");
    remove_header(headers, "content-length");
    true
}

/// Make a request body readable (decoded, no content-length). Returns whether it was encoded.
pub fn decode_request_body(req: &mut RequestParts) -> bool {
    decode_in_place(&mut req.headers, &mut req.body)
}

/// Make a response body readable (decoded, no content-length). Returns whether it was encoded.
pub fn decode_response_body(res: &mut ResponseParts) -> bool {
    decode_in_place(&mut res.headers, &mut res.body)
}

fn replace_body(headers: &mut Headers, body: &mut Bytes, find: &str, replace: &str, regex: bool) -> bool {
    if find.is_empty() {
        return false;
    }
    // Only text bodies are edited; binary payloads are left untouched.
    let mut candidate = body.clone();
    let mut candidate_headers = headers.clone();
    decode_in_place(&mut candidate_headers, &mut candidate);
    let Ok(text) = std::str::from_utf8(&candidate) else { return false };
    let replaced = if regex {
        match regex::Regex::new(find) {
            Ok(re) => re.replace_all(text, replace).into_owned(),
            Err(e) => {
                tracing::warn!(find, error = %e, "invalid regular expression in rewrite rule");
                return false;
            }
        }
    } else {
        text.replace(find, replace)
    };
    if replaced == text {
        return false;
    }
    *body = Bytes::from(replaced);
    *headers = candidate_headers;
    remove_header(headers, "content-length");
    true
}

fn edit_query(path: &mut String, edit: impl FnOnce(&mut Vec<(String, String)>)) {
    let (base, query) = match path.split_once('?') {
        Some((b, q)) => (b.to_string(), q.to_string()),
        None => (path.clone(), String::new()),
    };
    let mut pairs: Vec<(String, String)> =
        url::form_urlencoded::parse(query.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    edit(&mut pairs);
    *path = if pairs.is_empty() {
        base
    } else {
        let q = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&pairs).finish();
        format!("{base}?{q}")
    };
}

/// Apply request-phase operations. Returns whether the request changed.
pub fn apply_request_ops(ops: &[RewriteOp], req: &mut RequestParts) -> bool {
    let mut changed = false;
    for op in ops {
        match op {
            RewriteOp::SetHeader { name, value } => {
                set_header(&mut req.headers, name, value);
                changed = true;
            }
            RewriteOp::AddHeader { name, value } => {
                req.headers.push(name.clone(), value.clone());
                changed = true;
            }
            RewriteOp::RemoveHeader { name } => {
                remove_header(&mut req.headers, name);
                changed = true;
            }
            RewriteOp::SetQuery { name, value } => {
                edit_query(&mut req.path, |pairs| {
                    let first = pairs.iter().position(|(k, _)| k == name);
                    pairs.retain(|(k, _)| k != name);
                    let entry = (name.clone(), value.clone());
                    match first {
                        Some(i) => pairs.insert(i, entry),
                        None => pairs.push(entry),
                    }
                });
                changed = true;
            }
            RewriteOp::RemoveQuery { name } => {
                edit_query(&mut req.path, |pairs| pairs.retain(|(k, _)| k != name));
                changed = true;
            }
            RewriteOp::ReplaceBody { find, replace, regex } => {
                changed |= replace_body(&mut req.headers, &mut req.body, find, replace, *regex);
            }
            RewriteOp::SetStatus { .. } => {}
        }
    }
    changed
}

/// Apply response-phase operations. Returns whether the response changed.
pub fn apply_response_ops(ops: &[RewriteOp], res: &mut ResponseParts) -> bool {
    let mut changed = false;
    for op in ops {
        match op {
            RewriteOp::SetHeader { name, value } => {
                set_header(&mut res.headers, name, value);
                changed = true;
            }
            RewriteOp::AddHeader { name, value } => {
                res.headers.push(name.clone(), value.clone());
                changed = true;
            }
            RewriteOp::RemoveHeader { name } => {
                remove_header(&mut res.headers, name);
                changed = true;
            }
            RewriteOp::SetStatus { status } => {
                res.status = *status;
                changed = true;
            }
            RewriteOp::ReplaceBody { find, replace, regex } => {
                changed |= replace_body(&mut res.headers, &mut res.body, find, replace, *regex);
            }
            RewriteOp::SetQuery { .. } | RewriteOp::RemoveQuery { .. } => {}
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Headers {
        Headers(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    fn request(path: &str) -> RequestParts {
        RequestParts {
            method: "GET".into(),
            path: path.into(),
            headers: headers(&[("Accept", "*/*"), ("X-Debug", "0")]),
            body: Bytes::new(),
        }
    }

    fn response(body: &'static [u8]) -> ResponseParts {
        ResponseParts {
            status: 200,
            headers: headers(&[("content-type", "text/plain")]),
            body: Bytes::from_static(body),
        }
    }

    #[test]
    fn header_operations() {
        let mut req = request("/");
        let ops = [
            RewriteOp::SetHeader { name: "x-debug".into(), value: "1".into() },
            RewriteOp::AddHeader { name: "x-extra".into(), value: "a".into() },
            RewriteOp::AddHeader { name: "x-extra".into(), value: "b".into() },
            RewriteOp::RemoveHeader { name: "ACCEPT".into() },
        ];
        assert!(apply_request_ops(&ops, &mut req));
        assert_eq!(req.headers.get("x-debug"), Some("1"));
        assert_eq!(req.headers.get_all("x-extra").collect::<Vec<_>>(), ["a", "b"]);
        assert!(req.headers.get("accept").is_none());
        // set_header keeps the original position.
        assert_eq!(req.headers.0[0].0, "x-debug");
    }

    #[test]
    fn query_operations() {
        let mut req = request("/search?q=rust&page=2");
        apply_request_ops(&[RewriteOp::SetQuery { name: "page".into(), value: "3".into() }], &mut req);
        assert_eq!(req.path, "/search?q=rust&page=3");
        apply_request_ops(&[RewriteOp::SetQuery { name: "lang".into(), value: "pt BR".into() }], &mut req);
        assert_eq!(req.path, "/search?q=rust&page=3&lang=pt+BR");
        apply_request_ops(
            &[RewriteOp::RemoveQuery { name: "q".into() }, RewriteOp::RemoveQuery { name: "page".into() }],
            &mut req,
        );
        assert_eq!(req.path, "/search?lang=pt+BR");
        apply_request_ops(&[RewriteOp::RemoveQuery { name: "lang".into() }], &mut req);
        assert_eq!(req.path, "/search");
    }

    #[test]
    fn status_and_body() {
        let mut res = response(b"hello world, hello rust");
        res.headers.push("content-length", "23");
        let ops = [
            RewriteOp::SetStatus { status: 503 },
            RewriteOp::ReplaceBody { find: "hello".into(), replace: "bye".into(), regex: false },
        ];
        assert!(apply_response_ops(&ops, &mut res));
        assert_eq!(res.status, 503);
        assert_eq!(&res.body[..], b"bye world, bye rust");
        assert!(res.headers.get("content-length").is_none(), "length is recomputed by the proxy");
    }

    #[test]
    fn regex_body_with_captures() {
        let mut res = response(br#"{"id": 42, "name": "x"}"#);
        let op =
            RewriteOp::ReplaceBody { find: r#""id": (\d+)"#.into(), replace: r#""id": "user-$1""#.into(), regex: true };
        assert!(apply_response_ops(&[op], &mut res));
        assert_eq!(&res.body[..], br#"{"id": "user-42", "name": "x"}"#);
    }

    #[test]
    fn compressed_bodies_are_decoded_before_editing() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"secret=1").unwrap();
        let mut res = ResponseParts {
            status: 200,
            headers: headers(&[("content-encoding", "gzip"), ("content-length", "999")]),
            body: Bytes::from(enc.finish().unwrap()),
        };
        let op = RewriteOp::ReplaceBody { find: "secret".into(), replace: "public".into(), regex: false };
        assert!(apply_response_ops(&[op], &mut res));
        assert_eq!(&res.body[..], b"public=1");
        assert!(res.headers.get("content-encoding").is_none());
        assert!(res.headers.get("content-length").is_none());
    }

    #[test]
    fn binary_and_unmatched_bodies_are_left_alone() {
        let mut res = response(&[0xff, 0xfe, 0x00]);
        let op = RewriteOp::ReplaceBody { find: "a".into(), replace: "b".into(), regex: false };
        assert!(!apply_response_ops(std::slice::from_ref(&op), &mut res));
        let mut text = response(b"nothing to see");
        assert!(!apply_response_ops(&[op], &mut text));
        assert_eq!(&text.body[..], b"nothing to see");
    }

    #[test]
    fn decode_response_body_normalizes() {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"plain").unwrap();
        let mut res = ResponseParts {
            status: 200,
            headers: headers(&[("Content-Encoding", "gzip")]),
            body: Bytes::from(enc.finish().unwrap()),
        };
        assert!(decode_response_body(&mut res));
        assert_eq!(&res.body[..], b"plain");
        assert!(!decode_response_body(&mut res), "second call is a no-op");
    }

    #[test]
    fn phase_specific_ops_are_ignored_in_the_other_phase() {
        let mut req = request("/");
        assert!(!apply_request_ops(&[RewriteOp::SetStatus { status: 500 }], &mut req));
        let mut res = response(b"");
        assert!(!apply_response_ops(&[RewriteOp::SetQuery { name: "a".into(), value: "b".into() }], &mut res));
    }
}
