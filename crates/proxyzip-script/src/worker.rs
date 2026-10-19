//! A worker owns one QuickJS runtime. QuickJS values are not `Send`, so each worker lives on its own thread.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as B64, STANDARD_NO_PAD as B64_NO_PAD};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use proxyzip_core::LogLine;
use rquickjs::{Context, Ctx, Exception, Function, Runtime};
use sha1::Sha1;
use sha2::Sha256;

const PRELUDE: &str = include_str!("prelude.js");

/// Everything a worker needs to run one script call.
pub(crate) struct RunSpec {
    pub id: String,
    pub source: String,
    pub phase: &'static str,
    pub shared_json: String,
    pub request_json: String,
    pub response_json: String,
}

/// What came out of one script run.
pub(crate) struct Report {
    pub result: Result<String, String>,
    pub logs: Vec<LogLine>,
    /// The runtime hit a limit and should be rebuilt before it is used again.
    pub poisoned: bool,
}

pub(crate) struct Worker {
    _runtime: Runtime,
    context: Context,
    loaded: HashSet<String>,
    logs: Arc<Mutex<Vec<LogLine>>>,
    deadline_ms: Arc<AtomicU64>,
    timed_out: Arc<AtomicBool>,
    epoch: Instant,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

impl Worker {
    pub(crate) fn new(memory_bytes: usize) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(memory_bytes);
        runtime.set_max_stack_size(1024 * 1024);
        let epoch = Instant::now();
        let deadline_ms = Arc::new(AtomicU64::new(u64::MAX));
        let timed_out = Arc::new(AtomicBool::new(false));
        {
            let (deadline, flag) = (deadline_ms.clone(), timed_out.clone());
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let expired = epoch.elapsed().as_millis() as u64 > deadline.load(Ordering::Relaxed);
                if expired {
                    flag.store(true, Ordering::Relaxed);
                }
                expired
            })));
        }
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let logs = Arc::new(Mutex::new(Vec::new()));
        let sink = logs.clone();
        context
            .with(|ctx| -> rquickjs::Result<()> {
                let globals = ctx.globals();
                globals.set(
                    "__log",
                    Function::new(ctx.clone(), move |level: String, message: String| {
                        if let Ok(mut lines) = sink.lock() {
                            // A script that logs in a loop must not be able to fill memory.
                            if lines.len() < 500 {
                                lines.push(LogLine { level, message, timestamp: now_ms() });
                            }
                        }
                    })?,
                )?;
                globals.set("__md5", Function::new(ctx.clone(), |s: String| hex(Md5::digest(s.as_bytes())))?)?;
                globals.set("__sha1", Function::new(ctx.clone(), |s: String| hex(Sha1::digest(s.as_bytes())))?)?;
                globals.set("__sha256", Function::new(ctx.clone(), |s: String| hex(Sha256::digest(s.as_bytes())))?)?;
                globals.set(
                    "__hmac256",
                    Function::new(ctx.clone(), |key: String, message: String| {
                        let mut mac =
                            Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
                        mac.update(message.as_bytes());
                        hex(mac.finalize().into_bytes())
                    })?,
                )?;
                globals.set("__uuid", Function::new(ctx.clone(), || uuid::Uuid::new_v4().to_string())?)?;
                globals.set("__b64enc", Function::new(ctx.clone(), |s: String| B64.encode(s.as_bytes()))?)?;
                globals.set(
                    "__b64dec",
                    Function::new(ctx.clone(), |s: String| {
                        let trimmed = s.trim().trim_end_matches('=');
                        B64_NO_PAD.decode(trimmed).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
                    })?,
                )?;
                globals.set(
                    "__sleep",
                    Function::new(ctx.clone(), |ms: f64| {
                        std::thread::sleep(Duration::from_millis(ms.clamp(0.0, 1000.0) as u64));
                    })?,
                )?;
                ctx.eval::<(), _>(PRELUDE)?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { _runtime: runtime, context, loaded: HashSet::new(), logs, deadline_ms, timed_out, epoch })
    }

    pub(crate) fn run(&mut self, spec: &RunSpec, timeout: Duration) -> Report {
        let RunSpec { id, source, phase, shared_json, request_json, response_json } = spec;
        if let Ok(mut lines) = self.logs.lock() {
            lines.clear();
        }
        self.timed_out.store(false, Ordering::Relaxed);
        let deadline = self.epoch.elapsed() + timeout;
        self.deadline_ms.store(deadline.as_millis() as u64, Ordering::Relaxed);

        let already_loaded = self.loaded.contains(id.as_str());
        let mut registered = already_loaded;
        let result = self.context.with(|ctx| -> Result<String, String> {
            if !already_loaded {
                // The source is wrapped in a function so each script has its own scope and globals do not leak.
                let code = format!(
                    "__register({id:?}, function () {{\n{source}\n;return {{ onRequest: typeof onRequest === 'function' ? onRequest : undefined, onResponse: typeof onResponse === 'function' ? onResponse : undefined }};\n}});"
                );
                ctx.eval::<(), _>(code).map_err(|e| describe(&ctx, e))?;
                registered = true;
            }
            let run: Function = ctx.globals().get("__run").map_err(|e| describe(&ctx, e))?;
            run.call::<_, String>((id.as_str(), *phase, shared_json.as_str(), request_json.as_str(), response_json.as_str())).map_err(|e| describe(&ctx, e))
        });
        self.deadline_ms.store(u64::MAX, Ordering::Relaxed);
        if registered {
            self.loaded.insert(id.clone());
        }

        let timed_out = self.timed_out.load(Ordering::Relaxed);
        let result = if timed_out {
            Err(format!("the script ran for more than {} ms and was stopped", timeout.as_millis()))
        } else {
            result
        };
        let out_of_memory = result.as_ref().err().is_some_and(|e| e.to_ascii_lowercase().contains("out of memory"));
        let logs = self.logs.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default();
        Report { result, logs, poisoned: timed_out || out_of_memory }
    }
}

/// A readable message for a JavaScript exception.
fn describe(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let thrown = ctx.catch();
    if let Some(object) = thrown.as_object()
        && let Some(exception) = Exception::from_object(object.clone())
    {
        let message = exception.message().unwrap_or_else(|| "script error".into());
        let location = exception
            .stack()
            .and_then(|s| s.lines().find(|l| l.trim_start().starts_with("at ")).map(|l| l.trim().to_string()))
            .map(|l| format!(" ({l})"))
            .unwrap_or_default();
        return format!("{message}{location}");
    }
    if let Some(text) = thrown.as_string().and_then(|s| s.to_string().ok()) {
        return text;
    }
    "the script threw a value that is not an error".into()
}
