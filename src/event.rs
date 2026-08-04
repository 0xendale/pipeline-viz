//! Internal event stream: the only thing that flows from user code to the collector.
//!
//! Every event carries the timestamp stamped at the call site rather than at
//! consumption. That makes the collector a pure function of its event stream,
//! so its tests need no clock and no I/O.

use std::collections::BTreeMap;

use crate::model::{JobId, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    RegisterNode {
        node_id: NodeId,
        display_name: String,
        kind: NodeKind,
        inputs: Vec<NodeId>,
        at_ms: u64,
    },
    /// An item arrived at a node. Also the item's first appearance if unknown.
    JobEnter {
        job_id: JobId,
        job_type: String,
        node_id: NodeId,
        meta: BTreeMap<String, String>,
        at_ms: u64,
    },
    /// Park an item with an explanation. Sending it again replaces the reason.
    JobHold {
        job_id: JobId,
        reason: String,
        at_ms: u64,
    },
    JobResume {
        job_id: JobId,
        at_ms: u64,
    },
    JobComplete {
        job_id: JobId,
        at_ms: u64,
    },
    /// Guard dropped without `complete()`.
    JobAbandon {
        job_id: JobId,
        at_ms: u64,
    },
    JobMeta {
        job_id: JobId,
        key: String,
        value: String,
        at_ms: u64,
    },
    QueueDepth {
        node_id: NodeId,
        depth: u32,
        at_ms: u64,
    },
}

impl Event {
    pub(crate) fn at_ms(&self) -> u64 {
        match self {
            Event::RegisterNode { at_ms, .. }
            | Event::JobEnter { at_ms, .. }
            | Event::JobHold { at_ms, .. }
            | Event::JobResume { at_ms, .. }
            | Event::JobComplete { at_ms, .. }
            | Event::JobAbandon { at_ms, .. }
            | Event::JobMeta { at_ms, .. }
            | Event::QueueDepth { at_ms, .. } => *at_ms,
        }
    }
}
