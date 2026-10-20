//! Android emulators and devices, driven through `adb`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// `device`, `offline`, `unauthorized`, …
    pub state: String,
    pub model: Option<String>,
    pub emulator: bool,
}

/// Find `adb`: `ANDROID_HOME`, `ANDROID_SDK_ROOT`, the default SDK locations, then `PATH`.
pub fn adb_path() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut roots: Vec<PathBuf> =
        ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Library/Android/sdk"));
        roots.push(home.join("Android/Sdk"));
    }
    if let Some(local) = dirs::data_local_dir() {
        roots.push(local.join("Android/Sdk"));
    }
    roots.iter().map(|r| r.join("platform-tools").join(exe)).find(|p| p.is_file()).or_else(|| {
        let probe = Command::new(exe).arg("version").output().ok()?;
        probe.status.success().then(|| PathBuf::from(exe))
    })
}

fn adb(args: &[&str]) -> Result<String> {
    let adb = adb_path().context("adb was not found; install the Android SDK platform-tools or set ANDROID_HOME")?;
    let out = Command::new(&adb).args(args).output().with_context(|| format!("running {}", adb.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        bail!("adb {} failed: {}", args.join(" "), text.trim());
    }
    Ok(text)
}

fn adb_serial(serial: &str, args: &[&str]) -> Result<String> {
    let mut all = vec!["-s", serial];
    all.extend_from_slice(args);
    adb(&all)
}

/// Parse the output of `adb devices -l`.
pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let model = parts.find_map(|p| p.strip_prefix("model:")).map(|m| m.replace('_', " "));
            Some(Device { emulator: serial.starts_with("emulator-"), serial, state, model })
        })
        .collect()
}

pub fn list_devices() -> Result<Vec<Device>> {
    Ok(parse_devices(&adb(&["devices", "-l"])?))
}

/// Route the device's HTTP traffic to ProxyZip on this computer (`adb reverse` makes `127.0.0.1` reach it).
pub fn set_proxy(serial: &str, port: u16) -> Result<()> {
    let spec = format!("tcp:{port}");
    adb_serial(serial, &["reverse", &spec, &spec])?;
    adb_serial(serial, &["shell", "settings", "put", "global", "http_proxy", &format!("127.0.0.1:{port}")])?;
    Ok(())
}

pub fn clear_proxy(serial: &str) -> Result<()> {
    adb_serial(serial, &["shell", "settings", "put", "global", "http_proxy", ":0"])?;
    let _ = adb_serial(serial, &["reverse", "--remove-all"]);
    Ok(())
}

/// OpenSSL's `subject_hash_old`: the first four bytes of the MD5 of the DER subject, little-endian.
/// Android names system certificates `<hash>.0`.
pub fn subject_hash_old(cert_der: &[u8]) -> Result<String> {
    let (_, cert) =
        x509_parser::parse_x509_certificate(cert_der).map_err(|e| anyhow::anyhow!("invalid certificate: {e}"))?;
    let digest = Md5::digest(cert.subject().as_raw());
    Ok(format!("{:08x}", u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])))
}

/// Android 14 serves system roots from an APEX that cannot be remounted. This copies the certificates into a
/// tmpfs, adds ours and bind-mounts it over the APEX path in every app's namespace (it lasts until reboot).
const APEX_SCRIPT: &str = r#"set -e
CERT=/data/local/tmp/proxyzip-ca-file
rm -rf /data/local/tmp/pz-certs && mkdir -p -m 700 /data/local/tmp/pz-certs
cp /apex/com.android.conscrypt/cacerts/* /data/local/tmp/pz-certs/
cp "$CERT" /data/local/tmp/pz-certs/
mount -t tmpfs tmpfs /system/etc/security/cacerts
cp /data/local/tmp/pz-certs/* /system/etc/security/cacerts/
chown root:root /system/etc/security/cacerts/*
chmod 644 /system/etc/security/cacerts/*
chcon u:object_r:system_file:s0 /system/etc/security/cacerts/*
ZYGOTE=$(pidof zygote || true); ZYGOTE64=$(pidof zygote64 || true)
for Z in $ZYGOTE $ZYGOTE64; do
  nsenter --mount=/proc/$Z/ns/mnt -- /bin/mount --bind /system/etc/security/cacerts /apex/com.android.conscrypt/cacerts
done
APP_PIDS=$(echo "$ZYGOTE $ZYGOTE64" | xargs -n1 ps -o 'PID' -P | grep -v PID || true)
for P in $APP_PIDS; do
  nsenter --mount=/proc/$P/ns/mnt -- /bin/mount --bind /system/etc/security/cacerts /apex/com.android.conscrypt/cacerts &
done
wait
rm -rf /data/local/tmp/pz-certs
"#;

/// Install the CA as a *system* certificate. Works on emulators (and rooted devices), not on production builds.
/// Returns a note about how long it lasts.
pub fn install_system_cert(serial: &str, cert_pem_path: &Path, cert_der: &[u8]) -> Result<String> {
    let name = format!("{}.0", subject_hash_old(cert_der)?);
    let local = cert_pem_path.to_str().context("certificate path is not valid UTF-8")?;
    adb_serial(serial, &["root"])
        .context("this device cannot run adb as root (use an emulator without Google Play)")?;
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let sdk: u32 = adb_serial(serial, &["shell", "getprop", "ro.build.version.sdk"])?.trim().parse().unwrap_or(0);
    adb_serial(serial, &["push", local, "/data/local/tmp/proxyzip-ca-file"])?;
    if sdk >= 34 {
        adb_serial(serial, &["shell", APEX_SCRIPT])?;
        return Ok("Installed until the next reboot (Android 14 and newer).".into());
    }
    adb_serial(serial, &["remount"]).context("could not remount /system; start the emulator with -writable-system")?;
    let dest = format!("/system/etc/security/cacerts/{name}");
    adb_serial(
        serial,
        &[
            "shell",
            &format!("cp /data/local/tmp/proxyzip-ca-file {dest} && chmod 644 {dest} && chown root:root {dest}"),
        ],
    )?;
    Ok("Installed as a system certificate.".into())
}

/// Copy the CA to the device and open Android's certificate installer (works on any device; apps must opt in to user CAs).
pub fn install_user_cert(serial: &str, cert_der_path: &Path) -> Result<()> {
    let local = cert_der_path.to_str().context("certificate path is not valid UTF-8")?;
    adb_serial(serial, &["push", local, "/sdcard/Download/proxyzip-ca.crt"])?;
    adb_serial(
        serial,
        &[
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.VIEW",
            "-t",
            "application/x-x509-ca-cert",
            "-d",
            "file:///sdcard/Download/proxyzip-ca.crt",
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices() {
        let text = "List of devices attached\nemulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1\nR58M1234ABC            unauthorized transport_id:2\n\n";
        let d = parse_devices(text);
        assert_eq!(d.len(), 2);
        assert!(d[0].emulator);
        assert_eq!(d[0].state, "device");
        assert_eq!(d[0].model.as_deref(), Some("sdk gphone64 arm64"));
        assert_eq!(d[1].state, "unauthorized");
        assert!(!d[1].emulator);
    }

    #[test]
    fn ignores_daemon_noise_before_the_list() {
        let text =
            "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\n";
        assert!(parse_devices(text).is_empty());
    }

    /// Compare against the OpenSSL command that Android's own documentation uses.
    #[test]
    fn subject_hash_matches_openssl() {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name.push(rcgen::DnType::CommonName, "ProxyZip CA test");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let cert = params.self_signed(&key).unwrap();
        let ours = subject_hash_old(cert.der()).unwrap();
        assert_eq!(ours.len(), 8);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("c.pem");
        std::fs::write(&f, cert.pem()).unwrap();
        let Ok(out) = Command::new("openssl").args(["x509", "-subject_hash_old", "-noout", "-in"]).arg(&f).output()
        else {
            eprintln!("openssl not installed; skipping comparison");
            return;
        };
        if out.status.success() {
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), ours);
        }
    }
}
