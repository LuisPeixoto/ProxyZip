//! Trust management for the ProxyZip root certificate.
//!
//! | OS      | Store                                                         |
//! |---------|---------------------------------------------------------------|
//! | macOS   | login keychain, `trustRoot` (`security add-trusted-cert`)     |
//! | Windows | current-user Root store (`certutil -user -addstore`)          |
//! | Linux   | system anchors (+`update-ca-*` via `pkexec`) and the NSS DB   |

pub mod android;
pub mod firefox;
pub mod ios;
pub mod runtime;
pub mod terminal;

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use serde::Serialize;

/// Common-name prefix of every ProxyZip CA, used to find it in stores.
pub const CA_NAME: &str = "ProxyZip CA";

#[derive(Debug, Clone, Serialize)]
pub struct TrustStatus {
    pub installed: bool,
    pub detail: String,
}

fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| anyhow::anyhow!("running {cmd}: {e}"))?;
    if !out.status.success() {
        bail!(
            "{cmd} failed: {}{}",
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&out.stdout).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn path_str(p: &Path) -> Result<&str> {
    p.to_str().ok_or_else(|| anyhow::anyhow!("certificate path is not valid UTF-8"))
}

/// Add the certificate to the OS trust store (may show an OS authentication prompt).
pub fn install(cert_path: &Path) -> Result<()> {
    let p = path_str(cert_path)?;
    #[cfg(target_os = "macos")]
    {
        let keychain = dirs::home_dir().unwrap_or_default().join("Library/Keychains/login.keychain-db");
        run(
            "security",
            &["add-trusted-cert", "-r", "trustRoot", "-p", "ssl", "-p", "basic", "-k", path_str(&keychain)?, p],
        )?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        run("certutil", &["-user", "-addstore", "Root", p])?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let (dir, update): (&str, &[&str]) = if Path::new("/usr/local/share/ca-certificates").exists() {
            ("/usr/local/share/ca-certificates/proxyzip-ca.crt", &["update-ca-certificates"])
        } else if Path::new("/etc/pki/ca-trust/source/anchors").exists() {
            ("/etc/pki/ca-trust/source/anchors/proxyzip-ca.pem", &["update-ca-trust", "extract"])
        } else if Path::new("/etc/ca-certificates/trust-source/anchors").exists() {
            ("/etc/ca-certificates/trust-source/anchors/proxyzip-ca.crt", &["trust", "extract-compat"])
        } else {
            bail!("unknown Linux trust store layout; add {p} to your distribution's CA anchors manually");
        };
        let script = format!("install -m 644 '{p}' '{dir}' && {}", update.join(" "));
        run("pkexec", &["sh", "-c", &script])?;
        if let Err(e) = install_nss(cert_path) {
            tracing::warn!(error = %e, "could not add certificate to the NSS database (Chrome/Firefox)");
        }
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        bail!("automatic certificate installation is not supported on this platform")
    }
}

/// Remove every ProxyZip CA from the OS trust store.
pub fn uninstall(cert_path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = run("security", &["remove-trusted-cert", path_str(cert_path)?]);
        while run("security", &["delete-certificate", "-c", CA_NAME]).is_ok() {}
        return Ok(());
    }
    #[cfg(windows)]
    {
        let _ = cert_path;
        run("certutil", &["-user", "-delstore", "Root", CA_NAME])?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let _ = cert_path;
        let script = "rm -f /usr/local/share/ca-certificates/proxyzip-ca.crt /etc/pki/ca-trust/source/anchors/proxyzip-ca.pem \
            /etc/ca-certificates/trust-source/anchors/proxyzip-ca.crt; \
            (command -v update-ca-certificates && update-ca-certificates --fresh) || (command -v update-ca-trust && update-ca-trust extract) || (command -v trust && trust extract-compat) || true";
        run("pkexec", &["sh", "-c", script])?;
        if let Some(db) = nss_db() {
            let _ = run("certutil", &["-d", &db, "-D", "-n", CA_NAME]);
        }
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = cert_path;
        bail!("not supported on this platform")
    }
}

/// Whether the OS currently trusts the certificate.
pub fn status(cert_path: &Path) -> TrustStatus {
    let Ok(p) = path_str(cert_path) else {
        return TrustStatus { installed: false, detail: "invalid path".into() };
    };
    #[cfg(target_os = "macos")]
    {
        return match run("security", &["verify-cert", "-c", p, "-p", "ssl", "-L"]) {
            Ok(_) => TrustStatus { installed: true, detail: "Trusted in the macOS keychain".into() },
            Err(e) => TrustStatus { installed: false, detail: e.to_string() },
        };
    }
    #[cfg(windows)]
    {
        let _ = p;
        return match run("certutil", &["-user", "-store", "Root", CA_NAME]) {
            Ok(_) => TrustStatus { installed: true, detail: "Present in the current user's Root store".into() },
            Err(_) => TrustStatus { installed: false, detail: "Not in the current user's Root store".into() },
        };
    }
    #[cfg(target_os = "linux")]
    {
        let _ = p;
        let installed = [
            "/usr/local/share/ca-certificates/proxyzip-ca.crt",
            "/etc/pki/ca-trust/source/anchors/proxyzip-ca.pem",
            "/etc/ca-certificates/trust-source/anchors/proxyzip-ca.crt",
        ]
        .iter()
        .any(|f| Path::new(f).exists());
        return TrustStatus {
            installed,
            detail: if installed { "Installed in the system anchors".into() } else { "Not installed".into() },
        };
    }
    #[allow(unreachable_code)]
    TrustStatus { installed: false, detail: format!("unsupported platform ({p})") }
}

#[cfg(target_os = "linux")]
fn nss_db() -> Option<String> {
    let dir = dirs::home_dir()?.join(".pki/nssdb");
    dir.exists().then(|| format!("sql:{}", dir.display()))
}

/// Chrome/Chromium on Linux use the per-user NSS database (`certutil` from libnss3-tools).
#[cfg(target_os = "linux")]
fn install_nss(cert_path: &Path) -> Result<()> {
    let db = nss_db().ok_or_else(|| anyhow::anyhow!("no ~/.pki/nssdb"))?;
    run("certutil", &["-d", &db, "-A", "-t", "C,,", "-n", CA_NAME, "-i", path_str(cert_path)?])?;
    Ok(())
}

/// Install into every booted iOS Simulator (macOS + Xcode only).
pub fn install_ios_simulator(cert_path: &Path) -> Result<()> {
    run("xcrun", &["simctl", "keychain", "booted", "add-root-cert", path_str(cert_path)?])?;
    Ok(())
}

/// Shell snippet for the current shell that routes a terminal session through ProxyZip and makes common
/// runtimes trust its CA. Generates the supporting files next to the CA.
pub fn terminal_env(host: &str, port: u16, cert_path: &Path) -> String {
    terminal_env_for(runtime::Shell::detect(), host, port, cert_path)
}

/// Like [`terminal_env`] for a specific shell.
pub fn terminal_env_for(shell: runtime::Shell, host: &str, port: u16, cert_path: &Path) -> String {
    runtime::terminal_env_for(shell, host, port, cert_path)
        .unwrap_or_else(|e| format!("echo 'ProxyZip: could not prepare the environment: {e:#}' >&2"))
}
