//! Firefox keeps its own certificate store, separate from the operating system's.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde::Serialize;

use crate::CA_NAME;

const PREF: &str = "user_pref(\"security.enterprise_roots.enabled\", true);";

#[derive(Debug, Clone, Serialize, Default)]
pub struct FirefoxReport {
    /// Profiles that were configured.
    pub profiles: Vec<String>,
    /// Profiles where the CA was imported into the NSS database with `certutil`.
    pub imported: usize,
    /// Notes the user should read (for example, that Firefox must be restarted).
    pub notes: Vec<String>,
}

/// Directories that hold Firefox profiles on this computer.
pub fn profile_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let mut roots = vec![
        home.join("Library/Application Support/Firefox/Profiles"),
        home.join(".mozilla/firefox"),
        home.join("snap/firefox/common/.mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ];
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(app_data).join("Mozilla/Firefox/Profiles"));
    }
    roots
}

/// Profiles below `roots`: directories that contain `prefs.js` or `cert9.db`.
pub fn find_profiles(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && (p.join("prefs.js").exists() || p.join("cert9.db").exists()) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Make Firefox trust the operating system's roots, where the ProxyZip CA lives. Idempotent.
pub fn enable_enterprise_roots(profile: &Path) -> Result<bool> {
    let file = profile.join("user.js");
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    if current.contains(PREF) {
        return Ok(false);
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(PREF);
    next.push('\n');
    std::fs::write(&file, next)?;
    Ok(true)
}

fn import_nss(profile: &Path, cert: &Path) -> bool {
    Command::new("certutil")
        .args(["-d", &format!("sql:{}", profile.display()), "-A", "-t", "C,,", "-n", CA_NAME, "-i"])
        .arg(cert)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Configure every Firefox profile found under `roots`.
pub fn install_in(roots: &[PathBuf], cert: &Path) -> Result<FirefoxReport> {
    let mut report = FirefoxReport::default();
    for profile in find_profiles(roots) {
        enable_enterprise_roots(&profile)?;
        if import_nss(&profile, cert) {
            report.imported += 1;
        }
        report.profiles.push(profile.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
    if report.profiles.is_empty() {
        report.notes.push("No Firefox profile was found. Start Firefox once, then try again.".into());
    } else {
        report.notes.push("Restart Firefox to apply the change.".into());
        report
            .notes
            .push("Firefox now trusts the operating-system store, so install the certificate there too.".into());
    }
    Ok(report)
}

pub fn install(cert: &Path) -> Result<FirefoxReport> {
    install_in(&profile_roots(), cert)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_profiles_and_enables_the_preference_once() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("abcd.default-release");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("prefs.js"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("Crash Reports")).unwrap();
        let roots = vec![dir.path().to_path_buf(), dir.path().join("missing")];
        assert_eq!(find_profiles(&roots), vec![profile.clone()]);

        assert!(enable_enterprise_roots(&profile).unwrap());
        assert!(!enable_enterprise_roots(&profile).unwrap(), "second run changes nothing");
        let js = std::fs::read_to_string(profile.join("user.js")).unwrap();
        assert_eq!(js.matches("enterprise_roots").count(), 1);
    }

    #[test]
    fn keeps_existing_user_js_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("user.js"), "user_pref(\"a\", 1);").unwrap();
        enable_enterprise_roots(dir.path()).unwrap();
        let js = std::fs::read_to_string(dir.path().join("user.js")).unwrap();
        assert!(js.starts_with("user_pref(\"a\", 1);\n"));
        assert!(js.contains("enterprise_roots"));
    }

    #[test]
    fn reports_when_nothing_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let r = install_in(&[dir.path().to_path_buf()], Path::new("ca.pem")).unwrap();
        assert!(r.profiles.is_empty());
        assert!(r.notes[0].contains("No Firefox profile"));
    }
}
