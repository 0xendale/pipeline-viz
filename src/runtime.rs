//! Wraps the pure [`CollectorState`] in a background task.
//!
//! This is the only place a clock or a runtime is involved. The state machine
//! itself stays testable without either.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{broadcast, mpsc};

use crate::cancel::CancelToken;
use crate::collector::CollectorState;
use crate::event::Event;
use crate::model::{ServerMessage, Snapshot};

/// How many pending messages a slow dashboard client may fall behind before it
/// is disconnected and forced to reconnect for a fresh snapshot.
const BROADCAST_CAPACITY: usize = 256;

pub(crate) struct CollectorConfig {
    pub(crate) tick: Duration,
    pub(crate) max_retained_abandoned: usize,
}

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
    ///
    /// Reached from the public `PipelineTracker::snapshot`, so it recovers from
    /// a poisoned lock rather than panicking. Nothing under the lock can panic
    /// today, but a visualizer defect must degrade to a possibly-inconsistent
    /// dashboard, never to a panic in the host pipeline's thread.
    pub(crate) fn snapshot(&self) -> Snapshot {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.snapshot(now_ms())
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<ServerMessage>> {
        self.patches.subscribe()
    }
}

/// Start the collector task. Must be called from within a Tokio runtime.
///
/// The task ends on cancellation or when every sender is gone. Cancellation is
/// immediate: no final patch is flushed, because by then nothing owns the
/// tracker and there is nobody left to deliver it to.
pub(crate) fn spawn_collector(
    mut events: mpsc::Receiver<Event>,
    dropped: Arc<AtomicU64>,
    config: CollectorConfig,
    mut cancel: CancelToken,
) -> CollectorHandle {
    let state = Arc::new(Mutex::new(CollectorState::with_max_retained_abandoned(
        config.max_retained_abandoned,
    )));
    let (patches, _) = broadcast::channel(BROADCAST_CAPACITY);

    let handle = CollectorHandle {
        state: Arc::clone(&state),
        patches: patches.clone(),
    };

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(config.tick);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
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
