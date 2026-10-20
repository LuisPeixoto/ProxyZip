//! Forwards a stdio MCP session to a ProxyZip that is already running (its HTTP endpoint on localhost).

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::discovery::Endpoint;

/// POST `body` to `endpoint` and return `(status, response body)`. The endpoint is a plain-HTTP loopback URL.
pub async fn post(endpoint: &Endpoint, body: &str) -> Result<(u16, String)> {
    let rest = endpoint.url.strip_prefix("http://").context("only http:// endpoints are supported")?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let mut stream = TcpStream::connect(authority).await.with_context(|| format!("connecting to {authority}"))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        endpoint.token,
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw);
    let (head, payload) = text.split_once("\r\n\r\n").context("malformed HTTP response")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).context("malformed status line")?;
    // `Connection: close` with a body that may be chunked: ProxyZip answers with content-length, so the rest is the body.
    Ok((status, payload.to_string()))
}

fn rpc_error(request: &str, message: &str) -> Option<String> {
    let id = serde_json::from_str::<Value>(request).ok()?.get("id").cloned()?;
    Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } }).to_string())
}

pub async fn run(endpoint: Endpoint) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match post(&endpoint, &line).await {
            Ok((202, _)) => None,
            Ok((200, body)) => Some(body.trim().to_string()),
            Ok((401, _)) => rpc_error(
                &line,
                "ProxyZip refused the access token. Open Settings > AI assistants and copy the setup again.",
            ),
            Ok((status, _)) => rpc_error(&line, &format!("ProxyZip answered HTTP {status}")),
            Err(e) => rpc_error(
                &line,
                &format!(
                    "could not reach ProxyZip ({e:#}). Is the app running with the AI assistants server turned on?"
                ),
            ),
        };
        if let Some(r) = reply.filter(|r| !r.is_empty()) {
            out.write_all(r.as_bytes()).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

pub fn require_endpoint(home: &std::path::Path) -> Result<Endpoint> {
    match crate::discovery::read(home) {
        Some(e) => Ok(e),
        None => bail!(
            "no running ProxyZip with the AI assistants server turned on was found (looked for {})",
            crate::discovery::endpoint_file(home).display()
        ),
    }
}
