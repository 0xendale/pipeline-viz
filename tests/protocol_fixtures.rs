//! Keeps the Rust wire format and the TypeScript model from drifting apart.
//!
//! The fixtures under `ui/src/protocol/fixtures/` are parsed by the frontend's
//! own tests. Nothing else forces two descriptions of one format, in two
//! languages, to agree — so a change to the Rust model fails here, and a change
//! that the TypeScript model has not caught up with fails there.
//!
//! Regenerate after an intentional change:
//! `UPDATE_FIXTURES=1 cargo test --features viz --test protocol_fixtures`

#![cfg(feature = "viz")]

use std::collections::BTreeMap;
use std::path::Path;

use pipeline_viz::{
    JobPhase, JobState, NodeCounters, NodeKind, NodeState, Patch, ProcessStats, ServerMessage,
    Snapshot,
};

fn sample_node() -> NodeState {
    NodeState {
        node_id: "committer".into(),
        display_name: "Database Committer".into(),
        kind: NodeKind::Sink,
        inputs: vec!["validator".into()],
        counters: NodeCounters {
            in_flight: 3,
            queue_depth: 12,
            left_total: 1_402,
            throughput_per_sec: 2.49,
            p50_ms: 122,
            p95_ms: 310,
        },
    }
}

fn sample_job() -> JobState {
    JobState {
        job_id: "2049102".into(),
        job_type: "Block".into(),
        current_node: "committer".into(),
        phase: JobPhase::Held {
            reason: "Waiting for finality (2/12 confirmations)".into(),
        },
        entered_node_at_ms: 1_785_810_000_000,
        created_at_ms: 1_785_809_999_000,
        meta: BTreeMap::from([("tx_count".to_string(), "142".to_string())]),
    }
}

fn check(name: &str, value: &ServerMessage) {
    let encoded = serde_json::to_string_pretty(value).expect("serializes") + "\n";
    let path = Path::new("ui/src/protocol/fixtures").join(name);

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("creates the dir");
        std::fs::write(&path, &encoded).expect("writes the fixture");
        return;
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing fixture {}: {error}. Regenerate with UPDATE_FIXTURES=1",
            path.display()
        )
    });

    assert_eq!(
        existing, encoded,
        "the wire format changed. Update ui/src/protocol/types.ts to match, then \
         regenerate with UPDATE_FIXTURES=1"
    );
}

#[test]
fn snapshot_fixture_matches_the_current_wire_format() {
    check(
        "snapshot.json",
        &ServerMessage::Snapshot(Snapshot {
            ts_ms: 1_785_810_000_000,
            nodes: vec![sample_node()],
            jobs: vec![sample_job()],
            dropped_events: 0,
            process: Some(ProcessStats {
                cpu_pct: 12.4,
                ram_mb: 148.2,
            }),
        }),
    );
}

#[test]
fn patch_fixture_matches_the_current_wire_format() {
    check(
        "patch.json",
        &ServerMessage::Patch(Patch {
            ts_ms: 1_785_810_000_100,
            nodes: vec![sample_node()],
            jobs: vec![sample_job()],
            removed_jobs: vec!["2049101".into()],
            dropped_events: 7,
            process: None,
        }),
    );
}
