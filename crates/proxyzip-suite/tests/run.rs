use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxyzip_suite::{Assertion, Extract, RunOptions, Suite, SuiteRequest, run, to_junit};
use tokio::net::TcpListener;

async fn handler(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path().to_string();
    let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let suite_header = req.headers().get("x-proxyzip-suite").is_some();
    let body = req.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default();
    let json = |status: u16, text: &str| {
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .header("x-seen-suite", suite_header.to_string())
            .body(Full::new(Bytes::from(text.to_string())))
            .unwrap()
    };
    Ok(match path.as_str() {
        "/login" => json(
            200,
            &format!(
                r#"{{"token":"t-123","echo":{}}}"#,
                if body.is_empty() { "null".into() } else { String::from_utf8_lossy(&body).into_owned() }
            ),
        ),
        "/me" if auth == "Bearer t-123" => json(200, r#"{"name":"ada","roles":["admin"]}"#),
        "/me" => json(401, r#"{"error":"no"}"#),
        "/slow" => {
            tokio::time::sleep(Duration::from_millis(120)).await;
            json(200, "{}")
        }
        "/fail" => json(500, r#"{"error":"boom"}"#),
        _ => json(404, "{}"),
    })
}

async fn server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(s), service_fn(handler))
                    .await;
            });
        }
    });
    addr
}

fn request(name: &str, method: &str, url: &str) -> SuiteRequest {
    SuiteRequest {
        name: name.into(),
        enabled: true,
        method: method.into(),
        url: url.into(),
        headers: vec![],
        body: None,
        assertions: vec![],
        extract: vec![],
    }
}

fn suite(addr: SocketAddr) -> Suite {
    let mut s = Suite::new("login flow");
    s.variables.insert("base".into(), format!("http://{addr}"));
    s.variables.insert("user".into(), "ada".into());
    let mut login = request("login", "POST", "{{vars.base}}/login");
    login.body = Some(r#"{"user":"{{vars.user}}"}"#.into());
    login.assertions = vec![
        Assertion::Status { equals: Some(200), class: None },
        Assertion::Jq { expr: ".echo.user".into(), equals: Some("ada".into()) },
        Assertion::Header { name: "Content-Type".into(), equals: None, contains: Some("json".into()) },
    ];
    login.extract = vec![Extract { var: "token".into(), jq: Some(".token".into()), header: None }];
    let mut me = request("me", "GET", "{{vars.base}}/me");
    me.headers = vec![("Authorization".into(), "Bearer {{vars.token}}".into())];
    me.assertions = vec![
        Assertion::Status { equals: None, class: Some("2xx".into()) },
        Assertion::Jq { expr: ".roles[0]".into(), equals: Some("admin".into()) },
        Assertion::Jq { expr: ".name".into(), equals: None },
        Assertion::BodyContains { text: "ada".into() },
    ];
    s.requests = vec![login, me];
    s
}

#[tokio::test]
async fn values_flow_from_one_request_to_the_next() {
    let addr = server().await;
    let report = run(&suite(addr), &RunOptions::default()).await;
    assert!(report.ok(), "{}", report.to_text());
    assert_eq!((report.passed, report.failed), (2, 0));
    assert_eq!(report.results[0].extracted["token"], "t-123");
    assert!(report.results[1].assertions.iter().all(|a| a.passed));
}

#[tokio::test]
async fn failures_are_explained() {
    let addr = server().await;
    let mut s = suite(addr);
    s.requests[1].headers.clear();
    s.requests[1].assertions.push(Assertion::DurationBelow { ms: 0 });
    let mut fail = request("fail", "GET", "{{vars.base}}/fail");
    fail.assertions = vec![
        Assertion::Status { equals: Some(200), class: None },
        Assertion::Header { name: "x-nope".into(), equals: None, contains: None },
    ];
    s.requests.push(fail);
    let report = run(&s, &RunOptions::default()).await;
    assert!(!report.ok());
    let me = &report.results[1];
    assert_eq!(me.status, Some(401));
    assert!(me.assertions.iter().any(|a| !a.passed && a.message.as_deref() == Some("got 401")));
    let fail = &report.results[2];
    let messages: Vec<_> = fail.assertions.iter().filter_map(|a| a.message.clone()).collect();
    assert_eq!(messages, vec!["got 500", "the header is missing"]);
    let xml = to_junit(&report);
    assert!(xml.contains("failures=\"2\""));
}

#[tokio::test]
async fn environments_and_overrides_pick_the_variables() {
    let addr = server().await;
    let mut s = suite(addr);
    s.variables.insert("base".into(), "http://127.0.0.1:1".into());
    s.environments.insert("dev".into(), BTreeMap::from([("base".to_string(), format!("http://{addr}"))]));
    assert!(!run(&s, &RunOptions::default()).await.ok(), "the default base URL is unreachable");
    let dev = RunOptions { environment: Some("dev".into()), ..Default::default() };
    assert!(run(&s, &dev).await.ok());
    let wrong_user = RunOptions {
        environment: Some("dev".into()),
        variables: BTreeMap::from([("user".to_string(), "bob".to_string())]),
        ..Default::default()
    };
    assert!(!run(&s, &wrong_user).await.ok(), "an override beats the suite and the environment");
}

#[tokio::test]
async fn bail_and_disabled_requests() {
    let addr = server().await;
    let mut s = suite(addr);
    let mut fail = request("fail", "GET", "{{vars.base}}/fail");
    fail.assertions = vec![Assertion::Status { equals: Some(200), class: None }];
    s.requests.insert(0, fail);
    s.requests[2].enabled = false;
    let report = run(&s, &RunOptions { bail: true, ..Default::default() }).await;
    assert_eq!((report.passed, report.failed, report.skipped), (0, 1, 2));
    assert!(report.results[1].skipped && report.results[2].skipped);
    let report = run(&s, &RunOptions::default()).await;
    assert_eq!((report.passed, report.failed, report.skipped), (1, 1, 1));
}

#[tokio::test]
async fn an_unreachable_server_is_a_failure_not_a_crash() {
    let mut s = Suite::new("down");
    s.requests.push(request("a", "GET", "http://127.0.0.1:1/x"));
    s.requests.push(request("b", "BAD METHOD", "http://127.0.0.1:1/x"));
    let report = run(&s, &RunOptions { timeout: Duration::from_secs(2), ..Default::default() }).await;
    assert_eq!(report.failed, 2);
    assert!(
        report.results[0].error.is_some() && report.results[1].error.as_deref().unwrap().contains("invalid method")
    );
}

#[tokio::test]
async fn openapi_assertion_checks_the_exchange() {
    let addr = server().await;
    let dir = std::env::temp_dir().join(format!("pz-suite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("api.yaml"),
        format!(
            "openapi: 3.0.0\ninfo: {{title: t, version: '1'}}\nservers:\n  - url: http://127.0.0.1:{}\npaths:\n  /me:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema: {{type: object, required: [name], properties: {{name: {{type: integer}}}}}}\n",
            addr.port()
        ),
    )
    .unwrap();
    let mut s = Suite::new("contract");
    let mut me = request("me", "GET", &format!("http://{addr}/me"));
    me.headers = vec![("Authorization".into(), "Bearer t-123".into())];
    me.assertions = vec![Assertion::OpenApi { spec: "api.yaml".into() }];
    s.requests.push(me);
    let report = run(&s, &RunOptions { base_dir: dir.clone(), ..Default::default() }).await;
    assert!(!report.ok(), "`name` is a string in the response but the spec says integer");
    assert!(report.results[0].assertions[0].message.as_deref().unwrap().contains("integer"));
    let _ = std::fs::remove_dir_all(dir);
}
