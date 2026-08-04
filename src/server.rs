//! Serves the dashboard. Subscribes to collector output; never mutates state.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures_util::SinkExt;

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
            .route("/", get(index))
            .with_state(ServerState { collector });

        // A serving failure means no dashboard. It must never take the host
        // pipeline down with it.
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("pipeline-viz: dashboard stopped ({error})");
        }
    });
}

/// Placeholder until the dashboard UI is embedded in milestone 4.
async fn index() -> &'static str {
    "pipeline-viz is running. The dashboard UI is not built yet; connect to /ws for the event stream."
}

async fn websocket_upgrade(upgrade: WebSocketUpgrade, State(state): State<ServerState>) -> Response {
    upgrade.on_upgrade(move |socket| client_loop(socket, state))
}

/// One task per connected dashboard client.
async fn client_loop(mut socket: WebSocket, state: ServerState) {
    let snapshot = state.collector.snapshot();

    let Ok(encoded) = serde_json::to_string(&ServerMessage::Snapshot(snapshot)) else {
        return;
    };
    // A send failure means the client is gone, which needs no handling
    // beyond ending this task.
    let _ = socket.send(Message::Text(encoded.into())).await;
}
