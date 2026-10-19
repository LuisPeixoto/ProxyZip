use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use lru::LruCache;
use parking_lot::Mutex;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SerialNumber,
};
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::sign::CertifiedKey;
use time::{Duration, OffsetDateTime};

pub const CA_CERT_FILE: &str = "proxyzip-ca.pem";
pub const CA_KEY_FILE: &str = "proxyzip-ca-key.pem";

/// Root certificate authority used to sign intercepted hosts' certificates.
pub struct CertificateAuthority {
    cert_pem: String,
    cert_der: CertificateDer<'static>,
    issuer: Issuer<'static, KeyPair>,
    /// One key is shared by all leaf certificates: generating a key per host is
    /// the most expensive part of interception and buys nothing for a local CA.
    leaf_key: KeyPair,
    leaf_signing_key: Arc<dyn rustls::sign::SigningKey>,
    cache: Mutex<LruCache<String, Arc<CertifiedKey>>>,
    dir: Option<PathBuf>,
}

impl std::fmt::Debug for CertificateAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateAuthority").field("dir", &self.dir).finish_non_exhaustive()
    }
}

fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    let host = hostname();
    dn.push(DnType::CommonName, format!("ProxyZip CA ({host})"));
    dn.push(DnType::OrganizationName, "ProxyZip");
    dn.push(DnType::OrganizationalUnitName, "https://github.com/proxyzip/proxyzip");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(2);
    params.not_after = now + Duration::days(365 * 10);
    params.serial_number = Some(random_serial());
    params
}

fn hostname() -> String {
    std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "local".into())
}

fn random_serial() -> SerialNumber {
    // Serial numbers only need to be unique per issuer; a fresh key pair's DER is
    // random material we already have, so derive 16 bytes from it.
    let key = KeyPair::generate().expect("key generation");
    let der = key.serialize_der();
    let mut bytes: Vec<u8> = der.iter().rev().take(16).copied().collect();
    bytes[0] &= 0x7f; // keep it positive
    SerialNumber::from_slice(&bytes)
}

impl CertificateAuthority {
    /// Generate a brand-new CA held only in memory.
    pub fn generate() -> Result<Self> {
        let key = KeyPair::generate()?;
        let params = ca_params();
        let cert = params.self_signed(&key)?;
        Self::from_parts(cert.pem(), key, None)
    }

    /// Load the CA from `dir`, generating and persisting a new one if absent.
    pub fn load_or_create(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let cert_path = dir.join(CA_CERT_FILE);
        let key_path = dir.join(CA_KEY_FILE);
        if cert_path.exists() && key_path.exists() {
            let cert_pem =
                fs::read_to_string(&cert_path).with_context(|| format!("reading {}", cert_path.display()))?;
            let key_pem = fs::read_to_string(&key_path).with_context(|| format!("reading {}", key_path.display()))?;
            let key = KeyPair::from_pem(&key_pem).context("parsing CA private key")?;
            return Self::from_parts(cert_pem, key, Some(dir.to_path_buf()));
        }
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let ca = Self::generate()?;
        fs::write(&cert_path, &ca.cert_pem)?;
        write_private(&key_path, &ca.issuer.key().serialize_pem())?;
        tracing::info!(path = %cert_path.display(), "generated new root CA");
        Ok(Self { dir: Some(dir.to_path_buf()), ..ca })
    }

    /// Replace the CA in `dir` with a freshly generated one.
    pub fn regenerate(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let _ = fs::remove_file(dir.join(CA_CERT_FILE));
        let _ = fs::remove_file(dir.join(CA_KEY_FILE));
        Self::load_or_create(dir)
    }

    fn from_parts(cert_pem: String, key: KeyPair, dir: Option<PathBuf>) -> Result<Self> {
        let cert_der =
            rustls_pemfile::certs(&mut cert_pem.as_bytes()).next().context("CA PEM contains no certificate")??;
        let issuer = Issuer::from_ca_cert_pem(&cert_pem, key).context("parsing CA certificate")?;
        let leaf_key = KeyPair::generate()?;
        let leaf_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let leaf_signing_key = any_supported_type(&leaf_der).context("loading leaf key")?;
        Ok(Self {
            cert_pem,
            cert_der,
            issuer,
            leaf_key,
            leaf_signing_key,
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(2048).unwrap())),
            dir,
        })
    }

    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    /// Directory the CA was loaded from, if persisted.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn cert_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(CA_CERT_FILE))
    }

    /// Certificate + key for `host` (DNS name or IP literal), signed by this CA.
    pub fn leaf_for(&self, host: &str) -> Result<Arc<CertifiedKey>> {
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        if let Some(hit) = self.cache.lock().get(&host) {
            return Ok(hit.clone());
        }
        let mut params = CertificateParams::new(vec![host.clone()])?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, host.clone());
        dn.push(DnType::OrganizationName, "ProxyZip");
        params.distinguished_name = dn;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(2);
        // Apple platforms reject leaf certificates valid for more than 825 days.
        params.not_after = now + Duration::days(365);
        params.serial_number = Some(random_serial());
        params.use_authority_key_identifier_extension = true;
        let cert = params.signed_by(&self.leaf_key, &self.issuer)?;
        let chain = vec![cert.der().clone(), self.cert_der.clone()];
        let certified = Arc::new(CertifiedKey::new(chain, self.leaf_signing_key.clone()));
        self.cache.lock().put(host, certified.clone());
        Ok(certified)
    }
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(contents.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_and_caches_leaves() {
        crate::install_crypto_provider();
        let ca = CertificateAuthority::generate().unwrap();
        let a = ca.leaf_for("Example.com").unwrap();
        let b = ca.leaf_for("example.com").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.cert.len(), 2);
        ca.leaf_for("127.0.0.1").unwrap();
        ca.leaf_for("[::1]").unwrap();
    }

    #[test]
    fn persists_and_reloads() {
        let dir = std::env::temp_dir().join(format!("proxyzip-ca-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let a = CertificateAuthority::load_or_create(&dir).unwrap();
        let b = CertificateAuthority::load_or_create(&dir).unwrap();
        assert_eq!(a.cert_pem(), b.cert_pem());
        let c = CertificateAuthority::regenerate(&dir).unwrap();
        assert_ne!(a.cert_pem(), c.cert_pem());
        fs::remove_dir_all(&dir).unwrap();
    }
}
