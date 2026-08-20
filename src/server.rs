//! Serves the dashboard. Subscribes to collector output; never mutates state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::cancel::CancelToken;
use crate::model::ServerMessage;
use crate::runtime::CollectorHandle;

/// Shared with every request handler.
#[derive(Clone, Debug)]
pub(crate) struct ServerState {
    pub(crate) collector: CollectorHandle,
    pub(crate) cancel: CancelToken,
}

/// Clears the serving flag however the server task ends.
///
/// Created before the task is spawned, so the flag is also cleared when the
/// task is dropped without ever being polled — which is what happens when the
/// runtime is destroyed while the tracker outlives it.
#[derive(Debug)]
struct ServingGuard(Arc<AtomicBool>);

impl Drop for ServingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Take over an already-bound listener and serve until the tracker is dropped.
///
/// The listener is bound by the caller so that a bind failure is reported
/// synchronously, and so the real port is known before this task starts.
pub(crate) fn serve(
    listener: std::net::TcpListener,
    collector: CollectorHandle,
    mut cancel: CancelToken,
    serving: Arc<AtomicBool>,
) {
    serving.store(true, Ordering::Relaxed);
    let guard = ServingGuard(serving);
    let client_cancel = cancel.clone();

    tokio::spawn(async move {
        let _guard = guard;

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
            .with_state(ServerState {
                collector,
                cancel: client_cancel,
            });

        // A serving failure means no dashboard. It must never take the host
        // pipeline down with it.
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async move { cancel.cancelled().await })
            .await;
        if let Err(error) = served {
            eprintln!("pipeline-viz: dashboard stopped ({error})");
        }
    });
}

async fn websocket_upgrade(
    upgrade: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    upgrade.on_upgrade(move |socket| client_session(socket, state))
}

/// Runs one client until it disconnects or the tracker is dropped.
///
/// The whole session sits inside the `select!`, not just the receive: a client
/// that has stopped reading parks the server mid-send, and only dropping that
/// future releases the socket. The client then sees EOF, which is the correct
/// signal that the dashboard is gone.
///
/// That means shutdown closes the connection abruptly rather than sending a
/// close frame, so a browser reports code 1006. Sending one first would mean
/// awaiting a send that may be exactly the one that is parked, which is the
/// hang this arrangement exists to prevent.
async fn client_session(socket: WebSocket, state: ServerState) {
    let mut cancel = state.cancel.clone();
    tokio::select! {
        _ = client_loop(socket, &state) => {}
        _ = cancel.cancelled() => {}
    }
}

/// One task per connected dashboard client.
///
/// Subscription happens *before* the snapshot is taken, so no patch emitted
/// during setup is lost. The cost is that the buffer may hold patches older
/// than the snapshot; those are discarded by timestamp, because patch entries
/// carry whole values and replaying an old one would roll state backwards.
async fn client_loop(mut socket: WebSocket, state: &ServerState) {
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
