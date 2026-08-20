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
//!
//! # The dashboard
//!
//! The tracker serves it on the port you bound. The whole UI is compiled into
//! your binary — no static directory to deploy, no Node.js on the machine that
//! runs it, nothing to configure.
//!
//! # Where it is safe to run
//!
//! The dashboard binds `127.0.0.1` and nothing else, and it is
//! **unauthenticated**: anything that can reach the port can read every item
//! id, hold reason and metadata in the pipeline. There is no option to bind a
//! public address, because there is no safe way to offer one.
//!
//! That makes it a tool for local development, a staging box, and production
//! diagnosis reached over an SSH tunnel:
//!
//! ```text
//! ssh -N -L 9999:127.0.0.1:9999 you@your-host
//! ```
//!
//! # What it does not guarantee
//!
//! Instrumentation never applies backpressure to the pipeline it is watching.
//! When the event channel is full the event is dropped and counted, so under a
//! burst the dashboard can fall behind the truth:
//!
//! * A nonzero [`PipelineTracker::dropped_events`] means the displayed state
//!   **may be stale**. An enter, a completion or an abandonment can have been
//!   among the losses, so an item may show at a node it has already left.
//! * Nothing reconciles that afterwards. Reconnecting a browser repeats the
//!   collector's current state; it does not repair it.
//! * The mitigations are to raise [`TrackerBuilder::channel_capacity`], or to
//!   restart the tracker for a clean slate.
//!
//! Abandoned items are retained for diagnosis up to
//! [`TrackerBuilder::max_retained_abandoned`] records, oldest evicted first.
//! That bounds the record count, not the bytes they carry and not active or
//! held work.
//!
//! # Lifetime
//!
//! The dashboard belongs to the tracker. Keep a [`PipelineTracker`] alive for
//! as long as you want it; when the last clone drops, the collector, the
//! process sampler, the server and every open WebSocket stop and the port is
//! released. Nothing is flushed on the way out.
//!
//! # Attribute macros
//!
//! Enable the `macros` feature for [`track_node`] and [`track_job`], which turn
//! the calls above into two annotations:
//!
//! ```toml
//! [dependencies]
//! pipeline-viz = { version = "0.1", features = ["viz", "macros"] }
//! ```
//!
//! ```
//! # #[cfg(feature = "macros")] mod example {
//! use pipeline_viz::{track_job, track_node};
//!
//! #[track_node(id = "committer", kind = Sink, name = "Database Committer", inputs = ["indexer"])]
//! #[track_job(node = "committer", id = number, job_type = "Block", meta(tx_count = 142))]
//! async fn commit(number: u64) -> Result<(), &'static str> {
//!     // ... write to PostgreSQL ...
//!     Ok(())
//! }
//! # }
//! ```
//!
//! `track_node` registers the stage on the annotated function's first call.
//! `track_job` opens a [`JobGuard`] for the duration of the body and completes
//! it when the body returns — including an early `return` or a `?` that yielded
//! an `Err`. A panic drops the guard instead, which is what marks an item
//! abandoned.
//!
//! Both read the tracker from [`install`], because an annotated function has
//! nowhere to receive a handle. They expand to the same runtime calls shown
//! above and hold no state of their own, so the two surfaces cannot drift
//! apart. With nothing installed — or with `viz` off — the generated calls do
//! nothing.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]
#![warn(missing_docs)]

pub mod model;

mod global;

#[cfg(feature = "viz")]
mod assets;
#[cfg(feature = "viz")]
mod cancel;
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

pub use global::{global, install, InstallError};

pub use model::{
    JobId, JobPhase, JobState, NodeCounters, NodeId, NodeKind, NodeState, Patch, ProcessStats,
    ServerMessage, Snapshot,
};

/// Registers the annotated function as a pipeline node.
///
/// See the [module-level macro documentation](crate#attribute-macros).
#[cfg(feature = "macros")]
pub use pipeline_viz_macros::track_node;

/// Tracks one work item for the duration of the annotated function.
///
/// See the [module-level macro documentation](crate#attribute-macros).
#[cfg(feature = "macros")]
pub use pipeline_viz_macros::track_job;
