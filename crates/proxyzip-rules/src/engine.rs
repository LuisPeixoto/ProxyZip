//! Decides what happens to a request. Pure: no I/O.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use crate::matcher::{Matcher, wildcard_match};
use crate::model::Phase;
use crate::model::{
    BlockAction, LocalSource, MapLocalRule, ReverseRule, RewriteOp, RuleSet, ScriptRule, ThrottleProfile, UpstreamProxy,
};

/// What the engine needs to know about a request.
#[derive(Debug, Clone, Copy)]
pub struct RequestInfo<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub host: &'a str,
    pub port: u16,
    /// Path including the query string.
    pub path: &'a str,
}

/// A file that should answer a request.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalFile {
    pub path: PathBuf,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub content_type: Option<String>,
}

/// Where a request should go instead of its original destination. Empty fields keep the original.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remap {
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub keep_host_header: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    #[default]
    Forward,
    Block(BlockAction),
    MapLocal(LocalFile),
    /// Answered by a mock server.
    Mock(Box<crate::mock::MockMatch>),
}

/// Everything the proxy must do for one request.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub action: Action,
    pub remap: Option<Remap>,
    pub request_ops: Vec<RewriteOp>,
    pub response_ops: Vec<RewriteOp>,
    pub break_on_request: bool,
    pub break_on_response: bool,
    /// Scripts to run on the request, in order.
    pub request_scripts: Vec<ScriptRule>,
    /// Scripts to run on the response, in order.
    pub response_scripts: Vec<ScriptRule>,
    pub throttle: Option<ThrottleProfile>,
    /// Labels shown next to the request in the list: `blocked`, `map-local`, `map-remote`, `rewrite`, `breakpoint`.
    pub tags: Vec<String>,
}

impl Plan {
    /// Whether the response body must be held back (rewritten or edited) instead of streamed.
    pub fn needs_response_body(&self) -> bool {
        self.break_on_response
            || !self.response_scripts.is_empty()
            || self.response_ops.iter().any(|op| matches!(op, RewriteOp::ReplaceBody { .. }))
    }
}

impl RuleSet {
    fn is_match(&self, matcher: &Matcher, info: &RequestInfo) -> bool {
        matcher.matches(info.method, info.scheme, info.host, info.port, info.path)
    }

    /// Whether this request should be stored (the "record only" filter).
    pub fn should_record(&self, info: &RequestInfo) -> bool {
        !self.record_only.enabled || self.record_only.matchers.iter().any(|m| self.is_match(m, info))
    }

    /// Work out what to do with a request. The first matching rule of each tool wins.
    pub fn plan(&self, info: &RequestInfo) -> Plan {
        self.plan_with(info, None)
    }

    /// Like [`plan`](Self::plan); `mock_server` names the mock server whose own listener received the request.
    pub fn plan_with(&self, info: &RequestInfo, mock_server: Option<&str>) -> Plan {
        use crate::mock::MockOutcome;
        let mut plan = Plan::default();

        match self.mock_for(info, mock_server) {
            MockOutcome::Hit(hit) => {
                plan.action = Action::Mock(hit);
                plan.tags.push("mock".into());
                return plan;
            }
            MockOutcome::Miss { server, routes } => {
                let body = serde_json::json!({
                    "error": "no mock route matches this request",
                    "server": server,
                    "request": format!("{} {}", info.method, info.path),
                    "routes": routes,
                })
                .to_string();
                plan.action = Action::Mock(Box::new(crate::mock::MockMatch {
                    server,
                    route: String::new(),
                    status: 404,
                    headers: vec![("content-type".into(), "application/json".into())],
                    body: crate::model::MockBody::Inline { text: body },
                    template: false,
                    delay_ms: 0,
                    params: Default::default(),
                }));
                plan.tags.push("mock".into());
                return plan;
            }
            MockOutcome::None => {}
        }

        if self.tools.block
            && let Some(rule) = self.block.iter().find(|r| r.enabled && self.is_match(&r.matcher, info))
        {
            plan.action = Action::Block(rule.action.clone());
            plan.tags.push("blocked".into());
            return plan;
        }

        if self.tools.map_local {
            for rule in self.map_local.iter().filter(|r| r.enabled && self.is_match(&r.matcher, info)) {
                if let Some(file) = local_file(rule, info) {
                    plan.action = Action::MapLocal(file);
                    plan.tags.push("map-local".into());
                    return plan;
                }
            }
        }

        if self.tools.map_remote
            && let Some(rule) = self.map_remote.iter().find(|r| r.enabled && self.is_match(&r.matcher, info))
        {
            plan.remap = Some(Remap {
                scheme: rule.scheme.clone().filter(|s| !s.is_empty()),
                host: rule.host.clone().filter(|s| !s.is_empty()),
                port: rule.port,
                path: rule.path.clone().filter(|s| !s.is_empty()),
                keep_host_header: rule.keep_host_header,
            });
            plan.tags.push("map-remote".into());
        }

        if self.tools.rewrite {
            for rule in self.rewrite.iter().filter(|r| r.enabled && self.is_match(&r.matcher, info)) {
                match rule.phase {
                    Phase::Request => plan.request_ops.extend(rule.ops.iter().cloned()),
                    Phase::Response => plan.response_ops.extend(rule.ops.iter().cloned()),
                }
            }
            if !plan.request_ops.is_empty() || !plan.response_ops.is_empty() {
                plan.tags.push("rewrite".into());
            }
        }

        if self.tools.scripts {
            for rule in self.scripts.iter().filter(|r| r.enabled && self.is_match(&r.matcher, info)) {
                if rule.on_request {
                    plan.request_scripts.push(rule.clone());
                }
                if rule.on_response {
                    plan.response_scripts.push(rule.clone());
                }
            }
            if !plan.request_scripts.is_empty() || !plan.response_scripts.is_empty() {
                plan.tags.push("script".into());
            }
        }

        if self.tools.breakpoints {
            for rule in self.breakpoints.iter().filter(|r| r.enabled && self.is_match(&r.matcher, info)) {
                plan.break_on_request |= rule.on_request;
                plan.break_on_response |= rule.on_response;
            }
            if plan.break_on_request || plan.break_on_response {
                plan.tags.push("breakpoint".into());
            }
        }

        if self.throttle.enabled
            && (self.throttle.matchers.is_empty() || self.throttle.matchers.iter().any(|m| self.is_match(m, info)))
        {
            plan.throttle = Some(self.throttle.profile.clone());
            plan.tags.push("throttle".into());
        }
        plan
    }

    /// Protobuf message types configured for this request: `(request type, response type)`.
    pub fn proto_types(&self, info: &RequestInfo) -> Option<(Option<String>, Option<String>)> {
        self.proto_mappings.iter().find(|m| m.enabled && self.is_match(&m.matcher, info)).map(|m| {
            (m.request_type.clone().filter(|t| !t.is_empty()), m.response_type.clone().filter(|t| !t.is_empty()))
        })
    }

    /// Address configured for `host` by a DNS override, if any.
    pub fn dns_lookup(&self, host: &str) -> Option<IpAddr> {
        if !self.tools.dns {
            return None;
        }
        self.dns
            .iter()
            .filter(|r| r.enabled)
            .find(|r| !r.host.is_empty() && wildcard_match(&r.host, host))
            .and_then(|r| r.ip)
    }

    /// The upstream proxy to use for `host`, unless disabled or bypassed.
    pub fn upstream_for(&self, host: &str) -> Option<&UpstreamProxy> {
        let u = &self.upstream;
        let usable = u.enabled && !u.host.is_empty() && u.port != 0;
        (usable && !u.bypass.iter().any(|p| wildcard_match(p, host))).then_some(u)
    }

    /// Reverse proxy rules that should have a listener.
    pub fn active_reverse(&self) -> Vec<&ReverseRule> {
        if !self.tools.reverse {
            return Vec::new();
        }
        self.reverse.iter().filter(|r| r.enabled && r.listen_port != 0 && !r.target.is_empty()).collect()
    }
}

fn local_file(rule: &MapLocalRule, info: &RequestInfo) -> Option<LocalFile> {
    let path = match &rule.source {
        LocalSource::File { path } => path.clone(),
        LocalSource::Directory { path } => directory_file(path, &rule.matcher, info.path)?,
    };
    Some(LocalFile {
        path,
        status: rule.status,
        headers: rule.headers.clone(),
        content_type: rule.content_type.clone().filter(|c| !c.is_empty()),
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Map a request path to a file below `dir`. Refuses anything that could escape it.
fn directory_file(dir: &Path, matcher: &Matcher, request_path: &str) -> Option<PathBuf> {
    let path_only = request_path.split('?').next().unwrap_or("");
    let rest = if matcher.regex {
        path_only.rsplit('/').next().unwrap_or("")
    } else {
        let pattern = matcher.pattern.trim();
        let after_scheme = pattern.split_once("://").map_or(pattern, |(_, r)| r);
        let pattern_path = after_scheme.find('/').map_or("/", |i| &after_scheme[i..]);
        let literal = pattern_path.split(['*', '?']).next().unwrap_or("");
        match path_only.get(..literal.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(literal) => &path_only[literal.len()..],
            _ => path_only.rsplit('/').next().unwrap_or(""),
        }
    };
    let decoded = percent_decode(rest);
    let mut file = dir.to_path_buf();
    for component in decoded.split('/').filter(|c| !c.is_empty()) {
        if component == ".." || component == "." || component.contains(['\\', ':', '\0']) {
            return None;
        }
        file.push(component);
    }
    if decoded.is_empty() || decoded.ends_with('/') {
        file.push("index.html");
    }
    Some(file)
}

/// A reasonable `Content-Type` for a file name.
pub fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("txt" | "log" | "md") => "text/plain; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn info<'a>(method: &'a str, host: &'a str, path: &'a str) -> RequestInfo<'a> {
        RequestInfo { method, scheme: "https", host, port: 443, path }
    }

    #[test]
    fn nothing_configured_forwards_untouched() {
        let plan = RuleSet::default().plan(&info("GET", "example.com", "/"));
        assert_eq!(plan.action, Action::Forward);
        assert!(plan.remap.is_none() && plan.tags.is_empty() && plan.throttle.is_none());
        assert!(!plan.needs_response_body());
    }

    #[test]
    fn block_wins_and_respects_toggles() {
        let mut set = RuleSet::default();
        set.block.push(BlockRule { matcher: Matcher::new("ads.example.com"), ..Default::default() });
        set.map_remote.push(MapRemoteRule {
            matcher: Matcher::new("ads.example.com"),
            host: Some("x".into()),
            ..Default::default()
        });
        let plan = set.plan(&info("GET", "ads.example.com", "/pixel"));
        assert_eq!(plan.action, Action::Block(BlockAction::Status { code: 403 }));
        assert_eq!(plan.tags, ["blocked"]);
        set.tools.block = false;
        assert_eq!(set.plan(&info("GET", "ads.example.com", "/")).action, Action::Forward);
        set.block[0].enabled = false;
        set.tools.block = true;
        assert_eq!(set.plan(&info("GET", "ads.example.com", "/")).action, Action::Forward);
    }

    #[test]
    fn map_local_file_and_directory() {
        let mut set = RuleSet::default();
        set.map_local.push(MapLocalRule {
            matcher: Matcher::new("example.com/api/me"),
            source: LocalSource::File { path: "/tmp/me.json".into() },
            status: 201,
            ..Default::default()
        });
        set.map_local.push(MapLocalRule {
            matcher: Matcher::new("example.com/static/*"),
            source: LocalSource::Directory { path: "/srv/site".into() },
            ..Default::default()
        });
        let Action::MapLocal(f) = set.plan(&info("GET", "example.com", "/api/me?x=1")).action else { panic!() };
        assert_eq!((f.path, f.status), (PathBuf::from("/tmp/me.json"), 201));
        let Action::MapLocal(f) = set.plan(&info("GET", "example.com", "/static/js/app.js?v=3")).action else {
            panic!()
        };
        assert_eq!(f.path, PathBuf::from("/srv/site/js/app.js"));
        let Action::MapLocal(f) = set.plan(&info("GET", "example.com", "/static/")).action else { panic!() };
        assert_eq!(f.path, PathBuf::from("/srv/site/index.html"));
    }

    #[test]
    fn directory_mapping_cannot_escape() {
        let mut set = RuleSet::default();
        set.map_local.push(MapLocalRule {
            matcher: Matcher::new("example.com/static/*"),
            source: LocalSource::Directory { path: "/srv/site".into() },
            ..Default::default()
        });
        for evil in ["/static/../secret", "/static/%2e%2e/secret", "/static/a/..%2fb", "/static/c:%5cwin"] {
            assert_eq!(set.plan(&info("GET", "example.com", evil)).action, Action::Forward, "{evil}");
        }
        assert!(
            matches!(set.plan(&info("GET", "example.com", "/static/a%20b.txt")).action, Action::MapLocal(f) if f.path.ends_with("a b.txt"))
        );
    }

    #[test]
    fn map_remote_and_rewrites_combine() {
        let mut set = RuleSet::default();
        set.map_remote.push(MapRemoteRule {
            matcher: Matcher::new("api.example.com/*"),
            host: Some("localhost".into()),
            port: Some(3000),
            scheme: Some("http".into()),
            ..Default::default()
        });
        set.rewrite.push(RewriteRule {
            matcher: Matcher::new("api.example.com/*"),
            phase: Phase::Request,
            ops: vec![RewriteOp::SetHeader { name: "x-env".into(), value: "dev".into() }],
            ..Default::default()
        });
        set.rewrite.push(RewriteRule {
            matcher: Matcher::new("api.example.com/*"),
            phase: Phase::Response,
            ops: vec![RewriteOp::ReplaceBody { find: "a".into(), replace: "b".into(), regex: false }],
            ..Default::default()
        });
        let plan = set.plan(&info("GET", "api.example.com", "/v1/x"));
        let remap = plan.remap.as_ref().unwrap();
        assert_eq!(
            (remap.host.as_deref(), remap.port, remap.scheme.as_deref()),
            (Some("localhost"), Some(3000), Some("http"))
        );
        assert_eq!(plan.request_ops.len(), 1);
        assert_eq!(plan.response_ops.len(), 1);
        assert!(plan.needs_response_body());
        assert_eq!(plan.tags, ["map-remote", "rewrite"]);
    }

    #[test]
    fn breakpoints_and_throttle() {
        let mut set = RuleSet::default();
        set.breakpoints.push(BreakpointRule {
            matcher: Matcher::new("example.com/login"),
            on_request: true,
            on_response: true,
            ..Default::default()
        });
        set.throttle = Throttle {
            enabled: true,
            profile: ThrottleProfile::preset("3g").unwrap(),
            matchers: vec![Matcher::new("slow.dev")],
        };
        let plan = set.plan(&info("POST", "example.com", "/login"));
        assert!(plan.break_on_request && plan.break_on_response && plan.throttle.is_none());
        assert!(plan.needs_response_body());
        let plan = set.plan(&info("GET", "slow.dev", "/"));
        assert_eq!(plan.throttle.unwrap().name, "3G");
        set.throttle.matchers.clear();
        assert!(set.plan(&info("GET", "anything.dev", "/")).throttle.is_some());
    }

    #[test]
    fn scripts_are_planned_per_phase_and_hold_the_response() {
        let mut set = RuleSet::default();
        set.scripts.push(ScriptRule {
            name: "both".into(),
            matcher: Matcher::new("api.dev"),
            source: "function onRequest(){}".into(),
            ..Default::default()
        });
        set.scripts.push(ScriptRule {
            matcher: Matcher::new("api.dev"),
            on_request: false,
            on_response: true,
            ..Default::default()
        });
        set.scripts.push(ScriptRule { enabled: false, matcher: Matcher::new("api.dev"), ..Default::default() });
        let plan = set.plan(&info("GET", "api.dev", "/"));
        assert_eq!(plan.request_scripts.len(), 1);
        assert_eq!(plan.response_scripts.len(), 2);
        assert!(plan.needs_response_body(), "a response script needs the whole body");
        assert_eq!(plan.tags, ["script"]);
        assert!(set.plan(&info("GET", "other.dev", "/")).request_scripts.is_empty());
        set.tools.scripts = false;
        assert!(set.plan(&info("GET", "api.dev", "/")).request_scripts.is_empty());
    }

    #[test]
    fn proto_mappings_name_the_message_types() {
        let mut set = RuleSet::default();
        set.proto_mappings.push(ProtoMapping {
            matcher: Matcher::new("api.dev/rpc/*"),
            request_type: Some("demo.Req".into()),
            response_type: Some(String::new()),
            ..Default::default()
        });
        assert_eq!(set.proto_types(&info("POST", "api.dev", "/rpc/x")), Some((Some("demo.Req".into()), None)));
        assert_eq!(set.proto_types(&info("POST", "api.dev", "/other")), None);
    }

    #[test]
    fn record_only_filters_what_is_stored() {
        let mut set = RuleSet::default();
        assert!(set.should_record(&info("GET", "a.dev", "/")));
        set.record_only = RecordOnly { enabled: true, matchers: vec![Matcher::new("api.dev")] };
        assert!(set.should_record(&info("GET", "api.dev", "/")));
        assert!(!set.should_record(&info("GET", "other.dev", "/")));
    }

    #[test]
    fn dns_upstream_and_reverse_lookups() {
        let mut set = RuleSet::default();
        set.dns.push(DnsRule { host: "*.test".into(), ip: Some("127.0.0.1".parse().unwrap()), ..Default::default() });
        assert_eq!(set.dns_lookup("app.test"), Some("127.0.0.1".parse().unwrap()));
        assert_eq!(set.dns_lookup("example.com"), None);
        set.tools.dns = false;
        assert_eq!(set.dns_lookup("app.test"), None);

        set.upstream = UpstreamProxy {
            enabled: true,
            host: "proxy.corp".into(),
            port: 8080,
            bypass: vec!["*.internal".into()],
            ..Default::default()
        };
        assert!(set.upstream_for("example.com").is_some());
        assert!(set.upstream_for("db.internal").is_none());
        set.upstream.enabled = false;
        assert!(set.upstream_for("example.com").is_none());

        set.reverse.push(ReverseRule { listen_port: 8081, target: "https://api.dev".into(), ..Default::default() });
        set.reverse.push(ReverseRule { listen_port: 0, target: "https://bad.dev".into(), ..Default::default() });
        assert_eq!(set.active_reverse().len(), 1);
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type_for(Path::new("a/b.JSON")), "application/json");
        assert_eq!(content_type_for(Path::new("x.unknown")), "application/octet-stream");
    }
}
