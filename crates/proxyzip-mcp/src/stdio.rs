//! stdio transport: one JSON-RPC message per line on stdin, one response per line on stdout.

use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::McpServer;

pub async fn run(server: Arc<McpServer>) -> Result<()> {
    run_with(server, tokio::io::stdin(), tokio::io::stdout()).await
}

pub async fn run_with<R, W>(server: Arc<McpServer>, input: R, mut output: W) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(input).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(message) => server.handle(message).await,
            Err(_) => Some(
                serde_json::json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }),
            ),
        };
        if let Some(r) = response {
            output.write_all(serde_json::to_string(&r)?.as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}
