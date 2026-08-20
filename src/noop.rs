//! The entire public API, compiled to nothing.
//!
//! Selected when the `viz` feature is off, which is the default. Every function
//! here is an empty inlined body, and no dependency of the real implementation
//! is compiled at all — no axum, no tokio, no listening port in a production
//! binary. User code compiles unchanged either way.

use std::fmt::Display;
use std::time::Duration;

use crate::model::{NodeId, NodeKind, Snapshot};

/// Cannot occur when the `viz` feature is off; present so signatures match.
#[derive(Debug)]
pub enum Error {
    /// Never constructed in this build. With `viz` on it means
    /// `start_background` was called outside a Tokio runtime.
    NoRuntime,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pipeline-viz is compiled out (feature \"viz\" is off)")
    }
}

impl std::error::Error for Error {}

/// Configures a tracker before it starts. Configures nothing in this build.
#[derive(Debug, Default)]
pub struct TrackerBuilder;

impl TrackerBuilder {
    /// Accepted and discarded. No port is ever bound with `viz` off.
    #[inline(always)]
    pub fn bind_port(self, _port: u16) -> Self {
        self
    }

    /// Accepted and discarded. There is no event channel with `viz` off.
    #[inline(always)]
    pub fn channel_capacity(self, _capacity: usize) -> Self {
        self
    }

    /// Accepted and discarded. There is no collector to tick with `viz` off.
    #[inline(always)]
    pub fn tick(self, _tick: Duration) -> Self {
        self
    }

    /// Accepted and discarded. Nothing is sampled with `viz` off.
    #[inline(always)]
    pub fn enable_process_metrics(self, _enabled: bool) -> Self {
        self
    }

    /// Accepted and discarded. No item state is retained with `viz` off.
    #[inline(always)]
    pub fn max_retained_abandoned(self, _max: usize) -> Self {
        self
    }

    /// Returns a tracker that does nothing. Never fails, and needs no runtime.
    ///
    /// ```
    /// use pipeline_viz::PipelineTracker;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let tracker = PipelineTracker::builder().bind_port(9999).start_background()?;
    /// # let _ = tracker;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn start_background(self) -> Result<PipelineTracker, Error> {
        Ok(PipelineTracker)
    }
}

/// Handle used to instrument a pipeline. A zero-sized nothing in this build.
#[derive(Clone, Debug)]
pub struct PipelineTracker;

impl PipelineTracker {
    /// Starts configuring a tracker.
    #[inline(always)]
    pub fn builder() -> TrackerBuilder {
        TrackerBuilder
    }

    /// Declares a pipeline stage. Records nothing with `viz` off.
    #[inline(always)]
    pub fn register_node<I, S>(&self, _node_id: &str, _kind: NodeKind, _inputs: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<NodeId>,
    {
    }

    /// Declares a named pipeline stage. Records nothing with `viz` off.
    #[inline(always)]
    pub fn register_node_named<I, S>(
        &self,
        _node_id: &str,
        _display_name: &str,
        _kind: NodeKind,
        _inputs: I,
    ) where
        I: IntoIterator<Item = S>,
        S: Into<NodeId>,
    {
    }

    /// Begins describing an item. Every call on the result is also empty.
    ///
    /// ```
    /// # fn example(tracker: &pipeline_viz::PipelineTracker) {
    /// let job = tracker.job("committer").id("block_42").start();
    /// job.complete();
    /// # }
    /// ```
    #[inline(always)]
    pub fn job(&self, _node_id: &str) -> JobBuilder {
        JobBuilder
    }

    /// Reports a queue depth. Records nothing with `viz` off.
    #[inline(always)]
    pub fn report_queue_depth(&self, _node_id: &str, _depth: u32) {}

    /// Always `0`: no listener exists with `viz` off.
    #[inline(always)]
    pub fn port(&self) -> u16 {
        0
    }

    /// Always `false`: no listener exists with `viz` off.
    #[inline(always)]
    pub fn is_serving(&self) -> bool {
        false
    }

    /// Always `0`: there is no event channel to overflow with `viz` off.
    #[inline(always)]
    pub fn dropped_events(&self) -> u64 {
        0
    }

    /// Always empty: no state is kept with `viz` off.
    #[inline(always)]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            ts_ms: 0,
            nodes: Vec::new(),
            jobs: Vec::new(),
            dropped_events: 0,
            process: None,
        }
    }
}

/// Describes an item before tracking starts. Describes nothing in this build.
#[derive(Debug)]
pub struct JobBuilder;

impl JobBuilder {
    /// Accepted and discarded. No id is generated with `viz` off.
    #[inline(always)]
    pub fn id(self, _job_id: impl Display) -> Self {
        self
    }

    /// Accepted and discarded.
    #[inline(always)]
    pub fn job_type(self, _job_type: &str) -> Self {
        self
    }

    /// Accepted and discarded.
    #[inline(always)]
    pub fn meta(self, _key: &str, _value: impl Display) -> Self {
        self
    }

    /// Returns a guard that records nothing, including on drop.
    #[inline(always)]
    pub fn start(self) -> JobGuard {
        JobGuard
    }

    /// Returns a guard that records nothing, including on drop.
    #[inline(always)]
    pub fn hold(self, _reason: &str) -> JobGuard {
        JobGuard
    }
}

/// Owns an item's presence in the pipeline. Owns nothing in this build.
///
/// Note the difference from the `viz` build: dropping this guard without
/// calling [`complete`](Self::complete) marks nothing abandoned, because
/// nothing is being tracked.
#[derive(Debug)]
pub struct JobGuard;

impl JobGuard {
    /// Always `""`: no id is generated with `viz` off.
    #[inline(always)]
    pub fn id(&self) -> &str {
        ""
    }

    /// Parks the item. Records nothing with `viz` off.
    #[inline(always)]
    pub fn hold(&mut self, _reason: &str) {}

    /// Replaces the hold reason. Records nothing with `viz` off.
    #[inline(always)]
    pub fn update_reason(&mut self, _reason: &str) {}

    /// Returns the item to active work. Records nothing with `viz` off.
    #[inline(always)]
    pub fn resume(&mut self) {}

    /// Attaches detail to the item. Records nothing with `viz` off.
    #[inline(always)]
    pub fn meta(&mut self, _key: &str, _value: impl Display) {}

    /// Hands the item to the next node. Records nothing with `viz` off.
    #[inline(always)]
    pub fn move_to(&mut self, _node_id: &str) {}

    /// Ends tracking successfully. Records nothing with `viz` off.
    #[inline(always)]
    pub fn complete(self) {}
}
