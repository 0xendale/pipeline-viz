//! Wraps the pure [`CollectorState`] in a background task.
//!
//! This is the only place a clock or a runtime is involved. The state machine
//! itself stays testable without either.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{broadcast, mpsc};

use crate::collector::CollectorState;
use crate::event::Event;
use crate::model::{ServerMessage, Snapshot};

/// How many pending messages a slow dashboard client may fall behind before it
/// is disconnected and forced to reconnect for a fresh snapshot.
const BROADCAST_CAPACITY: usize = 256;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Read access to collector output, for the dashboard server.
#[derive(Clone, Debug)]
pub(crate) struct CollectorHandle {
    state: Arc<Mutex<CollectorState>>,
    patches: broadcast::Sender<Arc<ServerMessage>>,
}

impl CollectorHandle {
    /// Current full state, for a client that has just connected.
    pub(crate) fn snapshot(&self) -> Snapshot {
        let mut state = self.state.lock().expect("collector state poisoned");
        state.snapshot(now_ms())
    }

    #[allow(dead_code)] // Consumed by the dashboard server in the next milestone.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<ServerMessage>> {
        self.patches.subscribe()
    }
}

/// Start the collector task. Must be called from within a Tokio runtime.
pub(crate) fn spawn_collector(
    mut events: mpsc::Receiver<Event>,
    dropped: Arc<AtomicU64>,
    tick: Duration,
) -> CollectorHandle {
    let state = Arc::new(Mutex::new(CollectorState::new()));
    let (patches, _) = broadcast::channel(BROADCAST_CAPACITY);

    let handle = CollectorHandle {
        state: Arc::clone(&state),
        patches: patches.clone(),
    };

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tick);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                event = events.recv() => {
                    match event {
                        Some(event) => {
                            state.lock().expect("collector state poisoned").apply(event);
                        }
                        // Every tracker handle is gone; the pipeline is done with us.
                        None => break,
                    }
                }
                _ = ticker.tick() => {
                    let patch = {
                        let mut state = state.lock().expect("collector state poisoned");
                        state.set_dropped_events(dropped.load(Ordering::Relaxed));
                        state.take_patch(now_ms())
                    };
                    if let Some(patch) = patch {
                        // Errors here mean nobody is watching the dashboard,
                        // which is the normal case and not a problem.
                        let _ = patches.send(Arc::new(ServerMessage::Patch(patch)));
                    }
                }
            }
        }
    });

    handle
}
