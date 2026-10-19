//! The ProxyZip proxy engine.
//!
//! ```text
//! client ──HTTP/CONNECT──▶ listener ──▶ (TLS MITM | raw tunnel) ──▶ hyper service
//!                                                        │
//!                              FlowStore ◀── record ─────┤
//!                                                        ▼
//!                                              upstream (hyper + rustls)
//! ```

mod config;
mod forward;
mod local;
mod rewind;
mod server;
mod tunnel;
mod upstream;
mod util;
mod websocket;

pub use config::{ProxyConfig, ProxySettings, SslProxyingPolicy, wildcard_match};
pub use local::CERT_HOST;
pub use server::{ProxyHandle, ProxyServer};
