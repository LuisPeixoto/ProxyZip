use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use proxyzip_core::Flow;

/// Quote for POSIX shells: wrap in single quotes, escaping embedded ones.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

const SKIP: &[&str] = &["host", "content-length", "connection", "proxy-connection", "accept-encoding"];

/// Render the flow's request as a reproducible `curl` command.
pub fn to_curl(flow: &Flow) -> String {
    let req = &flow.request;
    let mut parts = vec!["curl".to_string()];
    // curl infers GET (no body) and POST (with body); spell out everything else.
    let implied = (req.method == "GET" && req.body.is_empty()) || (req.method == "POST" && !req.body.is_empty());
    if !implied {
        parts.push(format!("-X {}", req.method));
    }
    parts.push(sh_quote(&req.url()));
    let mut compressed = false;
    for (k, v) in req.headers.iter() {
        let lk = k.to_ascii_lowercase();
        if lk == "accept-encoding" {
            compressed = true;
        }
        if SKIP.contains(&lk.as_str()) || lk.starts_with(':') {
            continue;
        }
        parts.push(format!("-H {}", sh_quote(&format!("{k}: {v}"))));
    }
    if compressed {
        parts.push("--compressed".into());
    }
    if !req.body.is_empty() {
        match std::str::from_utf8(&req.body) {
            Ok(text) => parts.push(format!("--data-binary {}", sh_quote(text))),
            Err(_) => parts.push("--data-binary @request-body.bin".into()),
        }
    }
    if req.http_version == "HTTP/2" {
        parts.push("--http2".into());
    }
    parts.join(" \\\n  ")
}

/// A request described by a cURL command, ready to be edited and sent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComposedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Split a command line the way a POSIX shell does: quotes, escapes, `$'...'` and line continuations.
fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') => {
                    chars.next_if_eq(&'\n');
                }
                Some(next) => {
                    current.push(next);
                    in_token = true;
                }
                None => {}
            },
            '\'' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => current.push(ch),
                        None => return Err("unterminated single quote".into()),
                    }
                }
            }
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => current.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                current.push('\\');
                                current.push(ch);
                            }
                            None => return Err("unterminated double quote".into()),
                        },
                        Some(ch) => current.push(ch),
                        None => return Err("unterminated double quote".into()),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => current.push('\n'),
                            Some('r') => current.push('\r'),
                            Some('t') => current.push('\t'),
                            Some('\'') => current.push('\''),
                            Some('"') => current.push('"'),
                            Some('\\') => current.push('\\'),
                            Some(ch) => {
                                current.push('\\');
                                current.push(ch);
                            }
                            None => return Err("unterminated $'...' string".into()),
                        },
                        Some(ch) => current.push(ch),
                        None => return Err("unterminated $'...' string".into()),
                    }
                }
            }
            c => {
                current.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Short options that take a value (`-X POST`, `-XPOST`).
const SHORT_WITH_VALUE: &str = "XHduAebomxwTFKcDEyYzUtrC";
/// Long options that take a value and that we do not need.
const LONG_WITH_VALUE: &[&str] = &[
    "output",
    "max-time",
    "connect-timeout",
    "retry",
    "retry-delay",
    "retry-max-time",
    "proxy",
    "cacert",
    "capath",
    "cert",
    "key",
    "write-out",
    "upload-file",
    "resolve",
    "connect-to",
    "max-redirs",
    "config",
    "cookie-jar",
    "dump-header",
    "limit-rate",
    "proxy-user",
    "interface",
    "form",
    "form-string",
    "user-agent-extra",
    "tlsv1.2",
    "ciphers",
    "oauth2-bearer",
    "unix-socket",
    "range",
    "continue-at",
    "trace",
    "trace-ascii",
    "stderr",
];

fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Parse a cURL command (for example one copied from browser dev tools) into a request.
///
/// Understands the options that describe a request: `-X`, `-H`, `-d` and friends, `--json`, `-u`, `-A`, `-e`, `-b`,
/// `--compressed`, `-I`, `-G` and `--url`. Options that only affect how curl behaves (`-s`, `-k`, `-L`, …) are ignored.
pub fn parse(command: &str) -> Result<ComposedRequest, String> {
    let tokens = tokenize(command)?;
    let mut tokens = tokens.into_iter().peekable();
    if tokens.peek().is_some_and(|t| t == "curl") {
        tokens.next();
    }
    let mut request = ComposedRequest::default();
    let mut explicit_method: Option<String> = None;
    let mut data: Vec<String> = Vec::new();
    let mut get_mode = false;
    let mut head = false;
    let mut url: Option<String> = None;

    let header = |request: &mut ComposedRequest, name: &str, value: &str| {
        request.headers.push((name.to_string(), value.to_string()));
    };

    while let Some(token) = tokens.next() {
        if token == "--" {
            continue;
        }
        if let Some(long) = token.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let value = |tokens: &mut std::iter::Peekable<std::vec::IntoIter<String>>| {
                inline.clone().or_else(|| tokens.next()).ok_or_else(|| format!("--{name} needs a value"))
            };
            match name {
                "request" => explicit_method = Some(value(&mut tokens)?),
                "header" => {
                    let v = value(&mut tokens)?;
                    if let Some((k, val)) = v.split_once(':') {
                        header(&mut request, k.trim(), val.trim());
                    }
                }
                "data" | "data-raw" | "data-binary" | "data-ascii" => data.push(value(&mut tokens)?),
                "data-urlencode" => {
                    let v = value(&mut tokens)?;
                    data.push(match v.split_once('=') {
                        Some((k, val)) if !k.is_empty() => format!("{k}={}", urlencode(val)),
                        Some((_, val)) => urlencode(val),
                        None => urlencode(&v),
                    });
                }
                "json" => {
                    data.push(value(&mut tokens)?);
                    header(&mut request, "Content-Type", "application/json");
                    header(&mut request, "Accept", "application/json");
                }
                "user" => {
                    let v = value(&mut tokens)?;
                    header(&mut request, "Authorization", &format!("Basic {}", B64.encode(v)));
                }
                "user-agent" => header(&mut request, "User-Agent", &value(&mut tokens)?),
                "referer" => header(&mut request, "Referer", &value(&mut tokens)?),
                "cookie" => header(&mut request, "Cookie", &value(&mut tokens)?),
                "url" => url = Some(value(&mut tokens)?),
                "compressed" => header(&mut request, "Accept-Encoding", "gzip, deflate, br, zstd"),
                "head" => head = true,
                "get" => get_mode = true,
                other if LONG_WITH_VALUE.contains(&other) => {
                    value(&mut tokens)?;
                }
                _ => {}
            }
            continue;
        }
        if let Some(shorts) = token.strip_prefix('-').filter(|s| !s.is_empty()) {
            for (i, flag) in shorts.char_indices() {
                if SHORT_WITH_VALUE.contains(flag) {
                    let rest = &shorts[i + flag.len_utf8()..];
                    let value = if rest.is_empty() {
                        tokens.next().ok_or_else(|| format!("-{flag} needs a value"))?
                    } else {
                        rest.to_string()
                    };
                    match flag {
                        'X' => explicit_method = Some(value),
                        'H' => {
                            if let Some((k, v)) = value.split_once(':') {
                                header(&mut request, k.trim(), v.trim());
                            }
                        }
                        'd' => data.push(value),
                        'u' => header(&mut request, "Authorization", &format!("Basic {}", B64.encode(value))),
                        'A' => header(&mut request, "User-Agent", &value),
                        'e' => header(&mut request, "Referer", &value),
                        'b' => header(&mut request, "Cookie", &value),
                        _ => {}
                    }
                    break;
                }
                match flag {
                    'I' => head = true,
                    'G' => get_mode = true,
                    _ => {}
                }
            }
            continue;
        }
        if url.is_none() {
            url = Some(token);
        }
    }

    let mut url = url.ok_or("no URL found in the command")?;
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    let body = data.join("&");
    if get_mode && !body.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&body);
    } else if !body.is_empty() {
        request.body = body.into_bytes();
        let has_type = request.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_type {
            request.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
        }
    }
    request.method = match explicit_method {
        Some(m) => m.to_ascii_uppercase(),
        None if head => "HEAD".into(),
        None if !request.body.is_empty() => "POST".into(),
        None => "GET".into(),
    };
    request.url = url;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxyzip_core::{Headers, Request, Scheme};

    #[test]
    fn golden() {
        let flow = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: "POST".into(),
                scheme: Scheme::Https,
                host: "api.example.com".into(),
                port: 443,
                path: "/v1/items?x=1".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![
                    ("Host".into(), "api.example.com".into()),
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept-Encoding".into(), "gzip".into()),
                    ("X-Note".into(), "it's".into()),
                ]),
                body: bytes::Bytes::from_static(br#"{"a":1}"#),
                body_truncated: false,
            },
        );
        assert_eq!(
            to_curl(&flow),
            "curl \\\n  'https://api.example.com/v1/items?x=1' \\\n  -H 'Content-Type: application/json' \\\n  -H 'X-Note: it'\\''s' \\\n  --compressed \\\n  --data-binary '{\"a\":1}'"
        );
    }

    fn parsed(command: &str) -> ComposedRequest {
        parse(command).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn header<'a>(r: &'a ComposedRequest, name: &str) -> Option<&'a str> {
        r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    #[test]
    fn parses_a_browser_style_command() {
        let r = parsed(
            "curl 'https://api.example.com/v1/items?x=1' \\\n  -H 'accept: application/json' \\\n  -H 'content-type: application/json' \\\n  --data-raw '{\"a\":1}' \\\n  --compressed",
        );
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://api.example.com/v1/items?x=1");
        assert_eq!(header(&r, "accept"), Some("application/json"));
        assert_eq!(header(&r, "accept-encoding"), Some("gzip, deflate, br, zstd"));
        assert_eq!(r.body, br#"{"a":1}"#);
    }

    #[test]
    fn explicit_methods_and_attached_values() {
        assert_eq!(parsed("curl -XPUT -d 'a=1' https://x.dev").method, "PUT");
        assert_eq!(parsed("curl --request delete https://x.dev/1").method, "DELETE");
        assert_eq!(parsed("curl -I https://x.dev").method, "HEAD");
        assert_eq!(parsed("curl https://x.dev").method, "GET");
        assert_eq!(parsed("curl -H'X-A: b' --header=\"X-C: d\" x.dev").headers.len(), 2);
    }

    #[test]
    fn quotes_escapes_and_ansi_c_strings() {
        let r = parsed(r#"curl -d "say \"hi\" \$HOME" https://x.dev"#);
        assert_eq!(r.body, br#"say "hi" $HOME"#);
        let r = parsed("curl -d $'line1\\nline2\\'s' https://x.dev");
        assert_eq!(String::from_utf8(r.body).unwrap(), "line1\nline2's");
        let r = parsed("curl -H 'X-Empty:' https://x.dev");
        assert_eq!(header(&r, "x-empty"), Some(""));
    }

    #[test]
    fn data_forms_and_get_mode() {
        let r = parsed("curl -d a=1 -d b=2 https://x.dev");
        assert_eq!(r.body, b"a=1&b=2");
        assert_eq!(header(&r, "content-type"), Some("application/x-www-form-urlencoded"));
        let r = parsed("curl --data-urlencode 'q=hello world' https://x.dev");
        assert_eq!(r.body, b"q=hello+world");
        let r = parsed("curl -G -d a=1 https://x.dev/s?z=9");
        assert_eq!((r.method.as_str(), r.url.as_str(), r.body.len()), ("GET", "https://x.dev/s?z=9&a=1", 0));
        let r = parsed(r#"curl --json '{"k":true}' https://x.dev"#);
        assert_eq!((r.method.as_str(), header(&r, "content-type")), ("POST", Some("application/json")));
    }

    #[test]
    fn credentials_cookies_and_agents() {
        let r = parsed("curl -u user:pass -A agent/1 -b 'a=b' -e https://ref.dev https://x.dev");
        assert_eq!(header(&r, "authorization"), Some("Basic dXNlcjpwYXNz"));
        assert_eq!(header(&r, "user-agent"), Some("agent/1"));
        assert_eq!(header(&r, "cookie"), Some("a=b"));
        assert_eq!(header(&r, "referer"), Some("https://ref.dev"));
    }

    #[test]
    fn ignores_options_that_do_not_describe_the_request() {
        let r = parsed("curl -sSLk --max-time 5 -o /dev/null --retry 3 https://x.dev/path");
        assert_eq!((r.method.as_str(), r.url.as_str()), ("GET", "https://x.dev/path"));
        assert_eq!(parsed("curl --url x.dev/a").url, "http://x.dev/a");
    }

    #[test]
    fn reports_problems() {
        assert!(parse("curl -H").is_err());
        assert!(parse("curl 'unterminated").is_err());
        assert!(parse("curl -X POST").unwrap_err().contains("no URL"));
    }

    #[test]
    fn round_trips_what_to_curl_produces() {
        let original = Flow::new(
            1,
            "127.0.0.1:1".into(),
            Request {
                method: "PUT".into(),
                scheme: Scheme::Https,
                host: "api.example.com".into(),
                port: 8443,
                path: "/v1/x?y=2".into(),
                http_version: "HTTP/1.1".into(),
                headers: Headers(vec![("X-Note".into(), "it's".into()), ("Content-Type".into(), "text/plain".into())]),
                body: bytes::Bytes::from_static(b"hello 'world'"),
                body_truncated: false,
            },
        );
        let r = parsed(&to_curl(&original));
        assert_eq!(r.method, "PUT");
        assert_eq!(r.url, "https://api.example.com:8443/v1/x?y=2");
        assert_eq!(header(&r, "x-note"), Some("it's"));
        assert_eq!(r.body, b"hello 'world'");
    }
}
