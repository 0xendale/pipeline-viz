use super::*;
use crate::model::{NodeKind, ServerMessage};
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

fn abandon(job: &str, at_ms: u64) -> Event {
    Event::JobAbandon {
        job_id: job.into(),
        at_ms,
    }
}

fn hold(job: &str, at_ms: u64) -> Event {
    Event::JobHold {
        job_id: job.into(),
        reason: "waiting".into(),
        at_ms,
    }
}

fn resume(job: &str, at_ms: u64) -> Event {
    Event::JobResume {
        job_id: job.into(),
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
fn abandoned_retention_matches_transition_table() {
    struct Case {
        name: &'static str,
        cap: usize,
        events: Vec<Event>,
        retained: Vec<(&'static str, JobPhase)>,
        fifo: Vec<&'static str>,
        patch_jobs: Vec<(&'static str, JobPhase)>,
        removed: Vec<&'static str>,
    }

    let cases = vec![
        Case {
            name: "duplicate abandon preserves FIFO position",
            cap: 2,
            events: vec![enter("A", "indexer", 1), abandon("A", 2), abandon("A", 3)],
            retained: vec![("A", JobPhase::Abandoned)],
            fifo: vec!["A"],
            patch_jobs: vec![("A", JobPhase::Abandoned)],
            removed: vec![],
        },
        Case {
            name: "re-entry removes A before C is retained",
            cap: 2,
            events: vec![
                enter("A", "indexer", 1),
                abandon("A", 2),
                enter("B", "indexer", 3),
                abandon("B", 4),
                enter("A", "indexer", 5),
                enter("C", "indexer", 6),
                abandon("C", 7),
            ],
            retained: vec![
                ("A", JobPhase::Active),
                ("B", JobPhase::Abandoned),
                ("C", JobPhase::Abandoned),
            ],
            fifo: vec!["B", "C"],
            patch_jobs: vec![
                ("A", JobPhase::Active),
                ("B", JobPhase::Abandoned),
                ("C", JobPhase::Abandoned),
            ],
            removed: vec![],
        },
        Case {
            name: "same-tick re-entry cancels pending removal",
            cap: 1,
            events: vec![
                enter("A", "indexer", 1),
                abandon("A", 2),
                enter("B", "indexer", 3),
                abandon("B", 4),
                enter("A", "indexer", 5),
            ],
            retained: vec![("A", JobPhase::Active), ("B", JobPhase::Abandoned)],
            fifo: vec!["B"],
            patch_jobs: vec![("A", JobPhase::Active), ("B", JobPhase::Abandoned)],
            removed: vec![],
        },
        Case {
            name: "completion after abandon removes once",
            cap: 2,
            events: vec![
                enter("A", "indexer", 1),
                abandon("A", 2),
                Event::JobComplete {
                    job_id: "A".into(),
                    at_ms: 3,
                },
            ],
            retained: vec![],
            fifo: vec![],
            patch_jobs: vec![],
            removed: vec!["A"],
        },
        Case {
            name: "zero cap retains no abandoned records",
            cap: 0,
            events: vec![enter("A", "indexer", 1), abandon("A", 2)],
            retained: vec![],
            fifo: vec![],
            patch_jobs: vec![],
            removed: vec!["A"],
        },
        Case {
            name: "hold then abandon re-enqueues once",
            cap: 2,
            events: vec![
                enter("A", "indexer", 1),
                abandon("A", 2),
                hold("A", 3),
                abandon("A", 4),
            ],
            retained: vec![("A", JobPhase::Abandoned)],
            fifo: vec!["A"],
            patch_jobs: vec![("A", JobPhase::Abandoned)],
            removed: vec![],
        },
        Case {
            name: "resume then abandon re-enqueues once",
            cap: 2,
            events: vec![
                enter("A", "indexer", 1),
                abandon("A", 2),
                resume("A", 3),
                abandon("A", 4),
            ],
            retained: vec![("A", JobPhase::Abandoned)],
            fifo: vec!["A"],
            patch_jobs: vec![("A", JobPhase::Abandoned)],
            removed: vec![],
        },
    ];

    for case in cases {
        let mut state = CollectorState::with_max_retained_abandoned(case.cap);
        for event in case.events {
            state.apply(event);
        }

        let snapshot = state.snapshot(10);
        let retained: Vec<_> = snapshot
            .jobs
            .iter()
            .map(|job| (job.job_id.as_str(), job.phase.clone()))
            .collect();
        let fifo: Vec<_> = state.abandoned_fifo.iter().cloned().collect();
        let patch = state.take_patch(10).expect(case.name);
        let patch_jobs: Vec<_> = patch
            .jobs
            .iter()
            .map(|job| (job.job_id.as_str(), job.phase.clone()))
            .collect();
        let removed: Vec<_> = patch.removed_jobs.iter().map(String::as_str).collect();

        assert_eq!(retained, case.retained, "{}: retained jobs", case.name);
        assert_eq!(fifo, case.fifo, "{}: FIFO", case.name);
        assert_eq!(patch_jobs, case.patch_jobs, "{}: patch jobs", case.name);
        assert_eq!(removed, case.removed, "{}: removals", case.name);
    }
}

#[test]
fn eviction_then_next_tick_reentry_emits_active_update() {
    let mut state = CollectorState::with_max_retained_abandoned(1);
    state.apply(enter("A", "indexer", 1));
    state.apply(abandon("A", 2));
    state.apply(enter("B", "indexer", 3));
    state.apply(abandon("B", 4));

    let first = state.take_patch(5).expect("eviction changes state");
    assert_eq!(first.jobs.len(), 1);
    assert_eq!(first.jobs[0].job_id, "B");
    assert_eq!(first.jobs[0].phase, JobPhase::Abandoned);
    assert_eq!(first.removed_jobs, vec!["A"]);

    state.apply(enter("A", "indexer", 6));
    let second = state.take_patch(7).expect("re-entry changes state");
    assert_eq!(second.jobs.len(), 1);
    assert_eq!(second.jobs[0].job_id, "A");
    assert_eq!(second.jobs[0].phase, JobPhase::Active);
    assert!(second.removed_jobs.is_empty());
    assert_eq!(state.abandoned_fifo, VecDeque::from(["B".to_string()]));
}

#[test]
fn eviction_removal_is_not_repeated_on_later_ticks() {
    let mut state = CollectorState::with_max_retained_abandoned(0);
    state.apply(enter("A", "indexer", 1));
    state.apply(abandon("A", 2));

    let first = state.take_patch(3).expect("eviction changes state");
    assert_eq!(first.removed_jobs, vec!["A"]);
    assert!(state.take_patch(4).is_none());
}

#[test]
fn abandoned_fifo_uses_application_order_not_event_timestamps() {
    let mut state = CollectorState::with_max_retained_abandoned(1);
    state.apply(enter("A", "indexer", 100));
    state.apply(abandon("A", 100));
    state.apply(enter("B", "indexer", 1));
    state.apply(abandon("B", 1));

    let snapshot = state.snapshot(101);
    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.jobs[0].job_id, "B");
    assert_eq!(state.abandoned_fifo, VecDeque::from(["B".to_string()]));
}

#[test]
fn active_and_held_jobs_are_immune_to_abandoned_eviction() {
    let mut state = CollectorState::with_max_retained_abandoned(1);
    state.apply(enter("active", "indexer", 1));
    state.apply(enter("held", "indexer", 2));
    state.apply(hold("held", 3));
    state.apply(enter("old_abandoned", "indexer", 4));
    state.apply(abandon("old_abandoned", 5));
    state.apply(enter("new_abandoned", "indexer", 6));
    state.apply(abandon("new_abandoned", 7));

    let snapshot = state.snapshot(8);
    let ids: Vec<_> = snapshot
        .jobs
        .iter()
        .map(|job| job.job_id.as_str())
        .collect();
    assert_eq!(ids, vec!["active", "held", "new_abandoned"]);
    assert_eq!(
        state.abandoned_fifo,
        VecDeque::from(["new_abandoned".to_string()])
    );
}

#[test]
fn default_retains_latest_thousand_of_ten_thousand_abandons() {
    let mut state = CollectorState::new();
    for index in 0..10_000 {
        let id = format!("job_{index:05}");
        state.apply(enter(&id, "indexer", index * 2));
        state.apply(abandon(&id, index * 2 + 1));
    }

    let snapshot = state.snapshot(20_000);
    assert_eq!(snapshot.jobs.len(), 1_000);
    assert_eq!(snapshot.jobs[0].job_id, "job_09000");
    assert_eq!(snapshot.jobs[999].job_id, "job_09999");
    assert_eq!(state.abandoned_fifo.len(), 1_000);
    assert_eq!(
        state.abandoned_fifo.front().map(String::as_str),
        Some("job_09000")
    );
    assert_eq!(
        state.abandoned_fifo.back().map(String::as_str),
        Some("job_09999")
    );
}

#[test]
fn representative_thousand_record_payload_stays_below_one_mebibyte() {
    let reason = "r".repeat(80);
    let jobs = (0..1_000)
        .map(|index| JobState {
            job_id: format!("job_{index:04}"),
            job_type: "Block".into(),
            current_node: "indexer".into(),
            phase: JobPhase::Held {
                reason: reason.clone(),
            },
            entered_node_at_ms: 1_000,
            created_at_ms: 900,
            meta: BTreeMap::from([
                ("height".into(), index.to_string()),
                ("source".into(), "representative".into()),
            ]),
        })
        .collect();
    let snapshot = Snapshot {
        ts_ms: 2_000,
        nodes: vec![],
        jobs,
        dropped_events: 0,
        process: None,
    };

    let encoded = serde_json::to_vec(&ServerMessage::Snapshot(snapshot)).expect("serializes");
    assert!(
        encoded.len() < 1024 * 1024,
        "payload was {} bytes",
        encoded.len()
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
