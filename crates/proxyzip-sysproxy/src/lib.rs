//! Point the operating system's HTTP/HTTPS proxy at ProxyZip and back.
//!
//! - macOS: `networksetup` on every enabled network service (falls back to an
//!   administrator prompt when the user is not allowed to change settings).
//! - Windows: WinINet per-user registry settings + a settings-changed broadcast.
//! - Linux: GNOME (`gsettings`) and KDE (`kwriteconfig5/6`) desktop settings.

use anyhow::Result;
use serde::Serialize;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(windows)]
use windows as imp;

/// Hosts that should never go through the proxy.
pub const DEFAULT_BYPASS: &[&str] = &["localhost", "127.0.0.1", "::1", "*.local", "169.254/16"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SystemProxyStatus {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl SystemProxyStatus {
    pub fn points_to(&self, host: &str, port: u16) -> bool {
        self.enabled && self.host.as_deref() == Some(host) && self.port == Some(port)
    }
}

pub fn enable(host: &str, port: u16) -> Result<()> {
    tracing::info!(host, port, "enabling system proxy");
    imp::enable(host, port)
}

pub fn disable() -> Result<()> {
    tracing::info!("disabling system proxy");
    imp::disable()
}

pub fn status() -> Result<SystemProxyStatus> {
    imp::status()
}

#[cfg(unix)]
pub(crate) fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(cmd).args(args).output()?;
    if !out.status.success() {
        anyhow::bail!("{cmd} {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
