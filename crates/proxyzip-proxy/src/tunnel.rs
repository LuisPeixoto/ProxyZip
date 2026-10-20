use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use hyper_util::rt::TokioIo;
use proxyzip_core::{FlowState, Headers, Request, Scheme, TlsInfo, now_ms};
use rustls::ServerConfig;
use rustls::server::{Acceptor, ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_rustls::LazyConfigAcceptor;

use crate::rewind::Rewind;
use crate::server::{Conn, Ctx, serve_http};

const TLS_HANDSHAKE: u8 = 0x16;

/// Decide what to do with the bytes flowing through an established CONNECT tunnel.
pub(crate) async fn handle<I>(mut io: I, host: String, port: u16, ctx: Arc<Ctx>, peer: SocketAddr)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let intercept = ctx.settings.read().ssl.should_intercept(&host);
    let mut first = vec![0u8; 4096];
    let n = match io.read(&mut first).await {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };
    first.truncate(n);
    let prefix = Bytes::from(first);
    let io = Rewind::new(prefix.clone(), io);

    if prefix[0] == TLS_HANDSHAKE {
        if intercept {
            mitm_tls(io, host, port, ctx, peer).await;
        } else {
            raw_tunnel(io, host, port, ctx, peer).await;
        }
    } else if looks_like_http(&prefix) {
        // Plain HTTP (or ws://) sent through a CONNECT tunnel.
        let conn = Conn {
            peer,
            scheme: Scheme::Http,
            tunnel_target: Some((host, port)),
            tls: None,
            reverse: None,
            mock: None,
        };
        serve_http(TokioIo::new(io), ctx, conn).await;
    } else {
        raw_tunnel(io, host, port, ctx, peer).await;
    }
}

fn looks_like_http(b: &[u8]) -> bool {
    const METHODS: &[&[u8]] =
        &[b"GET ", b"POST ", b"PUT ", b"DELETE ", b"HEAD ", b"OPTIONS ", b"PATCH ", b"TRACE ", b"PRI * HTTP/2"];
    METHODS.iter().any(|m| b.starts_with(m))
}

#[derive(Debug)]
struct FixedCert(Arc<CertifiedKey>);

impl ResolvesServerCert for FixedCert {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

async fn mitm_tls<I>(io: Rewind<I>, host: String, port: u16, ctx: Arc<Ctx>, peer: SocketAddr)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let start = match LazyConfigAcceptor::new(Acceptor::default(), io).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, %host, "reading ClientHello failed");
            return;
        }
    };
    let hello = start.client_hello();
    let sni = hello.server_name().map(str::to_string);
    let offers_h2 = hello.alpn().is_some_and(|mut a| a.any(|p| p == b"h2"));
    let cert_host = sni.clone().unwrap_or_else(|| host.clone());

    let certified = match ctx.ca.leaf_for(&cert_host) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, host = %cert_host, "minting certificate failed");
            return;
        }
    };
    let mut config = ServerConfig::builder().with_no_client_auth().with_cert_resolver(Arc::new(FixedCert(certified)));
    config.alpn_protocols =
        if offers_h2 { vec![b"h2".to_vec(), b"http/1.1".to_vec()] } else { vec![b"http/1.1".to_vec()] };
    if let Some(k) = &ctx.keylog {
        config.key_log = k.clone();
    }

    let tls = match start.into_stream(Arc::new(config)).await {
        Ok(s) => s,
        Err(e) => {
            record_tls_failure(&ctx, peer, &cert_host, port, &e.to_string());
            return;
        }
    };
    let (_, session) = tls.get_ref();
    let info = TlsInfo {
        sni,
        alpn: session.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned()),
        version: session.protocol_version().map(|v| format!("{v:?}").replace('_', ".")),
        cipher: session.negotiated_cipher_suite().map(|c| format!("{:?}", c.suite())),
    };
    let conn = Conn {
        peer,
        scheme: Scheme::Https,
        tunnel_target: Some((host, port)),
        tls: Some(info),
        reverse: None,
        mock: None,
    };
    serve_http(TokioIo::new(tls), ctx, conn).await;
}

/// A client that rejects our certificate is the most common setup problem, so
/// surface it as a failed flow instead of silently dropping the connection.
fn record_tls_failure(ctx: &Ctx, peer: SocketAddr, host: &str, port: u16, error: &str) {
    if !ctx.settings.read().recording {
        return;
    }
    let id = ctx.store.create(peer.to_string(), connect_request(host, port, Scheme::Https));
    ctx.store.update(id, |f| {
        f.state = FlowState::Failed;
        f.timing.finished = Some(now_ms());
        f.error = Some(format!(
            "TLS handshake with client failed: {error}. The client may not trust the ProxyZip root certificate, \
             or it uses certificate pinning; add the host to the SSL proxying exclude list to tunnel it untouched."
        ));
    });
}

fn connect_request(host: &str, port: u16, scheme: Scheme) -> Request {
    Request {
        method: "CONNECT".into(),
        scheme,
        host: host.to_string(),
        port,
        path: String::new(),
        http_version: "HTTP/1.1".into(),
        headers: Headers::default(),
        body: Bytes::new(),
        body_truncated: false,
    }
}

/// Forward bytes untouched (SSL proxying disabled for host, or non-HTTP protocol).
async fn raw_tunnel<I>(mut io: Rewind<I>, host: String, port: u16, ctx: Arc<Ctx>, peer: SocketAddr)
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let recording = ctx.settings.read().recording;
    let id = recording.then(|| {
        let id = ctx.store.create(peer.to_string(), connect_request(&host, port, Scheme::Https));
        ctx.store.update(id, |f| f.state = FlowState::Tunnel);
        id
    });
    let result = async {
        let mut upstream = crate::upstream::connect_tcp(&ctx.settings, &host, port).await?;
        tokio::io::copy_bidirectional(&mut io, &mut upstream).await
    }
    .await;
    if let Some(id) = id {
        ctx.store.update(id, |f| {
            f.timing.finished = Some(now_ms());
            match result {
                Ok((up, down)) => {
                    f.state = FlowState::Completed;
                    f.comment = Some(format!("Tunnel: {up} bytes sent, {down} bytes received"));
                }
                Err(e) => {
                    f.state = FlowState::Failed;
                    f.error = Some(format!("tunnel to {host}:{port} failed: {e}"));
                }
            }
        });
    }
}
