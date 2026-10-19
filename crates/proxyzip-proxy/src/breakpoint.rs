//! Breakpoints: pause a flow, let the user edit it, then continue or abort.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use proxyzip_core::FlowId;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// A request as edited by the user at a breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEdit {
    pub method: String,
    /// Full URL, e.g. `https://api.example.com/v1/items?id=2`.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Body bytes, base64-encoded.
    pub body_base64: String,
}

/// A response as edited by the user at a breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEdit {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Body bytes, base64-encoded.
    pub body_base64: String,
}

/// What the user decided at a breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Decision {
    /// Continue unchanged.
    Continue,
    EditRequest(RequestEdit),
    EditResponse(ResponseEdit),
    /// Close the client connection without answering.
    Abort,
}

/// Tracks the flows that are currently paused.
#[derive(Default)]
pub struct BreakpointHub {
    pending: Mutex<HashMap<FlowId, oneshot::Sender<Decision>>>,
}

struct Cleanup {
    hub: Arc<BreakpointHub>,
    id: FlowId,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        // The client may disconnect while paused; forget the pause then.
        self.hub.pending.lock().remove(&self.id);
    }
}

impl BreakpointHub {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Wait until [`resolve`](Self::resolve) or [`release_all`](Self::release_all) is called for `id`.
    pub(crate) async fn pause(self: &Arc<Self>, id: FlowId) -> Decision {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let _cleanup = Cleanup { hub: self.clone(), id };
        rx.await.unwrap_or(Decision::Continue)
    }

    /// Answer a paused flow. Returns false when nothing was waiting (already resolved or disconnected).
    pub fn resolve(&self, id: FlowId, decision: Decision) -> bool {
        self.pending.lock().remove(&id).is_some_and(|tx| tx.send(decision).is_ok())
    }

    /// Let every paused flow continue unchanged.
    pub fn release_all(&self) {
        let all: Vec<_> = self.pending.lock().drain().collect();
        for (_, tx) in all {
            let _ = tx.send(Decision::Continue);
        }
    }

    pub fn paused(&self) -> Vec<FlowId> {
        let mut ids: Vec<_> = self.pending.lock().keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_wakes_the_waiter() {
        let hub = BreakpointHub::new();
        let waiter = tokio::spawn({
            let hub = hub.clone();
            async move { hub.pause(7).await }
        });
        while hub.paused().is_empty() {
            tokio::task::yield_now().await;
        }
        assert_eq!(hub.paused(), vec![7]);
        assert!(hub.resolve(7, Decision::Abort));
        assert!(matches!(waiter.await.unwrap(), Decision::Abort));
        assert!(!hub.resolve(7, Decision::Continue), "second resolve finds nothing");
        assert!(hub.paused().is_empty());
    }

    #[tokio::test]
    async fn release_all_continues_everything() {
        let hub = BreakpointHub::new();
        let waiters: Vec<_> = (1..=3)
            .map(|id| {
                let hub = hub.clone();
                tokio::spawn(async move { hub.pause(id).await })
            })
            .collect();
        while hub.paused().len() < 3 {
            tokio::task::yield_now().await;
        }
        hub.release_all();
        for w in waiters {
            assert!(matches!(w.await.unwrap(), Decision::Continue));
        }
    }

    #[tokio::test]
    async fn dropping_the_waiter_forgets_the_pause() {
        let hub = BreakpointHub::new();
        let waiter = tokio::spawn({
            let hub = hub.clone();
            async move { hub.pause(1).await }
        });
        while hub.paused().is_empty() {
            tokio::task::yield_now().await;
        }
        waiter.abort();
        let _ = waiter.await;
        assert!(hub.paused().is_empty());
    }
}
