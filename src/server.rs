//! Serves the dashboard. Subscribes to collector output; never mutates state.

use axum::routing::get;
use axum::Router;

use crate::runtime::CollectorHandle;

/// Shared with every request handler.
#[derive(Clone, Debug)]
pub(crate) struct ServerState {
    #[allow(dead_code)] // Read by the /ws handler once patch streaming lands.
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
