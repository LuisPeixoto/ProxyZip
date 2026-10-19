//! The ProxyZip rule engine.
//!
//! A [`RuleSet`] describes everything the user wants to *change* about traffic: which requests to block, answer from
//! disk, redirect, rewrite or pause, how to throttle them and how the network layer should behave. The engine is
//! pure: it looks at a request description and returns a plan, and the proxy performs the I/O.

mod apply;
mod engine;
mod matcher;
mod model;

pub use apply::{
    RequestParts, ResponseParts, apply_request_ops, apply_response_ops, decode_response_body, remove_header, set_header,
};
pub use engine::{Action, LocalFile, Plan, Remap, RequestInfo, content_type_for};
pub use matcher::{Matcher, wildcard_match};
pub use model::*;
