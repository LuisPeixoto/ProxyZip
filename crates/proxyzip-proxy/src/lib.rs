//! The ProxyZip proxy engine.
//!
//! ```text
//! client ──HTTP/CONNECT──▶ listener ──▶ (TLS MITM | raw tunnel) ──▶ hyper service
//!                                                        │
//!                              FlowStore ◀── record ─────┤
//!                                                        ▼
//!                                              upstream (hyper + rustls)
//! ```

mod breakpoint;
mod config;
mod forward;
mod local;
mod rewind;
mod server;
mod socks;
mod throttle;
mod tunnel;
mod upstream;
mod util;
mod websocket;

pub use breakpoint::{BreakpointHub, Decision, RequestEdit, ResponseEdit};
pub use config::{ProxyConfig, ProxySettings, SslProxyingPolicy};
pub use local::CERT_HOST;
pub use proxyzip_rules::wildcard_match;
pub use server::{ProxyHandle, ProxyServer};
