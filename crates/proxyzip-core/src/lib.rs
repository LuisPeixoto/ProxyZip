//! Core data model shared by every ProxyZip component.
//!
//! A [`Flow`] is one request/response exchange (plus WebSocket messages when the
//! connection was upgraded). Flows live in a [`FlowStore`], which broadcasts
//! [`FlowEvent`]s so UIs and exporters can follow traffic live.

mod filter;
mod flow;
mod store;

pub use filter::Filter;
pub use flow::*;
pub use store::{FlowEvent, FlowStore};

/// Per-user directory holding the CA, settings and saved sessions
/// (shared by the CLI and the desktop app). `PROXYZIP_HOME` overrides it.
pub fn data_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("PROXYZIP_HOME") {
        return dir.into();
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("ProxyZip")
}
