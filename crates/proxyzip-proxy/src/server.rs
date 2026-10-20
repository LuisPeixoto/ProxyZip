use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use parking_lot::{Mutex, RwLock};
use proxyzip_core::{FlowStore, Scheme, TlsInfo};
use proxyzip_rules::RuleSet;
use proxyzip_script::ScriptEngine;
use proxyzip_tls::{CertificateAuthority, FileKeyLog};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::breakpoint::BreakpointHub;
use crate::config::{ProxyConfig, ProxySettings};
use crate::upstream::Upstream;
use crate::util::{ConnectionDropped, ProxyBody, full, split_authority};
use crate::{forward, socks, tunnel};

/// Shared state for every connection handled by one proxy instance.
pub(crate) struct Ctx {
    pub store: FlowStore,
    pub ca: Arc<CertificateAuthority>,
    pub settings: Arc<RwLock<ProxySettings>>,
    pub upstream: Upstream,
    pub keylog: Option<Arc<dyn rustls::KeyLog>>,
    pub listen: SocketAddr,
    pub breakpoints: Arc<BreakpointHub>,
    pub scripts: Arc<ScriptEngine>,
    pub h3: crate::h3::H3Client,
    pub altsvc: crate::altsvc::AltSvcCache,
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
    /// Set on reverse proxy listeners: every request goes to this target.
    pub reverse: Option<Arc<ReverseTarget>>,
    /// Set on a mock server's own listener: only that server's routes answer.
    pub mock: Option<Arc<str>>,
}

/// Where a reverse proxy listener sends its traffic.
#[derive(Debug)]
pub(crate) struct ReverseTarget {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    /// Path prefix of the target URL, without a trailing slash.
    pub base_path: String,
}

impl ReverseTarget {
    pub(crate) fn parse(url: &str) -> Result<Self, String> {
        let uri: hyper::Uri = url.trim().parse().map_err(|e| format!("invalid target URL {url:?}: {e}"))?;
        let scheme = match uri.scheme_str() {
            Some("https") => Scheme::Https,
            Some("http") => Scheme::Http,
            _ => return Err(format!("target URL {url:?} must start with http:// or https://")),
        };
        let host = uri.host().ok_or_else(|| format!("target URL {url:?} has no host"))?;
        let port = uri.port_u16().unwrap_or(if scheme.is_secure() { 443 } else { 80 });
        let base_path = uri.path().trim_end_matches('/').to_string();
        Ok(Self { scheme, host: host.trim_start_matches('[').trim_end_matches(']').to_string(), port, base_path })
    }
}

/// Listeners that run next to the main one and follow the rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExtraListener {
    Reverse {
        port: u16,
        target: String,
    },
    /// The same reverse proxy over HTTP/3 (UDP).
    ReverseH3 {
        port: u16,
        target: String,
    },
    /// A mock server's own port.
    Mock {
        port: u16,
        server: String,
    },
    Socks {
        port: u16,
    },
}

struct Running {
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

fn desired_listeners(rules: &RuleSet) -> Vec<ExtraListener> {
    let mut wanted = Vec::new();
    for r in rules.active_reverse() {
        wanted.push(ExtraListener::Reverse { port: r.listen_port, target: r.target.clone() });
        if r.h3 {
            wanted.push(ExtraListener::ReverseH3 { port: r.listen_port, target: r.target.clone() });
        }
    }
    for (server, port) in rules.mock_listeners() {
        wanted.push(ExtraListener::Mock { port, server: server.to_string() });
    }
    if rules.socks.enabled && rules.socks.port != 0 {
        wanted.push(ExtraListener::Socks { port: rules.socks.port });
    }
    wanted
}

pub struct ProxyServer;

/// A running proxy. Dropping it does not stop the proxy; call [`ProxyHandle::shutdown`].
pub struct ProxyHandle {
    pub local_addr: SocketAddr,
    pub settings: Arc<RwLock<ProxySettings>>,
    /// Resolve flows paused at breakpoints.
    pub breakpoints: Arc<BreakpointHub>,
    /// The JavaScript engine that runs scripts, shared so the app can also test scripts against saved flows.
    pub scripts: Arc<ScriptEngine>,
    /// Reverse proxy and SOCKS5 listeners that could not start when the proxy was created.
    pub listener_errors: Vec<String>,
    ctx: Arc<Ctx>,
    extras: Mutex<HashMap<ExtraListener, Running>>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ProxyHandle {
    /// Stop accepting connections. In-flight connections finish on their own.
    pub async fn shutdown(self) {
        self.breakpoints.release_all();
        let extras: Vec<_> = self.extras.lock().drain().map(|(_, r)| r).collect();
        for running in extras {
            let _ = running.shutdown.send(true);
            let _ = running.task.await;
        }
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }

    /// Replace the rule set and bring the reverse proxy and SOCKS5 listeners in line with it.
    ///
    /// Returns one message for every listener that could not start (for example a port that is already in use).
    pub async fn apply_rules(&self, rules: RuleSet) -> Vec<String> {
        let wanted = desired_listeners(&rules);
        self.settings.write().rules = Arc::new(rules);
        sync_listeners(&self.ctx, &self.extras, wanted).await
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
        let breakpoints = BreakpointHub::new();
        let scripts = Arc::new(ScriptEngine::default());
        let ctx = Arc::new(Ctx {
            store,
            ca,
            settings: settings.clone(),
            upstream: Upstream::new(&config, keylog.clone(), settings.clone()),
            h3: crate::h3::H3Client::new(&config, keylog.clone()),
            altsvc: crate::altsvc::AltSvcCache::default(),
            keylog,
            listen: local_addr,
            breakpoints: breakpoints.clone(),
            scripts: scripts.clone(),
        });
        let ctx_for_handle = ctx.clone();
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
        let extras = Mutex::new(HashMap::new());
        let wanted = desired_listeners(&settings.read().rules);
        let listener_errors = sync_listeners(&ctx_for_handle, &extras, wanted).await;
        Ok(ProxyHandle {
            local_addr,
            settings,
            breakpoints,
            scripts,
            listener_errors,
            ctx: ctx_for_handle,
            extras,
            shutdown: tx,
            task,
        })
    }
}

async fn handle_client(stream: TcpStream, peer: SocketAddr, ctx: Arc<Ctx>) {
    let _ = stream.set_nodelay(true);
    let conn = Conn { peer, scheme: Scheme::Http, tunnel_target: None, tls: None, reverse: None, mock: None };
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
        async move { route(req, ctx, conn).await }
    });
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().preserve_header_case(true).title_case_headers(false);
    if let Err(e) = builder.serve_connection_with_upgrades(io, service).await {
        tracing::debug!(error = %e, "client connection ended with error");
    }
}

async fn route(req: Request<Incoming>, ctx: Arc<Ctx>, conn: Conn) -> Result<Response<ProxyBody>, ConnectionDropped> {
    if req.method() == Method::CONNECT {
        return Ok(connect(req, ctx, conn));
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

/// Start the wanted extra listeners and stop the ones that are no longer wanted.
async fn sync_listeners(
    ctx: &Arc<Ctx>,
    extras: &Mutex<HashMap<ExtraListener, Running>>,
    wanted: Vec<ExtraListener>,
) -> Vec<String> {
    let stale: Vec<Running> = {
        let mut running = extras.lock();
        let gone: Vec<_> = running.keys().filter(|k| !wanted.contains(k)).cloned().collect();
        gone.into_iter().filter_map(|k| running.remove(&k)).collect()
    };
    for running in stale {
        let _ = running.shutdown.send(true);
        let _ = running.task.await;
    }
    let mut errors = Vec::new();
    for kind in wanted {
        if extras.lock().contains_key(&kind) {
            continue;
        }
        match spawn_extra(ctx.clone(), kind.clone()).await {
            Ok(running) => {
                extras.lock().insert(kind, running);
            }
            Err(e) => errors.push(e),
        }
    }
    errors
}

async fn spawn_extra(ctx: Arc<Ctx>, kind: ExtraListener) -> Result<Running, String> {
    if let ExtraListener::ReverseH3 { port, target } = &kind {
        let target = Arc::new(ReverseTarget::parse(target)?);
        let addr = SocketAddr::new(ctx.listen.ip(), *port);
        return crate::h3::serve_reverse(ctx, addr, target).await.map(|(shutdown, task)| Running { shutdown, task });
    }
    let mock: Option<Arc<str>> = match &kind {
        ExtraListener::Mock { server, .. } => Some(server.as_str().into()),
        _ => None,
    };
    let (port, label, reverse) = match &kind {
        ExtraListener::Mock { port, server } => (*port, format!("mock server {server} on port {port}"), None),
        ExtraListener::Reverse { port, target } => {
            (*port, format!("reverse proxy on port {port}"), Some(Arc::new(ReverseTarget::parse(target)?)))
        }
        ExtraListener::ReverseH3 { .. } => unreachable!("handled above"),
        ExtraListener::Socks { port } => (*port, format!("SOCKS5 listener on port {port}"), None),
    };
    let addr = SocketAddr::new(ctx.listen.ip(), port);
    let listener = TcpListener::bind(addr).await.map_err(|e| format!("could not start the {label}: {e}"))?;
    tracing::info!(%addr, "{label} started");
    let (tx, mut rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = rx.changed() => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let ctx = ctx.clone();
                        if let Some(name) = &mock {
                            let name = name.clone();
                            tokio::spawn(async move { handle_mock(stream, peer, ctx, name).await });
                            continue;
                        }
                        match &reverse {
                            Some(target) => {
                                let target = target.clone();
                                tokio::spawn(async move { handle_reverse(stream, peer, ctx, target).await });
                            }
                            None => {
                                tokio::spawn(async move { socks::handle(stream, peer, ctx).await });
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    });
    Ok(Running { shutdown: tx, task })
}

async fn handle_mock(stream: TcpStream, peer: SocketAddr, ctx: Arc<Ctx>, server: Arc<str>) {
    let _ = stream.set_nodelay(true);
    let conn = Conn { peer, scheme: Scheme::Http, tunnel_target: None, tls: None, reverse: None, mock: Some(server) };
    serve_http(TokioIo::new(stream), ctx, conn).await;
}

async fn handle_reverse(stream: TcpStream, peer: SocketAddr, ctx: Arc<Ctx>, target: Arc<ReverseTarget>) {
    let _ = stream.set_nodelay(true);
    let conn = Conn { peer, scheme: target.scheme, tunnel_target: None, tls: None, reverse: Some(target), mock: None };
    serve_http(TokioIo::new(stream), ctx, conn).await;
}
