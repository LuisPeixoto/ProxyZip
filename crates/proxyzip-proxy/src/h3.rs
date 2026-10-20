//! HTTP/3 towards servers (and, for reverse proxies, from clients).
//!
//! QUIC runs over UDP, so it cannot be tunnelled through an HTTP or SOCKS5 upstream proxy. The proxy therefore only
//! talks HTTP/3 itself when a server advertised it with `Alt-Svc` ([`crate::altsvc`]) and falls back to TCP otherwise.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use http::{Request, Response};
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full};
use proxyzip_core::TlsInfo;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::ClientConfig;
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::ProxyConfig;
use crate::server::{Conn, Ctx, ReverseTarget};
use crate::upstream::{AcceptAnyCert, Settings, root_store};
use crate::util::{BoxError, UpBody};

type SendRequest = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;

/// Why an HTTP/3 request did not produce a response.
#[derive(Debug)]
pub enum H3Error {
    /// No connection could be made: nothing was sent, so the request can be retried over TCP.
    Connect(String),
    /// The connection worked but the exchange failed.
    Request(String),
}

impl std::fmt::Display for H3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H3Error::Connect(m) | H3Error::Request(m) => f.write_str(m),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    host: String,
    port: u16,
    insecure: bool,
}

pub struct H3Client {
    verified: quinn::ClientConfig,
    insecure: quinn::ClientConfig,
    endpoints: Mutex<HashMap<bool, quinn::Endpoint>>,
    pool: Mutex<HashMap<PoolKey, SendRequest>>,
}

fn quic_config(mut tls: ClientConfig) -> quinn::ClientConfig {
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let quic = QuicClientConfig::try_from(tls).expect("rustls config is valid for QUIC");
    let mut config = quinn::ClientConfig::new(Arc::new(quic));
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into().expect("valid timeout")));
    config.transport_config(Arc::new(transport));
    config
}

impl H3Client {
    pub fn new(config: &ProxyConfig, keylog: Option<Arc<dyn rustls::KeyLog>>) -> Self {
        let roots = root_store(config);
        let make = |insecure: bool| {
            let mut tls = if insecure {
                ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
                    .with_no_client_auth()
            } else {
                ClientConfig::builder().with_root_certificates(roots.clone()).with_no_client_auth()
            };
            if let Some(k) = &keylog {
                tls.key_log = k.clone();
            }
            quic_config(tls)
        };
        Self {
            verified: make(false),
            insecure: make(true),
            endpoints: Mutex::new(HashMap::new()),
            pool: Mutex::new(HashMap::new()),
        }
    }

    async fn endpoint(&self, ipv6: bool) -> Result<quinn::Endpoint, H3Error> {
        let mut endpoints = self.endpoints.lock().await;
        if let Some(e) = endpoints.get(&ipv6) {
            return Ok(e.clone());
        }
        let bind: SocketAddr = if ipv6 { "[::]:0" } else { "0.0.0.0:0" }.parse().expect("valid address");
        let endpoint = quinn::Endpoint::client(bind).map_err(|e| H3Error::Connect(format!("UDP socket: {e}")))?;
        endpoints.insert(ipv6, endpoint.clone());
        Ok(endpoint)
    }

    async fn resolve(settings: &Settings, host: &str, port: u16) -> Result<Vec<SocketAddr>, H3Error> {
        let forced = settings.read().rules.dns_lookup(host);
        if let Some(ip) = forced {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| H3Error::Connect(format!("resolving {host}: {e}")))?
            .collect();
        if addrs.is_empty() {
            return Err(H3Error::Connect(format!("{host} has no address")));
        }
        Ok(addrs)
    }

    async fn connect(&self, settings: &Settings, key: &PoolKey) -> Result<SendRequest, H3Error> {
        let addrs = Self::resolve(settings, &key.host, key.port).await?;
        let cfg = if key.insecure { &self.insecure } else { &self.verified };
        let mut last = H3Error::Connect("no address to connect to".into());
        for addr in addrs {
            let endpoint = self.endpoint(addr.is_ipv6()).await?;
            let connecting = match endpoint.connect_with(cfg.clone(), addr, &key.host) {
                Ok(c) => c,
                Err(e) => {
                    last = H3Error::Connect(format!("QUIC connect to {addr}: {e}"));
                    continue;
                }
            };
            let conn = match tokio::time::timeout(std::time::Duration::from_secs(5), connecting).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    last = H3Error::Connect(format!("QUIC handshake with {addr}: {e}"));
                    continue;
                }
                Err(_) => {
                    last = H3Error::Connect(format!("QUIC handshake with {addr} timed out"));
                    continue;
                }
            };
            let (mut driver, send) = h3::client::new(h3_quinn::Connection::new(conn))
                .await
                .map_err(|e| H3Error::Connect(format!("HTTP/3 setup with {addr}: {e}")))?;
            tokio::spawn(async move {
                let _ = driver.wait_idle().await;
            });
            return Ok(send);
        }
        Err(last)
    }

    /// Send `req` (whose URI must be absolute) to `host:port` over HTTP/3.
    pub async fn send(
        &self,
        settings: &Settings,
        host: &str,
        port: u16,
        insecure: bool,
        req: Request<Full<Bytes>>,
    ) -> Result<Response<UpBody>, H3Error> {
        let key = PoolKey { host: host.to_ascii_lowercase(), port, insecure };
        let (parts, body) = req.into_parts();
        let body = body.collect().await.map(|b| b.to_bytes()).unwrap_or_default();
        let mut head = Request::builder()
            .method(parts.method)
            .uri(parts.uri)
            .body(())
            .map_err(|e| H3Error::Request(e.to_string()))?;
        *head.headers_mut() = parts.headers;
        head.headers_mut().remove(http::header::HOST);
        head.headers_mut().remove(http::header::CONTENT_LENGTH);

        // A pooled connection may have been closed by the server; retry once on a new one.
        for attempt in 0..2 {
            let pooled = self.pool.lock().await.get(&key).cloned();
            let reused = pooled.is_some();
            let mut send = match pooled {
                Some(s) => s,
                None => {
                    let s = self.connect(settings, &key).await?;
                    self.pool.lock().await.insert(key.clone(), s.clone());
                    s
                }
            };
            let mut stream = match send.send_request(head.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    self.pool.lock().await.remove(&key);
                    if reused && attempt == 0 {
                        continue;
                    }
                    return Err(H3Error::Connect(format!("HTTP/3 request: {e}")));
                }
            };
            let outcome = async {
                if !body.is_empty() {
                    stream.send_data(body.clone()).await?;
                }
                stream.finish().await?;
                stream.recv_response().await
            }
            .await;
            let response = match outcome {
                Ok(r) => r,
                Err(e) => {
                    self.pool.lock().await.remove(&key);
                    return Err(H3Error::Request(format!("HTTP/3 exchange: {e}")));
                }
            };
            let (mut rparts, _) = response.into_parts();
            rparts.version = http::Version::HTTP_3;
            let (tx, rx) = mpsc::channel::<Result<Bytes, BoxError>>(16);
            tokio::spawn(async move {
                loop {
                    match stream.recv_data().await {
                        Ok(Some(mut chunk)) => {
                            let bytes = chunk.copy_to_bytes(chunk.remaining());
                            if tx.send(Ok(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tx.send(Err(Box::new(e))).await;
                            break;
                        }
                    }
                }
            });
            return Ok(Response::from_parts(rparts, ChannelBody { rx }.boxed()));
        }
        Err(H3Error::Connect("HTTP/3 connection kept closing".into()))
    }
}

/// A response body fed by a task that reads the QUIC stream.
struct ChannelBody {
    rx: mpsc::Receiver<Result<Bytes, BoxError>>,
}

impl Body for ChannelBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        self.rx.poll_recv(cx).map(|item| item.map(|r| r.map(Frame::data)))
    }
}

/// Certificates for the reverse proxy's QUIC listener: a leaf for whatever name the client asked for.
#[derive(Debug)]
struct CaResolver(Arc<proxyzip_tls::CertificateAuthority>);

impl rustls::server::ResolvesServerCert for CaResolver {
    fn resolve(&self, hello: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        self.0.leaf_for(hello.server_name().unwrap_or("localhost")).ok()
    }
}

/// Accept HTTP/3 on UDP `addr` and send every request to the reverse proxy `target`.
pub(crate) async fn serve_reverse(
    ctx: Arc<Ctx>,
    addr: SocketAddr,
    target: Arc<ReverseTarget>,
) -> Result<(watch::Sender<bool>, JoinHandle<()>), String> {
    let mut tls =
        rustls::ServerConfig::builder().with_no_client_auth().with_cert_resolver(Arc::new(CaResolver(ctx.ca.clone())));
    tls.alpn_protocols = vec![b"h3".to_vec()];
    if let Some(k) = &ctx.keylog {
        tls.key_log = k.clone();
    }
    let quic = QuicServerConfig::try_from(tls).map_err(|e| format!("HTTP/3 reverse proxy: {e}"))?;
    let endpoint = quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(quic)), addr)
        .map_err(|e| format!("could not start the HTTP/3 reverse proxy on UDP port {}: {e}", addr.port()))?;
    tracing::info!(%addr, "HTTP/3 reverse proxy started");
    let (tx, mut rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = rx.changed() => {
                    endpoint.close(0u32.into(), b"shutting down");
                    // Let the connections end so the UDP port is released.
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), endpoint.wait_idle()).await;
                    break;
                }
                incoming = endpoint.accept() => {
                    let Some(incoming) = incoming else { break };
                    let (ctx, target) = (ctx.clone(), target.clone());
                    tokio::spawn(async move { serve_connection(incoming, ctx, target).await });
                }
            }
        }
    });
    Ok((tx, task))
}

async fn serve_connection(incoming: quinn::Incoming, ctx: Arc<Ctx>, target: Arc<ReverseTarget>) {
    let Ok(conn) = incoming.await else { return };
    let peer = conn.remote_address();
    let server_name = conn
        .handshake_data()
        .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|d| d.server_name.clone());
    let Ok(mut h3) = h3::server::Connection::new(h3_quinn::Connection::new(conn)).await else { return };
    let tls = TlsInfo { sni: server_name, alpn: Some("h3".into()), version: Some("TLS 1.3".into()), cipher: None };
    while let Ok(Some(resolver)) = h3.accept().await {
        let (ctx, target, tls) = (ctx.clone(), target.clone(), tls.clone());
        tokio::spawn(async move {
            let Ok((request, mut stream)) = resolver.resolve_request().await else { return };
            let mut body = Vec::new();
            loop {
                match stream.recv_data().await {
                    Ok(Some(mut chunk)) => body.extend_from_slice(&chunk.copy_to_bytes(chunk.remaining())),
                    Ok(None) => break,
                    Err(_) => return,
                }
            }
            let (mut parts, _) = request.into_parts();
            parts.version = http::Version::HTTP_3;
            let request = Request::from_parts(parts, Full::new(Bytes::from(body)));
            let conn = Conn { peer, scheme: target.scheme, tunnel_target: None, tls: Some(tls), reverse: Some(target) };
            let Ok(response) = crate::forward::forward(request, ctx, conn).await else {
                // The flow already says why; resetting the stream tells the client.
                stream.stop_stream(h3::error::Code::H3_REQUEST_CANCELLED);
                return;
            };
            let (rparts, mut rbody) = response.into_parts();
            let mut head = Response::builder().status(rparts.status).body(()).expect("valid response head");
            *head.headers_mut() = rparts.headers;
            if stream.send_response(head).await.is_err() {
                return;
            }
            while let Some(frame) = rbody.frame().await {
                match frame {
                    Ok(frame) => {
                        if let Ok(data) = frame.into_data()
                            && !data.is_empty()
                            && stream.send_data(data).await.is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
            let _ = stream.finish().await;
        });
    }
}
