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
    NoRuntime,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pipeline-viz is compiled out (feature \"viz\" is off)")
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Default)]
pub struct TrackerBuilder;

impl TrackerBuilder {
    #[inline(always)]
    pub fn bind_port(self, _port: u16) -> Self {
        self
    }

    #[inline(always)]
    pub fn channel_capacity(self, _capacity: usize) -> Self {
        self
    }

    #[inline(always)]
    pub fn tick(self, _tick: Duration) -> Self {
        self
    }

    #[inline(always)]
    pub fn enable_process_metrics(self, _enabled: bool) -> Self {
        self
    }

    #[inline(always)]
    pub fn max_retained_abandoned(self, _max: usize) -> Self {
        self
    }

    #[inline(always)]
    pub fn start_background(self) -> Result<PipelineTracker, Error> {
        Ok(PipelineTracker)
    }
}

#[derive(Clone, Debug)]
pub struct PipelineTracker;

impl PipelineTracker {
    #[inline(always)]
    pub fn builder() -> TrackerBuilder {
        TrackerBuilder
    }

    #[inline(always)]
    pub fn register_node<I, S>(&self, _node_id: &str, _kind: NodeKind, _inputs: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<NodeId>,
    {
    }

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

    #[inline(always)]
    pub fn job(&self, _node_id: &str) -> JobBuilder {
        JobBuilder
    }

    #[inline(always)]
    pub fn report_queue_depth(&self, _node_id: &str, _depth: u32) {}

    #[inline(always)]
    pub fn port(&self) -> u16 {
        0
    }

    #[inline(always)]
    pub fn is_serving(&self) -> bool {
        false
    }

    #[inline(always)]
    pub fn dropped_events(&self) -> u64 {
        0
    }

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

#[derive(Debug)]
pub struct JobBuilder;

impl JobBuilder {
    #[inline(always)]
    pub fn id(self, _job_id: impl Display) -> Self {
        self
    }

    #[inline(always)]
    pub fn job_type(self, _job_type: &str) -> Self {
        self
    }

    #[inline(always)]
    pub fn meta(self, _key: &str, _value: impl Display) -> Self {
        self
    }

    #[inline(always)]
    pub fn start(self) -> JobGuard {
        JobGuard
    }

    #[inline(always)]
    pub fn hold(self, _reason: &str) -> JobGuard {
        JobGuard
    }
}

#[derive(Debug)]
pub struct JobGuard;

impl JobGuard {
    #[inline(always)]
    pub fn id(&self) -> &str {
        ""
    }

    #[inline(always)]
    pub fn hold(&mut self, _reason: &str) {}

    #[inline(always)]
    pub fn update_reason(&mut self, _reason: &str) {}

    #[inline(always)]
    pub fn resume(&mut self) {}

    #[inline(always)]
    pub fn meta(&mut self, _key: &str, _value: impl Display) {}

    #[inline(always)]
    pub fn move_to(&mut self, _node_id: &str) {}

    #[inline(always)]
    pub fn complete(self) {}
}
