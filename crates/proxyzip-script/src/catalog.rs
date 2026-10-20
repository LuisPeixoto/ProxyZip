//! The plugin catalog: a JSON index of plugins that ProxyZip can download.
//!
//! Every file is listed with its SHA-256, so a plugin is only installed when what was downloaded is exactly what the
//! catalog describes. This module is pure; the app does the downloading.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/LuisPeixoto/ProxyZip/main/plugins/catalog.json";
const MAX_FILES: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// What it adds: `hook`, `viewer`, `copy as`, `tab`.
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub settings: bool,
    pub base_url: String,
    pub files: Vec<CatalogFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Catalog {
    pub version: u32,
    pub plugins: Vec<CatalogEntry>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Only plain relative paths: no `..`, no absolute paths, no backslashes.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog> {
        let catalog: Catalog = serde_json::from_str(text).context("the plugin catalog is not valid")?;
        if catalog.version != 1 {
            bail!("the plugin catalog has version {}, which this ProxyZip does not understand", catalog.version);
        }
        for entry in &catalog.plugins {
            if entry.files.is_empty() || entry.files.len() > MAX_FILES {
                bail!("catalog entry {} lists an unreasonable number of files", entry.id);
            }
            if !entry.files.iter().all(|f| safe_path(&f.path)) {
                bail!("catalog entry {} has an unsafe file path", entry.id);
            }
            if !entry.files.iter().any(|f| f.path == "plugin.json") || !entry.files.iter().any(|f| f.path == "main.js")
            {
                bail!("catalog entry {} needs plugin.json and main.js", entry.id);
            }
        }
        Ok(catalog)
    }

    pub fn find(&self, id: &str) -> Option<&CatalogEntry> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

impl CatalogEntry {
    /// The URL to download one file from.
    pub fn url_of(&self, file: &CatalogFile) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), file.path)
    }

    /// Check downloaded files against the catalog and write them to `dir`, ready for [`crate::plugin::Registry::install`].
    pub fn stage(&self, downloaded: &[(String, Vec<u8>)], dir: &Path) -> Result<()> {
        for file in &self.files {
            let (_, bytes) = downloaded
                .iter()
                .find(|(p, _)| p == &file.path)
                .with_context(|| format!("{} was not downloaded", file.path))?;
            let actual = sha256_hex(bytes);
            if !actual.eq_ignore_ascii_case(&file.sha256) {
                bail!(
                    "{} does not match the catalog (expected {}, got {actual}); it was not installed",
                    file.path,
                    file.sha256
                );
            }
        }
        for file in &self.files {
            let bytes = &downloaded.iter().find(|(p, _)| p == &file.path).expect("checked above").1;
            let target = dir.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(main: &[u8]) -> CatalogEntry {
        CatalogEntry {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: String::new(),
            provides: vec![],
            settings: false,
            base_url: "https://example.test/p/demo/".into(),
            files: vec![
                CatalogFile { path: "plugin.json".into(), sha256: sha256_hex(b"{}") },
                CatalogFile { path: "main.js".into(), sha256: sha256_hex(main) },
            ],
        }
    }

    #[test]
    fn checksums_are_standard() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn staging_verifies_every_file_first() {
        let e = entry(b"function onRequest(){}");
        let dir = tempfile::tempdir().unwrap();
        let good = vec![
            ("plugin.json".to_string(), b"{}".to_vec()),
            ("main.js".to_string(), b"function onRequest(){}".to_vec()),
        ];
        e.stage(&good, dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("main.js")).unwrap(), b"function onRequest(){}");

        let tampered = vec![("plugin.json".to_string(), b"{}".to_vec()), ("main.js".to_string(), b"evil()".to_vec())];
        let empty = tempfile::tempdir().unwrap();
        let err = e.stage(&tampered, empty.path()).unwrap_err().to_string();
        assert!(err.contains("does not match the catalog"), "{err}");
        assert!(std::fs::read_dir(empty.path()).unwrap().next().is_none(), "nothing is written when a file is wrong");

        assert!(e.stage(&good[..1], empty.path()).unwrap_err().to_string().contains("was not downloaded"));
        assert_eq!(e.url_of(&e.files[1]), "https://example.test/p/demo/main.js");
    }

    #[test]
    fn parse_rejects_unsafe_catalogs() {
        let ok = serde_json::to_string(&Catalog { version: 1, plugins: vec![entry(b"x")] }).unwrap();
        assert!(Catalog::parse(&ok).is_ok());
        assert!(Catalog::parse(&ok.replace("\"version\":1,\"plugins\"", "\"version\":2,\"plugins\"")).is_err());
        assert!(Catalog::parse(&ok.replace("main.js", "../main.js")).is_err());
        assert!(Catalog::parse(&ok.replace("main.js", "/etc/passwd")).is_err());
        assert!(Catalog::parse(&ok.replace("main.js", "a\\\\b")).is_err());
        assert!(Catalog::parse("nope").is_err());
        let mut no_main = Catalog { version: 1, plugins: vec![entry(b"x")] };
        no_main.plugins[0].files.pop();
        assert!(Catalog::parse(&serde_json::to_string(&no_main).unwrap()).is_err());
    }
}
