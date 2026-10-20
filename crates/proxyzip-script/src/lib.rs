//! JavaScript scripting for ProxyZip.
//!
//! Scripts define `onRequest(ctx, request)` and/or `onResponse(ctx, request, response)`. They run in QuickJS, each
//! worker thread owning its own runtime, with a time limit, a memory limit and no access to the file system or the
//! network. A failing script never breaks traffic: the caller gets an error message and carries on without the edit.
//!
//! See `docs/scripting.md` for the script API.

mod convert;
mod engine;
pub mod plugin;
mod worker;

pub use convert::{ScriptRequest, ScriptResponse};
pub use engine::{Limits, PluginEntry, RunOutput, ScriptEngine, Verdict};
