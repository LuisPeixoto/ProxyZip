use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use proxyzip_core::Flow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: &str = "proxyzip-suite";
pub const VERSION: u32 = 1;

fn yes() -> bool {
    true
}

fn get() -> String {
    "GET".into()
}

/// Something that must be true of a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Assertion {
    /// The status equals `equals`, or is in a class such as `2xx`.
    Status {
        #[serde(default)]
        equals: Option<u16>,
        #[serde(default)]
        class: Option<String>,
    },
    /// A response header is present, and optionally equal to or containing a value.
    Header {
        name: String,
        #[serde(default)]
        equals: Option<String>,
        #[serde(default)]
        contains: Option<String>,
    },
    /// A jq expression applied to the JSON body. Without `equals` the first result must be truthy.
    Jq {
        expr: String,
        #[serde(default)]
        equals: Option<Value>,
    },
    BodyContains {
        text: String,
    },
    DurationBelow {
        ms: u64,
    },
    /// The exchange conforms to an OpenAPI specification (path relative to the suite file).
    OpenApi {
        spec: String,
    },
}

/// Take a value from the response and keep it in a variable for the requests that follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extract {
    pub var: String,
    /// jq expression on the JSON body; the first result is stored.
    #[serde(default)]
    pub jq: Option<String>,
    /// Name of a response header.
    #[serde(default)]
    pub header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteRequest {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "get")]
    pub method: String,
    /// May contain `{{vars.name}}` and the other [template](proxyzip_core::template) expressions.
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
    #[serde(default)]
    pub extract: Vec<Extract>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suite {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// Named sets of variables that override `variables` (`--env staging`).
    #[serde(default)]
    pub environments: BTreeMap<String, BTreeMap<String, String>>,
    pub requests: Vec<SuiteRequest>,
}

impl Suite {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            name: name.into(),
            variables: BTreeMap::new(),
            environments: BTreeMap::new(),
            requests: Vec::new(),
        }
    }

    pub fn from_json(text: &str) -> Result<Suite> {
        let suite: Suite = serde_json::from_str(text).context("this is not a ProxyZip suite file")?;
        if suite.format != FORMAT {
            bail!("this is not a ProxyZip suite file");
        }
        if suite.version > VERSION {
            bail!(
                "this suite was saved by a newer ProxyZip (format version {}); update the app to run it",
                suite.version
            );
        }
        Ok(suite)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl SuiteRequest {
    /// A request that replays a captured flow, asserting the status it returned.
    pub fn from_flow(flow: &Flow) -> SuiteRequest {
        let skip = [
            "host",
            "content-length",
            "connection",
            "proxy-connection",
            "keep-alive",
            "transfer-encoding",
            "te",
            "upgrade",
            "accept-encoding",
        ];
        let headers = flow
            .request
            .headers
            .iter()
            .filter(|(k, _)| !skip.contains(&k.to_ascii_lowercase().as_str()))
            .cloned()
            .collect();
        let (decoded, _, _) = proxyzip_codec::decode_content(&flow.request.headers, &flow.request.body);
        let body = (!decoded.is_empty()).then(|| String::from_utf8_lossy(&decoded).into_owned());
        let path = flow.request.path.split('?').next().unwrap_or("");
        SuiteRequest {
            name: format!("{} {}", flow.request.method, path),
            enabled: true,
            method: flow.request.method.clone(),
            url: flow.request.url(),
            headers,
            body,
            assertions: flow
                .response
                .as_ref()
                .map(|r| vec![Assertion::Status { equals: Some(r.status), class: None }])
                .unwrap_or_default(),
            extract: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use proxyzip_core::{Headers, Request, Response, Scheme};

    #[test]
    fn round_trips_and_applies_defaults() {
        let text = r#"{"format":"proxyzip-suite","version":1,"name":"s","requests":[{"name":"a","url":"http://x/","assertions":[{"type":"status","equals":200},{"type":"jq","expr":".a","equals":1}]}]}"#;
        let suite = Suite::from_json(text).unwrap();
        let r = &suite.requests[0];
        assert!(r.enabled && r.method == "GET" && r.headers.is_empty());
        assert_eq!(r.assertions[0], Assertion::Status { equals: Some(200), class: None });
        assert_eq!(Suite::from_json(&suite.to_json().unwrap()).unwrap(), suite);
    }

    #[test]
    fn rejects_other_files_and_newer_versions() {
        assert!(Suite::from_json("{}").is_err());
        assert!(Suite::from_json(r#"{"format":"x","version":1,"name":"s","requests":[]}"#).is_err());
        assert!(
            Suite::from_json(r#"{"format":"proxyzip-suite","version":9,"name":"s","requests":[]}"#)
                .unwrap_err()
                .to_string()
                .contains("newer ProxyZip")
        );
    }

    #[test]
    fn a_flow_becomes_a_request_that_expects_the_same_status() {
        let mut f = Flow::new(
            1,
            "x".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Https,
                host: "api.test".into(),
                port: 443,
                path: "/users?x=1".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![
                    ("Host".into(), "api.test".into()),
                    ("content-type".into(), "application/json".into()),
                    ("Content-Length".into(), "2".into()),
                ]),
                body: Bytes::from_static(b"{}"),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status: 201,
            reason: String::new(),
            http_version: "HTTP/1.1".into(),
            headers: Headers::default(),
            body: Bytes::new(),
            body_truncated: false,
        });
        let r = SuiteRequest::from_flow(&f);
        assert_eq!(r.name, "POST /users");
        assert_eq!(r.url, "https://api.test/users?x=1");
        assert_eq!(r.headers, vec![("content-type".to_string(), "application/json".to_string())]);
        assert_eq!(r.body.as_deref(), Some("{}"));
        assert_eq!(r.assertions, vec![Assertion::Status { equals: Some(201), class: None }]);
    }
}
