//! TLS interception support: a persistent root CA plus leaf certificates minted
//! on demand for every intercepted host (cached in an LRU).

mod ca;
mod keylog;

pub use ca::{CA_CERT_FILE, CA_KEY_FILE, CertificateAuthority};
pub use keylog::FileKeyLog;

/// Install the process-wide rustls crypto provider (idempotent).
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
