//! `.pzworkspace`: the configuration of a ProxyZip setup in one shareable file.
//!
//! It holds the rules (block, map, rewrite, mocks, scripts, breakpoints, DNS, reverse proxies), imported Protobuf
//! descriptor sets, the SSL proxying lists and which plugins are switched on. It does not hold captured traffic (that
//! is a [session](crate::session)) or secrets, unless you put them in a rule yourself: [`redact_rules`] hides the usual
//! suspects before sharing.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use proxyzip_rules::{RewriteOp, RuleSet};
use serde::{Deserialize, Serialize};

use crate::redact::{PLACEHOLDER, is_sensitive_header};

pub const FORMAT: &str = "proxyzip-workspace";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SslSettings {
    pub intercept_all: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// A Protobuf descriptor set (`protoc --descriptor_set_out`), stored by file name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtoFile {
    pub name: String,
    /// Base64 of the descriptor set.
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub format: String,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub name: String,
    pub rules: RuleSet,
    #[serde(default)]
    pub protos: Vec<ProtoFile>,
    /// Ids of the plugins that were enabled. Plugins themselves are not included.
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub ssl: Option<SslSettings>,
}

impl Workspace {
    pub fn new(name: impl Into<String>, rules: RuleSet) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            created_at: Utc::now(),
            name: name.into(),
            rules,
            protos: Vec::new(),
            plugins: Vec::new(),
            ssl: None,
        }
    }

    pub fn add_proto(&mut self, name: impl Into<String>, bytes: &[u8]) {
        self.protos.push(ProtoFile { name: name.into(), data: B64.encode(bytes) });
    }

    /// The decoded descriptor sets, with their names.
    pub fn proto_bytes(&self) -> Result<Vec<(String, Vec<u8>)>> {
        self.protos
            .iter()
            .map(|p| {
                Ok((p.name.clone(), B64.decode(&p.data).with_context(|| format!("descriptor {} is damaged", p.name))?))
            })
            .collect()
    }
}

/// Serialize a workspace. With `redact`, secrets in rules are replaced first.
pub fn write(workspace: &Workspace, redact: bool) -> Result<String> {
    let mut w = workspace.clone();
    if redact {
        redact_rules(&mut w.rules);
    }
    Ok(serde_json::to_string_pretty(&w)?)
}

pub fn read(text: &str) -> Result<Workspace> {
    let w: Workspace = serde_json::from_str(text).context("this is not a ProxyZip workspace file")?;
    if w.format != FORMAT {
        bail!("this is not a ProxyZip workspace file");
    }
    if w.version > VERSION {
        bail!("this workspace was saved by a newer ProxyZip (format version {}); update the app to open it", w.version);
    }
    Ok(w)
}

/// Replace secrets in rules: credential headers that rules set, and the upstream proxy's password.
pub fn redact_rules(rules: &mut RuleSet) {
    for rule in rules.rewrite.iter_mut() {
        for op in rule.ops.iter_mut() {
            match op {
                RewriteOp::SetHeader { name, value } | RewriteOp::AddHeader { name, value }
                    if is_sensitive_header(name) =>
                {
                    *value = PLACEHOLDER.into();
                }
                _ => {}
            }
        }
    }
    for rule in rules.map_local.iter_mut() {
        for (name, value) in rule.headers.iter_mut() {
            if is_sensitive_header(name) {
                *value = PLACEHOLDER.into();
            }
        }
    }
    if rules.upstream.password.is_some() {
        rules.upstream.password = Some(PLACEHOLDER.into());
    }
}

/// Add the rules of `other` to `base`. Lists are appended; single settings (throttling, the upstream proxy, the SOCKS5
/// listener, tool switches) of `base` stay as they are.
pub fn merge_rules(base: &mut RuleSet, other: RuleSet) {
    base.block.extend(other.block);
    base.map_local.extend(other.map_local);
    base.map_remote.extend(other.map_remote);
    base.rewrite.extend(other.rewrite);
    base.breakpoints.extend(other.breakpoints);
    base.scripts.extend(other.scripts);
    base.proto_mappings.extend(other.proto_mappings);
    base.dns.extend(other.dns);
    base.reverse.extend(other.reverse);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxyzip_rules::{BlockRule, Matcher, RewriteRule, UpstreamProxy};

    fn rules() -> RuleSet {
        let mut r = RuleSet::default();
        r.block.push(BlockRule { name: "ads".into(), matcher: Matcher::new("ads."), ..Default::default() });
        r.rewrite.push(RewriteRule {
            name: "auth".into(),
            ops: vec![
                RewriteOp::SetHeader { name: "Authorization".into(), value: "Bearer secret".into() },
                RewriteOp::SetHeader { name: "x-env".into(), value: "staging".into() },
            ],
            ..Default::default()
        });
        r.upstream = UpstreamProxy { password: Some("pw".into()), ..Default::default() };
        r
    }

    #[test]
    fn round_trips_with_descriptors_and_plugins() {
        let mut w = Workspace::new("team", rules());
        w.add_proto("api.desc", &[1, 2, 3, 255]);
        w.plugins = vec!["jwt-viewer".into()];
        w.ssl = Some(SslSettings { intercept_all: false, include: vec!["api.*".into()], exclude: vec![] });
        let text = write(&w, false).unwrap();
        let back = read(&text).unwrap();
        assert_eq!(back.name, "team");
        assert_eq!(back.rules.block[0].name, "ads");
        assert_eq!(back.proto_bytes().unwrap(), vec![("api.desc".to_string(), vec![1, 2, 3, 255])]);
        assert_eq!(back.plugins, vec!["jwt-viewer"]);
        assert_eq!(back.ssl.unwrap().include, vec!["api.*"]);
    }

    #[test]
    fn redaction_hides_credentials_only() {
        let text = write(&Workspace::new("t", rules()), true).unwrap();
        assert!(!text.contains("Bearer secret"));
        assert!(!text.contains("\"pw\""));
        assert!(text.contains("staging"), "ordinary values stay");
        assert!(write(&Workspace::new("t", rules()), false).unwrap().contains("Bearer secret"));
    }

    #[test]
    fn rejects_other_files_and_newer_versions() {
        assert!(read("{}").is_err());
        assert!(read("not json").is_err());
        let mut w = Workspace::new("x", RuleSet::default());
        w.version = 99;
        assert!(read(&serde_json::to_string(&w).unwrap()).unwrap_err().to_string().contains("newer ProxyZip"));
        w.version = 1;
        w.format = "other".into();
        assert!(read(&serde_json::to_string(&w).unwrap()).is_err());
    }

    #[test]
    fn merging_appends_lists_and_keeps_settings() {
        let mut base = rules();
        base.upstream.host = "mine".into();
        let mut other = rules();
        other.upstream.host = "theirs".into();
        merge_rules(&mut base, other);
        assert_eq!(base.block.len(), 2);
        assert_eq!(base.rewrite.len(), 2);
        assert_eq!(base.upstream.host, "mine");
    }
}
