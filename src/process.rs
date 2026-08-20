//! The only `sysinfo` caller in the crate.
//!
//! Reports the whole process, because that is the only resource figure that can
//! be measured honestly — see [`ProcessStats`](crate::model::ProcessStats).
//! Isolated in one file so a `sysinfo` API change stays a local fix.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::mpsc::error::TrySendError;

use crate::cancel::CancelToken;
use crate::event::Event;
use crate::runtime::now_ms;

/// CPU percentage is measured between two refreshes, so the first sample of a
/// process is always zero. One second is long enough to be meaningful and short
/// enough to feel live.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

/// Sample this process on an interval, emitting into the normal event stream.
///
/// Going through the event channel rather than writing state directly is what
/// keeps the collector a pure function of its input.
pub(crate) fn spawn_sampler(
    sender: tokio::sync::mpsc::Sender<Event>,
    dropped: Arc<AtomicU64>,
    mut cancel: CancelToken,
) {
    tokio::spawn(async move {
        let pid = Pid::from_u32(std::process::id());
        let mut system = System::new();
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = ticker.tick() => {}
            }

            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );

            let Some(process) = system.process(pid) else {
                continue;
            };

            let event = Event::ProcessStats {
                cpu_pct: f64::from(process.cpu_usage()),
                ram_mb: process.memory() as f64 / BYTES_PER_MB,
                at_ms: now_ms(),
            };

            // Same rule as every other event: never block the pipeline. A full
            // channel simply costs this sample, and is counted like any other
            // dropped event. A closed channel means the collector is gone.
            match sender.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Closed(_)) => break,
            }
        }
    });
}
