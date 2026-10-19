use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::OnUpgrade;
use hyper_util::rt::TokioIo;
use proxyzip_core::{FlowId, FlowState, FlowStore, WsDirection, WsKind, WsMessage, now_ms};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

/// Relay frames between client and server, recording every message.
pub(crate) async fn relay(client: OnUpgrade, server: OnUpgrade, store: FlowStore, id: Option<FlowId>) {
    let (client, server) = match tokio::try_join!(client, server) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::debug!(error = %e, "websocket upgrade failed");
            finish(&store, id, Some(e.to_string()));
            return;
        }
    };
    let client = WebSocketStream::from_raw_socket(TokioIo::new(client), Role::Server, None).await;
    let server = WebSocketStream::from_raw_socket(TokioIo::new(server), Role::Client, None).await;
    let (mut client_tx, mut client_rx) = client.split();
    let (mut server_tx, mut server_rx) = server.split();

    let record = |direction: WsDirection, msg: &Message| {
        let Some(id) = id else { return };
        let (kind, payload) = match msg {
            Message::Text(t) => (WsKind::Text, Bytes::copy_from_slice(t.as_bytes())),
            Message::Binary(b) => (WsKind::Binary, b.clone()),
            Message::Ping(b) => (WsKind::Ping, b.clone()),
            Message::Pong(b) => (WsKind::Pong, b.clone()),
            Message::Close(c) => (
                WsKind::Close,
                c.as_ref().map(|c| Bytes::from(format!("{} {}", u16::from(c.code), c.reason))).unwrap_or_default(),
            ),
            Message::Frame(_) => return,
        };
        store.push_ws_message(id, WsMessage { direction, kind, timestamp: now_ms(), payload });
    };

    let error: Option<String> = loop {
        tokio::select! {
            msg = client_rx.next() => match msg {
                Some(Ok(m)) => {
                    record(WsDirection::Outgoing, &m);
                    let close = m.is_close();
                    if let Err(e) = server_tx.send(m).await { break (!close).then(|| e.to_string()); }
                    if close { break None; }
                }
                Some(Err(e)) => break Some(format!("client: {e}")),
                None => break None,
            },
            msg = server_rx.next() => match msg {
                Some(Ok(m)) => {
                    record(WsDirection::Incoming, &m);
                    let close = m.is_close();
                    if let Err(e) = client_tx.send(m).await { break (!close).then(|| e.to_string()); }
                    if close { break None; }
                }
                Some(Err(e)) => break Some(format!("server: {e}")),
                None => break None,
            },
        }
    };
    let _ = client_tx.close().await;
    let _ = server_tx.close().await;
    finish(&store, id, error);
}

fn finish(store: &FlowStore, id: Option<FlowId>, error: Option<String>) {
    if let Some(id) = id {
        store.update(id, |f| {
            f.timing.finished = Some(now_ms());
            match error {
                Some(e) => {
                    f.state = FlowState::Failed;
                    f.error = Some(e);
                }
                None => f.state = FlowState::Completed,
            }
        });
    }
}
