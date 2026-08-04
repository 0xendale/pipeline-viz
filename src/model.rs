//! Wire-visible data model.
//!
//! Everything in this module is serialized to the dashboard. The collector owns
//! the authoritative instances; nothing else mutates them.

use std::collections::BTreeMap;

#[cfg(feature = "viz")]
use serde::{Deserialize, Serialize};

/// Stable identifier for a pipeline stage.
pub type NodeId = String;

/// Stable identifier for a single work item.
pub type JobId = String;

/// Role a node plays in the pipeline. Used only for display grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "viz", serde(rename_all = "snake_case"))]
pub enum NodeKind {
    /// Produces items (fetcher, listener, reader).
    Source,
    /// Consumes and emits items (indexer, mapper, validator).
    Transform,
    /// Terminal consumer (database writer, publisher).
    Sink,
}

/// What an item is currently doing at its node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "viz", serde(tag = "phase", rename_all = "snake_case"))]
pub enum JobPhase {
    /// Being worked on.
    Active,
    /// Deliberately parked, with an explanation the user supplied.
    Held { reason: String },
    /// Its guard was dropped without `complete()` — almost always a bug in the
    /// host pipeline, such as an early return through `?`.
    Abandoned,
}

/// A single work item and where it currently sits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct JobState {
    pub job_id: JobId,
    pub job_type: String,
    pub current_node: NodeId,
    #[cfg_attr(feature = "viz", serde(flatten))]
    pub phase: JobPhase,
    /// When the item arrived at `current_node`. Drives the "stuck for N seconds"
    /// reading in the dashboard.
    pub entered_node_at_ms: u64,
    pub created_at_ms: u64,
    pub meta: BTreeMap<String, String>,
}

/// Measurable per-node figures, all derived from the event stream.
///
/// Deliberately excludes per-node CPU and RAM: nodes are logical stages sharing
/// one process and one thread pool, so those values cannot be attributed
/// honestly. Process-wide resource use is reported separately.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct NodeCounters {
    /// Items currently at this node in `Active` or `Held` phase.
    pub in_flight: u32,
    /// Depth reported by the host application, if it reports one.
    pub queue_depth: u32,
    /// Items that have left this node since startup.
    pub left_total: u64,
    /// Items leaving this node per second, over the rolling window.
    pub throughput_per_sec: f64,
    /// Median time spent at this node, over the rolling window.
    pub p50_ms: u64,
    /// 95th-percentile time spent at this node, over the rolling window.
    pub p95_ms: u64,
}

/// A pipeline stage.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct NodeState {
    pub node_id: NodeId,
    pub display_name: String,
    pub kind: NodeKind,
    /// Node ids that feed this one. Defines the graph edges.
    pub inputs: Vec<NodeId>,
    pub counters: NodeCounters,
}

#[cfg(feature = "viz")]
impl NodeState {
    pub(crate) fn placeholder(node_id: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            display_name: node_id.to_string(),
            kind: NodeKind::Transform,
            inputs: Vec::new(),
            counters: NodeCounters::default(),
        }
    }
}

/// Resource use of the **whole process**, not of any single node.
///
/// Nodes are logical stages sharing one process and one thread pool, so a
/// per-node CPU or RAM figure cannot be attributed honestly. This is the honest
/// version of that number, and the dashboard labels it as process-wide.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct ProcessStats {
    pub cpu_pct: f64,
    pub ram_mb: f64,
}

/// Complete state, sent once when a dashboard client connects.
///
/// Sending full state on connect is what lets the protocol drop event replay
/// entirely: a new browser tab is correct immediately.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct Snapshot {
    pub ts_ms: u64,
    pub nodes: Vec<NodeState>,
    pub jobs: Vec<JobState>,
    pub dropped_events: u64,
    /// `None` when process metrics are disabled or not yet sampled.
    pub process: Option<ProcessStats>,
}

/// Coalesced delta, emitted on the collector tick.
///
/// `nodes` and `jobs` carry only entries that changed since the previous tick,
/// each already at its final value for this tick. An item that crossed five
/// nodes within one tick appears once, at the fifth.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct Patch {
    pub ts_ms: u64,
    pub nodes: Vec<NodeState>,
    pub jobs: Vec<JobState>,
    pub removed_jobs: Vec<JobId>,
    pub dropped_events: u64,
    /// `None` when the reading has not changed since the previous patch.
    pub process: Option<ProcessStats>,
}

/// Everything the server can push to a client.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "viz", serde(tag = "type", rename_all = "snake_case"))]
pub enum ServerMessage {
    Snapshot(Snapshot),
    Patch(Patch),
}
