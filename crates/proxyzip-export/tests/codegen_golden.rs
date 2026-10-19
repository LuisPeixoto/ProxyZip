//! Golden tests for the code generators.
//!
//! Every language is rendered for three requests (a JSON POST with awkward characters, a PATCH with a binary body and
//! a plain GET) and compared with the files in `tests/golden/codegen`. After an intentional change, regenerate with
//! `UPDATE_GOLDEN=1 cargo test -p proxyzip-export --test codegen_golden` and review the diff.

use std::path::PathBuf;

use bytes::Bytes;
use proxyzip_core::{Flow, Headers, Request, Scheme};
use proxyzip_export::codegen::{Language, generate};

fn flow(method: &str, body: &[u8]) -> Flow {
    Flow::new(
        1,
        "127.0.0.1:1".into(),
        Request {
            method: method.into(),
            scheme: Scheme::Https,
            host: "api.example.com".into(),
            port: 443,
            path: "/v1/items?id=2".into(),
            http_version: "HTTP/2".into(),
            headers: Headers(vec![
                ("Host".into(), "api.example.com".into()),
                ("Content-Type".into(), "application/json".into()),
                ("Authorization".into(), "Bearer abc".into()),
                ("X-Note".into(), "it's \"quoted\" $5 #{x}".into()),
                ("Accept-Encoding".into(), "gzip".into()),
                ("Content-Length".into(), "999".into()),
            ]),
            body: Bytes::copy_from_slice(body),
            body_truncated: false,
        },
    )
}

fn golden_path(case: &str, language: Language) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/codegen").join(format!("{case}.{}.txt", language.id()))
}

#[test]
fn every_language_matches_its_golden_file() {
    let cases: [(&str, Flow); 3] = [
        ("post-json", flow("POST", b"{\"name\":\"ada\",\"note\":\"line1\\nline2 \\\"q\\\" $x\"}")),
        ("patch-binary", flow("PATCH", &[0xff, 0x00, 0x01])),
        ("get", flow("GET", b"")),
    ];
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut mismatches = Vec::new();
    for (case, flow) in &cases {
        for language in Language::ALL {
            let actual = generate(flow, language);
            let path = golden_path(case, language);
            if update {
                std::fs::write(&path, &actual).unwrap();
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(expected) if expected == actual => {}
                Ok(_) => mismatches.push(format!("{case}/{}: output changed", language.id())),
                Err(_) => mismatches.push(format!("{case}/{}: missing golden file {}", language.id(), path.display())),
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn language_ids_round_trip() {
    for language in Language::ALL {
        assert_eq!(Language::from_id(language.id()), Some(language));
        assert!(!language.label().is_empty() && !language.syntax().is_empty());
    }
    assert_eq!(Language::from_id("PYTHON"), Some(Language::Python));
    assert_eq!(Language::from_id("cobol"), None);
}

#[test]
fn compressed_request_bodies_are_decoded_for_the_generated_code() {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(br#"{"a":1}"#).unwrap();
    let mut f = flow("POST", &encoder.finish().unwrap());
    f.request.headers.push("Content-Encoding", "gzip");
    let code = generate(&f, Language::Python);
    assert!(code.contains(r#"{\"a\":1}"#), "{code}");
    assert!(!code.to_ascii_lowercase().contains("content-encoding"), "the header no longer matches the body:\n{code}");
}
