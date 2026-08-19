//! Sole owner of pipeline state.
//!
//! [`CollectorState`] is a pure function of its event stream: same events in,
//! same state out, no clock and no I/O. Every hard piece of logic — coalescing,
//! percentiles, throughput, in-flight accounting — lives here so it can be
//! tested without a runtime, a socket, or a browser.
// allow: SIZE_OK — CollectorState is one event-fold state machine; splitting transition and patch logic would obscure coalescing and retention invariants.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::event::Event;
use crate::model::{
    JobId, JobPhase, JobState, NodeCounters, NodeId, NodeState, Patch, ProcessStats, Snapshot,
};

/// Rolling window for throughput and percentiles.
const WINDOW_MS: u64 = 60_000;

/// Per-node cap on retained duration samples, so a fast node cannot grow the
/// window without bound between prunes.
const MAX_SAMPLES: usize = 4096;

pub(crate) const DEFAULT_MAX_RETAINED_ABANDONED: usize = 1_000;

#[derive(Debug)]
pub(crate) struct CollectorState {
    nodes: BTreeMap<NodeId, NodeState>,
    jobs: BTreeMap<JobId, JobState>,
    /// Per node: (timestamp of leaving, time spent at the node).
    samples: BTreeMap<NodeId, VecDeque<(u64, u64)>>,
    dirty_nodes: BTreeSet<NodeId>,
    dirty_jobs: BTreeSet<JobId>,
    removed_jobs: BTreeSet<JobId>,
    abandoned_fifo: VecDeque<JobId>,
    max_retained_abandoned: usize,
    dropped_events: u64,
    last_patch_dropped: u64,
    started_at_ms: Option<u64>,
    latest_ts_ms: u64,
    process: Option<ProcessStats>,
    last_patch_process: Option<ProcessStats>,
}

impl Default for CollectorState {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            jobs: BTreeMap::new(),
            samples: BTreeMap::new(),
            dirty_nodes: BTreeSet::new(),
            dirty_jobs: BTreeSet::new(),
            removed_jobs: BTreeSet::new(),
            abandoned_fifo: VecDeque::new(),
            max_retained_abandoned: DEFAULT_MAX_RETAINED_ABANDONED,
            dropped_events: 0,
            last_patch_dropped: 0,
            started_at_ms: None,
            latest_ts_ms: 0,
            process: None,
            last_patch_process: None,
        }
    }
}

impl CollectorState {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_max_retained_abandoned(max_retained_abandoned: usize) -> Self {
        Self {
            max_retained_abandoned,
            ..Self::default()
        }
    }

    /// Number of events discarded because the channel was full.
    ///
    /// Set by the runtime from a counter the senders bump. Surfaced to the
    /// dashboard rather than hidden: dropping events is the price of never
    /// blocking the host pipeline, and the user is entitled to know.
    pub(crate) fn set_dropped_events(&mut self, dropped: u64) {
        self.dropped_events = dropped;
    }

    pub(crate) fn apply(&mut self, event: Event) {
        let at_ms = event.at_ms();
        self.started_at_ms.get_or_insert(at_ms);
        self.latest_ts_ms = self.latest_ts_ms.max(at_ms);

        match event {
            Event::RegisterNode {
                node_id,
                display_name,
                kind,
                inputs,
                ..
            } => {
                let node = self
                    .nodes
                    .entry(node_id.clone())
                    .or_insert_with(|| NodeState::placeholder(&node_id));
                node.display_name = display_name;
                node.kind = kind;
                node.inputs = inputs;
                self.dirty_nodes.insert(node_id);
            }

            Event::JobEnter {
                job_id,
                job_type,
                node_id,
                meta,
                at_ms,
            } => {
                self.abandoned_fifo.retain(|queued| queued != &job_id);
                self.ensure_node(&node_id);

                let previous = self
                    .jobs
                    .get(&job_id)
                    .map(|job| (job.current_node.clone(), job.entered_node_at_ms));

                if let Some((previous_node, entered_at)) = previous {
                    if previous_node != node_id {
                        self.record_leave(&previous_node, at_ms, at_ms.saturating_sub(entered_at));
                    }
                }

                match self.jobs.get_mut(&job_id) {
                    Some(job) => {
                        job.current_node = node_id;
                        job.entered_node_at_ms = at_ms;
                        job.phase = JobPhase::Active;
                        if !job_type.is_empty() {
                            job.job_type = job_type;
                        }
                        job.meta.extend(meta);
                    }
                    None => {
                        self.jobs.insert(
                            job_id.clone(),
                            JobState {
                                job_id: job_id.clone(),
                                job_type,
                                current_node: node_id,
                                phase: JobPhase::Active,
                                entered_node_at_ms: at_ms,
                                created_at_ms: at_ms,
                                meta,
                            },
                        );
                    }
                }

                self.removed_jobs.remove(&job_id);
                self.dirty_jobs.insert(job_id);
            }

            Event::JobHold { job_id, reason, .. } => {
                self.abandoned_fifo.retain(|queued| queued != &job_id);
                self.set_phase(&job_id, JobPhase::Held { reason });
            }

            Event::JobResume { job_id, .. } => {
                self.abandoned_fifo.retain(|queued| queued != &job_id);
                self.set_phase(&job_id, JobPhase::Active);
            }

            Event::JobAbandon { job_id, .. } => {
                let newly_abandoned = self
                    .jobs
                    .get(&job_id)
                    .is_some_and(|job| job.phase != JobPhase::Abandoned);
                if newly_abandoned {
                    self.abandoned_fifo.retain(|queued| queued != &job_id);
                    self.set_phase(&job_id, JobPhase::Abandoned);
                    self.abandoned_fifo.push_back(job_id);

                    while self.abandoned_fifo.len() > self.max_retained_abandoned {
                        let Some(evicted) = self.abandoned_fifo.pop_front() else {
                            break;
                        };
                        self.jobs.remove(&evicted);
                        self.dirty_jobs.remove(&evicted);
                        self.removed_jobs.insert(evicted);
                    }
                }
            }

            Event::JobComplete { job_id, at_ms } => {
                self.abandoned_fifo.retain(|queued| queued != &job_id);
                if let Some(job) = self.jobs.remove(&job_id) {
                    let spent = at_ms.saturating_sub(job.entered_node_at_ms);
                    self.record_leave(&job.current_node, at_ms, spent);
                    self.dirty_jobs.remove(&job_id);
                    self.removed_jobs.insert(job_id);
                }
            }

            Event::JobMeta {
                job_id, key, value, ..
            } => {
                if let Some(job) = self.jobs.get_mut(&job_id) {
                    job.meta.insert(key, value);
                    self.dirty_jobs.insert(job_id);
                }
            }

            Event::QueueDepth { node_id, depth, .. } => {
                self.ensure_node(&node_id);
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.counters.queue_depth = depth;
                }
                self.dirty_nodes.insert(node_id);
            }

            Event::ProcessStats {
                cpu_pct, ram_mb, ..
            } => {
                self.process = Some(ProcessStats { cpu_pct, ram_mb });
            }
        }
    }

    /// Build the delta for this tick, or `None` if nothing changed.
    ///
    /// Coalescing falls out of tracking dirty *keys* rather than queuing
    /// messages: however many times an entry changed since the last tick, it is
    /// emitted once, at its final value.
    pub(crate) fn take_patch(&mut self, now_ms: u64) -> Option<Patch> {
        self.latest_ts_ms = self.latest_ts_ms.max(now_ms);
        self.recompute_counters(now_ms);

        let dropped_changed = self.dropped_events != self.last_patch_dropped;
        let process_changed = self.process != self.last_patch_process;
        if self.dirty_nodes.is_empty()
            && self.dirty_jobs.is_empty()
            && self.removed_jobs.is_empty()
            && !dropped_changed
            && !process_changed
        {
            return None;
        }

        let nodes = std::mem::take(&mut self.dirty_nodes)
            .into_iter()
            .filter_map(|id| self.nodes.get(&id).cloned())
            .collect();

        let jobs = std::mem::take(&mut self.dirty_jobs)
            .into_iter()
            .filter_map(|id| self.jobs.get(&id).cloned())
            .collect();

        let removed_jobs = std::mem::take(&mut self.removed_jobs).into_iter().collect();

        self.last_patch_dropped = self.dropped_events;

        // Omitted when unchanged, so the gauge is not resent on every tick.
        let process = if process_changed { self.process } else { None };
        self.last_patch_process = self.process;

        Some(Patch {
            ts_ms: now_ms,
            nodes,
            jobs,
            removed_jobs,
            dropped_events: self.dropped_events,
            process,
        })
    }

    /// Full state for a newly connected client.
    pub(crate) fn snapshot(&mut self, now_ms: u64) -> Snapshot {
        self.recompute_counters(now_ms);
        Snapshot {
            ts_ms: now_ms,
            nodes: self.nodes.values().cloned().collect(),
            jobs: self.jobs.values().cloned().collect(),
            dropped_events: self.dropped_events,
            process: self.process,
        }
    }

    fn ensure_node(&mut self, node_id: &str) {
        if !self.nodes.contains_key(node_id) {
            self.nodes
                .insert(node_id.to_string(), NodeState::placeholder(node_id));
            self.dirty_nodes.insert(node_id.to_string());
        }
    }

    fn set_phase(&mut self, job_id: &str, phase: JobPhase) {
        if let Some(job) = self.jobs.get_mut(job_id) {
            if job.phase != phase {
                job.phase = phase;
                self.dirty_jobs.insert(job_id.to_string());
            }
        }
    }

    fn record_leave(&mut self, node_id: &str, at_ms: u64, spent_ms: u64) {
        self.ensure_node(node_id);
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.counters.left_total += 1;
        }
        let window = self.samples.entry(node_id.to_string()).or_default();
        window.push_back((at_ms, spent_ms));
        while window.len() > MAX_SAMPLES {
            window.pop_front();
        }
        self.dirty_nodes.insert(node_id.to_string());
    }

    /// Recompute everything derived. In-flight counts are *derived* from the
    /// job map rather than incremented and decremented alongside it, which
    /// removes any possibility of the two drifting apart.
    fn recompute_counters(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(WINDOW_MS);

        let mut in_flight: BTreeMap<&str, u32> = BTreeMap::new();
        for job in self.jobs.values() {
            if job.phase != JobPhase::Abandoned {
                *in_flight.entry(job.current_node.as_str()).or_insert(0) += 1;
            }
        }
        let in_flight: BTreeMap<String, u32> = in_flight
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let elapsed_ms = self
            .started_at_ms
            .map(|start| now_ms.saturating_sub(start).min(WINDOW_MS))
            .unwrap_or(0)
            .max(1);

        let node_ids: Vec<NodeId> = self.nodes.keys().cloned().collect();
        for node_id in node_ids {
            let window = self.samples.entry(node_id.clone()).or_default();
            while window.front().is_some_and(|(ts, _)| *ts < cutoff) {
                window.pop_front();
            }

            let mut durations: Vec<u64> = window.iter().map(|(_, spent)| *spent).collect();
            durations.sort_unstable();

            let throughput = (window.len() as f64) * 1000.0 / (elapsed_ms as f64);

            let Some(node) = self.nodes.get_mut(&node_id) else {
                continue;
            };
            let updated = NodeCounters {
                in_flight: in_flight.get(&node_id).copied().unwrap_or(0),
                queue_depth: node.counters.queue_depth,
                left_total: node.counters.left_total,
                throughput_per_sec: round_2dp(throughput),
                p50_ms: percentile(&durations, 50),
                p95_ms: percentile(&durations, 95),
            };

            if node.counters != updated {
                node.counters = updated;
                self.dirty_nodes.insert(node_id);
            }
        }
    }
}

/// Nearest-rank percentile over a pre-sorted slice.
fn percentile(sorted: &[u64], pct: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (pct * sorted.len() as u64).div_ceil(100).max(1);
    sorted[(rank as usize - 1).min(sorted.len() - 1)]
}

fn round_2dp(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests;
