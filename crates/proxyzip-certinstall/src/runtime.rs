//! Environment that makes command-line tools and language runtimes use ProxyZip.
//!
//! Everything generated lives next to the CA, in `<data>/runtime`:
//! a CA bundle (system roots + ProxyZip), a Node.js preload and, when a JDK is found, a Java trust store.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use base64::Engine;

const NODE_PRELOAD: &str = include_str!("assets/node-preload.cjs");

/// Password of the generated Java trust store (the JDK default for `cacerts`).
pub const JAVA_STORE_PASSWORD: &str = "changeit";

/// Command-line shells the environment snippet can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Posix,
    Fish,
    PowerShell,
    Cmd,
}

impl Shell {
    pub fn from_name(name: &str) -> Option<Shell> {
        match name.to_ascii_lowercase().as_str() {
            "sh" | "bash" | "zsh" | "posix" => Some(Shell::Posix),
            "fish" => Some(Shell::Fish),
            "powershell" | "pwsh" | "ps1" => Some(Shell::PowerShell),
            "cmd" | "batch" => Some(Shell::Cmd),
            _ => None,
        }
    }

    /// The shell of the current session, from `$SHELL`, or PowerShell on Windows.
    pub fn detect() -> Shell {
        if cfg!(windows) {
            return Shell::PowerShell;
        }
        let shell = std::env::var("SHELL").unwrap_or_default();
        let name = shell.rsplit('/').next().unwrap_or("");
        Shell::from_name(name).unwrap_or(Shell::Posix)
    }
}

/// Files that [`prepare`] generates.
#[derive(Debug, Clone)]
pub struct RuntimeFiles {
    /// System root certificates plus the ProxyZip CA, for tools that replace (not extend) their roots.
    pub ca_bundle: PathBuf,
    pub node_preload: PathBuf,
    /// A copy of the JDK trust store that also trusts ProxyZip, with its keystore type.
    pub java_truststore: Option<(PathBuf, &'static str)>,
}

pub fn runtime_dir(cert_path: &Path) -> PathBuf {
    cert_path.parent().unwrap_or_else(|| Path::new(".")).join("runtime")
}

fn pem(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Build the PEM bundle text: the operating system's roots followed by `ca_pem`.
pub fn bundle_text(ca_pem: &str) -> String {
    let mut out = String::new();
    for cert in rustls_native_certs::load_native_certs().certs {
        out.push_str(&pem("CERTIFICATE", cert.as_ref()));
    }
    out.push_str(ca_pem);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
        return Ok(());
    }
    std::fs::write(path, content).with_context(|| format!("writing {}", path.display()))
}

/// Generate (or refresh) the runtime files for the CA at `cert_path`.
pub fn prepare(cert_path: &Path) -> Result<RuntimeFiles> {
    let dir = runtime_dir(cert_path);
    std::fs::create_dir_all(&dir)?;
    let ca_pem = std::fs::read_to_string(cert_path).with_context(|| format!("reading {}", cert_path.display()))?;
    let ca_bundle = dir.join("ca-bundle.pem");
    write_if_changed(&ca_bundle, &bundle_text(&ca_pem))?;
    let node_preload = dir.join("node-preload.cjs");
    write_if_changed(&node_preload, NODE_PRELOAD)?;
    let java_truststore = match java_truststore(cert_path, &dir) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(error = %e, "no Java trust store");
            None
        }
    };
    Ok(RuntimeFiles { ca_bundle, node_preload, java_truststore })
}

fn java_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("JAVA_HOME").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(h);
    }
    if cfg!(target_os = "macos")
        && let Ok(out) = Command::new("/usr/libexec/java_home").output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    let which = if cfg!(windows) { "where" } else { "which" };
    let out = Command::new(which).arg("java").output().ok()?;
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    let real = std::fs::canonicalize(first).ok()?;
    real.parent()?.parent().map(Path::to_path_buf)
}

fn cacerts_of(home: &Path) -> Option<PathBuf> {
    ["lib/security/cacerts", "jre/lib/security/cacerts"].iter().map(|r| home.join(r)).find(|p| p.is_file())
}

/// `"PKCS12"` or `"JKS"`, from the keystore's magic bytes.
fn store_type(path: &Path) -> &'static str {
    match std::fs::read(path).ok().as_deref().and_then(|b| b.get(..4)) {
        Some([0xFE, 0xED, 0xFE, 0xED]) => "JKS",
        _ => "PKCS12",
    }
}

/// Copy the JDK's `cacerts`, add the ProxyZip CA with `keytool`, and return the result.
fn java_truststore(cert_path: &Path, dir: &Path) -> Result<Option<(PathBuf, &'static str)>> {
    let Some(home) = java_home() else { return Ok(None) };
    let Some(cacerts) = cacerts_of(&home) else { return Ok(None) };
    let store = dir.join("java-truststore");
    let fresh = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(s), Some(c)) = (fresh(&store), fresh(cert_path))
        && s >= c
    {
        return Ok(Some((store.clone(), store_type(&store))));
    }
    std::fs::copy(&cacerts, &store)?;
    let keytool = home.join("bin").join(if cfg!(windows) { "keytool.exe" } else { "keytool" });
    let status = Command::new(keytool)
        .args(["-importcert", "-noprompt", "-alias", "proxyzip-ca", "-storepass", JAVA_STORE_PASSWORD, "-file"])
        .arg(cert_path)
        .arg("-keystore")
        .arg(&store)
        .output()?;
    if !status.status.success() {
        let _ = std::fs::remove_file(&store);
        anyhow::bail!("keytool failed: {}", String::from_utf8_lossy(&status.stderr).trim());
    }
    Ok(Some((store.clone(), store_type(&store))))
}

fn java_options(host: &str, port: u16, store: &(PathBuf, &'static str)) -> String {
    format!(
        "-Dhttp.proxyHost={host} -Dhttp.proxyPort={port} -Dhttps.proxyHost={host} -Dhttps.proxyPort={port} -Djavax.net.ssl.trustStore={} -Djavax.net.ssl.trustStorePassword={JAVA_STORE_PASSWORD} -Djavax.net.ssl.trustStoreType={}",
        store.0.display(),
        store.1
    )
}

/// The variables to export, in order. `NODE_OPTIONS` and `JAVA_TOOL_OPTIONS` are appended to what the user already has.
struct Vars {
    plain: Vec<(&'static str, String)>,
    node_option: String,
    java_option: Option<String>,
}

fn vars(host: &str, port: u16, cert_path: &Path, files: &RuntimeFiles) -> Vars {
    let proxy = format!("http://{host}:{port}");
    let ca = cert_path.display().to_string();
    let bundle = files.ca_bundle.display().to_string();
    let plain = vec![
        ("HTTP_PROXY", proxy.clone()),
        ("HTTPS_PROXY", proxy.clone()),
        ("http_proxy", proxy.clone()),
        ("https_proxy", proxy.clone()),
        ("NO_PROXY", "localhost,127.0.0.1,::1".into()),
        ("no_proxy", "localhost,127.0.0.1,::1".into()),
        ("NODE_USE_ENV_PROXY", "1".into()),
        ("NODE_EXTRA_CA_CERTS", ca.clone()),
        ("DENO_CERT", ca),
        ("SSL_CERT_FILE", bundle.clone()),
        ("CURL_CA_BUNDLE", bundle.clone()),
        ("REQUESTS_CA_BUNDLE", bundle.clone()),
        ("GIT_SSL_CAINFO", bundle.clone()),
        ("PIP_CERT", bundle.clone()),
        ("AWS_CA_BUNDLE", bundle.clone()),
        ("CARGO_HTTP_CAINFO", bundle),
    ];
    Vars {
        plain,
        node_option: format!("--require \"{}\"", files.node_preload.display()),
        java_option: files.java_truststore.as_ref().map(|s| java_options(host, port, s)),
    }
}

fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A snippet for `shell` that routes the session through ProxyZip, for a proxy on `host:port` and the CA at `cert_path`.
pub fn render(shell: Shell, host: &str, port: u16, cert_path: &Path, files: &RuntimeFiles) -> String {
    let v = vars(host, port, cert_path, files);
    let mut out = String::new();
    match shell {
        Shell::Posix => {
            for (k, val) in &v.plain {
                out.push_str(&format!("export {k}={}\n", sq(val)));
            }
            out.push_str(&format!(
                "export NODE_OPTIONS=\"${{NODE_OPTIONS:+$NODE_OPTIONS }}{}\"\n",
                v.node_option.replace('"', "\\\"")
            ));
            if let Some(j) = &v.java_option {
                out.push_str(&format!("export JAVA_TOOL_OPTIONS=\"${{JAVA_TOOL_OPTIONS:+$JAVA_TOOL_OPTIONS }}{j}\"\n"));
            }
        }
        Shell::Fish => {
            for (k, val) in &v.plain {
                out.push_str(&format!("set -gx {k} {}\n", fish_quote(val)));
            }
            out.push_str(&format!("set -gx NODE_OPTIONS \"$NODE_OPTIONS {}\"\n", v.node_option.replace('"', "\\\"")));
            if let Some(j) = &v.java_option {
                out.push_str(&format!("set -gx JAVA_TOOL_OPTIONS \"$JAVA_TOOL_OPTIONS {j}\"\n"));
            }
        }
        Shell::PowerShell => {
            for (k, val) in &v.plain {
                out.push_str(&format!("$env:{k}='{}'\n", val.replace('\'', "''")));
            }
            out.push_str(&format!("$env:NODE_OPTIONS=\"$env:NODE_OPTIONS {}\"\n", v.node_option.replace('"', "`\"")));
            if let Some(j) = &v.java_option {
                out.push_str(&format!("$env:JAVA_TOOL_OPTIONS=\"$env:JAVA_TOOL_OPTIONS {j}\"\n"));
            }
        }
        Shell::Cmd => {
            for (k, val) in &v.plain {
                out.push_str(&format!("set \"{k}={val}\"\n"));
            }
            out.push_str(&format!("set \"NODE_OPTIONS=%NODE_OPTIONS% {}\"\n", v.node_option));
            if let Some(j) = &v.java_option {
                out.push_str(&format!("set \"JAVA_TOOL_OPTIONS=%JAVA_TOOL_OPTIONS% {j}\"\n"));
            }
        }
    }
    out
}

/// Prepare the runtime files and render the snippet for `shell`.
pub fn terminal_env_for(shell: Shell, host: &str, port: u16, cert_path: &Path) -> Result<String> {
    let files = prepare(cert_path)?;
    Ok(render(shell, host, port, cert_path, &files))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(dir: &Path, java: bool) -> RuntimeFiles {
        RuntimeFiles {
            ca_bundle: dir.join("runtime/ca-bundle.pem"),
            node_preload: dir.join("runtime/node-preload.cjs"),
            java_truststore: java.then(|| (dir.join("runtime/java-truststore"), "PKCS12")),
        }
    }

    #[test]
    fn shells_are_recognised_by_name() {
        assert_eq!(Shell::from_name("zsh"), Some(Shell::Posix));
        assert_eq!(Shell::from_name("fish"), Some(Shell::Fish));
        assert_eq!(Shell::from_name("pwsh"), Some(Shell::PowerShell));
        assert_eq!(Shell::from_name("cmd"), Some(Shell::Cmd));
        assert_eq!(Shell::from_name("nushell"), None);
    }

    #[test]
    fn posix_snippet_points_tools_at_the_bundle_and_the_proxy() {
        let d = Path::new("/data/proxyzip");
        let s = render(Shell::Posix, "127.0.0.1", 9393, &d.join("proxyzip-ca.pem"), &files(d, false));
        assert!(s.contains("export HTTPS_PROXY='http://127.0.0.1:9393'"));
        assert!(s.contains("export SSL_CERT_FILE='/data/proxyzip/runtime/ca-bundle.pem'"));
        assert!(s.contains("export NODE_EXTRA_CA_CERTS='/data/proxyzip/proxyzip-ca.pem'"));
        assert!(s.contains("export NODE_USE_ENV_PROXY='1'"));
        assert!(s.contains("--require \\\"/data/proxyzip/runtime/node-preload.cjs\\\""));
        assert!(!s.contains("JAVA_TOOL_OPTIONS"), "no JDK, no Java options");
    }

    #[test]
    fn java_options_are_added_when_a_trust_store_exists() {
        let d = Path::new("/data/proxyzip");
        let s = render(Shell::Posix, "127.0.0.1", 9393, &d.join("proxyzip-ca.pem"), &files(d, true));
        assert!(s.contains("-Dhttps.proxyHost=127.0.0.1 -Dhttps.proxyPort=9393"));
        assert!(s.contains("-Djavax.net.ssl.trustStore=/data/proxyzip/runtime/java-truststore"));
        assert!(s.contains("trustStoreType=PKCS12"));
    }

    #[test]
    fn every_shell_renders_the_proxy() {
        let d = Path::new("/data/p");
        let f = files(d, false);
        let ca = d.join("ca.pem");
        assert!(render(Shell::Fish, "h", 1, &ca, &f).contains("set -gx HTTPS_PROXY 'http://h:1'"));
        assert!(render(Shell::PowerShell, "h", 1, &ca, &f).contains("$env:HTTPS_PROXY='http://h:1'"));
        assert!(render(Shell::Cmd, "h", 1, &ca, &f).contains("set \"HTTPS_PROXY=http://h:1\""));
    }

    #[test]
    fn single_quotes_in_paths_are_escaped() {
        assert_eq!(sq("a'b"), "'a'\\''b'");
    }

    #[test]
    fn bundle_ends_with_the_proxyzip_ca() {
        let ca = "-----BEGIN CERTIFICATE-----\nQUJD\n-----END CERTIFICATE-----\n";
        let b = bundle_text(ca);
        assert!(b.ends_with(ca));
        assert!(b.matches("BEGIN CERTIFICATE").count() >= 1);
    }

    #[test]
    fn prepare_writes_the_bundle_and_the_node_preload() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("proxyzip-ca.pem");
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\nQUJD\n-----END CERTIFICATE-----\n").unwrap();
        let f = prepare(&ca).unwrap();
        assert!(std::fs::read_to_string(&f.ca_bundle).unwrap().contains("QUJD"));
        assert!(std::fs::read_to_string(&f.node_preload).unwrap().contains("CONNECT"));
    }

    /// The preload opens a CONNECT tunnel through the proxy for https requests.
    #[test]
    fn node_preload_sends_connect_to_the_proxy() {
        use std::io::{Read, Write};
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("node not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("proxyzip-ca.pem");
        std::fs::write(&ca, "x").unwrap();
        let f = prepare(&ca).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let child = Command::new("node")
            .args(["-r"])
            .arg(&f.node_preload)
            .args(["-e", "require('https').get('https://example.test/x').on('error',()=>process.exit(0))"])
            .env("HTTPS_PROXY", format!("http://127.0.0.1:{port}"))
            .env("NO_PROXY", "")
            .spawn()
            .unwrap();
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = [0u8; 512];
        let n = conn.read(&mut buf).unwrap();
        let _ = conn.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n");
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let mut child = child;
        let _ = child.wait();
        assert!(head.starts_with("CONNECT example.test:443 HTTP/1.1"), "{head}");
    }
}
