use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use proxyzip_core::template::{TemplateContext, render};
use proxyzip_core::{Flow, Headers, Scheme};
use serde_json::Value;

use crate::model::{Assertion, Extract, Suite, SuiteRequest};
use crate::report::{AssertionResult, Report, RequestResult};

/// How to run a suite.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Name of an entry of `Suite::environments`.
    pub environment: Option<String>,
    /// Variables that override the suite's and the environment's.
    pub variables: BTreeMap<String, String>,
    /// Send everything through this proxy, for example `http://127.0.0.1:9393`, so the requests appear in ProxyZip.
    pub proxy: Option<String>,
    /// PEM of a certificate authority to trust (the ProxyZip CA, when going through the proxy).
    pub extra_root_pem: Option<String>,
    /// Do not verify server certificates.
    pub insecure: bool,
    pub timeout: Duration,
    /// Skip the remaining requests after the first failure.
    pub bail: bool,
    /// Where relative paths (OpenAPI specifications) are resolved from.
    pub base_dir: PathBuf,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            environment: None,
            variables: BTreeMap::new(),
            proxy: None,
            extra_root_pem: None,
            insecure: false,
            timeout: Duration::from_secs(30),
            bail: false,
            base_dir: PathBuf::from("."),
        }
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    duration_ms: u64,
}

fn client(options: &RunOptions) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(options.timeout).danger_accept_invalid_certs(options.insecure);
    if let Some(proxy) = &options.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("invalid proxy {proxy}: {e}"))?);
    }
    if let Some(pem) = &options.extra_root_pem {
        let cert = reqwest::Certificate::from_pem(pem.as_bytes()).map_err(|e| format!("invalid certificate: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    builder.build().map_err(|e| e.to_string())
}

fn context(vars: &BTreeMap<String, String>) -> TemplateContext {
    TemplateContext {
        vars: vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<HashMap<_, _>>(),
        ..Default::default()
    }
}

async fn send(
    client: &reqwest::Client,
    suite: &Suite,
    request: &SuiteRequest,
    vars: &BTreeMap<String, String>,
    via_proxy: bool,
) -> Result<(Response, String, Vec<(String, String)>, Option<String>), String> {
    let ctx = context(vars);
    let url = render(&request.url, &ctx);
    let method = reqwest::Method::from_bytes(request.method.trim().to_ascii_uppercase().as_bytes())
        .map_err(|_| format!("invalid method {:?}", request.method))?;
    let mut builder = client.request(method, &url);
    let mut sent_headers = Vec::new();
    for (name, value) in &request.headers {
        let value = render(value, &ctx);
        builder = builder.header(name.as_str(), value.as_str());
        sent_headers.push((name.clone(), value));
    }
    if via_proxy {
        builder = builder.header("x-proxyzip-suite", suite.name.as_str());
    }
    let body = request.body.as_ref().map(|b| render(b, &ctx));
    if let Some(b) = &body {
        builder = builder.body(b.clone());
    }
    let started = Instant::now();
    let response = builder.send().await.map_err(|e| describe(&e))?;
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let bytes = response.bytes().await.map_err(|e| describe(&e))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    Ok((Response { status, headers, body: bytes, duration_ms }, url, sent_headers, body))
}

fn describe(e: &reqwest::Error) -> String {
    let mut message = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        message.push_str(&format!(": {s}"));
        source = s.source();
    }
    message
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn describe_assertion(a: &Assertion) -> String {
    match a {
        Assertion::Status { equals: Some(s), .. } => format!("status is {s}"),
        Assertion::Status { class: Some(c), .. } => format!("status is {c}"),
        Assertion::Status { .. } => "status".into(),
        Assertion::Header { name, equals: Some(v), .. } => format!("header {name} equals {v}"),
        Assertion::Header { name, contains: Some(v), .. } => format!("header {name} contains {v}"),
        Assertion::Header { name, .. } => format!("header {name} is present"),
        Assertion::Jq { expr, equals: Some(v) } => format!("{expr} equals {v}"),
        Assertion::Jq { expr, .. } => format!("{expr} is truthy"),
        Assertion::BodyContains { text } => format!("body contains {text:?}"),
        Assertion::DurationBelow { ms } => format!("responds within {ms} ms"),
        Assertion::OpenApi { spec } => format!("conforms to {spec}"),
    }
}

fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false)) && v != &Value::String(String::new()) && v != &Value::from(0)
}

struct Exchange<'a> {
    method: &'a str,
    url: &'a str,
    request_headers: &'a [(String, String)],
    request_body: Option<&'a str>,
}

fn check(
    a: &Assertion,
    ex: &Exchange,
    res: &Response,
    vars: &BTreeMap<String, String>,
    base: &std::path::Path,
) -> Result<(), String> {
    let ctx = context(vars);
    match a {
        Assertion::Status { equals, class } => {
            if let Some(want) = equals
                && res.status != *want
            {
                return Err(format!("got {}", res.status));
            }
            if let Some(class) = class {
                let digit = class.chars().next().and_then(|c| c.to_digit(10)).ok_or("invalid class")? as u16;
                if res.status / 100 != digit {
                    return Err(format!("got {}", res.status));
                }
            }
            Ok(())
        }
        Assertion::Header { name, equals, contains } => {
            let Some(value) = header(&res.headers, name) else { return Err("the header is missing".into()) };
            if let Some(want) = equals {
                let want = render(want, &ctx);
                if value != want {
                    return Err(format!("got {value:?}"));
                }
            }
            if let Some(part) = contains {
                let part = render(part, &ctx);
                if !value.contains(&part) {
                    return Err(format!("got {value:?}"));
                }
            }
            Ok(())
        }
        Assertion::Jq { expr, equals } => {
            let expr = render(expr, &ctx);
            let results = proxyzip_codec::query::run(&res.body, &expr)?;
            let first = results.first().ok_or("the expression produced no result")?;
            match equals {
                Some(want) => {
                    let want = match want {
                        Value::String(s) => Value::String(render(s, &ctx)),
                        other => other.clone(),
                    };
                    if first == &want { Ok(()) } else { Err(format!("got {first}")) }
                }
                None => {
                    if truthy(first) {
                        Ok(())
                    } else {
                        Err(format!("got {first}"))
                    }
                }
            }
        }
        Assertion::BodyContains { text } => {
            let text = render(text, &ctx);
            if String::from_utf8_lossy(&res.body).contains(&text) {
                Ok(())
            } else {
                Err("the text was not found".into())
            }
        }
        Assertion::DurationBelow { ms } => {
            if res.duration_ms <= *ms {
                Ok(())
            } else {
                Err(format!("took {} ms", res.duration_ms))
            }
        }
        Assertion::OpenApi { spec } => {
            let path = base.join(spec);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let spec = proxyzip_openapi::Spec::parse(&text).map_err(|e| format!("{e:#}"))?;
            let flow = exchange_flow(ex, res)?;
            let report = proxyzip_openapi::validate_flows(&spec, &[flow]);
            if report.violations.is_empty() {
                Ok(())
            } else {
                Err(report.violations.iter().take(3).map(|v| v.message.clone()).collect::<Vec<_>>().join("; "))
            }
        }
    }
}

/// The exchange as a flow, so the OpenAPI checker can look at it.
fn exchange_flow(ex: &Exchange, res: &Response) -> Result<Flow, String> {
    let url = url::Url::parse(ex.url).map_err(|e| format!("invalid URL: {e}"))?;
    let scheme = if url.scheme() == "https" { Scheme::Https } else { Scheme::Http };
    let mut path = url.path().to_string();
    if let Some(q) = url.query() {
        path.push('?');
        path.push_str(q);
    }
    let mut flow = Flow::new(
        1,
        "suite".into(),
        proxyzip_core::Request {
            method: ex.method.to_ascii_uppercase(),
            scheme,
            host: url.host_str().unwrap_or("").to_string(),
            port: url.port_or_known_default().unwrap_or(80),
            path,
            http_version: "HTTP/1.1".into(),
            headers: Headers(ex.request_headers.to_vec()),
            body: Bytes::from(ex.request_body.unwrap_or("").to_string()),
            body_truncated: false,
        },
    );
    flow.response = Some(proxyzip_core::Response {
        status: res.status,
        reason: String::new(),
        http_version: "HTTP/1.1".into(),
        headers: Headers(res.headers.clone()),
        body: res.body.clone(),
        body_truncated: false,
    });
    Ok(flow)
}

fn extract(e: &Extract, res: &Response, vars: &BTreeMap<String, String>) -> Result<String, String> {
    if let Some(expr) = &e.jq {
        let expr = render(expr, &context(vars));
        let results = proxyzip_codec::query::run(&res.body, &expr)?;
        let first = results.first().ok_or_else(|| format!("{expr} produced no result"))?;
        return Ok(match first {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    }
    if let Some(name) = &e.header {
        return header(&res.headers, name).map(str::to_string).ok_or_else(|| format!("header {name} is missing"));
    }
    Err("an extraction needs `jq` or `header`".into())
}

fn skipped(request: &SuiteRequest, reason: &str) -> RequestResult {
    RequestResult {
        name: request.name.clone(),
        method: request.method.clone(),
        url: request.url.clone(),
        status: None,
        duration_ms: 0,
        passed: false,
        error: Some(reason.to_string()),
        assertions: Vec::new(),
        extracted: BTreeMap::new(),
        skipped: true,
    }
}

/// Run every enabled request of `suite`, in order.
pub async fn run(suite: &Suite, options: &RunOptions) -> Report {
    let started = Instant::now();
    let mut vars = suite.variables.clone();
    if let Some(env) = &options.environment
        && let Some(values) = suite.environments.get(env)
    {
        vars.extend(values.clone());
    }
    vars.extend(options.variables.clone());

    let mut report = Report {
        suite: suite.name.clone(),
        environment: options.environment.clone(),
        duration_ms: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        results: Vec::new(),
    };
    let client = client(options);
    let mut stopped = false;
    for request in &suite.requests {
        if !request.enabled {
            report.skipped += 1;
            report.results.push(skipped(request, "disabled"));
            continue;
        }
        if stopped {
            report.skipped += 1;
            report.results.push(skipped(request, "skipped after an earlier failure"));
            continue;
        }
        let result = match &client {
            Err(e) => RequestResult { error: Some(e.clone()), skipped: false, ..skipped(request, "") },
            Ok(client) => run_one(client, suite, request, &mut vars, options).await,
        };
        if result.passed {
            report.passed += 1;
        } else {
            report.failed += 1;
            stopped = options.bail;
        }
        report.results.push(result);
    }
    report.duration_ms = started.elapsed().as_millis() as u64;
    report
}

async fn run_one(
    client: &reqwest::Client,
    suite: &Suite,
    request: &SuiteRequest,
    vars: &mut BTreeMap<String, String>,
    options: &RunOptions,
) -> RequestResult {
    let mut result = RequestResult {
        name: request.name.clone(),
        method: request.method.to_ascii_uppercase(),
        url: render(&request.url, &context(vars)),
        status: None,
        duration_ms: 0,
        passed: false,
        error: None,
        assertions: Vec::new(),
        extracted: BTreeMap::new(),
        skipped: false,
    };
    let (response, url, sent_headers, sent_body) =
        match send(client, suite, request, vars, options.proxy.is_some()).await {
            Ok(v) => v,
            Err(e) => {
                result.error = Some(e);
                return result;
            }
        };
    result.status = Some(response.status);
    result.duration_ms = response.duration_ms;
    result.url = url.clone();
    let exchange = Exchange {
        method: &request.method,
        url: &url,
        request_headers: &sent_headers,
        request_body: sent_body.as_deref(),
    };
    // Without assertions, "passed" means the server answered.
    let mut passed = true;
    for a in &request.assertions {
        let outcome = check(a, &exchange, &response, vars, &options.base_dir);
        passed &= outcome.is_ok();
        result.assertions.push(AssertionResult {
            description: describe_assertion(a),
            passed: outcome.is_ok(),
            message: outcome.err(),
        });
    }
    for e in &request.extract {
        match extract(e, &response, vars) {
            Ok(value) => {
                vars.insert(e.var.clone(), value.clone());
                result.extracted.insert(e.var.clone(), value);
            }
            Err(message) => {
                passed = false;
                result.assertions.push(AssertionResult {
                    description: format!("extract {}", e.var),
                    passed: false,
                    message: Some(message),
                });
            }
        }
    }
    result.passed = passed;
    result
}
