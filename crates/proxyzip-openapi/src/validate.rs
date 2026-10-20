//! Checking captured traffic against a specification.

use proxyzip_core::{Flow, FlowId};
use proxyzip_rules::match_path;
use serde::Serialize;
use serde_json::Value;

use crate::schema::{resolve, to_json_schema};
use crate::spec::{Operation, Spec, to_route_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationKind {
    UndocumentedPath,
    UndocumentedMethod,
    MissingParameter,
    InvalidParameter,
    MissingRequestBody,
    InvalidRequestBody,
    UndocumentedStatus,
    InvalidResponseBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub flow_id: FlowId,
    pub method: String,
    pub url: String,
    /// The operation it was checked against, e.g. `GET /pets/{petId}`.
    pub operation: Option<String>,
    pub kind: ViolationKind,
    pub message: String,
    /// JSON Pointer to the offending value inside the body.
    pub pointer: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Flows that belong to the API and were checked.
    pub checked: usize,
    /// Flows ignored because they are for another host or outside the API.
    pub skipped: usize,
    pub violations: Vec<Violation>,
}

impl Report {
    /// Distinct flows with at least one problem.
    pub fn flows_with_violations(&self) -> Vec<FlowId> {
        let mut ids: Vec<FlowId> = self.violations.iter().map(|v| v.flow_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

const MAX_BODY_ERRORS: usize = 5;

/// Check each flow against `spec`.
pub fn validate_flows(spec: &Spec, flows: &[Flow]) -> Report {
    let operations = spec.operations();
    let servers = spec.servers();
    let mut report = Report::default();
    for flow in flows {
        let Some(path) = relative_path(&servers, &flow.request.host, &flow.request.path) else {
            report.skipped += 1;
            continue;
        };
        match find_operation(&operations, &flow.request.method, &path) {
            Lookup::NoPath => {
                if servers.is_empty() {
                    report.skipped += 1;
                } else {
                    report.checked += 1;
                    report.violations.push(violation(
                        flow,
                        None,
                        ViolationKind::UndocumentedPath,
                        format!("{} is not in the specification", path_only(&path)),
                        None,
                    ));
                }
            }
            Lookup::NoMethod(allowed) => {
                report.checked += 1;
                report.violations.push(violation(
                    flow,
                    None,
                    ViolationKind::UndocumentedMethod,
                    format!(
                        "{} is not documented for {} (documented: {})",
                        flow.request.method,
                        path_only(&path),
                        allowed.join(", ")
                    ),
                    None,
                ));
            }
            Lookup::Found(op, params) => {
                report.checked += 1;
                check_operation(spec, flow, op, &params, &path, &mut report.violations);
            }
        }
    }
    report
}

enum Lookup<'a> {
    NoPath,
    NoMethod(Vec<String>),
    Found(&'a Operation, std::collections::HashMap<String, String>),
}

fn path_only(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// The path below the server's base path, when the host belongs to the API.
fn relative_path(servers: &[(String, String)], host: &str, path: &str) -> Option<String> {
    if servers.is_empty() {
        return Some(path.to_string());
    }
    servers.iter().find_map(|(h, base)| {
        if !h.eq_ignore_ascii_case(host) {
            return None;
        }
        let rest = path.strip_prefix(base.as_str())?;
        (rest.is_empty() || rest.starts_with('/') || rest.starts_with('?'))
            .then(|| if rest.is_empty() { "/".to_string() } else { rest.to_string() })
    })
}

fn find_operation<'a>(operations: &'a [Operation], method: &str, path: &str) -> Lookup<'a> {
    let mut candidates: Vec<(&Operation, std::collections::HashMap<String, String>)> =
        operations.iter().filter_map(|op| match_path(&to_route_path(&op.path), path).map(|p| (op, p))).collect();
    if candidates.is_empty() {
        return Lookup::NoPath;
    }
    // Prefer `/pets/mine` over `/pets/{id}`.
    candidates.sort_by_key(|(op, params)| params.len() + op.path.matches('{').count());
    if let Some((op, params)) = candidates.iter().find(|(op, _)| op.method.eq_ignore_ascii_case(method)) {
        return Lookup::Found(op, params.clone());
    }
    let mut allowed: Vec<String> = candidates.iter().map(|(op, _)| op.method.clone()).collect();
    allowed.sort();
    allowed.dedup();
    Lookup::NoMethod(allowed)
}

fn violation(
    flow: &Flow,
    op: Option<&Operation>,
    kind: ViolationKind,
    message: String,
    pointer: Option<String>,
) -> Violation {
    Violation {
        flow_id: flow.id,
        method: flow.request.method.clone(),
        url: flow.request.url(),
        operation: op.map(|o| format!("{} {}", o.method, o.path)),
        kind,
        message,
        pointer,
    }
}

fn check_operation(
    spec: &Spec,
    flow: &Flow,
    op: &Operation,
    path_params: &std::collections::HashMap<String, String>,
    path: &str,
    out: &mut Vec<Violation>,
) {
    let doc = spec.doc();
    let query = path.split_once('?').map(|(_, q)| proxyzip_codec::parse_query(q)).unwrap_or_default();
    for p in &op.parameters {
        let name = p.get("name").and_then(Value::as_str).unwrap_or("");
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        let value: Option<String> = match p.get("in").and_then(Value::as_str) {
            Some("query") => query.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
            Some("header") => flow.request.headers.get(name).map(str::to_string),
            Some("path") => path_params.get(name).cloned(),
            _ => continue,
        };
        match value {
            None if required => out.push(violation(
                flow,
                Some(op),
                ViolationKind::MissingParameter,
                format!("required {} parameter `{name}` is missing", p["in"].as_str().unwrap_or("")),
                None,
            )),
            None => {}
            Some(v) => {
                if let Some(schema) = p.get("schema")
                    && let Some(problem) = check_scalar(doc, schema, &v)
                {
                    out.push(violation(
                        flow,
                        Some(op),
                        ViolationKind::InvalidParameter,
                        format!("parameter `{name}`: {problem}"),
                        None,
                    ));
                }
            }
        }
    }

    if let Some(body_spec) = op.value.get("requestBody") {
        let body_spec = resolve(doc, body_spec);
        let (decoded, _, _) = proxyzip_codec::decode_content(&flow.request.headers, &flow.request.body);
        let required = body_spec.get("required").and_then(Value::as_bool).unwrap_or(false);
        if decoded.is_empty() {
            if required {
                out.push(violation(
                    flow,
                    Some(op),
                    ViolationKind::MissingRequestBody,
                    "the request body is required but empty".into(),
                    None,
                ));
            }
        } else if is_json(flow.request.headers.get("content-type"))
            && let Some(schema) = json_schema_of(&body_spec)
        {
            for (pointer, message) in check_json(doc, schema, &decoded) {
                out.push(violation(flow, Some(op), ViolationKind::InvalidRequestBody, message, pointer));
            }
        }
    }

    let Some(response) = &flow.response else { return };
    let responses = op.value.get("responses").and_then(Value::as_object);
    let status = response.status.to_string();
    let class = format!("{}XX", response.status / 100);
    let declared = responses.and_then(|r| r.get(&status).or_else(|| r.get(&class)).or_else(|| r.get("default")));
    let Some(declared) = declared else {
        let known: Vec<&str> = responses.map(|r| r.keys().map(String::as_str).collect()).unwrap_or_default();
        out.push(violation(
            flow,
            Some(op),
            ViolationKind::UndocumentedStatus,
            format!("status {status} is not documented (documented: {})", known.join(", ")),
            None,
        ));
        return;
    };
    let declared = resolve(doc, declared);
    let (decoded, _, _) = proxyzip_codec::decode_content(&response.headers, &response.body);
    if !decoded.is_empty()
        && is_json(response.headers.get("content-type"))
        && let Some(schema) = json_schema_of(&declared)
    {
        for (pointer, message) in check_json(doc, schema, &decoded) {
            out.push(violation(flow, Some(op), ViolationKind::InvalidResponseBody, message, pointer));
        }
    }
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|c| {
        let c = c.to_ascii_lowercase();
        c.contains("json")
    })
}

/// The schema of the JSON media type of a request body or response object.
fn json_schema_of(object: &Value) -> Option<&Value> {
    let content = object.get("content")?.as_object()?;
    let media = content.iter().find(|(k, _)| k.to_ascii_lowercase().contains("json")).map(|(_, v)| v)?;
    media.get("schema")
}

fn check_json(doc: &Value, schema: &Value, body: &[u8]) -> Vec<(Option<String>, String)> {
    let Ok(instance) = serde_json::from_slice::<Value>(body) else {
        return vec![(None, "the body is not valid JSON".into())];
    };
    let schema = to_json_schema(doc, schema);
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => return vec![(None, format!("the schema could not be used: {e}"))],
    };
    validator
        .iter_errors(&instance)
        .take(MAX_BODY_ERRORS)
        .map(|e| {
            let pointer = e.instance_path().to_string();
            ((!pointer.is_empty()).then_some(pointer), e.to_string())
        })
        .collect()
}

/// Query, header and path values arrive as text. Check the ones whose schema is a number or a boolean.
fn check_scalar(doc: &Value, schema: &Value, text: &str) -> Option<String> {
    let schema = to_json_schema(doc, schema);
    let kind = match schema.get("type") {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("").to_string(),
        _ => String::new(),
    };
    let instance = match kind.as_str() {
        "integer" => text.parse::<i64>().ok().map(Value::from),
        "number" => text.parse::<f64>().ok().and_then(|f| serde_json::Number::from_f64(f).map(Value::Number)),
        "boolean" => text.parse::<bool>().ok().map(Value::from),
        _ => Some(Value::String(text.to_string())),
    };
    let Some(instance) = instance else { return Some(format!("`{text}` is not a valid {kind}")) };
    let validator = jsonschema::validator_for(&schema).ok()?;
    validator.iter_errors(&instance).next().map(|e| format!("`{text}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use proxyzip_core::{Headers, Request, Response, Scheme};

    const PETSTORE: &str = include_str!("../tests/fixtures/petstore.yaml");
    const ORDERS: &str = include_str!("../tests/fixtures/orders-3.1.json");

    #[allow(clippy::too_many_arguments)] // a test helper that reads better as one call
    fn flow(
        id: u64,
        method: &str,
        host: &str,
        path: &str,
        req_headers: &[(&str, &str)],
        req_body: &str,
        status: u16,
        res_body: &str,
    ) -> Flow {
        let h = |v: &[(&str, &str)]| Headers(v.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let mut f = Flow::new(
            id,
            "127.0.0.1:1".into(),
            Request {
                method: method.into(),
                scheme: Scheme::Https,
                host: host.into(),
                port: 443,
                path: path.into(),
                http_version: "HTTP/1.1".into(),
                headers: h(req_headers),
                body: Bytes::from(req_body.to_string()),
                body_truncated: false,
            },
        );
        f.response = Some(Response {
            status,
            reason: String::new(),
            http_version: "HTTP/1.1".into(),
            headers: h(&[("content-type", "application/json")]),
            body: Bytes::from(res_body.to_string()),
            body_truncated: false,
        });
        f
    }

    fn pets() -> Spec {
        Spec::parse(PETSTORE).unwrap()
    }

    fn kinds(r: &Report) -> Vec<ViolationKind> {
        r.violations.iter().map(|v| v.kind).collect()
    }

    #[test]
    fn a_conforming_exchange_has_no_violations() {
        let flows = vec![
            flow(
                1,
                "GET",
                "petstore.example.com",
                "/v1/pets/7",
                &[("x-trace", "t")],
                "",
                200,
                r#"{"id":7,"name":"Rex","tag":null,"status":"available","owner":{"email":"a@b.co"}}"#,
            ),
            flow(
                2,
                "POST",
                "petstore.example.com",
                "/v1/pets",
                &[("content-type", "application/json")],
                r#"{"name":"Rex"}"#,
                201,
                r#"{"id":8,"name":"Rex"}"#,
            ),
            flow(3, "GET", "petstore.example.com", "/v1/pets?limit=10", &[], "", 200, "[]"),
            flow(4, "GET", "other.com", "/anything", &[], "", 200, "{}"),
        ];
        let report = validate_flows(&pets(), &flows);
        assert!(report.violations.is_empty(), "{:#?}", report.violations);
        assert_eq!((report.checked, report.skipped), (3, 1));
    }

    #[test]
    fn response_bodies_are_checked_against_the_schema() {
        let flows = vec![flow(
            1,
            "GET",
            "petstore.example.com",
            "/v1/pets/7",
            &[("x-trace", "t")],
            "",
            200,
            r#"{"id":"seven","status":"gone","owner":{}}"#,
        )];
        let report = validate_flows(&pets(), &flows);
        assert!(report.violations.iter().all(|v| v.kind == ViolationKind::InvalidResponseBody));
        let text: Vec<_> =
            report.violations.iter().map(|v| (v.pointer.clone().unwrap_or_default(), v.message.clone())).collect();
        assert!(text.iter().any(|(p, _)| p == "/id"), "{text:?}");
        assert!(text.iter().any(|(p, _)| p == "/status"), "{text:?}");
        assert!(text.iter().any(|(_, m)| m.contains("name")), "the missing `name` is reported: {text:?}");
        assert!(report.violations[0].operation.as_deref() == Some("GET /pets/{petId}"));
    }

    #[test]
    fn parameters_bodies_and_statuses() {
        let flows = vec![
            flow(1, "GET", "petstore.example.com", "/v1/pets/abc", &[], "", 200, r#"{"id":1,"name":"x"}"#),
            flow(2, "GET", "petstore.example.com", "/v1/pets?limit=1000", &[], "", 200, "[]"),
            flow(
                3,
                "POST",
                "petstore.example.com",
                "/v1/pets",
                &[("content-type", "application/json")],
                "",
                201,
                r#"{"id":1,"name":"x"}"#,
            ),
            flow(
                4,
                "POST",
                "petstore.example.com",
                "/v1/pets",
                &[("content-type", "application/json")],
                r#"{"name":""}"#,
                500,
                "{}",
            ),
        ];
        let report = validate_flows(&pets(), &flows);
        let by_flow =
            |id| report.violations.iter().filter(move |v| v.flow_id == id).map(|v| v.kind).collect::<Vec<_>>();
        // `petId` must be an integer and the required `X-Trace` header is missing.
        assert_eq!(
            by_flow(1),
            vec![ViolationKind::InvalidParameter, ViolationKind::MissingParameter],
            "{:#?}",
            report.violations
        );
        assert_eq!(by_flow(2), vec![ViolationKind::InvalidParameter]);
        assert_eq!(by_flow(3), vec![ViolationKind::MissingRequestBody]);
        assert!(
            by_flow(4).contains(&ViolationKind::InvalidRequestBody)
                && by_flow(4).contains(&ViolationKind::UndocumentedStatus)
        );
        assert_eq!(report.flows_with_violations(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn unknown_paths_and_methods() {
        let flows = vec![
            flow(1, "GET", "petstore.example.com", "/v1/unicorns", &[], "", 200, "{}"),
            flow(2, "DELETE", "petstore.example.com", "/v1/pets", &[], "", 200, "{}"),
            flow(3, "GET", "petstore.example.com", "/v2/pets", &[], "", 200, "[]"),
        ];
        let report = validate_flows(&pets(), &flows);
        assert_eq!(kinds(&report), vec![ViolationKind::UndocumentedPath, ViolationKind::UndocumentedMethod]);
        assert!(report.violations[1].message.contains("GET, POST"));
        assert_eq!(report.skipped, 1, "outside the server's base path");
    }

    #[test]
    fn openapi_31_types_and_default_responses() {
        let spec = Spec::parse(ORDERS).unwrap();
        let ok = flow(
            1,
            "GET",
            "any.host",
            "/orders/3fa85f64-5717-4562-b3fc-2c963f66afa6",
            &[],
            "",
            200,
            r#"{"id":"3fa85f64-5717-4562-b3fc-2c963f66afa6","total":null,"items":["a"]}"#,
        );
        let bad = flow(
            2,
            "GET",
            "any.host",
            "/orders/3fa85f64-5717-4562-b3fc-2c963f66afa6",
            &[],
            "",
            200,
            r#"{"id":"x","total":-1,"items":[]}"#,
        );
        let error = flow(3, "GET", "any.host", "/orders/3fa85f64-5717-4562-b3fc-2c963f66afa6", &[], "", 503, "{}");
        let unrelated = flow(4, "GET", "any.host", "/health", &[], "", 200, "{}");
        let report = validate_flows(&spec, &[ok, bad, error, unrelated]);
        assert_eq!(report.flows_with_violations(), vec![2], "{:#?}", report.violations);
        assert_eq!(report.skipped, 1, "without `servers`, unknown paths are not this API's business");
        assert!(report.violations.len() >= 2, "minimum and minItems");
    }
}
