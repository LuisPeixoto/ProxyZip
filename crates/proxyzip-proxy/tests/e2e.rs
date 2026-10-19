//! End-to-end tests: real client → ProxyZip → real upstream (HTTP, HTTPS/h2, WebSocket).

use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxyzip_core::{FlowState, FlowStore, Scheme, WsDirection};
use proxyzip_proxy::{ProxyConfig, ProxyServer, ProxySettings};
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
}

async fn start_proxy(extra_root: Option<CertificateDer<'static>>) -> Harness {
    let ca = Arc::new(CertificateAuthority::generate().unwrap());
    let store = FlowStore::default();
    let config = ProxyConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        extra_upstream_roots: extra_root.into_iter().collect(),
        keylog_path: None,
    };
    let handle = ProxyServer::start(config, ProxySettings::default(), ca.clone(), store.clone()).await.unwrap();
    let proxy = handle.local_addr;
    std::mem::forget(handle);
    Harness { proxy, store, ca }
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
