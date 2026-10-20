//! HTTP/3: the proxy learns `Alt-Svc`, hides it from clients, and uses QUIC towards servers that offer it.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use proxyzip_core::{FlowState, FlowStore};
use proxyzip_proxy::{ProxyConfig, ProxyServer, ProxySettings};
use proxyzip_tls::CertificateAuthority;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;

struct Server {
    tls: Arc<rustls::ServerConfig>,
    root: CertificateDer<'static>,
}

fn server_tls() -> Server {
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
    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    Server { tls: Arc::new(cfg), root: ca_cert.der().clone() }
}

/// An HTTPS (h2) server whose answers advertise HTTP/3 on `h3_port`.
async fn spawn_tcp_server(server: &Server, h3_port: Option<u16>) -> SocketAddr {
    let mut cfg = (*server.tls).clone();
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(s).await else { return };
                let handler = service_fn(move |_req: Request<hyper::body::Incoming>| async move {
                    let mut res = Response::builder().header("content-type", "text/plain");
                    if let Some(port) = h3_port {
                        res = res.header("alt-svc", format!("h3=\":{port}\"; ma=60, h2=\":443\"; ma=60"));
                    }
                    Ok::<_, Infallible>(res.body(Full::new(Bytes::from_static(b"over tcp"))).unwrap())
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), handler)
                    .await;
            });
        }
    });
    addr
}

/// An HTTP/3 server on UDP. Counts the requests it answers.
fn spawn_h3_server(server: &Server) -> (SocketAddr, Arc<AtomicUsize>) {
    let mut tls = (*server.tls).clone();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let quic = quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap();
    let endpoint =
        quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(quic)), "127.0.0.1:0".parse().unwrap())
            .unwrap();
    let addr = endpoint.local_addr().unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = seen.clone();
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let counter = counter.clone();
            tokio::spawn(async move {
                let Ok(conn) = incoming.await else { return };
                let Ok(mut h3) = h3::server::Connection::new(h3_quinn::Connection::new(conn)).await else { return };
                while let Ok(Some(resolver)) = h3.accept().await {
                    let counter = counter.clone();
                    tokio::spawn(async move {
                        let Ok((req, mut stream)) = resolver.resolve_request().await else { return };
                        counter.fetch_add(1, Ordering::SeqCst);
                        let ua = req.headers().get("x-test").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
                        let res = Response::builder()
                            .header("content-type", "text/plain")
                            .header("x-seen", ua)
                            .body(())
                            .unwrap();
                        let _ = stream.send_response(res).await;
                        let _ = stream.send_data(Bytes::from_static(b"over h3")).await;
                        let _ = stream.finish().await;
                    });
                }
            });
        }
    });
    (addr, seen)
}

struct Harness {
    proxy: SocketAddr,
    store: FlowStore,
    ca: Arc<CertificateAuthority>,
    _handle: proxyzip_proxy::ProxyHandle,
}

async fn start_proxy(root: CertificateDer<'static>, settings: ProxySettings) -> Harness {
    let ca = Arc::new(CertificateAuthority::generate().unwrap());
    let store = FlowStore::default();
    let config =
        ProxyConfig { listen: "127.0.0.1:0".parse().unwrap(), extra_upstream_roots: vec![root], keylog_path: None };
    let handle = ProxyServer::start(config, settings, ca.clone(), store.clone()).await.unwrap();
    Harness { proxy: handle.local_addr, store, ca, _handle: handle }
}

fn client(h: &Harness) -> reqwest::Client {
    reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://{}", h.proxy)).unwrap())
        .add_root_certificate(reqwest::Certificate::from_pem(h.ca.cert_pem().as_bytes()).unwrap())
        .build()
        .unwrap()
}

async fn finished(store: &FlowStore, count: usize) -> Vec<proxyzip_core::Flow> {
    for _ in 0..200 {
        let flows = store.all();
        if flows.len() >= count && flows.iter().all(|f| matches!(f.state, FlowState::Completed | FlowState::Failed)) {
            return flows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("flows did not finish: {:#?}", store.summaries());
}

#[tokio::test]
async fn clients_never_see_http3_advertisements_by_default() {
    let server = server_tls();
    let tcp = spawn_tcp_server(&server, Some(4433)).await;
    let h = start_proxy(server.root.clone(), ProxySettings::default()).await;

    let res = client(&h).get(format!("https://127.0.0.1:{}/x", tcp.port())).send().await.unwrap();
    let alt: Vec<_> = res.headers().get_all("alt-svc").iter().map(|v| v.to_str().unwrap().to_string()).collect();
    assert_eq!(alt, vec![r#"h2=":443"; ma=60"#], "h3 is removed and the other alternatives stay");
    assert_eq!(res.text().await.unwrap(), "over tcp");

    // The capture keeps what the server really sent.
    let flows = finished(&h.store, 1).await;
    let recorded = flows[0].response.as_ref().unwrap().headers.get("alt-svc").unwrap().to_string();
    assert!(recorded.contains("h3=\":4433\""), "{recorded}");
}

#[tokio::test]
async fn the_advertisement_can_be_left_alone() {
    let server = server_tls();
    let tcp = spawn_tcp_server(&server, Some(4433)).await;
    let settings = ProxySettings { strip_alt_svc: false, ..Default::default() };
    let h = start_proxy(server.root.clone(), settings).await;
    let res = client(&h).get(format!("https://127.0.0.1:{}/x", tcp.port())).send().await.unwrap();
    assert!(res.headers()["alt-svc"].to_str().unwrap().contains("h3="));
}

#[tokio::test]
async fn a_server_that_offers_http3_is_reached_over_quic() {
    let server = server_tls();
    let (udp, h3_seen) = spawn_h3_server(&server);
    let tcp = spawn_tcp_server(&server, Some(udp.port())).await;
    let settings = ProxySettings { http3_upstream: true, ..Default::default() };
    let h = start_proxy(server.root.clone(), settings).await;
    let c = client(&h);
    let url = format!("https://127.0.0.1:{}/x", tcp.port());

    // The first request goes over TCP and teaches the proxy about HTTP/3.
    assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over tcp");
    assert_eq!(h3_seen.load(Ordering::SeqCst), 0);

    let res = c.get(&url).header("x-test", "hello").send().await.unwrap();
    assert_eq!(res.headers()["x-seen"], "hello", "request headers reach the HTTP/3 server");
    assert_eq!(res.text().await.unwrap(), "over h3");
    assert_eq!(h3_seen.load(Ordering::SeqCst), 1);

    let flows = finished(&h.store, 2).await;
    let second = flows.iter().find(|f| f.id == 2).unwrap();
    assert_eq!(second.response.as_ref().unwrap().http_version, "HTTP/3");
    assert_eq!(&second.response.as_ref().unwrap().body[..], b"over h3");
    assert_eq!(second.state, FlowState::Completed);

    // Connections are reused.
    for _ in 0..3 {
        assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over h3");
    }
}

#[tokio::test]
async fn http3_is_off_unless_asked_for() {
    let server = server_tls();
    let (udp, h3_seen) = spawn_h3_server(&server);
    let tcp = spawn_tcp_server(&server, Some(udp.port())).await;
    let h = start_proxy(server.root.clone(), ProxySettings::default()).await;
    let c = client(&h);
    let url = format!("https://127.0.0.1:{}/x", tcp.port());
    for _ in 0..3 {
        assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over tcp");
    }
    assert_eq!(h3_seen.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_unreachable_http3_endpoint_falls_back_to_tcp() {
    let server = server_tls();
    // Advertise a UDP port nobody listens on.
    let dead = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let dead_port = dead.local_addr().unwrap().port();
    drop(dead);
    let tcp = spawn_tcp_server(&server, Some(dead_port)).await;
    let settings = ProxySettings { http3_upstream: true, ..Default::default() };
    let h = start_proxy(server.root.clone(), settings).await;
    let c = client(&h);
    let url = format!("https://127.0.0.1:{}/x", tcp.port());
    assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over tcp");
    // The second request tries QUIC, fails to connect and still succeeds over TCP.
    assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over tcp");
    // Afterwards the origin is skipped for a while, so this one is quick.
    let started = std::time::Instant::now();
    assert_eq!(c.get(&url).send().await.unwrap().text().await.unwrap(), "over tcp");
    assert!(started.elapsed() < Duration::from_secs(2));
    let flows = finished(&h.store, 3).await;
    assert!(flows.iter().all(|f| f.state == FlowState::Completed));
}

/// A plain HTTP server that answers every request with the path it was asked for.
async fn spawn_plain_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let handler = service_fn(|req: Request<hyper::body::Incoming>| async move {
                    let body = format!("you asked for {}", req.uri().path());
                    Ok::<_, Infallible>(
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body(Full::new(Bytes::from(body)))
                            .unwrap(),
                    )
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(s), handler)
                    .await;
            });
        }
    });
    addr
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn a_reverse_proxy_can_accept_http3_clients() {
    use bytes::Buf;
    let upstream = spawn_plain_server().await;
    let server = server_tls();
    let h = start_proxy(server.root.clone(), ProxySettings::default()).await;
    let port = free_udp_port();
    let mut rules = proxyzip_rules::RuleSet::default();
    rules.reverse.push(proxyzip_rules::ReverseRule {
        name: "api".into(),
        listen_port: port,
        target: format!("http://{upstream}"),
        h3: true,
        ..Default::default()
    });
    let errors = h._handle.apply_rules(rules).await;
    assert!(errors.iter().all(|e| !e.contains("HTTP/3")), "{errors:?}");

    // An HTTP/3 client that trusts the ProxyZip CA.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(h.ca.cert_der().clone()).unwrap();
    let mut tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let client_config =
        quinn::ClientConfig::new(Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap()));
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config);
    let conn = endpoint.connect(format!("127.0.0.1:{port}").parse().unwrap(), "localhost").unwrap().await.unwrap();
    let (mut driver, mut send) = h3::client::new(h3_quinn::Connection::new(conn)).await.unwrap();
    tokio::spawn(async move {
        let _ = driver.wait_idle().await;
    });
    let req = Request::builder().uri("https://localhost/greeting?x=1").body(()).unwrap();
    let mut stream = send.send_request(req).await.unwrap();
    stream.finish().await.unwrap();
    let res = stream.recv_response().await.unwrap();
    assert_eq!(res.status(), 200);
    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await.unwrap() {
        body.extend_from_slice(&chunk.copy_to_bytes(chunk.remaining()));
    }
    assert_eq!(body, b"you asked for /greeting");

    let flows = finished(&h.store, 1).await;
    let flow = &flows[0];
    assert_eq!(flow.request.http_version, "HTTP/3");
    assert_eq!(flow.request.path, "/greeting?x=1");
    assert_eq!(flow.tls.as_ref().unwrap().alpn.as_deref(), Some("h3"));
    assert_eq!(flow.tls.as_ref().unwrap().sni.as_deref(), Some("localhost"));
    assert_eq!(&flow.response.as_ref().unwrap().body[..], b"you asked for /greeting");

    // Turning the option off closes the UDP listener.
    let mut rules = proxyzip_rules::RuleSet::default();
    rules.reverse.push(proxyzip_rules::ReverseRule {
        listen_port: port,
        target: format!("http://{upstream}"),
        h3: false,
        ..Default::default()
    });
    h._handle.apply_rules(rules).await;
    let mut freed = false;
    for _ in 0..50 {
        if std::net::UdpSocket::bind(format!("127.0.0.1:{port}")).is_ok() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(freed, "the UDP port is free again");
}
