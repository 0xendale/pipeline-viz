//! The public instrumentation surface.
//!
//! Everything here is a thin sender: it stamps a timestamp, builds an [`Event`],
//! and hands it to the collector. No state is kept on this side, and no call
//! ever blocks — a full channel drops the event rather than slowing the host
//! pipeline down.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::cancel::CancelSignal;
use crate::collector::DEFAULT_MAX_RETAINED_ABANDONED;
use crate::event::Event;
use crate::model::{JobId, NodeId, NodeKind, Snapshot};
use crate::runtime::{now_ms, spawn_collector, CollectorConfig, CollectorHandle};

/// Default bound on queued events. Reaching it means the host pipeline is
/// producing events faster than they can be folded into state, at which point
/// events are dropped and counted.
const DEFAULT_CHANNEL_CAPACITY: usize = 4096;

/// Default collector tick. Queue depths and hold states do not benefit from
/// 60Hz; 100ms cuts message volume roughly sixfold against a 16ms tick.
const DEFAULT_TICK: Duration = Duration::from_millis(100);

static NEXT_TRACKER_INSTANCE: AtomicU64 = AtomicU64::new(1);

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
///
/// ```no_run
/// use std::time::Duration;
///
/// use pipeline_viz::PipelineTracker;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let tracker = PipelineTracker::builder()
///     .bind_port(9999)
///     .channel_capacity(8192)
///     .tick(Duration::from_millis(100))
///     .max_retained_abandoned(1000)
///     .enable_process_metrics(true)
///     .start_background()?;
/// # let _ = tracker;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct TrackerBuilder {
    port: u16,
    channel_capacity: usize,
    tick: Duration,
    process_metrics: bool,
    max_retained_abandoned: usize,
}

impl Default for TrackerBuilder {
    fn default() -> Self {
        Self {
            port: 9999,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            tick: DEFAULT_TICK,
            process_metrics: true,
            max_retained_abandoned: DEFAULT_MAX_RETAINED_ABANDONED,
        }
    }
}

impl TrackerBuilder {
    /// Port the dashboard will be served on.
    ///
    /// The listener is always bound to `127.0.0.1`, and there is no option to
    /// change that. The dashboard is unauthenticated: anything that can reach
    /// the port can read every item id, hold reason and metadata in the
    /// pipeline. Reach a remote machine's dashboard with an SSH tunnel rather
    /// than by exposing the port.
    ///
    /// Port `0` asks the operating system for a free port; read the result back
    /// with [`PipelineTracker::port`].
    pub fn bind_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Bound on queued events before dropping begins. Defaults to 4096.
    ///
    /// Instrumentation never applies backpressure to the pipeline it watches,
    /// so a full channel costs the event rather than the caller's time. Raise
    /// this if [`PipelineTracker::dropped_events`] is climbing; see there for
    /// what a nonzero count means for what the dashboard is showing.
    ///
    /// Values below 1 are raised to 1.
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity.max(1);
        self
    }

    /// How often coalesced patches are emitted. Defaults to 100ms.
    ///
    /// Everything that happened to an item within one tick is sent as its final
    /// value, so message volume stays flat as throughput rises.
    pub fn tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Whether to sample process-wide CPU and RAM once a second. On by default.
    ///
    /// Reports the whole process, never a single node: nodes share one process
    /// and one thread pool, so per-node attribution is not measurable.
    pub fn enable_process_metrics(mut self, enabled: bool) -> Self {
        self.process_metrics = enabled;
        self
    }

    /// Maximum abandoned item records retained for diagnostics. Defaults to 1000.
    ///
    /// An abandoned item is kept so the failure can still be seen after the
    /// fact. Once the limit is reached, the oldest abandoned record is evicted
    /// and reported as a removal; a re-entering item leaves the queue.
    ///
    /// This bounds the abandoned record *count* only. It does not bound
    /// metadata bytes, and it never evicts active or held work — a pipeline
    /// that holds a million items still holds a million items. Zero retains
    /// nothing: an abandoned item is removed on the next tick.
    ///
    /// # Examples
    /// ```
    /// use pipeline_viz::PipelineTracker;
    ///
    /// let _builder = PipelineTracker::builder().max_retained_abandoned(100);
    /// ```
    pub fn max_retained_abandoned(mut self, max: usize) -> Self {
        self.max_retained_abandoned = max;
        self
    }

    /// Start the collector and the dashboard server on the current Tokio runtime.
    ///
    /// Binding happens here rather than inside the spawned task so the caller
    /// learns the real port immediately, and so a busy port is reported as a
    /// warning rather than vanishing into a detached task. A bind failure
    /// leaves the tracker fully functional with no dashboard.
    ///
    /// Keep the returned handle alive for as long as you want the dashboard.
    /// When the last clone drops, the collector, the process sampler, the
    /// server and every open WebSocket stop and the port is released; nothing
    /// is flushed on the way out.
    ///
    /// # Errors
    ///
    /// [`Error::NoRuntime`] if called outside a Tokio runtime. Nothing else can
    /// fail: every other problem degrades to "no dashboard".
    ///
    /// ```no_run
    /// use pipeline_viz::PipelineTracker;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let tracker = PipelineTracker::builder()
    ///     .bind_port(9999)
    ///     .max_retained_abandoned(500)
    ///     .start_background()?;
    /// println!("dashboard on http://127.0.0.1:{}", tracker.port());
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_background(self) -> Result<PipelineTracker, Error> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(Error::NoRuntime);
        }

        let (sender, receiver) = mpsc::channel(self.channel_capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let cancel = CancelSignal::new();
        let collector = spawn_collector(
            receiver,
            Arc::clone(&dropped),
            CollectorConfig {
                tick: self.tick,
                max_retained_abandoned: self.max_retained_abandoned,
            },
            cancel.token(),
        );

        if self.process_metrics {
            crate::process::spawn_sampler(sender.clone(), Arc::clone(&dropped), cancel.token());
        }

        let serving = Arc::new(AtomicBool::new(false));
        let listener = std::net::TcpListener::bind(("127.0.0.1", self.port));
        let bound_port = match listener {
            Ok(listener) => {
                let port = listener
                    .local_addr()
                    .map(|addr| addr.port())
                    .unwrap_or(self.port);
                match listener.set_nonblocking(true) {
                    Ok(()) => {
                        crate::server::serve(
                            listener,
                            collector.clone(),
                            cancel.token(),
                            Arc::clone(&serving),
                        );
                        port
                    }
                    Err(error) => {
                        eprintln!("pipeline-viz: dashboard disabled ({error})");
                        self.port
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "pipeline-viz: dashboard disabled, port {} unavailable ({error})",
                    self.port
                );
                self.port
            }
        };

        Ok(PipelineTracker {
            inner: Arc::new(TrackerInner {
                sender,
                dropped,
                collector,
                cancel,
                port: bound_port,
                serving,
                instance: NEXT_TRACKER_INSTANCE.fetch_add(1, Ordering::Relaxed),
                next_job_sequence: AtomicU64::new(1),
            }),
        })
    }
}

#[derive(Debug)]
struct TrackerInner {
    sender: mpsc::Sender<Event>,
    dropped: Arc<AtomicU64>,
    collector: CollectorHandle,
    cancel: CancelSignal,
    port: u16,
    serving: Arc<AtomicBool>,
    instance: u64,
    next_job_sequence: AtomicU64,
}

/// The dashboard belongs to the tracker, not to the process.
///
/// When the last handle goes — including the clones held by a `JobBuilder` or a
/// `JobGuard` — the collector, the sampler, the HTTP server and every open
/// WebSocket stop, and the port is released. Nothing is flushed on the way out:
/// by this point no handle exists to observe the result.
impl Drop for TrackerInner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Handle used to instrument a pipeline. Cheap to clone and share.
///
/// Every clone — including the ones held inside a [`JobBuilder`] and a
/// [`JobGuard`] — is an owner. The dashboard runs for as long as at least one
/// exists, and stops when the last one drops.
#[derive(Clone, Debug)]
pub struct PipelineTracker {
    inner: Arc<TrackerInner>,
}

impl PipelineTracker {
    /// Starts configuring a tracker.
    ///
    /// ```no_run
    /// use pipeline_viz::PipelineTracker;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let tracker = PipelineTracker::builder().bind_port(9999).start_background()?;
    /// # let _ = tracker;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> TrackerBuilder {
        TrackerBuilder::default()
    }

    /// Declare a pipeline stage and the stages that feed it.
    ///
    /// Optional: an item entering an unknown node registers that node
    /// automatically. Registering explicitly gives it a display name, a kind,
    /// and its incoming edges.
    ///
    /// ```no_run
    /// use pipeline_viz::NodeKind;
    ///
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// tracker.register_node("committer", NodeKind::Sink, ["indexer"]);
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// use pipeline_viz::NodeKind;
    ///
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// tracker.register_node_named("committer", "Database Committer", NodeKind::Sink, ["indexer"]);
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("indexer").id("block_42").job_type("Block").start();
    /// job.complete();
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker, queue: &[u8]) {
    /// tracker.report_queue_depth("committer", queue.len() as u32);
    /// # }
    /// ```
    pub fn report_queue_depth(&self, node_id: &str, depth: u32) {
        self.emit(Event::QueueDepth {
            node_id: node_id.to_string(),
            depth,
            at_ms: now_ms(),
        });
    }

    /// Port the dashboard is actually bound to, on `127.0.0.1`.
    ///
    /// Resolves `bind_port(0)` to the port the operating system chose. If
    /// binding failed this is the port that was asked for, and
    /// [`is_serving`](Self::is_serving) is false.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// println!("http://127.0.0.1:{}", tracker.port());
    /// # }
    /// ```
    pub fn port(&self) -> u16 {
        self.inner.port
    }

    /// Whether the dashboard server is actually listening *right now*.
    ///
    /// False when the port was unavailable, and false again once the server has
    /// stopped — including when the Tokio runtime it was started on is
    /// destroyed while this handle lives on. The tracker still works either
    /// way; there is simply nothing to connect a browser to.
    ///
    /// Binding is attempted once, at startup. This never becomes true again on
    /// its own if the port later frees up; start another tracker for that.
    pub fn is_serving(&self) -> bool {
        self.inner.serving.load(Ordering::Relaxed)
    }

    /// Events discarded because the event channel was full.
    ///
    /// Nonzero means what the dashboard shows **may be stale**: an enter, a
    /// completion or an abandonment can have been among the losses, so an item
    /// may appear at a node it has already left, or be missing entirely.
    /// Nothing reconciles this — reconnecting a browser repeats the collector's
    /// current state rather than repairing it. Reduce it by raising
    /// [`TrackerBuilder::channel_capacity`], or clear it by restarting the
    /// tracker.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// if tracker.dropped_events() > 0 {
    ///     eprintln!("pipeline-viz dropped events; the dashboard may be stale");
    /// }
    /// # }
    /// ```
    pub fn dropped_events(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }

    /// Current full state. Primarily for the dashboard server and for tests.
    ///
    /// Subject to the same caveat as [`dropped_events`](Self::dropped_events):
    /// this is what the collector believes, which is only as complete as the
    /// event stream that reached it.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let held = tracker.snapshot().jobs.len();
    /// println!("{held} items in the pipeline");
    /// # }
    /// ```
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

    fn next_job_id(&self) -> JobId {
        let sequence = self.inner.next_job_sequence.fetch_add(1, Ordering::Relaxed);
        format!("job_{}_{}", self.inner.instance, sequence)
    }
}

/// Describes an item before it starts being tracked at a node.
///
/// ```no_run
/// # fn example(tracker: &pipeline_viz::PipelineTracker) {
/// let job = tracker
///     .job("committer")
///     .id("block_42")
///     .job_type("Block")
///     .meta("tx_count", 142)
///     .start();
/// job.complete();
/// # }
/// ```
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
    ///
    /// Explicit ids are used unchanged and are not checked for uniqueness,
    /// including values shaped like generated ids. Two items sharing an id are
    /// one item as far as the collector is concerned, so uniqueness is the
    /// caller's to guarantee.
    ///
    /// Omit this and an id of the form `job_<instance>_<sequence>` is generated,
    /// unique within the process across every clone of the tracker. That shape
    /// is not a reserved namespace: an explicit id may look exactly like one.
    pub fn id(mut self, job_id: impl Display) -> Self {
        self.job_id = Some(job_id.to_string());
        self
    }

    /// Category shown in the dashboard, such as `"Block"` or `"Receipt"`.
    ///
    /// Applies when the item first enters the pipeline; a later
    /// [`JobGuard::move_to`] leaves it unchanged.
    pub fn job_type(mut self, job_type: &str) -> Self {
        self.job_type = job_type.to_string();
        self
    }

    /// Arbitrary detail shown when inspecting the item.
    ///
    /// Metadata is retained for as long as the item is, including while it sits
    /// in the abandoned records. It is not counted against
    /// [`TrackerBuilder::max_retained_abandoned`], which bounds records rather
    /// than bytes, so keep values small.
    pub fn meta(mut self, key: &str, value: impl Display) -> Self {
        self.meta.insert(key.to_string(), value.to_string());
        self
    }

    /// Start tracking. The returned guard owns the item's presence at this node.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("indexer").id("block_42").start();
    /// job.complete();
    /// # }
    /// ```
    pub fn start(self) -> JobGuard {
        let job_id = self.job_id.unwrap_or_else(|| self.tracker.next_job_id());

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
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("committer").id("block_42").hold("Waiting for finality");
    /// job.complete();
    /// # }
    /// ```
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
    /// Identity of the item this guard owns.
    ///
    /// Either the id passed to [`JobBuilder::id`], or a generated
    /// `job_<instance>_<sequence>` if none was.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("indexer").id("block_42").start();
    /// assert_eq!(job.id(), "block_42");
    /// # }
    /// ```
    pub fn id(&self) -> &str {
        &self.job_id
    }

    /// Park the item with an explanation. Calling it again replaces the reason.
    ///
    /// The reason is the whole point: it is what turns "block 42 has not moved
    /// in forty seconds" into an answer.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let mut job = tracker.job("committer").id("block_42").start();
    /// job.hold("Waiting for finality (2/12 confirmations)");
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let mut job = tracker.job("committer").id("block_42").hold("Waiting for finality");
    /// job.resume();
    /// # }
    /// ```
    pub fn resume(&mut self) {
        self.tracker.emit(Event::JobResume {
            job_id: self.job_id.clone(),
            at_ms: now_ms(),
        });
    }

    /// Attach detail to the item.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let mut job = tracker.job("committer").id("block_42").start();
    /// job.meta("tx_count", 142);
    /// # }
    /// ```
    pub fn meta(&mut self, key: &str, value: impl Display) {
        self.tracker.emit(Event::JobMeta {
            job_id: self.job_id.clone(),
            key: key.to_string(),
            value: value.to_string(),
            at_ms: now_ms(),
        });
    }

    /// Hand the item to the next node. Time spent here is recorded.
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let mut job = tracker.job("indexer").id("block_42").start();
    /// job.move_to("committer");
    /// # }
    /// ```
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
    ///
    /// ```no_run
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("committer").id("block_42").start();
    /// job.complete();
    /// # }
    /// ```
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
