//! Matching requests against mock routes. Pure: rendering and I/O happen in the proxy.

use std::collections::HashMap;

use crate::engine::RequestInfo;
use crate::matcher::wildcard_match;
use crate::model::{MockBody, MockRoute, MockServer, RuleSet};

/// A route that matched, with what it captured from the path.
#[derive(Debug, Clone, PartialEq)]
pub struct MockMatch {
    pub server: String,
    pub route: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: MockBody,
    pub template: bool,
    pub delay_ms: u64,
    /// `:name` segments of the route.
    pub params: HashMap<String, String>,
}

/// What the mock tool says about a request.
#[derive(Debug, Clone, PartialEq)]
pub enum MockOutcome {
    /// Not a mock request: carry on with the other rules.
    None,
    Hit(Box<MockMatch>),
    /// The host belongs to a mock server but no route fits and the server does not pass requests through.
    Miss {
        server: String,
        routes: Vec<String>,
    },
}

/// Match a path pattern against a request path. Returns the `:name` captures.
pub fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let want: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let got: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut params = HashMap::new();
    let mut i = 0;
    for (n, part) in want.iter().enumerate() {
        if *part == "**" {
            // Everything that is left, including nothing.
            return Some(params);
        }
        let segment = got.get(i)?;
        if let Some(name) = part.strip_prefix(':') {
            params.insert(name.to_string(), percent_decode(segment));
        } else if *part != "*" && !part.eq_ignore_ascii_case(segment) && percent_decode(segment) != *part {
            return None;
        }
        i += 1;
        if n + 1 == want.len() && i != got.len() {
            return None;
        }
    }
    (i == got.len()).then_some(params)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn method_matches(route: &MockRoute, method: &str) -> bool {
    route.method.is_empty() || route.method.eq_ignore_ascii_case("any") || route.method.eq_ignore_ascii_case(method)
}

fn find_route<'a>(server: &'a MockServer, info: &RequestInfo) -> Option<(&'a MockRoute, HashMap<String, String>)> {
    server
        .routes
        .iter()
        .filter(|r| r.enabled && method_matches(r, info.method))
        .find_map(|r| match_path(&r.path, info.path).map(|params| (r, params)))
}

fn outcome(server: &MockServer, info: &RequestInfo) -> MockOutcome {
    match find_route(server, info) {
        Some((route, params)) => MockOutcome::Hit(Box::new(MockMatch {
            server: server.name.clone(),
            route: route.name.clone(),
            status: route.status,
            headers: route.headers.clone(),
            body: route.body.clone(),
            template: route.template,
            delay_ms: route.delay_ms,
            params,
        })),
        None if server.passthrough => MockOutcome::None,
        None => MockOutcome::Miss {
            server: server.name.clone(),
            routes: server
                .routes
                .iter()
                .filter(|r| r.enabled)
                .map(|r| format!("{} {}", if r.method.is_empty() { "ANY" } else { &r.method }, r.path))
                .collect(),
        },
    }
}

impl RuleSet {
    /// Ask the mock tool about a request. `only` restricts it to one server (requests that arrived on its own listener,
    /// whatever host they name); otherwise servers are chosen by host.
    pub fn mock_for(&self, info: &RequestInfo, only: Option<&str>) -> MockOutcome {
        if !self.tools.mocks {
            return MockOutcome::None;
        }
        match only {
            Some(name) => self
                .mocks
                .iter()
                .find(|s| s.enabled && s.name == name)
                .map(|s| outcome(s, info))
                .unwrap_or(MockOutcome::None),
            None => self
                .mocks
                .iter()
                .filter(|s| s.enabled && !s.host.is_empty() && wildcard_match(&s.host, info.host))
                .map(|s| outcome(s, info))
                .find(|o| !matches!(o, MockOutcome::None))
                .unwrap_or(MockOutcome::None),
        }
    }

    /// Mock servers that want their own local port.
    pub fn mock_listeners(&self) -> Vec<(&str, u16)> {
        if !self.tools.mocks {
            return Vec::new();
        }
        self.mocks
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| s.listen_port.filter(|p| *p != 0).map(|p| (s.name.as_str(), p)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(method: &'a str, host: &'a str, path: &'a str) -> RequestInfo<'a> {
        RequestInfo { method, scheme: "https", host, port: 443, path }
    }

    fn server() -> MockServer {
        MockServer {
            name: "api".into(),
            host: "api.mock".into(),
            routes: vec![
                MockRoute {
                    name: "one".into(),
                    method: "GET".into(),
                    path: "/users/:id".into(),
                    status: 200,
                    ..Default::default()
                },
                MockRoute {
                    name: "create".into(),
                    method: "POST".into(),
                    path: "/users".into(),
                    status: 201,
                    ..Default::default()
                },
                MockRoute { name: "files".into(), path: "/files/**".into(), status: 200, ..Default::default() },
                MockRoute { name: "off".into(), enabled: false, path: "/off".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn rules(s: MockServer) -> RuleSet {
        RuleSet { mocks: vec![s], ..Default::default() }
    }

    #[test]
    fn path_patterns() {
        assert_eq!(match_path("/users/:id", "/users/42?x=1").unwrap()["id"], "42");
        assert_eq!(match_path("/users/:id", "/users/a%20b").unwrap()["id"], "a b");
        assert!(match_path("/users/:id", "/users").is_none());
        assert!(match_path("/users/:id", "/users/1/extra").is_none());
        assert!(match_path("/a/*", "/a/x").is_some() && match_path("/a/*", "/a/x/y").is_none());
        assert!(match_path("/a/**", "/a").is_some() && match_path("/a/**", "/a/x/y/z").is_some());
        assert!(match_path("/", "/").is_some() && match_path("/", "/x").is_none());
        assert!(match_path("/Users", "/users/").is_some(), "case and a trailing slash do not matter");
        assert!(match_path("/**", "/anything/at/all").is_some());
    }

    #[test]
    fn routes_are_chosen_by_method_and_path_in_order() {
        let r = rules(server());
        let MockOutcome::Hit(hit) = r.mock_for(&info("GET", "API.mock", "/users/7"), None) else { panic!() };
        assert_eq!((hit.route.as_str(), hit.params["id"].as_str()), ("one", "7"));
        let MockOutcome::Hit(hit) = r.mock_for(&info("POST", "api.mock", "/users"), None) else { panic!() };
        assert_eq!(hit.status, 201);
        assert!(
            matches!(r.mock_for(&info("DELETE", "api.mock", "/users/7"), None), MockOutcome::Miss { .. }),
            "wrong method"
        );
        assert!(
            matches!(r.mock_for(&info("GET", "api.mock", "/off"), None), MockOutcome::Miss { .. }),
            "disabled route"
        );
        assert!(matches!(r.mock_for(&info("GET", "api.mock", "/files/a/b"), None), MockOutcome::Hit(_)));
    }

    #[test]
    fn unrelated_hosts_and_disabled_tools_are_left_alone() {
        let mut r = rules(server());
        assert_eq!(r.mock_for(&info("GET", "other.com", "/users/1"), None), MockOutcome::None);
        r.tools.mocks = false;
        assert_eq!(r.mock_for(&info("GET", "api.mock", "/users/1"), None), MockOutcome::None);
        let mut off = server();
        off.enabled = false;
        assert_eq!(rules(off).mock_for(&info("GET", "api.mock", "/users/1"), None), MockOutcome::None);
    }

    #[test]
    fn a_miss_lists_the_routes_unless_passing_through() {
        let r = rules(server());
        let MockOutcome::Miss { server, routes } = r.mock_for(&info("GET", "api.mock", "/nope"), None) else {
            panic!()
        };
        assert_eq!(server, "api");
        assert_eq!(routes, vec!["GET /users/:id", "POST /users", "ANY /files/**"]);
        let mut p = server_pass();
        p.passthrough = true;
        assert_eq!(rules(p).mock_for(&info("GET", "api.mock", "/nope"), None), MockOutcome::None);
    }

    fn server_pass() -> MockServer {
        server()
    }

    #[test]
    fn an_own_listener_ignores_the_host() {
        let mut s = server();
        s.host = String::new();
        s.listen_port = Some(4010);
        let r = rules(s);
        assert!(matches!(r.mock_for(&info("GET", "localhost", "/users/1"), Some("api")), MockOutcome::Hit(_)));
        assert_eq!(
            r.mock_for(&info("GET", "localhost", "/users/1"), None),
            MockOutcome::None,
            "no host, no capture by host"
        );
        assert_eq!(r.mock_listeners(), vec![("api", 4010)]);
        assert_eq!(r.mock_for(&info("GET", "x", "/"), Some("missing")), MockOutcome::None);
    }

    #[test]
    fn serializes_with_defaults() {
        let s: MockServer =
            serde_json::from_str(r#"{"name":"x","routes":[{"path":"/a","body":{"type":"inline","text":"hi"}}]}"#)
                .unwrap();
        assert!(s.enabled && s.routes[0].enabled && s.routes[0].status == 200);
        assert_eq!(s.routes[0].body, MockBody::Inline { text: "hi".into() });
    }
}
