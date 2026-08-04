//! The public instrumentation surface.
//!
//! Everything here is a thin sender: it stamps a timestamp, builds an [`Event`],
//! and hands it to the collector. No state is kept on this side, and no call
//! ever blocks — a full channel drops the event rather than slowing the host
//! pipeline down.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::event::Event;
use crate::model::{JobId, NodeId, NodeKind, Snapshot};
use crate::runtime::{now_ms, spawn_collector, CollectorHandle};

/// Default bound on queued events. Reaching it means the host pipeline is
/// producing events faster than they can be folded into state, at which point
/// events are dropped and counted.
const DEFAULT_CHANNEL_CAPACITY: usize = 4096;

/// Default collector tick. Queue depths and hold states do not benefit from
/// 60Hz; 100ms cuts message volume roughly sixfold against a 16ms tick.
const DEFAULT_TICK: Duration = Duration::from_millis(100);

/// Things that can go wrong starting the tracker.
///
/// Note what is absent: no variant reports a failure of the *pipeline*. A
/// visualizer problem degrades to "no dashboard", never to a broken application.
#[derive(Debug)]
pub enum Error {
    /// `start_background` was called outside a Tokio runtime.
    NoRuntime,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoRuntime => write!(
                f,
                "pipeline-viz must be started from within a Tokio runtime \
                 (call start_background() inside #[tokio::main] or a runtime block)"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Configures a tracker before it starts.
#[derive(Debug)]
pub struct TrackerBuilder {
    port: u16,
    channel_capacity: usize,
    tick: Duration,
}

impl Default for TrackerBuilder {
    fn default() -> Self {
        Self {
            port: 9999,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            tick: DEFAULT_TICK,
        }
    }
}

impl TrackerBuilder {
    /// Port the dashboard will be served on.
    pub fn bind_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Bound on queued events before dropping begins.
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity.max(1);
        self
    }

    /// How often coalesced patches are emitted.
    pub fn tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Start the collector and the dashboard server on the current Tokio runtime.
    ///
    /// Binding happens here rather than inside the spawned task so the caller
    /// learns the real port immediately, and so a busy port is reported as a
    /// warning rather than vanishing into a detached task. A bind failure
    /// leaves the tracker fully functional with no dashboard.
    pub fn start_background(self) -> Result<PipelineTracker, Error> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(Error::NoRuntime);
        }

        let (sender, receiver) = mpsc::channel(self.channel_capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let collector = spawn_collector(receiver, Arc::clone(&dropped), self.tick);

        let listener = std::net::TcpListener::bind(("127.0.0.1", self.port));
        let (bound_port, serving) = match listener {
            Ok(listener) => {
                let port = listener
                    .local_addr()
                    .map(|addr| addr.port())
                    .unwrap_or(self.port);
                match listener.set_nonblocking(true) {
                    Ok(()) => {
                        crate::server::serve(listener, collector.clone());
                        (port, true)
                    }
                    Err(error) => {
                        eprintln!("pipeline-viz: dashboard disabled ({error})");
                        (self.port, false)
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "pipeline-viz: dashboard disabled, port {} unavailable ({error})",
                    self.port
                );
                (self.port, false)
            }
        };

        Ok(PipelineTracker {
            inner: Arc::new(TrackerInner {
                sender,
                dropped,
                collector,
                port: bound_port,
                serving,
            }),
        })
    }
}

#[derive(Debug)]
struct TrackerInner {
    sender: mpsc::Sender<Event>,
    dropped: Arc<AtomicU64>,
    collector: CollectorHandle,
    port: u16,
    serving: bool,
}

/// Handle used to instrument a pipeline. Cheap to clone and share.
#[derive(Clone, Debug)]
pub struct PipelineTracker {
    inner: Arc<TrackerInner>,
}

impl PipelineTracker {
    pub fn builder() -> TrackerBuilder {
        TrackerBuilder::default()
    }

    /// Declare a pipeline stage and the stages that feed it.
    ///
    /// Optional: an item entering an unknown node registers that node
    /// automatically. Registering explicitly gives it a display name, a kind,
    /// and its incoming edges.
    pub fn register_node<I, S>(&self, node_id: &str, kind: NodeKind, inputs: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<NodeId>,
    {
        self.emit(Event::RegisterNode {
            node_id: node_id.to_string(),
            display_name: node_id.to_string(),
            kind,
            inputs: inputs.into_iter().map(Into::into).collect(),
            at_ms: now_ms(),
        });
    }

    /// Same as [`register_node`](Self::register_node), with a human-facing name.
    pub fn register_node_named<I, S>(
        &self,
        node_id: &str,
        display_name: &str,
        kind: NodeKind,
        inputs: I,
    ) where
        I: IntoIterator<Item = S>,
        S: Into<NodeId>,
    {
        self.emit(Event::RegisterNode {
            node_id: node_id.to_string(),
            display_name: display_name.to_string(),
            kind,
            inputs: inputs.into_iter().map(Into::into).collect(),
            at_ms: now_ms(),
        });
    }

    /// Begin describing an item arriving at a node.
    pub fn job(&self, node_id: &str) -> JobBuilder {
        JobBuilder {
            tracker: self.clone(),
            node_id: node_id.to_string(),
            job_id: None,
            job_type: String::new(),
            meta: BTreeMap::new(),
        }
    }

    /// Report a queue depth the host application already tracks.
    pub fn report_queue_depth(&self, node_id: &str, depth: u32) {
        self.emit(Event::QueueDepth {
            node_id: node_id.to_string(),
            depth,
            at_ms: now_ms(),
        });
    }

    /// Port the dashboard is actually bound to.
    pub fn port(&self) -> u16 {
        self.inner.port
    }

    /// Whether the dashboard server is actually listening.
    ///
    /// False when the port was unavailable. The tracker still works; there is
    /// simply nothing to connect a browser to.
    pub fn is_serving(&self) -> bool {
        self.inner.serving
    }

    /// Events discarded because the channel was full.
    pub fn dropped_events(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }

    /// Current full state. Primarily for the dashboard server and for tests.
    pub fn snapshot(&self) -> Snapshot {
        self.inner.collector.snapshot()
    }

    /// Hand an event to the collector, or drop it.
    ///
    /// `try_send` rather than `send`: instrumentation must never apply
    /// backpressure to the pipeline it is watching. A dropped event increments a
    /// counter that the dashboard displays, so the loss is visible rather than
    /// silent.
    fn emit(&self, event: Event) {
        if self.inner.sender.try_send(event).is_err() {
            self.inner.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Describes an item before it starts being tracked at a node.
#[derive(Debug)]
pub struct JobBuilder {
    tracker: PipelineTracker,
    node_id: NodeId,
    job_id: Option<JobId>,
    job_type: String,
    meta: BTreeMap<String, String>,
}

impl JobBuilder {
    /// Identity of the item, stable across the whole pipeline.
    pub fn id(mut self, job_id: impl Display) -> Self {
        self.job_id = Some(job_id.to_string());
        self
    }

    /// Category shown in the dashboard, such as `"Block"` or `"Receipt"`.
    pub fn job_type(mut self, job_type: &str) -> Self {
        self.job_type = job_type.to_string();
        self
    }

    /// Arbitrary detail shown when inspecting the item.
    pub fn meta(mut self, key: &str, value: impl Display) -> Self {
        self.meta.insert(key.to_string(), value.to_string());
        self
    }

    /// Start tracking. The returned guard owns the item's presence at this node.
    pub fn start(self) -> JobGuard {
        let job_id = self.job_id.unwrap_or_else(|| format!("job_{}", now_ms()));

        self.tracker.emit(Event::JobEnter {
            job_id: job_id.clone(),
            job_type: self.job_type,
            node_id: self.node_id,
            meta: self.meta,
            at_ms: now_ms(),
        });

        JobGuard {
            tracker: self.tracker,
            job_id,
            finished: false,
        }
    }

    /// Start tracking and immediately park the item with a reason.
    pub fn hold(self, reason: &str) -> JobGuard {
        let mut guard = self.start();
        guard.hold(reason);
        guard
    }
}

/// Owns an item's presence in the pipeline.
///
/// Dropping without calling [`complete`](Self::complete) marks the item
/// **abandoned** rather than letting it vanish. That is deliberate: the usual
/// cause is an early return through `?`, which is exactly the case where items
/// really do disappear and the user has no way to see it.
#[derive(Debug)]
pub struct JobGuard {
    tracker: PipelineTracker,
    job_id: JobId,
    finished: bool,
}

impl JobGuard {
    pub fn id(&self) -> &str {
        &self.job_id
    }

    /// Park the item with an explanation. Calling it again replaces the reason.
    pub fn hold(&mut self, reason: &str) {
        self.tracker.emit(Event::JobHold {
            job_id: self.job_id.clone(),
            reason: reason.to_string(),
            at_ms: now_ms(),
        });
    }

    /// Replace the current hold reason. Alias of [`hold`](Self::hold), for
    /// readability at call sites that are updating rather than parking.
    pub fn update_reason(&mut self, reason: &str) {
        self.hold(reason);
    }

    /// Return the item to active work.
    pub fn resume(&mut self) {
        self.tracker.emit(Event::JobResume {
            job_id: self.job_id.clone(),
            at_ms: now_ms(),
        });
    }

    /// Attach detail to the item.
    pub fn meta(&mut self, key: &str, value: impl Display) {
        self.tracker.emit(Event::JobMeta {
            job_id: self.job_id.clone(),
            key: key.to_string(),
            value: value.to_string(),
            at_ms: now_ms(),
        });
    }

    /// Hand the item to the next node. Time spent here is recorded.
    pub fn move_to(&mut self, node_id: &str) {
        self.tracker.emit(Event::JobEnter {
            job_id: self.job_id.clone(),
            job_type: String::new(),
            node_id: node_id.to_string(),
            meta: BTreeMap::new(),
            at_ms: now_ms(),
        });
    }

    /// The item left the pipeline successfully.
    pub fn complete(mut self) {
        self.finished = true;
        self.tracker.emit(Event::JobComplete {
            job_id: self.job_id.clone(),
            at_ms: now_ms(),
        });
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.tracker.emit(Event::JobAbandon {
                job_id: self.job_id.clone(),
                at_ms: now_ms(),
            });
        }
    }
}
