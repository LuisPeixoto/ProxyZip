use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::{BreakpointPhase, Flow, FlowId, FlowSummary, Request, WsMessage};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowEvent {
    Created {
        flow: FlowSummary,
    },
    Updated {
        flow: FlowSummary,
    },
    WsMessage {
        id: FlowId,
        message: WsMessage,
    },
    /// A flow stopped at a breakpoint and needs a decision.
    BreakpointHit {
        id: FlowId,
        phase: BreakpointPhase,
    },
    Removed {
        ids: Vec<FlowId>,
    },
    Cleared,
}

struct Inner {
    flows: RwLock<BTreeMap<FlowId, Flow>>,
    next_id: AtomicU64,
    events: broadcast::Sender<FlowEvent>,
    max_flows: usize,
}

/// Thread-safe, in-memory store of captured flows. Cheap to clone.
#[derive(Clone)]
pub struct FlowStore {
    inner: Arc<Inner>,
}

impl Default for FlowStore {
    fn default() -> Self {
        Self::new(50_000)
    }
}

impl FlowStore {
    /// `max_flows` bounds memory: the oldest unpinned flows are evicted first.
    pub fn new(max_flows: usize) -> Self {
        let (events, _) = broadcast::channel(16_384);
        Self {
            inner: Arc::new(Inner {
                flows: RwLock::new(BTreeMap::new()),
                next_id: AtomicU64::new(1),
                events,
                max_flows: max_flows.max(1),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FlowEvent> {
        self.inner.events.subscribe()
    }

    fn emit(&self, event: FlowEvent) {
        // No subscribers is fine.
        let _ = self.inner.events.send(event);
    }

    pub fn create(&self, client_addr: String, request: Request) -> FlowId {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.insert(Flow::new(id, client_addr, request));
        id
    }

    /// Insert an already-built flow (used by importers). Re-assigns the id.
    pub fn import(&self, mut flow: Flow) -> FlowId {
        flow.id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let id = flow.id;
        self.insert(flow);
        id
    }

    fn insert(&self, flow: Flow) {
        let summary = flow.summary();
        let evicted = {
            let mut flows = self.inner.flows.write();
            flows.insert(flow.id, flow);
            let mut evicted = Vec::new();
            while flows.len() > self.inner.max_flows {
                let victim = flows.iter().find(|(_, f)| !f.pinned).map(|(id, _)| *id);
                match victim {
                    Some(v) => {
                        flows.remove(&v);
                        evicted.push(v);
                    }
                    None => break,
                }
            }
            evicted
        };
        self.emit(FlowEvent::Created { flow: summary });
        if !evicted.is_empty() {
            self.emit(FlowEvent::Removed { ids: evicted });
        }
    }

    /// Mutate a flow in place and broadcast the new summary.
    pub fn update<R>(&self, id: FlowId, f: impl FnOnce(&mut Flow) -> R) -> Option<R> {
        let (result, summary) = {
            let mut flows = self.inner.flows.write();
            let flow = flows.get_mut(&id)?;
            let r = f(flow);
            (r, flow.summary())
        };
        self.emit(FlowEvent::Updated { flow: summary });
        Some(result)
    }

    pub fn notify_breakpoint(&self, id: FlowId, phase: BreakpointPhase) {
        self.emit(FlowEvent::BreakpointHit { id, phase });
    }

    pub fn push_ws_message(&self, id: FlowId, message: WsMessage) {
        let ok = {
            let mut flows = self.inner.flows.write();
            match flows.get_mut(&id) {
                Some(flow) => {
                    flow.ws_messages.push(message.clone());
                    true
                }
                None => false,
            }
        };
        if ok {
            self.emit(FlowEvent::WsMessage { id, message });
        }
    }

    pub fn get(&self, id: FlowId) -> Option<Flow> {
        self.inner.flows.read().get(&id).cloned()
    }

    pub fn summaries(&self) -> Vec<FlowSummary> {
        self.inner.flows.read().values().map(Flow::summary).collect()
    }

    pub fn all(&self) -> Vec<Flow> {
        self.inner.flows.read().values().cloned().collect()
    }

    pub fn get_many(&self, ids: &[FlowId]) -> Vec<Flow> {
        let flows = self.inner.flows.read();
        ids.iter().filter_map(|id| flows.get(id).cloned()).collect()
    }

    pub fn remove(&self, ids: &[FlowId]) {
        {
            let mut flows = self.inner.flows.write();
            for id in ids {
                flows.remove(id);
            }
        }
        self.emit(FlowEvent::Removed { ids: ids.to_vec() });
    }

    pub fn clear(&self) {
        self.inner.flows.write().clear();
        self.emit(FlowEvent::Cleared);
    }

    pub fn len(&self) -> usize {
        self.inner.flows.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Headers, Scheme};

    fn req() -> Request {
        Request {
            method: "GET".into(),
            scheme: Scheme::Https,
            host: "example.com".into(),
            port: 443,
            path: "/a?b=1".into(),
            http_version: "HTTP/1.1".into(),
            headers: Headers::default(),
            body: Default::default(),
            body_truncated: false,
        }
    }

    #[test]
    fn create_update_and_evict() {
        let store = FlowStore::new(2);
        let mut rx = store.subscribe();
        let a = store.create("127.0.0.1:1".into(), req());
        store.update(a, |f| f.pinned = true);
        let b = store.create("127.0.0.1:1".into(), req());
        let c = store.create("127.0.0.1:1".into(), req());
        assert_eq!(store.len(), 2);
        assert!(store.get(a).is_some(), "pinned flow survives eviction");
        assert!(store.get(b).is_none());
        assert!(store.get(c).is_some());
        assert!(matches!(rx.try_recv().unwrap(), FlowEvent::Created { .. }));
        assert_eq!(store.get(a).unwrap().request.url(), "https://example.com/a?b=1");
    }
}
