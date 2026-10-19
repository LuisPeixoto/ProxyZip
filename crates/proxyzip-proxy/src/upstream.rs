use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use bytes::Bytes;
use http::{HeaderValue, Uri};
use http_body_util::Full;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name};
use hyper_util::client::legacy::connect::proxy::{SocksV5, Tunnel};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::RwLock;
use proxyzip_rules::UpstreamKind;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};

use tokio::net::TcpStream;
use tower_service::Service;

use crate::config::{ProxyConfig, ProxySettings};
use crate::util::format_authority;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Settings = Arc<RwLock<ProxySettings>>;

pub type UpstreamClient = Client<HttpsConnector<ProxyConnector>, Full<Bytes>>;

/// Resolves host names, honouring the DNS override rules and falling back to the system resolver.
#[derive(Clone)]
pub struct OverrideResolver {
    settings: Settings,
    system: GaiResolver,
}

impl Service<Name> for OverrideResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = io::Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let forced = self.settings.read().rules.dns_lookup(name.as_str());
        if let Some(ip) = forced {
            // The connector fills in the port.
            return Box::pin(std::future::ready(Ok(vec![SocketAddr::new(ip, 0)].into_iter())));
        }
        let lookup = self.system.call(name);
        Box::pin(async move { Ok(lookup.await?.collect::<Vec<_>>().into_iter()) })
    }
}

/// Opens TCP connections, applying DNS overrides and the upstream proxy rule at connect time, so changing the
/// rules takes effect for new connections without rebuilding any client.
#[derive(Clone)]
pub struct ProxyConnector {
    http: HttpConnector<OverrideResolver>,
    settings: Settings,
}

impl ProxyConnector {
    pub fn new(settings: Settings) -> Self {
        let resolver = OverrideResolver { settings: settings.clone(), system: GaiResolver::new() };
        let mut http = HttpConnector::new_with_resolver(resolver);
        http.enforce_http(false);
        http.set_nodelay(true);
        http.set_connect_timeout(Some(Duration::from_secs(20)));
        Self { http, settings }
    }
}

impl Service<Uri> for ProxyConnector {
    type Response = TokioIo<TcpStream>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.http.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let upstream = dst.host().and_then(|host| self.settings.read().rules.upstream_for(host).cloned());
        let Some(proxy) = upstream else {
            let connecting = self.http.call(dst);
            return Box::pin(async move { Ok(connecting.await?) });
        };
        let http = self.http.clone();
        Box::pin(async move {
            let proxy_uri: Uri = format!("http://{}", format_authority(&proxy.host, proxy.port))
                .parse()
                .map_err(|e| format!("invalid upstream proxy address: {e}"))?;
            let credentials = proxy
                .username
                .clone()
                .filter(|u| !u.is_empty())
                .map(|u| (u, proxy.password.clone().unwrap_or_default()));
            match proxy.kind {
                UpstreamKind::Http => {
                    let mut tunnel = Tunnel::new(proxy_uri, http);
                    if let Some((user, pass)) = credentials
                        && let Ok(value) =
                            HeaderValue::from_str(&format!("Basic {}", B64.encode(format!("{user}:{pass}"))))
                    {
                        tunnel = tunnel.with_auth(value);
                    }
                    Ok(tunnel.call(dst).await?)
                }
                UpstreamKind::Socks5 => {
                    let mut socks = SocksV5::new(proxy_uri, http);
                    if let Some((user, pass)) = credentials {
                        socks = socks.with_auth(user, pass);
                    }
                    Ok(socks.call(dst).await.map_err(|e| format!("SOCKS5 upstream proxy: {e}"))?)
                }
            }
        })
    }
}

/// Open a raw TCP connection to `host:port` the way HTTP traffic would be (DNS rules and upstream proxy apply).
pub async fn connect_tcp(settings: &Settings, host: &str, port: u16) -> io::Result<TcpStream> {
    let dst: Uri = format!("http://{}", format_authority(host, port))
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid address: {e}")))?;
    let io = ProxyConnector::new(settings.clone()).call(dst).await.map_err(|e| io::Error::other(e.to_string()))?;
    Ok(io.into_inner())
}

/// Upstream HTTP clients. WebSockets need a dedicated HTTP/1-only client because
/// the upgrade mechanism does not exist on HTTP/2 connections.
pub struct Upstream {
    pub verified: UpstreamClient,
    pub insecure: UpstreamClient,
    pub ws_verified: UpstreamClient,
    pub ws_insecure: UpstreamClient,
}

impl Upstream {
    pub fn new(config: &ProxyConfig, keylog: Option<Arc<dyn rustls::KeyLog>>, settings: Settings) -> Self {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = roots.add(cert);
        }
        for cert in &config.extra_upstream_roots {
            let _ = roots.add(cert.clone());
        }
        let roots = Arc::new(roots);

        let make = |insecure: bool, h2: bool| {
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
            let http = ProxyConnector::new(settings.clone());
            let builder = hyper_rustls::HttpsConnectorBuilder::new().with_tls_config(tls).https_or_http();
            let connector = if h2 {
                builder.enable_http1().enable_http2().wrap_connector(http)
            } else {
                builder.enable_http1().wrap_connector(http)
            };
            Client::builder(TokioExecutor::new()).pool_idle_timeout(Duration::from_secs(60)).build(connector)
        };
        Self {
            verified: make(false, true),
            insecure: make(true, true),
            ws_verified: make(false, false),
            ws_insecure: make(true, false),
        }
    }

    pub fn pick(&self, insecure: bool, websocket: bool) -> &UpstreamClient {
        match (insecure, websocket) {
            (false, false) => &self.verified,
            (true, false) => &self.insecure,
            (false, true) => &self.ws_verified,
            (true, true) => &self.ws_insecure,
        }
    }
}

#[derive(Debug)]
struct AcceptAnyCert;

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}
