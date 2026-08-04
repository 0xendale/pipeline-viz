//! Live item-level visibility for Rust data pipelines.
//!
//! `tracing` shows spans, `tokio-console` shows tasks, Prometheus shows
//! counters. None of them answer the question that actually stalls a debugging
//! session: *where is block 2049102 right now, and why has it not moved in
//! forty seconds?*
//!
//! `pipeline-viz` answers exactly that. Declare your stages, wrap each work item
//! in a guard, and give a reason whenever an item is parked.
//!
//! ```no_run
//! use pipeline_viz::{NodeKind, PipelineTracker};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let tracker = PipelineTracker::builder().bind_port(9999).start_background()?;
//! tracker.register_node("committer", NodeKind::Sink, ["indexer"]);
//!
//! let mut job = tracker.job("committer").id(2_049_102).job_type("Block").start();
//! job.hold("Waiting for finality (2/12 confirmations)");
//! // ... wait for confirmations ...
//! job.update_reason("Writing to PostgreSQL");
//! // ... write ...
//! job.complete();
//! # Ok(())
//! # }
//! ```
//!
//! # Enabling it
//!
//! The crate does nothing until the `viz` feature is on, and it is **off by
//! default**. That is deliberate — nothing should be able to ship a listening
//! port to production by accident.
//!
//! ```toml
//! [dependencies]
//! pipeline-viz = { version = "0.1", features = ["viz"] }
//! ```
//!
//! With the feature off, every call above compiles to an empty inlined body and
//! none of the implementation's dependencies are built.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod model;

#[cfg(feature = "viz")]
mod collector;
#[cfg(feature = "viz")]
mod event;
#[cfg(feature = "viz")]
mod process;
#[cfg(feature = "viz")]
mod runtime;
#[cfg(feature = "viz")]
mod server;
#[cfg(feature = "viz")]
mod tracker;

#[cfg(not(feature = "viz"))]
mod noop;

#[cfg(feature = "viz")]
pub use tracker::{Error, JobBuilder, JobGuard, PipelineTracker, TrackerBuilder};

#[cfg(not(feature = "viz"))]
pub use noop::{Error, JobBuilder, JobGuard, PipelineTracker, TrackerBuilder};

pub use model::{
    JobId, JobPhase, JobState, NodeCounters, NodeId, NodeKind, NodeState, Patch, ServerMessage,
    Snapshot,
};
