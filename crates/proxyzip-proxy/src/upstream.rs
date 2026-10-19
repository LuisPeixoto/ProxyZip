use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};

use crate::config::ProxyConfig;

pub type UpstreamClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

/// Upstream HTTP clients. WebSockets need a dedicated HTTP/1-only client because
/// the upgrade mechanism does not exist on HTTP/2 connections.
pub struct Upstream {
    pub verified: UpstreamClient,
    pub insecure: UpstreamClient,
    pub ws_verified: UpstreamClient,
    pub ws_insecure: UpstreamClient,
}

impl Upstream {
    pub fn new(config: &ProxyConfig, keylog: Option<Arc<dyn rustls::KeyLog>>) -> Self {
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
            let mut http = HttpConnector::new();
            http.enforce_http(false);
            http.set_nodelay(true);
            http.set_connect_timeout(Some(Duration::from_secs(20)));
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
