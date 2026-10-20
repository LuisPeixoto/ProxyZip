//! The AI-assistants (MCP) server that runs inside the desktop app.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use proxyzip_mcp::discovery::{self, Endpoint};
use proxyzip_mcp::{McpServer, McpState, RulesBackend};
use proxyzip_rules::RuleSet;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Rules edited by an assistant are saved and applied like rules edited in the Tools window.
struct DesktopRules(AppHandle);

impl RulesBackend for DesktopRules {
    fn get(&self) -> RuleSet {
        self.0.state::<AppState>().rules.read().clone()
    }

    fn set<'a>(
        &'a self,
        rules: RuleSet,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.0.state::<AppState>();
            let errors = state.apply_rules(rules).await.map_err(|e| format!("{e:#}"))?;
            if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
        })
    }
}

/// Start the server if the preference is on. Replaces a running one.
pub async fn restart(app: &AppHandle) {
    let state = app.state::<AppState>();
    stop(&state).await;
    let (enabled, port, allow) = {
        let p = state.prefs.read();
        (p.mcp_enabled, p.mcp_port, p.mcp_allow_changes)
    };
    if !enabled {
        return;
    }
    match start(app, port, allow).await {
        Ok(()) => *state.mcp_error.write() = None,
        Err(e) => {
            tracing::warn!(error = %e, "could not start the MCP server");
            *state.mcp_error.write() = Some(format!("{e:#}"));
        }
    }
}

async fn start(app: &AppHandle, port: u16, allow: bool) -> Result<()> {
    let state = app.state::<AppState>();
    let token = discovery::load_or_create_token(&state.home)?;
    let status_app = app.clone();
    let mcp_state = Arc::new(
        McpState::new(state.store.clone())
            .with_rules(Arc::new(DesktopRules(app.clone())))
            .with_status(move || {
                let s = status_app.state::<AppState>();
                let listen = (*s.listen.read()).map(|a| a.to_string());
                json!({ "running": listen.is_some(), "proxy": listen, "app": "ProxyZip desktop" })
            })
            .allow_changes(allow),
    );
    let server = Arc::new(McpServer::new(mcp_state.clone()));
    let http = proxyzip_mcp::http::serve(SocketAddr::from(([127, 0, 0, 1], port)), server, token.clone()).await?;
    discovery::publish(&state.home, &Endpoint { url: http.url(), token })?;
    *state.mcp_state.write() = Some(mcp_state);
    *state.mcp.lock().await = Some(http);
    Ok(())
}

pub async fn stop(state: &AppState) {
    if let Some(http) = state.mcp.lock().await.take() {
        http.shutdown();
    }
    *state.mcp_state.write() = None;
    discovery::unpublish(&state.home);
}
