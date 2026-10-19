use anyhow::{Result, bail};

use crate::{DEFAULT_BYPASS, SystemProxyStatus, run};

fn has(cmd: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn kwrite() -> Option<&'static str> {
    ["kwriteconfig6", "kwriteconfig5"].into_iter().find(|c| has(c))
}

pub fn enable(host: &str, port: u16) -> Result<()> {
    let mut done = false;
    if has("gsettings") {
        let p = port.to_string();
        let ignore = format!("[{}]", DEFAULT_BYPASS.iter().map(|h| format!("'{h}'")).collect::<Vec<_>>().join(", "));
        for (schema, key, val) in [
            ("org.gnome.system.proxy.http", "host", host),
            ("org.gnome.system.proxy.http", "port", p.as_str()),
            ("org.gnome.system.proxy.https", "host", host),
            ("org.gnome.system.proxy.https", "port", p.as_str()),
            ("org.gnome.system.proxy", "ignore-hosts", ignore.as_str()),
            ("org.gnome.system.proxy", "mode", "manual"),
        ] {
            run("gsettings", &["set", schema, key, val])?;
        }
        done = true;
    }
    if let Some(kw) = kwrite() {
        let url = format!("http://{host} {port}");
        for (key, val) in [("httpProxy", url.as_str()), ("httpsProxy", url.as_str()), ("ProxyType", "1")] {
            run(kw, &["--file", "kioslaverc", "--group", "Proxy Settings", "--key", key, val])?;
        }
        done = true;
    }
    if !done {
        bail!(
            "no supported desktop proxy settings found (GNOME/KDE). Export HTTP_PROXY=http://{host}:{port} and HTTPS_PROXY=http://{host}:{port} instead"
        );
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    if has("gsettings") {
        run("gsettings", &["set", "org.gnome.system.proxy", "mode", "none"])?;
    }
    if let Some(kw) = kwrite() {
        run(kw, &["--file", "kioslaverc", "--group", "Proxy Settings", "--key", "ProxyType", "0"])?;
    }
    Ok(())
}

pub fn status() -> Result<SystemProxyStatus> {
    if has("gsettings") {
        let mode = run("gsettings", &["get", "org.gnome.system.proxy", "mode"])?;
        let host = run("gsettings", &["get", "org.gnome.system.proxy.https", "host"])?;
        let port = run("gsettings", &["get", "org.gnome.system.proxy.https", "port"])?;
        return Ok(SystemProxyStatus {
            enabled: mode.trim().trim_matches('\'') == "manual",
            host: Some(host.trim().trim_matches('\'').to_string()).filter(|h| !h.is_empty()),
            port: port.trim().parse().ok(),
        });
    }
    Ok(SystemProxyStatus { enabled: false, host: None, port: None })
}
