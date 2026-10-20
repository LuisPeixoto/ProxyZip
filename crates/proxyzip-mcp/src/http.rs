//! Streamable HTTP transport: `POST /mcp` with a JSON-RPC message, protected by a bearer token.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::McpServer;

const MAX_BODY: usize = 4 * 1024 * 1024;

pub struct HttpHandle {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
}

impl HttpHandle {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

fn reply(status: StatusCode, body: impl Into<Bytes>, json: bool) -> Response<Full<Bytes>> {
    let mut b = Response::builder().status(status);
    if json {
        b = b.header("content-type", "application/json");
    }
    b.body(Full::new(body.into())).expect("valid response")
}

/// Browsers send an `Origin`; refuse pages that are not local, so a website cannot talk to the server (DNS rebinding).
fn origin_allowed(origin: &str) -> bool {
    let rest = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    match rest {
        Some(host) => {
            let host = host.split('/').next().unwrap_or("");
            let name = if host.starts_with('[') {
                host.split(']').next().unwrap_or("").trim_start_matches('[')
            } else {
                host.split(':').next().unwrap_or("")
            };
            matches!(name, "localhost" | "127.0.0.1" | "::1")
        }
        None => origin.starts_with("tauri://"),
    }
}

fn authorized(req: &Request<Incoming>, token: &str) -> bool {
    let Some(value) = req.headers().get("authorization").and_then(|v| v.to_str().ok()) else { return false };
    let given = value.strip_prefix("Bearer ").unwrap_or("").trim();
    // Compare every byte so the time taken does not reveal how much of a guess was right.
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

async fn route(req: Request<Incoming>, server: Arc<McpServer>, token: Arc<str>) -> Response<Full<Bytes>> {
    if req.uri().path() != "/mcp" {
        return reply(StatusCode::NOT_FOUND, "not found", false);
    }
    if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok())
        && !origin_allowed(origin)
    {
        return reply(StatusCode::FORBIDDEN, "origin not allowed", false);
    }
    if !authorized(&req, &token) {
        let mut r = reply(StatusCode::UNAUTHORIZED, "missing or wrong bearer token", false);
        r.headers_mut().insert("www-authenticate", "Bearer".parse().expect("static header"));
        return r;
    }
    if req.method() != Method::POST {
        let mut r = reply(StatusCode::METHOD_NOT_ALLOWED, "use POST", false);
        r.headers_mut().insert("allow", "POST".parse().expect("static header"));
        return r;
    }
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return reply(StatusCode::PAYLOAD_TOO_LARGE, "request too large", false),
    };
    let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return reply(
            StatusCode::BAD_REQUEST,
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#,
            true,
        );
    };
    match server.handle(message).await {
        Some(response) => reply(StatusCode::OK, serde_json::to_vec(&response).unwrap_or_default(), true),
        None => reply(StatusCode::ACCEPTED, "", false),
    }
}

/// Listen on `addr` (use port 0 for any). Only loopback addresses are accepted.
pub async fn serve(addr: SocketAddr, server: Arc<McpServer>, token: String) -> Result<HttpHandle> {
    anyhow::ensure!(addr.ip().is_loopback(), "the MCP server only listens on loopback addresses");
    let listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let (tx, mut rx) = oneshot::channel::<()>();
    let token: Arc<str> = token.into();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let (server, token) = (server.clone(), token.clone());
                    tokio::spawn(async move {
                        let service = service_fn(move |req| {
                            let (server, token) = (server.clone(), token.clone());
                            async move { Ok::<_, std::convert::Infallible>(route(req, server, token).await) }
                        });
                        let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                    });
                }
            }
        }
    });
    Ok(HttpHandle { addr, shutdown: Some(tx) })
}

#[cfg(test)]
mod tests {
    use super::origin_allowed;

    #[test]
    fn only_local_origins_pass() {
        assert!(origin_allowed("http://localhost:3000"));
        assert!(origin_allowed("http://127.0.0.1"));
        assert!(origin_allowed("http://[::1]:8080"));
        assert!(origin_allowed("tauri://localhost"));
        assert!(!origin_allowed("https://evil.example"));
        assert!(!origin_allowed("http://localhost.evil.example"));
        assert!(!origin_allowed("null"));
    }
}
