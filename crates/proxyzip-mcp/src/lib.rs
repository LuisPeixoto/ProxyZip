//! A Model Context Protocol server for ProxyZip.
//!
//! It speaks JSON-RPC 2.0 over stdio ([`stdio::run`]) or streamable HTTP ([`http::serve`]) and exposes the
//! captured traffic as tools and resources, so an AI assistant can list, search and explain requests.
//! Tools that change state (rules, clearing the list) only work when the user allowed changes.

pub mod bridge;
pub mod discovery;
pub mod http;
mod protocol;
pub mod stdio;
mod tools;

pub use protocol::{McpServer, McpState, RulesBackend, SUPPORTED_VERSIONS};
