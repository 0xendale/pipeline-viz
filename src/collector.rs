//! Sole owner of pipeline state.
//!
//! [`CollectorState`] is a pure function of its event stream: same events in,
//! same state out, no clock and no I/O. Every hard piece of logic — coalescing,
//! percentiles, throughput, in-flight accounting — lives here so it can be
//! tested without a runtime, a socket, or a browser.

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

#[derive(Debug, Default)]
pub(crate) struct CollectorState {
    nodes: BTreeMap<NodeId, NodeState>,
    jobs: BTreeMap<JobId, JobState>,
    /// Per node: (timestamp of leaving, time spent at the node).
    samples: BTreeMap<NodeId, VecDeque<(u64, u64)>>,
    dirty_nodes: BTreeSet<NodeId>,
    dirty_jobs: BTreeSet<JobId>,
    removed_jobs: BTreeSet<JobId>,
    dropped_events: u64,
    last_patch_dropped: u64,
    started_at_ms: Option<u64>,
    latest_ts_ms: u64,
    process: Option<ProcessStats>,
    last_patch_process: Option<ProcessStats>,
}

impl CollectorState {
    pub(crate) fn new() -> Self {
        Self::default()
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
                self.set_phase(&job_id, JobPhase::Held { reason });
            }

            Event::JobResume { job_id, .. } => {
                self.set_phase(&job_id, JobPhase::Active);
            }

            Event::JobAbandon { job_id, .. } => {
                self.set_phase(&job_id, JobPhase::Abandoned);
            }

            Event::JobComplete { job_id, at_ms } => {
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
mod tests {
    use super::*;
    use crate::model::NodeKind;
    use std::collections::BTreeMap;

    fn register(id: &str, at_ms: u64) -> Event {
        Event::RegisterNode {
            node_id: id.into(),
            display_name: id.to_uppercase(),
            kind: NodeKind::Transform,
            inputs: vec![],
            at_ms,
        }
    }

    fn enter(job: &str, node: &str, at_ms: u64) -> Event {
        Event::JobEnter {
            job_id: job.into(),
            job_type: "Block".into(),
            node_id: node.into(),
            meta: BTreeMap::new(),
            at_ms,
        }
    }

    fn state_with_nodes() -> CollectorState {
        let mut state = CollectorState::new();
        state.apply(register("fetcher", 0));
        state.apply(register("indexer", 0));
        state.apply(register("committer", 0));
        state.take_patch(0);
        state
    }

    fn node<'a>(patch: &'a Patch, id: &str) -> &'a NodeState {
        patch
            .nodes
            .iter()
            .find(|n| n.node_id == id)
            .unwrap_or_else(|| panic!("node {id} missing from patch"))
    }

    #[test]
    fn registers_nodes_with_display_metadata() {
        let mut state = CollectorState::new();
        state.apply(Event::RegisterNode {
            node_id: "committer".into(),
            display_name: "Database Committer".into(),
            kind: NodeKind::Sink,
            inputs: vec!["indexer".into()],
            at_ms: 10,
        });

        let patch = state.take_patch(10).expect("registration is a change");
        let committer = node(&patch, "committer");
        assert_eq!(committer.display_name, "Database Committer");
        assert_eq!(committer.kind, NodeKind::Sink);
        assert_eq!(committer.inputs, vec!["indexer".to_string()]);
    }

    #[test]
    fn unregistered_nodes_are_auto_created_so_items_are_never_lost() {
        let mut state = CollectorState::new();
        state.apply(enter("block_1", "mystery_stage", 5));

        let patch = state.take_patch(5).unwrap();
        assert_eq!(node(&patch, "mystery_stage").counters.in_flight, 1);
    }

    #[test]
    fn tracks_item_custody_and_hold_reason() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 100));
        state.apply(Event::JobHold {
            job_id: "block_1".into(),
            reason: "Waiting for finality".into(),
            at_ms: 120,
        });

        let patch = state.take_patch(150).unwrap();
        let job = &patch.jobs[0];
        assert_eq!(job.current_node, "committer");
        assert_eq!(job.entered_node_at_ms, 100);
        assert_eq!(
            job.phase,
            JobPhase::Held {
                reason: "Waiting for finality".into()
            }
        );
    }

    #[test]
    fn holding_again_replaces_the_reason() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 100));
        state.apply(Event::JobHold {
            job_id: "block_1".into(),
            reason: "Waiting for finality".into(),
            at_ms: 110,
        });
        state.take_patch(110);

        state.apply(Event::JobHold {
            job_id: "block_1".into(),
            reason: "Writing to PostgreSQL".into(),
            at_ms: 120,
        });

        let patch = state.take_patch(120).unwrap();
        assert_eq!(
            patch.jobs[0].phase,
            JobPhase::Held {
                reason: "Writing to PostgreSQL".into()
            }
        );
    }

    #[test]
    fn patch_coalesces_a_multi_node_traversal_into_one_entry() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "fetcher", 10));
        state.apply(enter("block_1", "indexer", 20));
        state.apply(enter("block_1", "committer", 30));

        let patch = state.take_patch(100).unwrap();

        assert_eq!(patch.jobs.len(), 1, "one entry, not one per transition");
        assert_eq!(patch.jobs[0].current_node, "committer");
        assert_eq!(patch.jobs[0].entered_node_at_ms, 30);
    }

    #[test]
    fn moving_between_nodes_transfers_in_flight_count() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "fetcher", 10));
        state.take_patch(10);

        state.apply(enter("block_1", "indexer", 20));
        let patch = state.take_patch(20).unwrap();

        assert_eq!(node(&patch, "fetcher").counters.in_flight, 0);
        assert_eq!(node(&patch, "indexer").counters.in_flight, 1);
        assert_eq!(node(&patch, "fetcher").counters.left_total, 1);
    }

    #[test]
    fn completion_removes_the_item_and_reports_it_once() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 10));
        state.take_patch(10);

        state.apply(Event::JobComplete {
            job_id: "block_1".into(),
            at_ms: 40,
        });

        let patch = state.take_patch(40).unwrap();
        assert_eq!(patch.removed_jobs, vec!["block_1".to_string()]);
        assert!(patch.jobs.is_empty(), "a removed item carries no state");
        assert_eq!(node(&patch, "committer").counters.in_flight, 0);
        assert_eq!(node(&patch, "committer").counters.left_total, 1);
    }

    #[test]
    fn enter_and_complete_within_one_tick_emit_only_a_removal() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 10));
        state.apply(Event::JobComplete {
            job_id: "block_1".into(),
            at_ms: 15,
        });

        let patch = state.take_patch(100).unwrap();
        assert!(patch.jobs.is_empty());
        assert_eq!(patch.removed_jobs, vec!["block_1".to_string()]);
    }

    #[test]
    fn dropped_guard_marks_the_item_abandoned_rather_than_losing_it() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "indexer", 10));
        state.take_patch(10);

        state.apply(Event::JobAbandon {
            job_id: "block_1".into(),
            at_ms: 50,
        });

        let patch = state.take_patch(50).unwrap();
        assert_eq!(patch.jobs[0].phase, JobPhase::Abandoned);
        assert!(
            patch.removed_jobs.is_empty(),
            "abandonment is a bug signal, so the item stays visible"
        );
        assert_eq!(
            node(&patch, "indexer").counters.in_flight,
            0,
            "an abandoned item is no longer in flight"
        );
    }

    #[test]
    fn quiet_pipeline_produces_no_patch() {
        let mut state = state_with_nodes();
        assert!(state.take_patch(1_000).is_none());
        assert!(state.take_patch(2_000).is_none());
    }

    #[test]
    fn dropped_event_count_alone_forces_a_patch() {
        let mut state = state_with_nodes();
        assert!(state.take_patch(1_000).is_none());

        state.set_dropped_events(7);
        let patch = state.take_patch(2_000).expect("the user must be told");
        assert_eq!(patch.dropped_events, 7);

        assert!(
            state.take_patch(3_000).is_none(),
            "an unchanged count is not news"
        );
    }

    #[test]
    fn percentiles_measure_time_spent_at_a_node() {
        let mut state = state_with_nodes();
        for (index, spent) in [10_u64, 20, 30, 40, 50, 60, 70, 80, 90, 100]
            .into_iter()
            .enumerate()
        {
            let job = format!("block_{index}");
            state.apply(enter(&job, "indexer", 1_000));
            state.apply(Event::JobComplete {
                job_id: job,
                at_ms: 1_000 + spent,
            });
        }

        let patch = state.take_patch(2_000).unwrap();
        let counters = node(&patch, "indexer").counters;
        assert_eq!(counters.p50_ms, 50);
        assert_eq!(counters.p95_ms, 100);
        assert_eq!(counters.left_total, 10);
    }

    #[test]
    fn samples_older_than_the_window_stop_counting() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "indexer", 1_000));
        state.apply(Event::JobComplete {
            job_id: "block_1".into(),
            at_ms: 1_500,
        });
        let patch = state.take_patch(2_000).unwrap();
        assert_eq!(node(&patch, "indexer").counters.p50_ms, 500);

        let patch = state.take_patch(1_500 + WINDOW_MS + 1).unwrap();
        let counters = node(&patch, "indexer").counters;
        assert_eq!(counters.p50_ms, 0, "the sample aged out of the window");
        assert_eq!(counters.throughput_per_sec, 0.0);
        assert_eq!(
            counters.left_total, 1,
            "the lifetime total is not a windowed figure"
        );
    }

    #[test]
    fn throughput_is_items_leaving_per_second() {
        let mut state = CollectorState::new();
        state.apply(register("indexer", 0));
        for index in 0..120 {
            let job = format!("block_{index}");
            state.apply(enter(&job, "indexer", 0));
            state.apply(Event::JobComplete {
                job_id: job,
                at_ms: 1,
            });
        }

        let patch = state.take_patch(WINDOW_MS).unwrap();
        assert_eq!(node(&patch, "indexer").counters.throughput_per_sec, 2.0);
    }

    #[test]
    fn snapshot_carries_full_state_so_a_new_client_needs_no_replay() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 10));
        state.apply(Event::JobHold {
            job_id: "block_1".into(),
            reason: "Waiting for finality".into(),
            at_ms: 20,
        });
        state.take_patch(30);

        let snapshot = state.snapshot(30);
        assert_eq!(snapshot.nodes.len(), 3);
        assert_eq!(snapshot.jobs.len(), 1);
        assert_eq!(
            snapshot.jobs[0].phase,
            JobPhase::Held {
                reason: "Waiting for finality".into()
            }
        );
    }

    #[test]
    fn snapshot_does_not_consume_pending_changes() {
        let mut state = state_with_nodes();
        state.apply(enter("block_1", "committer", 10));

        let _ = state.snapshot(10);
        let patch = state
            .take_patch(10)
            .expect("a connecting client must not starve existing ones");
        assert_eq!(patch.jobs.len(), 1);
    }

    #[test]
    fn queue_depth_is_reported_verbatim() {
        let mut state = state_with_nodes();
        state.apply(Event::QueueDepth {
            node_id: "indexer".into(),
            depth: 12,
            at_ms: 10,
        });

        let patch = state.take_patch(10).unwrap();
        assert_eq!(node(&patch, "indexer").counters.queue_depth, 12);
    }

    #[test]
    fn process_stats_are_reported_and_only_resent_when_they_change() {
        let mut state = state_with_nodes();

        state.apply(Event::ProcessStats {
            cpu_pct: 12.4,
            ram_mb: 148.2,
            at_ms: 100,
        });
        let patch = state.take_patch(100).unwrap();
        let process = patch.process.expect("first sample is a change");
        assert_eq!(process.cpu_pct, 12.4);
        assert_eq!(process.ram_mb, 148.2);

        state.apply(Event::ProcessStats {
            cpu_pct: 12.4,
            ram_mb: 148.2,
            at_ms: 200,
        });
        assert!(
            state.take_patch(200).is_none(),
            "an unchanged sample is not news"
        );
    }

    #[test]
    fn a_patch_omits_process_stats_that_did_not_change() {
        let mut state = state_with_nodes();
        state.apply(Event::ProcessStats {
            cpu_pct: 12.4,
            ram_mb: 148.2,
            at_ms: 100,
        });
        state.take_patch(100);

        state.apply(enter("block_1", "indexer", 110));
        let patch = state.take_patch(110).unwrap();
        assert!(
            patch.process.is_none(),
            "an unchanged reading is not resent on every tick"
        );
    }

    #[test]
    fn snapshot_includes_the_latest_process_stats() {
        let mut state = state_with_nodes();
        state.apply(Event::ProcessStats {
            cpu_pct: 9.0,
            ram_mb: 64.0,
            at_ms: 10,
        });

        let snapshot = state.snapshot(10);
        assert_eq!(snapshot.process.map(|p| p.ram_mb), Some(64.0));
    }

    #[test]
    fn percentile_of_nothing_is_zero() {
        assert_eq!(percentile(&[], 50), 0);
        assert_eq!(percentile(&[7], 50), 7);
        assert_eq!(percentile(&[7], 95), 7);
    }
}
