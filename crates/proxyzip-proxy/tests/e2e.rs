//! End-to-end tests: real client → ProxyZip → real upstream (HTTP, HTTPS/h2, WebSocket).

// Building a rule set field by field reads better in tests than one giant struct literal.
#![allow(clippy::field_reassign_with_default)]

use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxyzip_core::{FlowState, FlowStore, Scheme, WsDirection};
use proxyzip_proxy::{BreakpointHub, Decision, ProxyConfig, ProxyServer, ProxySettings, RequestEdit, ResponseEdit};
use proxyzip_rules::{
    BlockAction, BlockRule, BreakpointRule, DnsRule, LocalSource, MapLocalRule, MapRemoteRule, Matcher, Phase,
    RecordOnly, ReverseRule, RewriteOp, RewriteRule, RuleSet, SocksListener, Throttle, ThrottleProfile, UpstreamKind,
    UpstreamProxy,
};
use proxyzip_tls::CertificateAuthority;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

async fn upstream_handler(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path().to_string();
    let version = format!("{:?}", req.version());
    Ok(match path.as_str() {
        "/hello" => Response::builder()
            .header("content-type", "text/plain")
            .header("x-upstream-version", version)
            .body(Full::new(Bytes::from_static(b"hi there")))
            .unwrap(),
        "/gzip" => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(br#"{"ok":true}"#).unwrap();
            Response::builder()
                .header("content-type", "application/json")
                .header("content-encoding", "gzip")
                .body(Full::new(Bytes::from(enc.finish().unwrap())))
                .unwrap()
        }
        "/headers" => {
            let mut text = format!("query: {}\n", req.uri().query().unwrap_or(""));
            for (k, v) in req.headers() {
                text.push_str(&format!("{k}: {}\n", v.to_str().unwrap_or("?")));
            }
            Response::builder().header("content-type", "text/plain").body(Full::new(Bytes::from(text))).unwrap()
        }
        "/big" => Response::new(Full::new(Bytes::from(vec![b'x'; 20_000]))),
        "/echo" => {
            let body = req.into_body().collect().await.unwrap().to_bytes();
            Response::new(Full::new(body))
        }
        _ => Response::builder().status(404).body(Full::new(Bytes::new())).unwrap(),
    })
}

struct TestCa {
    pem: String,
    der: CertificateDer<'static>,
    server_config: Arc<rustls::ServerConfig>,
}

fn upstream_tls() -> TestCa {
    proxyzip_tls::install_crypto_provider();
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();
    let mut cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    TestCa { pem: ca_cert.pem(), der: ca_cert.der().clone(), server_config: Arc::new(cfg) }
}

async fn spawn_https_upstream(cfg: Arc<rustls::ServerConfig>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(cfg);
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(s).await else { return };
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), service_fn(upstream_handler))
                    .await;
            });
        }
    });
    addr
}

async fn spawn_http_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(s), service_fn(upstream_handler))
                    .await;
            });
        }
    });
    addr
}

async fn spawn_ws_echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
                while let Some(Ok(m)) = ws.next().await {
                    if m.is_text() || m.is_binary() {
                        ws.send(m).await.unwrap();
                    } else if m.is_close() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

struct Harness {
    proxy: SocketAddr,
    store: FlowStore,
    ca: Arc<CertificateAuthority>,
    settings: Arc<parking_lot::RwLock<ProxySettings>>,
    breakpoints: Arc<BreakpointHub>,
    handle: proxyzip_proxy::ProxyHandle,
}

async fn start_proxy(extra_root: Option<CertificateDer<'static>>) -> Harness {
    start_proxy_with(extra_root, ProxySettings::default()).await
}

async fn start_proxy_with(extra_root: Option<CertificateDer<'static>>, settings: ProxySettings) -> Harness {
    let ca = Arc::new(CertificateAuthority::generate().unwrap());
    let store = FlowStore::default();
    let config = ProxyConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        extra_upstream_roots: extra_root.into_iter().collect(),
        keylog_path: None,
    };
    let handle = ProxyServer::start(config, settings, ca.clone(), store.clone()).await.unwrap();
    let proxy = handle.local_addr;
    let settings = handle.settings.clone();
    let breakpoints = handle.breakpoints.clone();
    Harness { proxy, store, ca, settings, breakpoints, handle }
}

fn client(h: &Harness, http2: bool) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{}", h.proxy)).unwrap())
        .add_root_certificate(reqwest::Certificate::from_pem(h.ca.cert_pem().as_bytes()).unwrap())
        .no_gzip();
    if !http2 {
        b = b.http1_only();
    }
    b.build().unwrap()
}

async fn wait_completed(store: &FlowStore, count: usize) -> Vec<proxyzip_core::Flow> {
    for _ in 0..100 {
        let flows = store.all();
        if flows.len() >= count && flows.iter().all(|f| f.state == FlowState::Completed || f.state == FlowState::Failed)
        {
            return flows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("flows did not complete: {:#?}", store.summaries());
}

#[tokio::test]
async fn plain_http_is_captured() {
    let upstream = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let resp = client(&h, false).post(format!("http://{upstream}/echo")).body("ping").send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "ping");
    let flows = wait_completed(&h.store, 1).await;
    let f = &flows[0];
    assert_eq!(f.request.scheme, Scheme::Http);
    assert_eq!(f.request.method, "POST");
    assert_eq!(&f.request.body[..], b"ping");
    assert_eq!(f.response.as_ref().unwrap().status, 200);
    assert_eq!(&f.response.as_ref().unwrap().body[..], b"ping");
}

#[tokio::test]
async fn https_mitm_http2_and_gzip() {
    let tls = upstream_tls();
    let upstream = spawn_https_upstream(tls.server_config.clone()).await;
    let h = start_proxy(Some(tls.der.clone())).await;
    let c = client(&h, true);

    let resp = c.get(format!("https://localhost:{}/hello", upstream.port())).send().await.unwrap();
    assert_eq!(resp.version(), reqwest::Version::HTTP_2, "client side negotiated h2 with the MITM");
    assert_eq!(resp.headers()["x-upstream-version"], "HTTP/2.0", "upstream side negotiated h2");
    assert_eq!(resp.text().await.unwrap(), "hi there");

    let resp = c.get(format!("https://localhost:{}/gzip", upstream.port())).send().await.unwrap();
    assert_eq!(resp.headers()["content-encoding"], "gzip");
    let _ = resp.bytes().await.unwrap();

    let flows = wait_completed(&h.store, 2).await;
    let hello = flows.iter().find(|f| f.request.path == "/hello").unwrap();
    assert_eq!(hello.request.scheme, Scheme::Https);
    assert_eq!(hello.request.host, "localhost");
    assert_eq!(hello.tls.as_ref().unwrap().alpn.as_deref(), Some("h2"));
    assert_eq!(&hello.response.as_ref().unwrap().body[..], b"hi there");

    let gz = flows.iter().find(|f| f.request.path == "/gzip").unwrap();
    let r = gz.response.as_ref().unwrap();
    // The capture keeps the wire bytes (still gzip-encoded).
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&r.body[..]), &mut decoded).unwrap();
    assert_eq!(decoded, br#"{"ok":true}"#);
    let _ = tls.pem;
}

#[tokio::test]
async fn https_http1_client() {
    let tls = upstream_tls();
    let upstream = spawn_https_upstream(tls.server_config.clone()).await;
    let h = start_proxy(Some(tls.der.clone())).await;
    let resp = client(&h, false).get(format!("https://127.0.0.1:{}/hello", upstream.port())).send().await.unwrap();
    assert_eq!(resp.version(), reqwest::Version::HTTP_11);
    assert_eq!(resp.text().await.unwrap(), "hi there");
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(flows[0].request.host, "127.0.0.1");
}

#[tokio::test]
async fn untrusted_upstream_is_reported() {
    let tls = upstream_tls();
    let upstream = spawn_https_upstream(tls.server_config.clone()).await;
    let h = start_proxy(None).await; // proxy does not trust the upstream CA
    let resp = client(&h, false).get(format!("https://localhost:{}/hello", upstream.port())).send().await.unwrap();
    assert_eq!(resp.status(), 502);
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(flows[0].state, FlowState::Failed);
    assert!(flows[0].error.as_ref().unwrap().contains("upstream request"));
}

#[tokio::test]
async fn excluded_host_is_tunnelled() {
    let tls = upstream_tls();
    let upstream = spawn_https_upstream(tls.server_config.clone()).await;
    let h = start_proxy(None).await;
    // Tunnel untouched: the client must then trust the *upstream* CA directly.
    let mut settings = ProxySettings::default();
    settings.ssl.exclude.push("localhost".into());
    let ca = Arc::new(CertificateAuthority::generate().unwrap());
    let store = FlowStore::default();
    let handle = ProxyServer::start(
        ProxyConfig { listen: "127.0.0.1:0".parse().unwrap(), ..Default::default() },
        settings,
        ca,
        store.clone(),
    )
    .await
    .unwrap();
    let c = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{}", handle.local_addr)).unwrap())
        .add_root_certificate(reqwest::Certificate::from_pem(tls.pem.as_bytes()).unwrap())
        .build()
        .unwrap();
    let resp = c.get(format!("https://localhost:{}/hello", upstream.port())).send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "hi there");
    drop(c);
    let flows = wait_completed(&store, 1).await;
    assert_eq!(flows[0].request.method, "CONNECT");
    let _ = h;
}

#[tokio::test]
async fn websocket_through_connect_tunnel() {
    let ws = spawn_ws_echo().await;
    let h = start_proxy(None).await;
    let mut stream = TcpStream::connect(h.proxy).await.unwrap();
    stream.write_all(format!("CONNECT {ws} HTTP/1.1\r\nHost: {ws}\r\n\r\n").as_bytes()).await.unwrap();
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).await.unwrap();
    assert!(std::str::from_utf8(&buf[..n]).unwrap().starts_with("HTTP/1.1 200"));

    let (mut socket, resp) = tokio_tungstenite::client_async(format!("ws://{ws}/chat"), stream).await.unwrap();
    assert_eq!(resp.status(), 101);
    socket.send(Message::text("hello ws")).await.unwrap();
    let echoed = socket.next().await.unwrap().unwrap();
    assert_eq!(echoed.into_text().unwrap().as_str(), "hello ws");
    socket.close(None).await.unwrap();

    let flows = wait_completed(&h.store, 1).await;
    let f = &flows[0];
    assert_eq!(f.request.scheme, Scheme::Ws);
    assert_eq!(f.response.as_ref().unwrap().status, 101);
    assert!(f.ws_messages.iter().any(|m| m.direction == WsDirection::Outgoing && &m.payload[..] == b"hello ws"));
    assert!(f.ws_messages.iter().any(|m| m.direction == WsDirection::Incoming && &m.payload[..] == b"hello ws"));
}

#[tokio::test]
async fn serves_certificate_page() {
    let h = start_proxy(None).await;
    let c =
        reqwest::Client::builder().proxy(reqwest::Proxy::http(format!("http://{}", h.proxy)).unwrap()).build().unwrap();
    let pem = c.get("http://proxyzip.cert/ca.pem").send().await.unwrap().text().await.unwrap();
    assert_eq!(pem, h.ca.cert_pem());
    assert!(h.store.is_empty(), "local pages are not recorded");
}

#[tokio::test]
async fn client_abort_marks_flow_failed() {
    // Upstream accepts but never answers.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let (s, _) = listener.accept().await.unwrap();
            held.push(s);
        }
    });
    let h = start_proxy(None).await;
    let c = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{}", h.proxy)).unwrap())
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap();
    assert!(c.get(format!("http://{upstream}/hang")).send().await.is_err());
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(flows[0].state, FlowState::Failed);
    assert!(flows[0].error.as_ref().unwrap().contains("client closed"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Rules: block, map local, map remote, rewrite, record only
// ---------------------------------------------------------------------------------------------------------------------

fn set_rules(h: &Harness, rules: RuleSet) {
    h.settings.write().rules = Arc::new(rules);
}

fn plain_client(h: &Harness) -> reqwest::Client {
    reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{}", h.proxy)).unwrap())
        .no_gzip()
        .build()
        .unwrap()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("proxyzip-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn flow_for<'a>(flows: &'a [proxyzip_core::Flow], path: &str) -> &'a proxyzip_core::Flow {
    flows.iter().find(|f| f.request.path == path).unwrap_or_else(|| panic!("no flow for {path}"))
}

#[tokio::test]
async fn block_rules_answer_or_drop_the_connection() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.block.push(BlockRule { matcher: Matcher::new(format!("{up}/blocked/*")), ..Default::default() });
    rules.block.push(BlockRule {
        matcher: Matcher::new(format!("{up}/dropped")),
        action: BlockAction::Drop,
        ..Default::default()
    });
    set_rules(&h, rules);
    let c = plain_client(&h);

    let blocked = c.get(format!("http://{up}/blocked/x")).send().await.unwrap();
    assert_eq!(blocked.status(), 403);
    assert_eq!(blocked.headers()["x-proxyzip-rule"], "block");
    assert!(c.get(format!("http://{up}/dropped")).send().await.is_err());
    assert_eq!(c.get(format!("http://{up}/hello")).send().await.unwrap().text().await.unwrap(), "hi there");

    let flows = wait_completed(&h.store, 3).await;
    let blocked = flow_for(&flows, "/blocked/x");
    assert_eq!(blocked.response.as_ref().unwrap().status, 403);
    assert!(blocked.tags.contains(&"blocked".to_string()));
    let dropped = flow_for(&flows, "/dropped");
    assert_eq!(dropped.state, FlowState::Failed);
    assert!(dropped.error.as_ref().unwrap().contains("dropped"));
    assert!(flow_for(&flows, "/hello").tags.is_empty());
}

#[tokio::test]
async fn map_local_serves_files_and_directories_without_a_server() {
    let dir = temp_dir("maplocal");
    std::fs::create_dir_all(dir.join("js")).unwrap();
    std::fs::write(dir.join("data.json"), r#"{"mocked":true}"#).unwrap();
    std::fs::write(dir.join("js/app.js"), "console.log(1)").unwrap();
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    // Port 1 never answers: proof that the request does not reach the network.
    rules.map_local.push(MapLocalRule {
        matcher: Matcher::new("127.0.0.1:1/api/me"),
        source: LocalSource::File { path: dir.join("data.json") },
        status: 201,
        headers: vec![("x-mock".into(), "yes".into())],
        ..Default::default()
    });
    rules.map_local.push(MapLocalRule {
        matcher: Matcher::new("127.0.0.1:1/static/*"),
        source: LocalSource::Directory { path: dir.clone() },
        ..Default::default()
    });
    set_rules(&h, rules);
    let c = plain_client(&h);

    let me = c.get("http://127.0.0.1:1/api/me?x=1").send().await.unwrap();
    assert_eq!(me.status(), 201);
    assert_eq!(me.headers()["content-type"], "application/json");
    assert_eq!(me.headers()["x-mock"], "yes");
    assert_eq!(me.text().await.unwrap(), r#"{"mocked":true}"#);

    let js = c.get("http://127.0.0.1:1/static/js/app.js").send().await.unwrap();
    assert!(js.headers()["content-type"].to_str().unwrap().starts_with("text/javascript"));
    assert_eq!(js.text().await.unwrap(), "console.log(1)");

    let missing = c.get("http://127.0.0.1:1/static/nope.css").send().await.unwrap();
    assert_eq!(missing.status(), 404);
    assert!(missing.text().await.unwrap().contains("cannot read"));

    let flows = wait_completed(&h.store, 3).await;
    assert!(flows.iter().all(|f| f.tags.contains(&"map-local".to_string())));
    assert_eq!(&flow_for(&flows, "/api/me?x=1").response.as_ref().unwrap().body[..], br#"{"mocked":true}"#);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn map_remote_redirects_to_another_server() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.map_remote.push(MapRemoteRule {
        matcher: Matcher::new("127.0.0.1:1/*"),
        host: Some("127.0.0.1".into()),
        port: Some(up.port()),
        path: Some("/hello".into()),
        ..Default::default()
    });
    set_rules(&h, rules);
    let resp = plain_client(&h).get("http://127.0.0.1:1/anything?q=1").send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "hi there");
    let flows = wait_completed(&h.store, 1).await;
    let f = &flows[0];
    assert_eq!(f.request.port, up.port(), "the flow shows where the request really went");
    assert_eq!(f.request.path, "/hello?q=1", "the query string survives a path replacement");
    assert!(f.tags.contains(&"map-remote".to_string()));
}

#[tokio::test]
async fn rewrite_changes_request_and_response_headers_and_status() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.rewrite.push(RewriteRule {
        matcher: Matcher::new(format!("{up}/headers")),
        phase: Phase::Request,
        ops: vec![
            RewriteOp::SetHeader { name: "x-env".into(), value: "dev".into() },
            RewriteOp::SetQuery { name: "debug".into(), value: "1".into() },
        ],
        ..Default::default()
    });
    rules.rewrite.push(RewriteRule {
        matcher: Matcher::new(format!("{up}/headers")),
        phase: Phase::Response,
        ops: vec![
            RewriteOp::SetStatus { status: 202 },
            RewriteOp::SetHeader { name: "x-rewritten".into(), value: "1".into() },
        ],
        ..Default::default()
    });
    set_rules(&h, rules);
    let resp = plain_client(&h).get(format!("http://{up}/headers?a=b")).send().await.unwrap();
    assert_eq!(resp.status(), 202);
    assert_eq!(resp.headers()["x-rewritten"], "1");
    let body = resp.text().await.unwrap();
    assert!(body.contains("x-env: dev"), "{body}");
    assert!(body.contains("query: a=b&debug=1"), "{body}");

    let flows = wait_completed(&h.store, 1).await;
    assert!(flows[0].tags.contains(&"rewrite".to_string()));
    assert_eq!(flows[0].response.as_ref().unwrap().status, 202);
    assert_eq!(flows[0].request.headers.get("x-env"), Some("dev"));
}

#[tokio::test]
async fn rewrite_edits_a_compressed_body() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.rewrite.push(RewriteRule {
        matcher: Matcher::new(format!("{up}/gzip")),
        phase: Phase::Response,
        ops: vec![RewriteOp::ReplaceBody { find: "true".into(), replace: "false".into(), regex: false }],
        ..Default::default()
    });
    set_rules(&h, rules);
    let resp = plain_client(&h).get(format!("http://{up}/gzip")).send().await.unwrap();
    assert!(resp.headers().get("content-encoding").is_none(), "the client gets plain bytes after an edit");
    assert_eq!(resp.text().await.unwrap(), r#"{"ok":false}"#);
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(&flows[0].response.as_ref().unwrap().body[..], br#"{"ok":false}"#);
}

#[tokio::test]
async fn record_only_proxies_everything_but_stores_matches() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.record_only = RecordOnly { enabled: true, matchers: vec![Matcher::new(format!("{up}/echo"))] };
    set_rules(&h, rules);
    let c = plain_client(&h);
    assert_eq!(c.get(format!("http://{up}/hello")).send().await.unwrap().text().await.unwrap(), "hi there");
    assert_eq!(c.post(format!("http://{up}/echo")).body("x").send().await.unwrap().text().await.unwrap(), "x");
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].request.path, "/echo");
}

// ---------------------------------------------------------------------------------------------------------------------
// Breakpoints
// ---------------------------------------------------------------------------------------------------------------------

async fn wait_paused(h: &Harness) -> u64 {
    for _ in 0..250 {
        if let Some(id) = h.breakpoints.paused().first().copied() {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("nothing paused: {:#?}", h.store.summaries());
}

fn breakpoint_rules(pattern: String, on_request: bool, on_response: bool) -> RuleSet {
    let mut rules = RuleSet::default();
    rules.breakpoints.push(BreakpointRule {
        matcher: Matcher::new(pattern),
        on_request,
        on_response,
        ..Default::default()
    });
    rules
}

#[tokio::test]
async fn request_breakpoint_lets_the_user_edit_the_request() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    set_rules(&h, breakpoint_rules(format!("{up}/echo"), true, false));
    let c = plain_client(&h);
    let url = format!("http://{up}/echo");
    let call = tokio::spawn(async move { c.post(url).body("ping").send().await.unwrap().text().await.unwrap() });

    let id = wait_paused(&h).await;
    let flow = h.store.get(id).unwrap();
    assert_eq!(flow.state, FlowState::Paused);
    assert_eq!(&flow.request.body[..], b"ping");

    let edit = RequestEdit {
        method: "POST".into(),
        url: flow.request.url(),
        headers: flow.request.headers.0.clone(),
        body_base64: B64.encode("pong-edited"),
    };
    assert!(h.breakpoints.resolve(id, Decision::EditRequest(edit)));
    assert_eq!(call.await.unwrap(), "pong-edited", "content-length was recomputed for the longer body");

    let flows = wait_completed(&h.store, 1).await;
    assert!(flows[0].tags.contains(&"breakpoint".to_string()) && flows[0].tags.contains(&"edited".to_string()));
    assert_eq!(&flows[0].request.body[..], b"pong-edited");
}

#[tokio::test]
async fn request_breakpoint_can_redirect_by_editing_the_url() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    set_rules(&h, breakpoint_rules("127.0.0.1:1/*".into(), true, false));
    let c = plain_client(&h);
    let call =
        tokio::spawn(async move { c.get("http://127.0.0.1:1/nowhere").send().await.unwrap().text().await.unwrap() });
    let id = wait_paused(&h).await;
    let flow = h.store.get(id).unwrap();
    let edit = RequestEdit {
        method: "GET".into(),
        url: format!("http://{up}/hello"),
        headers: flow.request.headers.0.clone(),
        body_base64: String::new(),
    };
    h.breakpoints.resolve(id, Decision::EditRequest(edit));
    assert_eq!(call.await.unwrap(), "hi there");
    let flows = wait_completed(&h.store, 1).await;
    assert_eq!((flows[0].request.port, flows[0].request.path.as_str()), (up.port(), "/hello"));
}

#[tokio::test]
async fn response_breakpoint_can_edit_or_abort() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    set_rules(&h, breakpoint_rules(format!("{up}/hello"), false, true));
    let c = plain_client(&h);

    // Edit the response.
    let url = format!("http://{up}/hello");
    let call = tokio::spawn({
        let (c, url) = (c.clone(), url.clone());
        async move { c.get(url).send().await.unwrap() }
    });
    let id = wait_paused(&h).await;
    let paused = h.store.get(id).unwrap();
    assert_eq!(&paused.response.as_ref().unwrap().body[..], b"hi there", "the real response is visible while paused");
    h.breakpoints.resolve(
        id,
        Decision::EditResponse(ResponseEdit {
            status: 418,
            headers: vec![("content-type".into(), "text/plain".into())],
            body_base64: B64.encode("teapot"),
        }),
    );
    let resp = call.await.unwrap();
    assert_eq!(resp.status(), 418);
    assert_eq!(resp.text().await.unwrap(), "teapot");

    // Abort the next one.
    let call = tokio::spawn({
        let c = c.clone();
        async move { c.get(url).send().await }
    });
    let id = wait_paused(&h).await;
    h.breakpoints.resolve(id, Decision::Abort);
    assert!(call.await.unwrap().is_err());

    let flows = wait_completed(&h.store, 2).await;
    assert_eq!(flows.iter().filter(|f| f.state == FlowState::Failed).count(), 1);
    assert!(flows.iter().any(|f| f.tags.contains(&"edited".to_string())));
}

#[tokio::test]
async fn continue_resumes_a_paused_flow_unchanged() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    set_rules(&h, breakpoint_rules(format!("{up}/hello"), true, true));
    let c = plain_client(&h);
    let url = format!("http://{up}/hello");
    let call = tokio::spawn(async move { c.get(url).send().await.unwrap().text().await.unwrap() });
    let id = wait_paused(&h).await;
    h.breakpoints.resolve(id, Decision::Continue); // request phase
    let id = wait_paused(&h).await;
    h.breakpoints.resolve(id, Decision::Continue); // response phase
    assert_eq!(call.await.unwrap(), "hi there");
}

// ---------------------------------------------------------------------------------------------------------------------
// Network tools: throttling, DNS, upstream proxies, SOCKS5, reverse proxy, PAC
// ---------------------------------------------------------------------------------------------------------------------

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn settings_with(rules: RuleSet) -> ProxySettings {
    ProxySettings { rules: Arc::new(rules), ..ProxySettings::default() }
}

#[tokio::test]
async fn throttling_adds_latency_and_limits_bandwidth() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let c = plain_client(&h);

    let started = std::time::Instant::now();
    assert_eq!(c.get(format!("http://{up}/big")).send().await.unwrap().bytes().await.unwrap().len(), 20_000);
    let baseline = started.elapsed();

    let mut rules = RuleSet::default();
    rules.throttle = Throttle {
        enabled: true,
        profile: ThrottleProfile { name: "test".into(), latency_ms: 300, download_kbps: 160, upload_kbps: 0 },
        matchers: vec![],
    };
    set_rules(&h, rules);
    let started = std::time::Instant::now();
    let body = c.get(format!("http://{up}/big")).send().await.unwrap().bytes().await.unwrap();
    let throttled = started.elapsed();
    assert_eq!(body.len(), 20_000, "no byte is lost");
    // 20 kB at 20 kB/s is one second, plus 300 ms of latency.
    assert!(throttled >= Duration::from_millis(1100), "throttled={throttled:?} baseline={baseline:?}");
    assert!(throttled < Duration::from_secs(6), "throttled={throttled:?}");

    let flows = wait_completed(&h.store, 2).await;
    assert!(flows.iter().any(|f| f.tags.contains(&"throttle".to_string())));
}

#[tokio::test]
async fn dns_overrides_point_names_at_chosen_addresses() {
    let up = spawn_http_upstream().await;
    let h = start_proxy(None).await;
    let c = plain_client(&h);
    let url = format!("http://made-up-host.test:{}/hello", up.port());

    let unresolved = c.get(&url).send().await.unwrap();
    assert_eq!(unresolved.status(), 502, "without a rule the name does not resolve");

    let mut rules = RuleSet::default();
    rules.dns.push(DnsRule { host: "*.test".into(), ip: Some("127.0.0.1".parse().unwrap()), ..Default::default() });
    set_rules(&h, rules);
    assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "hi there");
}

#[tokio::test]
async fn http_upstream_proxy_chains_two_instances() {
    let up = spawn_http_upstream().await;
    let second = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.upstream = UpstreamProxy {
        enabled: true,
        kind: UpstreamKind::Http,
        host: "127.0.0.1".into(),
        port: second.proxy.port(),
        ..Default::default()
    };
    let first = start_proxy_with(None, settings_with(rules)).await;
    let resp = plain_client(&first).get(format!("http://{up}/hello")).send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "hi there");
    let seen_by_first = wait_completed(&first.store, 1).await;
    let seen_by_second = wait_completed(&second.store, 1).await;
    assert_eq!(seen_by_first[0].request.path, "/hello");
    assert_eq!(seen_by_second[0].request.path, "/hello", "the request really went through the second proxy");
}

#[tokio::test]
async fn upstream_bypass_skips_the_chained_proxy() {
    let up = spawn_http_upstream().await;
    let unreachable = free_port();
    let mut rules = RuleSet::default();
    rules.upstream = UpstreamProxy {
        enabled: true,
        host: "127.0.0.1".into(),
        port: unreachable,
        bypass: vec!["127.0.0.1".into()],
        ..Default::default()
    };
    let h = start_proxy_with(None, settings_with(rules)).await;
    // The configured proxy is dead, so this only works because the host is bypassed.
    let resp = plain_client(&h).get(format!("http://{up}/hello")).send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "hi there");
}

#[tokio::test]
async fn socks5_listener_feeds_the_interception_pipeline() {
    let tls = upstream_tls();
    let https_up = spawn_https_upstream(tls.server_config.clone()).await;
    let http_up = spawn_http_upstream().await;
    let port = free_port();
    let mut rules = RuleSet::default();
    rules.socks = SocksListener { enabled: true, port };
    let h = start_proxy_with(Some(tls.der.clone()), settings_with(rules)).await;
    assert!(h.handle.listener_errors.is_empty(), "{:?}", h.handle.listener_errors);

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("socks5h://127.0.0.1:{port}")).unwrap())
        .add_root_certificate(reqwest::Certificate::from_pem(h.ca.cert_pem().as_bytes()).unwrap())
        .no_gzip()
        .build()
        .unwrap();
    assert_eq!(client.get(format!("http://{http_up}/hello")).send().await.unwrap().text().await.unwrap(), "hi there");
    let secure = client.get(format!("https://localhost:{}/hello", https_up.port())).send().await.unwrap();
    assert_eq!(secure.text().await.unwrap(), "hi there");

    let flows = wait_completed(&h.store, 2).await;
    assert!(flows.iter().any(|f| f.request.scheme == Scheme::Http && f.request.path == "/hello"));
    let https_flow = flows.iter().find(|f| f.request.scheme == Scheme::Https).expect("decrypted HTTPS flow");
    assert_eq!(&https_flow.response.as_ref().unwrap().body[..], b"hi there");
}

#[tokio::test]
async fn socks5_upstream_proxy_chains_through_another_instance() {
    let up = spawn_http_upstream().await;
    let socks_port = free_port();
    let mut second_rules = RuleSet::default();
    second_rules.socks = SocksListener { enabled: true, port: socks_port };
    let second = start_proxy_with(None, settings_with(second_rules)).await;

    let mut rules = RuleSet::default();
    rules.upstream = UpstreamProxy {
        enabled: true,
        kind: UpstreamKind::Socks5,
        host: "127.0.0.1".into(),
        port: socks_port,
        ..Default::default()
    };
    let first = start_proxy_with(None, settings_with(rules)).await;
    let resp = plain_client(&first).get(format!("http://{up}/hello")).send().await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "hi there");
    assert_eq!(wait_completed(&second.store, 1).await[0].request.path, "/hello");
}

#[tokio::test]
async fn reverse_proxy_exposes_a_target_on_a_local_port() {
    let up = spawn_http_upstream().await;
    let port = free_port();
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.reverse.push(ReverseRule { listen_port: port, target: format!("http://{up}"), ..Default::default() });
    assert!(h.handle.apply_rules(rules).await.is_empty());

    // A plain client with no proxy configured: it just talks to the local port.
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/headers?x=1")).await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains(&format!("host: {up}")), "the server sees its own host name:\n{body}");
    assert!(body.contains("query: x=1"));

    let flows = wait_completed(&h.store, 1).await;
    assert_eq!(flows[0].request.port, up.port());
    assert_eq!(flows[0].request.host, "127.0.0.1");
}

#[tokio::test]
async fn listeners_follow_the_rules_and_report_bind_failures() {
    let up = spawn_http_upstream().await;
    let port = free_port();
    let h = start_proxy(None).await;
    let mut rules = RuleSet::default();
    rules.reverse.push(ReverseRule { listen_port: port, target: format!("http://{up}"), ..Default::default() });
    assert!(h.handle.apply_rules(rules).await.is_empty());
    assert!(reqwest::get(format!("http://127.0.0.1:{port}/hello")).await.is_ok());

    // Removing the rule closes the port.
    assert!(h.handle.apply_rules(RuleSet::default()).await.is_empty());
    assert!(reqwest::get(format!("http://127.0.0.1:{port}/hello")).await.is_err());

    // A port that is already taken is reported instead of silently ignored.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut rules = RuleSet::default();
    rules.reverse.push(ReverseRule {
        listen_port: taken.local_addr().unwrap().port(),
        target: format!("http://{up}"),
        ..Default::default()
    });
    let errors = h.handle.apply_rules(rules).await;
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("reverse proxy"), "{errors:?}");

    // So is a target that is not a URL.
    let mut rules = RuleSet::default();
    rules.reverse.push(ReverseRule { listen_port: free_port(), target: "not a url".into(), ..Default::default() });
    assert_eq!(h.handle.apply_rules(rules).await.len(), 1);
}

#[tokio::test]
async fn serves_a_pac_file_pointing_back_at_the_proxy() {
    let h = start_proxy(None).await;
    let direct = reqwest::get(format!("http://{}/proxy.pac", h.proxy)).await.unwrap();
    assert_eq!(direct.headers()["content-type"], "application/x-ns-proxy-autoconfig");
    let script = direct.text().await.unwrap();
    assert!(script.contains("FindProxyForURL"));
    assert!(script.contains(&format!("PROXY {}", h.proxy)), "{script}");

    // Through the proxy, by the friendly name.
    let via_proxy = plain_client(&h).get("http://proxyzip.cert/proxy.pac").send().await.unwrap().text().await.unwrap();
    assert!(via_proxy.contains(&format!("PROXY 127.0.0.1:{}", h.proxy.port())), "{via_proxy}");
    assert!(h.store.is_empty(), "local pages are not recorded");
}
