//! Serves the dashboard. Subscribes to collector output; never mutates state.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::model::ServerMessage;
use crate::runtime::CollectorHandle;

/// Shared with every request handler.
#[derive(Clone, Debug)]
pub(crate) struct ServerState {
    pub(crate) collector: CollectorHandle,
}

/// Take over an already-bound listener and serve until the process exits.
///
/// The listener is bound by the caller so that a bind failure is reported
/// synchronously, and so the real port is known before this task starts.
pub(crate) fn serve(listener: std::net::TcpListener, collector: CollectorHandle) {
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("pipeline-viz: dashboard disabled ({error})");
                return;
            }
        };

        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/ws", get(websocket_upgrade))
            .route("/", get(crate::assets::index))
            // Everything else is the embedded bundle, which also absorbs
            // unknown paths so a reloaded dashboard route still loads.
            .fallback(get(crate::assets::asset))
            .with_state(ServerState { collector });

        // A serving failure means no dashboard. It must never take the host
        // pipeline down with it.
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("pipeline-viz: dashboard stopped ({error})");
        }
    });
}

async fn websocket_upgrade(
    upgrade: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    upgrade.on_upgrade(move |socket| client_loop(socket, state))
}

/// One task per connected dashboard client.
///
/// Subscription happens *before* the snapshot is taken, so no patch emitted
/// during setup is lost. The cost is that the buffer may hold patches older
/// than the snapshot; those are discarded by timestamp, because patch entries
/// carry whole values and replaying an old one would roll state backwards.
async fn client_loop(mut socket: WebSocket, state: ServerState) {
    let mut patches = state.collector.subscribe();
    let snapshot = state.collector.snapshot();
    let snapshot_ts = snapshot.ts_ms;

    let Ok(encoded) = serde_json::to_string(&ServerMessage::Snapshot(snapshot)) else {
        return;
    };
    if socket.send(Message::Text(encoded.into())).await.is_err() {
        return;
    }

    loop {
        match patches.recv().await {
            Ok(message) => {
                if let ServerMessage::Patch(patch) = message.as_ref() {
                    if patch.ts_ms < snapshot_ts {
                        continue;
                    }
                }
                let Ok(encoded) = serde_json::to_string(message.as_ref()) else {
                    continue;
                };
                if socket.send(Message::Text(encoded.into())).await.is_err() {
                    return;
                }
            }
            // The client stopped reading and missed messages. Closing forces a
            // reconnect, which gets a fresh snapshot; continuing would deliver
            // a stream with an invisible gap and leave the client silently wrong.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            // The collector shut down; the host pipeline is finished with us.
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}
