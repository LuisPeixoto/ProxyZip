//! The filter language of the request list, evaluated in Rust so the desktop app, the MCP server and
//! other front-ends agree on what a query means. The user-facing description is `docs/filtering.md`.

use regex::{Regex, RegexBuilder};

use crate::FlowSummary;

enum Term {
    Method(String),
    StatusClass(u16),
    StatusExact(String),
    Host(String),
    Type(String),
    Path(String),
    Graphql(String),
    Regex(Option<Regex>),
    /// A `key:value` with an unknown key: match the whole token against the URL.
    UrlContains(String),
    /// Plain text: the URL or the GraphQL operation contains it.
    Text(String),
}

struct Clause {
    negate: bool,
    term: Term,
}

/// A parsed query. All clauses must match (AND).
pub struct Filter {
    clauses: Vec<Clause>,
}

impl Filter {
    pub fn parse(query: &str) -> Filter {
        let clauses = query.split_whitespace().map(parse_clause).collect();
        Filter { clauses }
    }

    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    pub fn matches(&self, f: &FlowSummary) -> bool {
        self.clauses.iter().all(|c| term_matches(&c.term, f) != c.negate)
    }
}

fn parse_clause(raw: &str) -> Clause {
    let negate = raw.starts_with('-') && raw.len() > 1;
    let tok = if negate { &raw[1..] } else { raw };
    Clause { negate, term: parse_term(tok) }
}

fn parse_term(tok: &str) -> Term {
    if tok.len() > 2 && tok.starts_with('/') && tok.ends_with('/') {
        return Term::Regex(RegexBuilder::new(&tok[1..tok.len() - 1]).case_insensitive(true).build().ok());
    }
    if let Some((key, value)) = split_key(tok) {
        let v = value.to_lowercase();
        return match key.to_lowercase().as_str() {
            "method" => Term::Method(v),
            "status" => match v.as_bytes() {
                [d @ b'0'..=b'9', b'x', b'x'] => Term::StatusClass((d - b'0') as u16),
                _ => Term::StatusExact(v),
            },
            "host" | "domain" => Term::Host(v),
            "type" => Term::Type(v),
            "path" => Term::Path(v),
            "gql" | "graphql" => Term::Graphql(v),
            _ => Term::UrlContains(tok.to_lowercase()),
        };
    }
    Term::Text(tok.to_lowercase())
}

/// `key:value` where the key is word characters only.
fn split_key(tok: &str) -> Option<(&str, &str)> {
    let (key, value) = tok.split_once(':')?;
    (!key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some((key, value))
}

fn term_matches(term: &Term, f: &FlowSummary) -> bool {
    let lower = |s: &str| s.to_lowercase();
    match term {
        Term::Method(v) => lower(&f.method) == *v,
        Term::StatusClass(c) => f.status.is_some_and(|s| s / 100 == *c),
        Term::StatusExact(v) => f.status.map(|s| s.to_string()).unwrap_or_default() == *v,
        Term::Host(v) => lower(&f.host).contains(v),
        Term::Type(v) => lower(f.content_type.as_deref().unwrap_or("")).contains(v),
        Term::Path(v) => lower(&f.path).contains(v),
        Term::Graphql(v) => lower(f.graphql.as_deref().unwrap_or("")).contains(v),
        Term::Regex(Some(re)) => re.is_match(&f.url),
        Term::Regex(None) => true,
        Term::UrlContains(v) => lower(&f.url).contains(v),
        Term::Text(v) => lower(&f.url).contains(v) || lower(f.graphql.as_deref().unwrap_or("")).contains(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FlowState, Scheme};

    fn flow(
        method: &str,
        url_host: &str,
        path: &str,
        status: Option<u16>,
        ct: Option<&str>,
        gql: Option<&str>,
    ) -> FlowSummary {
        FlowSummary {
            id: 1,
            created_at: 0,
            method: method.into(),
            scheme: Scheme::Https,
            host: url_host.into(),
            port: 443,
            path: path.into(),
            url: format!("https://{url_host}{path}"),
            http_version: "HTTP/1.1".into(),
            status,
            content_type: ct.map(Into::into),
            request_size: 0,
            response_size: None,
            duration_ms: None,
            state: FlowState::Completed,
            error: None,
            client_addr: "127.0.0.1:1".into(),
            ws_message_count: 0,
            pinned: false,
            has_comment: false,
            tags: vec![],
            graphql: gql.map(Into::into),
            has_logs: false,
        }
    }

    fn check(q: &str, f: &FlowSummary) -> bool {
        Filter::parse(q).matches(f)
    }

    // These cases mirror docs/filtering.md and the TypeScript implementation in the desktop app.
    #[test]
    fn documented_examples() {
        let login = flow("POST", "api.example.com", "/v2/login", Some(404), Some("application/json"), None);
        let page = flow("GET", "analytics.io", "/index.html", Some(200), Some("text/html"), None);
        let gql =
            flow("POST", "api.example.com", "/graphql", Some(200), Some("application/json"), Some("query GetUser"));

        assert!(check("login", &login));
        assert!(!check("login", &page));
        assert!(check("method:post", &login) && check("method:POST", &login));
        assert!(!check("method:get", &login));
        assert!(check("status:4xx", &login) && check("status:404", &login));
        assert!(!check("status:5xx", &login) && !check("status:200", &login));
        assert!(check("host:api", &login) && check("domain:API", &login) && !check("host:api", &page));
        assert!(check("type:json", &login) && !check("type:json", &page));
        assert!(check("path:/v2/", &login));
        assert!(check("-analytics", &login) && !check("-analytics", &page));
        assert!(check("-method:get", &login));
        assert!(check(r"/\/v\d+\/users?|login$/", &login));
        assert!(check("host:api method:post", &login));
        assert!(check("status:4xx -host:google", &login));
        assert!(check("gql:getuser", &gql) && check("graphql:GetUser", &gql) && !check("gql:getuser", &login));
        assert!(check("GetUser", &gql), "plain text also looks at the GraphQL operation");
    }

    #[test]
    fn edge_cases_match_the_desktop_filter() {
        let f = flow("GET", "a.test", "/x?y=z:w", None, None, None);
        assert!(Filter::parse("").matches(&f) && Filter::parse("   ").is_empty());
        assert!(!check("foo:bar", &f), "unknown prefix is plain text against the URL");
        assert!(check("y=z:w", &f), "a colon inside a value is fine");
        assert!(check("/[unclosed/", &f), "an invalid regular expression matches everything");
        assert!(!check("-", &f), "a lone dash is text, not a negation");
        assert!(!check("status:200", &f), "no status yet");
    }
}
