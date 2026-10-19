use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

use crate::{DEFAULT_BYPASS, SystemProxyStatus};

const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn key(write: bool) -> Result<RegKey> {
    let flags = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(PATH, flags).context("opening Internet Settings")
}

/// Tell WinINet (and therefore most apps) that settings changed.
fn refresh() {
    use windows_sys::Win32::Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    };
    unsafe {
        InternetSetOptionW(std::ptr::null_mut(), INTERNET_OPTION_SETTINGS_CHANGED, std::ptr::null_mut(), 0);
        InternetSetOptionW(std::ptr::null_mut(), INTERNET_OPTION_REFRESH, std::ptr::null_mut(), 0);
    }
}

pub fn enable(host: &str, port: u16) -> Result<()> {
    let k = key(true)?;
    k.set_value("ProxyServer", &format!("{host}:{port}"))?;
    let mut bypass: Vec<String> = DEFAULT_BYPASS.iter().map(|s| s.to_string()).collect();
    bypass.push("<local>".into());
    k.set_value("ProxyOverride", &bypass.join(";"))?;
    k.set_value("ProxyEnable", &1u32)?;
    refresh();
    Ok(())
}

pub fn disable() -> Result<()> {
    key(true)?.set_value("ProxyEnable", &0u32)?;
    refresh();
    Ok(())
}

pub fn status() -> Result<SystemProxyStatus> {
    let k = key(false)?;
    let enabled: u32 = k.get_value("ProxyEnable").unwrap_or(0);
    let server: String = k.get_value("ProxyServer").unwrap_or_default();
    // Either "host:port" or "http=host:port;https=host:port".
    let entry = server
        .split(';')
        .find(|s| s.starts_with("https="))
        .or_else(|| server.split(';').next())
        .unwrap_or("")
        .trim_start_matches("https=")
        .trim_start_matches("http=");
    let (host, port) = match entry.rsplit_once(':') {
        Some((h, p)) => (Some(h.to_string()), p.parse().ok()),
        None => (None, None),
    };
    Ok(SystemProxyStatus { enabled: enabled == 1, host, port })
}
