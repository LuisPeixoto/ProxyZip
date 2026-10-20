//! Plugins: a folder (or `.zip`) with a `plugin.json` manifest and a `main.js`.
//!
//! A plugin can contribute traffic hooks (`onRequest` / `onResponse`, like a script), body viewers
//! (`decode(body, info)`) and "copy as" generators (`generate(request)`). All of it runs in the same sandbox
//! as scripts: no file, network or process access, with the same time and memory limits.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The plugin API version this build understands.
pub const API_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Hooks {
    /// Substring of the URL (or a regular expression when `regex` is set). Empty matches every request.
    pub pattern: String,
    pub regex: bool,
    pub methods: Vec<String>,
    pub request: bool,
    pub response: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Viewer {
    pub id: String,
    pub name: String,
    /// Content types the viewer wants, matched as substrings, for example `application/x-custom`.
    #[serde(default)]
    pub content_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyAs {
    pub id: String,
    pub name: String,
    /// Syntax highlighting for the result: json, javascript, xml, html, css or text.
    #[serde(default = "text")]
    pub syntax: String,
}

fn text() -> String {
    "text".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Contributes {
    pub hooks: Option<Hooks>,
    pub viewers: Vec<Viewer>,
    pub copy_as: Vec<CopyAs>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// Plugin API version the plugin was written for.
    #[serde(default = "one")]
    pub api: u32,
    #[serde(default)]
    pub contributes: Contributes,
}

fn one() -> u32 {
    1
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(json).context("plugin.json is not valid")?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<()> {
        let ok_id = |id: &str| {
            !id.is_empty()
                && id.len() <= 64
                && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        };
        if !ok_id(&self.id) || self.id.starts_with('.') {
            bail!("the plugin id must use only a-z, 0-9, '.', '_' and '-' (got {:?})", self.id);
        }
        if self.name.trim().is_empty() {
            bail!("the plugin needs a name");
        }
        if self.api > API_VERSION {
            bail!(
                "this plugin needs plugin API {} but this ProxyZip supports {API_VERSION}; update ProxyZip",
                self.api
            );
        }
        let mut seen = BTreeSet::new();
        for id in self.contributes.viewers.iter().map(|v| &v.id).chain(self.contributes.copy_as.iter().map(|c| &c.id)) {
            if !ok_id(id) || !seen.insert(id.clone()) {
                bail!("viewer and copyAs ids must be unique and use only a-z, 0-9, '.', '_' and '-' (got {id:?})");
            }
        }
        if let Some(h) = &self.contributes.hooks {
            if !h.request && !h.response {
                bail!("hooks must enable `request` or `response`");
            }
            if h.regex && regex::Regex::new(&h.pattern).is_err() {
                bail!("hooks.pattern is not a valid regular expression");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Plugin {
    #[serde(flatten)]
    pub manifest: Manifest,
    pub enabled: bool,
    #[serde(skip)]
    pub source: String,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// A traffic hook ready to be turned into a script rule.
#[derive(Debug, Clone)]
pub struct HookSpec {
    pub plugin_id: String,
    pub name: String,
    pub source: String,
    pub hooks: Hooks,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadError {
    pub folder: String,
    pub message: String,
}

#[derive(Default)]
pub struct Registry {
    dir: PathBuf,
    pub plugins: Vec<Plugin>,
    pub errors: Vec<LoadError>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    disabled: BTreeSet<String>,
}

impl Registry {
    /// Load every plugin below `dir`. Broken plugins are reported in `errors` and skipped.
    pub fn load(dir: &Path) -> Registry {
        let state: State = std::fs::read_to_string(dir.join("state.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut reg = Registry { dir: dir.to_path_buf(), ..Default::default() };
        let Ok(entries) = std::fs::read_dir(dir) else { return reg };
        let mut folders: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        folders.sort();
        for folder in folders {
            let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match read_plugin(&folder) {
                Ok((manifest, source)) => {
                    let enabled = !state.disabled.contains(&manifest.id);
                    reg.plugins.push(Plugin { manifest, enabled, source, dir: folder });
                }
                Err(e) => reg.errors.push(LoadError { folder: name, message: format!("{e:#}") }),
            }
        }
        reg
    }

    fn save_state(&self) -> Result<()> {
        let state =
            State { disabled: self.plugins.iter().filter(|p| !p.enabled).map(|p| p.manifest.id.clone()).collect() };
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join("state.json"), serde_json::to_string_pretty(&state)?)?;
        Ok(())
    }

    /// Install from a folder containing `plugin.json`, or from a `.zip` of one. Replaces a plugin with the same id.
    pub fn install(&mut self, from: &Path) -> Result<Manifest> {
        let staging = tempfile_dir(&self.dir)?;
        let result = (|| -> Result<Manifest> {
            if from.is_dir() {
                copy_package(from, &staging)?;
            } else {
                extract_zip(from, &staging)?;
            }
            let root = package_root(&staging)?;
            let (manifest, _) = read_plugin(&root)?;
            let dest = self.dir.join(&manifest.id);
            if dest.exists() {
                std::fs::remove_dir_all(&dest)?;
            }
            std::fs::create_dir_all(&self.dir)?;
            std::fs::rename(&root, &dest).or_else(|_| copy_package(&root, &dest))?;
            Ok(manifest)
        })();
        let _ = std::fs::remove_dir_all(&staging);
        let manifest = result?;
        let id = manifest.id.clone();
        *self = {
            let mut fresh = Registry::load(&self.dir);
            // A reinstall enables the plugin.
            if let Some(p) = fresh.plugins.iter_mut().find(|p| p.manifest.id == id) {
                p.enabled = true;
            }
            fresh
        };
        self.save_state()?;
        Ok(manifest)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let p = self.plugins.iter_mut().find(|p| p.manifest.id == id).with_context(|| format!("no plugin {id}"))?;
        p.enabled = enabled;
        self.save_state()
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let at = self.plugins.iter().position(|p| p.manifest.id == id).with_context(|| format!("no plugin {id}"))?;
        let p = self.plugins.remove(at);
        std::fs::remove_dir_all(&p.dir)?;
        self.save_state()
    }

    pub fn enabled(&self) -> impl Iterator<Item = &Plugin> {
        self.plugins.iter().filter(|p| p.enabled)
    }

    pub fn hook_specs(&self) -> Vec<HookSpec> {
        self.enabled()
            .filter_map(|p| {
                p.manifest.contributes.hooks.clone().map(|hooks| HookSpec {
                    plugin_id: p.manifest.id.clone(),
                    name: p.manifest.name.clone(),
                    source: p.source.clone(),
                    hooks,
                })
            })
            .collect()
    }

    /// Enabled viewers whose content types match, with their plugin.
    pub fn viewers_for<'a>(&'a self, content_type: &'a str) -> impl Iterator<Item = (&'a Plugin, &'a Viewer)> + 'a {
        let ct = content_type.to_ascii_lowercase();
        self.enabled().flat_map(move |p| {
            let ct = ct.clone();
            p.manifest
                .contributes
                .viewers
                .iter()
                .filter(move |v| v.content_types.iter().any(|t| ct.contains(&t.to_ascii_lowercase())))
                .map(move |v| (p, v))
        })
    }

    pub fn find_copy_as(&self, plugin: &str, id: &str) -> Option<(&Plugin, &CopyAs)> {
        let p = self.enabled().find(|p| p.manifest.id == plugin)?;
        p.manifest.contributes.copy_as.iter().find(|c| c.id == id).map(|c| (p, c))
    }
}

fn read_plugin(folder: &Path) -> Result<(Manifest, String)> {
    let manifest =
        Manifest::parse(&std::fs::read_to_string(folder.join("plugin.json")).context("plugin.json is missing")?)?;
    let main = folder.join("main.js");
    let len = std::fs::metadata(&main).context("main.js is missing")?.len();
    if len > MAX_SOURCE_BYTES {
        bail!("main.js is larger than {} KB", MAX_SOURCE_BYTES / 1024);
    }
    Ok((manifest, std::fs::read_to_string(&main).context("main.js is not valid UTF-8")?))
}

fn tempfile_dir(base: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    let dir = base.join(format!(".install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Copy a plugin folder, refusing symlinks and oversized packages.
fn copy_package(from: &Path, to: &Path) -> Result<()> {
    fn walk(from: &Path, to: &Path, total: &mut u64, depth: usize) -> Result<()> {
        if depth > 6 {
            bail!("the plugin folder is nested too deeply");
        }
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let dest = to.join(entry.file_name());
            if kind.is_symlink() {
                continue;
            } else if kind.is_dir() {
                walk(&entry.path(), &dest, total, depth + 1)?;
            } else {
                *total += entry.metadata()?.len();
                if *total > MAX_PACKAGE_BYTES {
                    bail!("the plugin is larger than {} MB", MAX_PACKAGE_BYTES / 1024 / 1024);
                }
                std::fs::copy(entry.path(), dest)?;
            }
        }
        Ok(())
    }
    walk(from, to, &mut 0, 0)
}

fn extract_zip(zip_path: &Path, to: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path).with_context(|| format!("opening {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("not a valid .zip file")?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        // `enclosed_name` rejects absolute paths and `..` components.
        let Some(rel) = entry.enclosed_name() else {
            bail!("the archive contains an unsafe path: {}", String::from_utf8_lossy(entry.name_raw()))
        };
        let dest = to.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut data = Vec::new();
        entry.by_ref().take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut data)?;
        total += data.len() as u64;
        if total > MAX_PACKAGE_BYTES {
            bail!("the plugin is larger than {} MB once unpacked", MAX_PACKAGE_BYTES / 1024 / 1024);
        }
        std::fs::write(dest, data)?;
    }
    Ok(())
}

/// The folder that holds `plugin.json`: the staging folder itself, or its only subfolder (zips of a folder).
fn package_root(staging: &Path) -> Result<PathBuf> {
    if staging.join("plugin.json").exists() {
        return Ok(staging.to_path_buf());
    }
    let subs: Vec<_> = std::fs::read_dir(staging)?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    match subs.as_slice() {
        [only] if only.join("plugin.json").exists() => Ok(only.clone()),
        _ => bail!("plugin.json was not found at the top of the plugin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MANIFEST: &str = r#"{
        "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1,
        "contributes": {
            "hooks": { "pattern": "example.com", "request": true },
            "viewers": [{ "id": "v", "name": "Custom", "content_types": ["application/x-custom"] }],
            "copy_as": [{ "id": "c", "name": "Pseudo", "syntax": "text" }]
        }
    }"#;

    fn package(dir: &Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.json"), manifest).unwrap();
        std::fs::write(dir.join("main.js"), "function onRequest(ctx, r) { return r; }").unwrap();
    }

    #[test]
    fn manifest_validation() {
        assert!(Manifest::parse(MANIFEST).is_ok());
        assert!(
            Manifest::parse(r#"{"id":"Bad Id","name":"x","version":"1"}"#)
                .unwrap_err()
                .to_string()
                .contains("plugin id")
        );
        assert!(
            Manifest::parse(r#"{"id":"x","name":"x","version":"1","api":99}"#)
                .unwrap_err()
                .to_string()
                .contains("update ProxyZip")
        );
        assert!(
            Manifest::parse(r#"{"id":"x","name":"x","version":"1","contributes":{"hooks":{"pattern":"a"}}}"#).is_err()
        );
        assert!(Manifest::parse(r#"{"id":"x","name":"x","version":"1","contributes":{"hooks":{"pattern":"(","regex":true,"request":true}}}"#).is_err());
        assert!(Manifest::parse(r#"{"id":"x","name":"x","version":"1","contributes":{"viewers":[{"id":"a","name":"A"},{"id":"a","name":"B"}]}}"#).is_err());
        assert!(Manifest::parse("{").is_err());
    }

    #[test]
    fn installs_from_a_folder_and_manages_state() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        package(&src, MANIFEST);
        let mut reg = Registry::load(&tmp.path().join("plugins"));
        assert!(reg.plugins.is_empty());
        let m = reg.install(&src).unwrap();
        assert_eq!(m.id, "demo");
        assert_eq!(reg.plugins.len(), 1);
        assert!(reg.plugins[0].enabled);
        assert_eq!(reg.hook_specs().len(), 1);
        assert_eq!(reg.viewers_for("application/x-custom+bin").count(), 1);
        assert_eq!(reg.viewers_for("text/html").count(), 0);
        assert!(reg.find_copy_as("demo", "c").is_some());

        reg.set_enabled("demo", false).unwrap();
        assert!(reg.hook_specs().is_empty() && reg.find_copy_as("demo", "c").is_none());
        let reloaded = Registry::load(&tmp.path().join("plugins"));
        assert!(!reloaded.plugins[0].enabled, "the switch survives a restart");

        let mut reloaded = reloaded;
        reloaded.remove("demo").unwrap();
        assert!(Registry::load(&tmp.path().join("plugins")).plugins.is_empty());
    }

    #[test]
    fn installs_from_a_zip_with_a_top_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("demo.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("demo-plugin/plugin.json", opts).unwrap();
            z.write_all(MANIFEST.as_bytes()).unwrap();
            z.start_file("demo-plugin/main.js", opts).unwrap();
            z.write_all(b"function decode(body, info) { return { json: { ok: true } }; }").unwrap();
            z.finish().unwrap();
        }
        let mut reg = Registry::load(&tmp.path().join("plugins"));
        reg.install(&zip_path).unwrap();
        assert!(reg.plugins[0].source.contains("decode"));
    }

    #[test]
    fn rejects_zip_path_traversal_and_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            z.start_file("../escape.txt", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let mut reg = Registry::load(&tmp.path().join("plugins"));
        assert!(reg.install(&zip_path).unwrap_err().to_string().contains("unsafe path"));
        assert!(!tmp.path().join("escape.txt").exists());

        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(reg.install(&empty).is_err());
        assert!(reg.plugins.is_empty());
    }

    #[test]
    fn reports_broken_plugins_without_failing_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plugins");
        package(&dir.join("good"), MANIFEST);
        package(&dir.join("bad"), r#"{"id":"BAD","name":"b","version":"1"}"#);
        let reg = Registry::load(&dir);
        assert_eq!(reg.plugins.len(), 1);
        assert_eq!(reg.errors.len(), 1);
        assert_eq!(reg.errors[0].folder, "bad");
    }
}
