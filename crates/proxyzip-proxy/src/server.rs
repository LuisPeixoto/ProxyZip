use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use parking_lot::RwLock;
use proxyzip_core::{FlowStore, Scheme, TlsInfo};
use proxyzip_tls::{CertificateAuthority, FileKeyLog};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::{ProxyConfig, ProxySettings};
use crate::upstream::Upstream;
use crate::util::{ProxyBody, full, split_authority};
use crate::{forward, tunnel};

/// Shared state for every connection handled by one proxy instance.
pub(crate) struct Ctx {
    pub store: FlowStore,
    pub ca: Arc<CertificateAuthority>,
    pub settings: Arc<RwLock<ProxySettings>>,
    pub upstream: Upstream,
    pub keylog: Option<Arc<dyn rustls::KeyLog>>,
    pub listen: SocketAddr,
}

/// What we know about the client connection a request arrived on.
#[derive(Clone)]
pub(crate) struct Conn {
    pub peer: SocketAddr,
    /// `Http` on the main listener; `Https`/`Http` inside a CONNECT tunnel.
    pub scheme: Scheme,
    /// CONNECT target when inside a tunnel.
    pub tunnel_target: Option<(String, u16)>,
    pub tls: Option<TlsInfo>,
}

pub struct ProxyServer;

/// A running proxy. Dropping it does not stop the proxy; call [`ProxyHandle::shutdown`].
pub struct ProxyHandle {
    pub local_addr: SocketAddr,
    pub settings: Arc<RwLock<ProxySettings>>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ProxyHandle {
    /// Stop accepting connections. In-flight connections finish on their own.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

impl ProxyServer {
    pub async fn start(
        config: ProxyConfig,
        settings: ProxySettings,
        ca: Arc<CertificateAuthority>,
        store: FlowStore,
    ) -> Result<ProxyHandle> {
        proxyzip_tls::install_crypto_provider();
        let listener = TcpListener::bind(config.listen)
            .await
            .with_context(|| format!("binding proxy listener on {}", config.listen))?;
        let local_addr = listener.local_addr()?;
        let keylog: Option<Arc<dyn rustls::KeyLog>> = match &config.keylog_path {
            Some(p) => Some(Arc::new(FileKeyLog::open(p).with_context(|| format!("opening key log {}", p.display()))?)),
            None => None,
        };
        let settings = Arc::new(RwLock::new(settings));
        let ctx = Arc::new(Ctx {
            store,
            ca,
            settings: settings.clone(),
            upstream: Upstream::new(&config, keylog.clone()),
            keylog,
            listen: local_addr,
        });
        let (tx, mut rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = rx.changed() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, peer)) => {
                            let ctx = ctx.clone();
                            tokio::spawn(async move { handle_client(stream, peer, ctx).await });
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "accept failed");
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    }
                }
            }
        });
        tracing::info!(%local_addr, "proxy listening");
        Ok(ProxyHandle { local_addr, settings, shutdown: tx, task })
    }
}

async fn handle_client(stream: TcpStream, peer: SocketAddr, ctx: Arc<Ctx>) {
    let _ = stream.set_nodelay(true);
    let conn = Conn { peer, scheme: Scheme::Http, tunnel_target: None, tls: None };
    serve_http(TokioIo::new(stream), ctx, conn).await;
}

/// Serve HTTP/1.1 or HTTP/2 (auto-detected) on any IO, routing through the proxy.
pub(crate) async fn serve_http<I>(io: I, ctx: Arc<Ctx>, conn: Conn)
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let service = service_fn(move |req| {
        let ctx = ctx.clone();
        let conn = conn.clone();
        async move { Ok::<_, Infallible>(route(req, ctx, conn).await) }
    });
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().preserve_header_case(true).title_case_headers(false);
    if let Err(e) = builder.serve_connection_with_upgrades(io, service).await {
        tracing::debug!(error = %e, "client connection ended with error");
    }
}

async fn route(req: Request<Incoming>, ctx: Arc<Ctx>, conn: Conn) -> Response<ProxyBody> {
    if req.method() == Method::CONNECT {
        return connect(req, ctx, conn);
    }
    forward::forward(req, ctx, conn).await
}

fn connect(req: Request<Incoming>, ctx: Arc<Ctx>, conn: Conn) -> Response<ProxyBody> {
    let Some(authority) = req.uri().authority().map(|a| a.to_string()) else {
        return Response::builder().status(400).body(full("CONNECT requires an authority")).unwrap();
    };
    let (host, port) = split_authority(&authority, 443);
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => tunnel::handle(TokioIo::new(upgraded), host, port, ctx, conn.peer).await,
            Err(e) => tracing::debug!(error = %e, "CONNECT upgrade failed"),
        }
    });
    Response::new(full(""))
}
