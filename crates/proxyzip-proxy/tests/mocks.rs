//! Mock servers: answered by ProxyZip, through the proxy or on their own port.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxyzip_core::{FlowState, FlowStore};
use proxyzip_proxy::{ProxyConfig, ProxyHandle, ProxyServer, ProxySettings};
use proxyzip_rules::{MockBody, MockRoute, MockServer, RuleSet};
use proxyzip_tls::CertificateAuthority;
use tokio::net::TcpListener;

struct Harness {
    proxy: SocketAddr,
    store: FlowStore,
    handle: ProxyHandle,
}

async fn start(rules: RuleSet) -> Harness {
    let ca = Arc::new(CertificateAuthority::generate().unwrap());
    let store = FlowStore::default();
    let settings = ProxySettings { rules: Arc::new(rules), ..Default::default() };
    let config =
        ProxyConfig { listen: "127.0.0.1:0".parse().unwrap(), extra_upstream_roots: vec![], keylog_path: None };
    let handle = ProxyServer::start(config, settings, ca, store.clone()).await.unwrap();
    Harness { proxy: handle.local_addr, store, handle }
}

fn client(h: &Harness) -> reqwest::Client {
    reqwest::Client::builder().proxy(reqwest::Proxy::http(format!("http://{}", h.proxy)).unwrap()).build().unwrap()
}

fn direct() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn route(name: &str, method: &str, path: &str, body: &str) -> MockRoute {
    MockRoute {
        name: name.into(),
        method: method.into(),
        path: path.into(),
        body: MockBody::Inline { text: body.into() },
        ..Default::default()
    }
}

fn server(routes: Vec<MockRoute>) -> MockServer {
    MockServer { name: "api".into(), host: "api.mock".into(), routes, ..Default::default() }
}

fn rules(s: MockServer) -> RuleSet {
    RuleSet { mocks: vec![s], ..Default::default() }
}

async fn real_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let handler = service_fn(|_req: Request<hyper::body::Incoming>| async {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"the real server"))))
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(s), handler)
                    .await;
            });
        }
    });
    addr
}

async fn done(store: &FlowStore, count: usize) -> Vec<proxyzip_core::Flow> {
    for _ in 0..100 {
        let flows = store.all();
        if flows.len() >= count && flows.iter().all(|f| matches!(f.state, FlowState::Completed | FlowState::Failed)) {
            return flows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("flows did not finish: {:#?}", store.summaries());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn a_mock_host_is_answered_with_params_query_and_body_filled_in() {
    let mut create =
        route("create", "POST", "/users", r#"{"id":"{{uuid}}","name":"{{body.name}}","page":"{{query.page}}"}"#);
    create.status = 201;
    create.template = true;
    create.headers.push(("x-name".into(), "{{body.name}}".into()));
    let mut one = route("one", "GET", "/users/:id", r#"{"id":{{params.id}}}"#);
    one.template = true;
    let h = start(rules(server(vec![one, create]))).await;
    let c = client(&h);

    let res = c.get("http://api.mock/users/42").send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-proxyzip-rule"], "mock");
    assert_eq!(res.text().await.unwrap(), r#"{"id":42}"#);

    let res = c
        .post("http://api.mock/users?page=3")
        .header("content-type", "application/json")
        .body(r#"{"name":"ada"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers()["x-name"], "ada");
    let body: serde_json::Value = serde_json::from_str(&res.text().await.unwrap()).unwrap();
    assert_eq!(body["name"], "ada");
    assert_eq!(body["page"], "3");
    assert_eq!(body["id"].as_str().unwrap().len(), 36);

    let flows = done(&h.store, 2).await;
    assert!(flows.iter().all(|f| f.tags.iter().any(|t| t == "mock")));
    assert_eq!(flows[0].request.host, "api.mock");
}

#[tokio::test]
async fn delays_are_applied() {
    let mut slow = route("slow", "GET", "/slow", "ok");
    slow.delay_ms = 200;
    let h = start(rules(server(vec![slow]))).await;
    let started = Instant::now();
    assert_eq!(client(&h).get("http://api.mock/slow").send().await.unwrap().text().await.unwrap(), "ok");
    assert!(started.elapsed() >= Duration::from_millis(190));
}

#[tokio::test]
async fn an_unmatched_request_lists_the_routes_or_passes_through() {
    let upstream = real_upstream().await;
    let mut s = server(vec![route("one", "GET", "/known", "x")]);
    s.host = "127.0.0.1".into();
    let h = start(rules(s.clone())).await;
    let res = client(&h).get(format!("http://127.0.0.1:{}/unknown", upstream.port())).send().await.unwrap();
    assert_eq!(res.status(), 404);
    let body: serde_json::Value = serde_json::from_str(&res.text().await.unwrap()).unwrap();
    assert_eq!(body["routes"][0], "GET /known");
    assert_eq!(body["request"], "GET /unknown");

    s.passthrough = true;
    h.handle.apply_rules(rules(s)).await;
    let res = client(&h).get(format!("http://127.0.0.1:{}/unknown", upstream.port())).send().await.unwrap();
    assert_eq!(res.text().await.unwrap(), "the real server");
}

#[tokio::test]
async fn a_mock_can_have_its_own_port() {
    let mut s = server(vec![route("one", "GET", "/users/:id", r#"{"id":"{{params.id}}"}"#)]);
    s.host = String::new();
    s.listen_port = Some(free_port());
    s.routes[0].template = true;
    let port = s.listen_port.unwrap();
    let h = start(RuleSet::default()).await;
    let errors = h.handle.apply_rules(rules(s)).await;
    assert!(errors.is_empty(), "{errors:?}");

    let res = direct().get(format!("http://127.0.0.1:{port}/users/9")).send().await.unwrap();
    assert_eq!(res.text().await.unwrap(), r#"{"id":"9"}"#);
    let res = direct().get(format!("http://localhost:{port}/nope")).send().await.unwrap();
    assert_eq!(res.status(), 404, "an unmatched request to the mock's own port never reaches the network");

    let flows = done(&h.store, 2).await;
    assert!(flows.iter().all(|f| f.tags.iter().any(|t| t == "mock")));
}

#[tokio::test]
async fn bodies_can_come_from_a_file() {
    let dir = std::env::temp_dir().join(format!("pz-mock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("users.json");
    std::fs::write(&file, r#"[{"id":1}]"#).unwrap();
    let mut r = route("file", "GET", "/users", "");
    r.body = MockBody::File { path: file };
    let mut missing = route("missing", "GET", "/missing", "");
    missing.body = MockBody::File { path: dir.join("nope.json") };
    let h = start(rules(server(vec![r, missing]))).await;
    assert_eq!(client(&h).get("http://api.mock/users").send().await.unwrap().text().await.unwrap(), r#"[{"id":1}]"#);
    let res = client(&h).get("http://api.mock/missing").send().await.unwrap();
    assert_eq!(res.status(), 500);
    assert_eq!(res.headers()["x-proxyzip-rule"], "mock-error");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn binary_files_are_served_byte_for_byte() {
    let dir = std::env::temp_dir().join(format!("pz-mock-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("frame.bin");
    let bytes: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x05, 0xff, 0xfe, 0x80, 0x0a, 0x7b];
    std::fs::write(&file, &bytes).unwrap();
    let mut r = route("bin", "POST", "/frame", "");
    r.body = MockBody::File { path: file };
    r.template = true; // a binary file is never rendered
    r.headers = vec![("content-type".into(), "application/octet-stream".into())];
    let h = start(rules(server(vec![r]))).await;
    let res = client(&h).post("http://api.mock/frame").send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().to_vec(), bytes);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn turning_the_tool_off_stops_mocking() {
    let upstream = real_upstream().await;
    let mut s = server(vec![route("any", "", "/**", "mocked")]);
    s.host = "127.0.0.1".into();
    let mut r = rules(s);
    r.tools.mocks = false;
    let h = start(r).await;
    let res = client(&h).get(format!("http://127.0.0.1:{}/x", upstream.port())).send().await.unwrap();
    assert_eq!(res.text().await.unwrap(), "the real server");
}

#[tokio::test]
async fn requests_from_a_replay_suite_are_tagged_and_the_marker_stays_private() {
    let h = start(rules(server(vec![route("one", "GET", "/x", "ok")]))).await;
    let res = client(&h).get("http://api.mock/x").header("x-proxyzip-suite", "checkout").send().await.unwrap();
    assert_eq!(res.text().await.unwrap(), "ok");
    let flows = done(&h.store, 1).await;
    let tags = &flows[0].tags;
    assert!(tags.iter().any(|t| t == "suite") && tags.iter().any(|t| t == "mock"), "{tags:?}");
    assert!(flows[0].request.headers.get("x-proxyzip-suite").is_none());
}
