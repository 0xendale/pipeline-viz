//! One optional process-wide tracker, for code that cannot thread a handle.
//!
//! The attribute macros need a tracker at a call site that never received one
//! as an argument, and so does deeply nested pipeline code. This module holds
//! that handle and nothing else — `collector` remains the single owner of
//! `PipelineState`, and every call made through [`global`] is the same runtime
//! call a user would write by hand.
//!
//! Installing is optional. With nothing installed, [`global`] returns `None`
//! and every macro-generated call disappears.

use std::fmt::{self, Display, Formatter};
use std::sync::OnceLock;

use crate::PipelineTracker;

static GLOBAL: OnceLock<PipelineTracker> = OnceLock::new();

/// Returned by [`install`] when a tracker is already installed.
///
/// The first tracker keeps the slot. Replacing it would leave already-running
/// stages reporting into an abandoned collector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallError;

impl Display for InstallError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a pipeline-viz tracker is already installed")
    }
}

impl std::error::Error for InstallError {}

/// Installs the process-wide tracker. Succeeds once.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let tracker = pipeline_viz::PipelineTracker::builder()
///     .bind_port(9999)
///     .start_background()?;
/// pipeline_viz::install(tracker)?;
/// # Ok(())
/// # }
/// ```
pub fn install(tracker: PipelineTracker) -> Result<(), InstallError> {
    GLOBAL.set(tracker).map_err(|_| InstallError)
}

/// The installed tracker, or `None` if [`install`] was never called.
///
/// ```
/// // Nothing installed: instrumentation is skipped, the pipeline runs on.
/// if let Some(tracker) = pipeline_viz::global() {
///     tracker.report_queue_depth("committer", 12);
/// }
/// ```
pub fn global() -> Option<&'static PipelineTracker> {
    GLOBAL.get()
}
