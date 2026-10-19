//! The serializable description of every rule. Stored as `rules.json` in the data directory.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::matcher::Matcher;

fn status_ok() -> u16 {
    200
}

/// Switches that turn a whole tool on or off without deleting its rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolToggles {
    pub block: bool,
    pub map_local: bool,
    pub map_remote: bool,
    pub rewrite: bool,
    pub breakpoints: bool,
    pub scripts: bool,
    pub dns: bool,
    pub reverse: bool,
}

impl Default for ToolToggles {
    fn default() -> Self {
        Self {
            block: true,
            map_local: true,
            map_remote: true,
            rewrite: true,
            breakpoints: true,
            scripts: true,
            dns: true,
            reverse: true,
        }
    }
}

/// Everything the user can configure beyond plain capturing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleSet {
    pub tools: ToolToggles,
    pub block: Vec<BlockRule>,
    pub record_only: RecordOnly,
    pub map_local: Vec<MapLocalRule>,
    pub map_remote: Vec<MapRemoteRule>,
    pub rewrite: Vec<RewriteRule>,
    pub breakpoints: Vec<BreakpointRule>,
    pub scripts: Vec<ScriptRule>,
    pub proto_mappings: Vec<ProtoMapping>,
    pub throttle: Throttle,
    pub dns: Vec<DnsRule>,
    pub upstream: UpstreamProxy,
    pub reverse: Vec<ReverseRule>,
    pub socks: SocksListener,
}

impl RuleSet {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockAction {
    /// Answer with this status and an explanatory page.
    Status { code: u16 },
    /// Close the connection without answering.
    Drop,
}

impl Default for BlockAction {
    fn default() -> Self {
        BlockAction::Status { code: 403 }
    }
}

/// Stop matching requests from reaching the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    pub action: BlockAction,
}

impl Default for BlockRule {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, matcher: Matcher::default(), action: BlockAction::default() }
    }
}

/// When enabled, only matching requests are recorded. Everything else is still proxied.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordOnly {
    pub enabled: bool,
    pub matchers: Vec<Matcher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalSource {
    /// Always answer with this file.
    File { path: PathBuf },
    /// Answer with the file in this directory named like the part of the URL that follows the literal part of the
    /// pattern (`example.com/static/*` + `/static/app.js` → `<dir>/app.js`).
    Directory { path: PathBuf },
}

impl Default for LocalSource {
    fn default() -> Self {
        LocalSource::File { path: PathBuf::new() }
    }
}

/// Answer matching requests from disk instead of the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MapLocalRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    pub source: LocalSource,
    #[serde(default = "status_ok")]
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Guessed from the file extension when empty.
    pub content_type: Option<String>,
}

impl Default for MapLocalRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            matcher: Matcher::default(),
            source: LocalSource::default(),
            status: 200,
            headers: Vec::new(),
            content_type: None,
        }
    }
}

/// Send matching requests somewhere else. Empty fields keep the original value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MapRemoteRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Replaces the whole path (the query string is kept unless this contains one).
    pub path: Option<String>,
    /// Keep the original `Host` header instead of using the new host.
    pub keep_host_header: bool,
}

impl Default for MapRemoteRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            matcher: Matcher::default(),
            scheme: None,
            host: None,
            port: None,
            path: None,
            keep_host_header: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Request,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewriteOp {
    /// Replace every header called `name`, or add it.
    SetHeader {
        name: String,
        value: String,
    },
    /// Append a header, keeping existing ones.
    AddHeader {
        name: String,
        value: String,
    },
    RemoveHeader {
        name: String,
    },
    /// Request only.
    SetQuery {
        name: String,
        value: String,
    },
    /// Request only.
    RemoveQuery {
        name: String,
    },
    /// Response only.
    SetStatus {
        status: u16,
    },
    /// Replace text in the (decoded) body.
    ReplaceBody {
        find: String,
        replace: String,
        #[serde(default)]
        regex: bool,
    },
}

/// Edit matching requests or responses on the fly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RewriteRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    pub phase: Phase,
    pub ops: Vec<RewriteOp>,
}

impl Default for RewriteRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            matcher: Matcher::default(),
            phase: Phase::Response,
            ops: Vec::new(),
        }
    }
}

/// Pause matching traffic so it can be edited before it continues.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BreakpointRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    pub on_request: bool,
    pub on_response: bool,
}

impl Default for BreakpointRule {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, matcher: Matcher::default(), on_request: true, on_response: false }
    }
}

/// JavaScript that runs on matching requests and/or responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptRule {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    /// JavaScript defining `onRequest(ctx, request)` and/or `onResponse(ctx, request, response)`.
    pub source: String,
    pub on_request: bool,
    pub on_response: bool,
}

impl Default for ScriptRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            matcher: Matcher::default(),
            source: String::new(),
            on_request: true,
            on_response: true,
        }
    }
}

/// Tells the inspector which Protobuf message types a plain (non-gRPC) endpoint uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtoMapping {
    pub name: String,
    pub enabled: bool,
    pub matcher: Matcher,
    /// Fully qualified type of the request body, such as `demo.HelloRequest`.
    pub request_type: Option<String>,
    pub response_type: Option<String>,
}

impl Default for ProtoMapping {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            matcher: Matcher::default(),
            request_type: None,
            response_type: None,
        }
    }
}

/// Simulated network conditions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThrottleProfile {
    pub name: String,
    /// Added once before each request is sent.
    pub latency_ms: u64,
    /// Kilobits per second; 0 means unlimited.
    pub download_kbps: u64,
    pub upload_kbps: u64,
}

impl Default for ThrottleProfile {
    fn default() -> Self {
        Self { name: "Custom".into(), latency_ms: 0, download_kbps: 0, upload_kbps: 0 }
    }
}

impl ThrottleProfile {
    /// Built-in presets: `edge`, `3g`, `4g` and `wifi-poor`.
    pub fn preset(name: &str) -> Option<Self> {
        let (label, latency_ms, down, up) = match name.to_ascii_lowercase().as_str() {
            "edge" => ("Edge", 400, 240, 200),
            "3g" => ("3G", 200, 780, 330),
            "4g" | "lte" => ("4G / LTE", 70, 12_000, 5_000),
            "wifi-poor" | "poor-wifi" => ("Poor Wi-Fi", 150, 1_500, 750),
            _ => return None,
        };
        Some(Self { name: label.into(), latency_ms, download_kbps: down, upload_kbps: up })
    }

    pub const PRESETS: &'static [&'static str] = &["edge", "3g", "4g", "wifi-poor"];

    pub fn download_bytes_per_sec(&self) -> Option<u64> {
        (self.download_kbps > 0).then(|| self.download_kbps * 1000 / 8)
    }

    pub fn upload_bytes_per_sec(&self) -> Option<u64> {
        (self.upload_kbps > 0).then(|| self.upload_kbps * 1000 / 8)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Throttle {
    pub enabled: bool,
    pub profile: ThrottleProfile,
    /// Empty means every request.
    pub matchers: Vec<Matcher>,
}

/// Resolve `host` (wildcards allowed) to a fixed address, bypassing DNS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DnsRule {
    pub name: String,
    pub enabled: bool,
    pub host: String,
    pub ip: Option<IpAddr>,
}

impl Default for DnsRule {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, host: String::new(), ip: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKind {
    #[default]
    Http,
    Socks5,
}

/// Chain ProxyZip through another proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpstreamProxy {
    pub enabled: bool,
    pub kind: UpstreamKind,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Hosts (wildcards allowed) that skip the upstream proxy.
    pub bypass: Vec<String>,
}

impl Default for UpstreamProxy {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: UpstreamKind::Http,
            host: String::new(),
            port: 8080,
            username: None,
            password: None,
            bypass: Vec::new(),
        }
    }
}

/// Expose `target` on a local port: requests to it are forwarded there.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverseRule {
    pub name: String,
    pub enabled: bool,
    pub listen_port: u16,
    /// Full URL such as `https://api.example.com/v1`.
    pub target: String,
}

impl Default for ReverseRule {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, listen_port: 0, target: String::new() }
    }
}

/// A SOCKS5 listener that feeds the same interception pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SocksListener {
    pub enabled: bool,
    pub port: u16,
}

impl Default for SocksListener {
    fn default() -> Self {
        Self { enabled: false, port: 9394 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_is_a_valid_rule_set() {
        let set = RuleSet::from_json("{}").unwrap();
        assert!(set.block.is_empty() && set.tools.block && !set.throttle.enabled && !set.socks.enabled);
    }

    #[test]
    fn round_trips() {
        let mut set = RuleSet::default();
        set.block.push(BlockRule {
            name: "ads".into(),
            matcher: Matcher::new("ads.example.com"),
            ..Default::default()
        });
        set.rewrite.push(RewriteRule {
            ops: vec![
                RewriteOp::SetHeader { name: "x-a".into(), value: "1".into() },
                RewriteOp::SetStatus { status: 418 },
            ],
            ..Default::default()
        });
        let json = set.to_json().unwrap();
        let back = RuleSet::from_json(&json).unwrap();
        assert_eq!(back.block[0].name, "ads");
        assert_eq!(back.rewrite[0].ops.len(), 2);
    }

    #[test]
    fn upstream_defaults_to_a_common_port_but_stays_off() {
        let u = UpstreamProxy::default();
        assert!(!u.enabled && u.port == 8080 && u.host.is_empty());
    }

    #[test]
    fn presets() {
        assert_eq!(ThrottleProfile::preset("3G").unwrap().latency_ms, 200);
        assert!(ThrottleProfile::preset("nope").is_none());
        assert_eq!(ThrottleProfile::preset("edge").unwrap().download_bytes_per_sec(), Some(30_000));
        assert_eq!(ThrottleProfile::default().download_bytes_per_sec(), None);
        for p in ThrottleProfile::PRESETS {
            assert!(ThrottleProfile::preset(p).is_some());
        }
    }
}
