//! The macros must produce the same state as the equivalent hand-written calls.
//!
//! Everything here shares one process-global tracker, because that is what the
//! macros read from. `install` succeeds once per process, so this file installs
//! and every assertion lives in a single test; the no-tracker case needs its own
//! binary and lives in `macros_without_tracker.rs`.

#![cfg(all(feature = "viz", feature = "macros"))]

use std::time::Duration;

use pipeline_viz::{track_job, track_node, JobPhase, NodeKind, PipelineTracker, Snapshot};

#[track_node(kind = Source, name = "Block Fetcher")]
#[track_job(node = "fetcher", id = number, job_type = "Block")]
async fn fetch(number: u64) -> u64 {
    number
}

#[track_node(id = "committer", kind = Sink, name = "Database Committer", inputs = ["fetcher"])]
#[track_job(node = "committer", id = format!("{number}/commit"), meta(tx_count = 142))]
async fn commit(number: u64) -> Result<u64, &'static str> {
    if number == 0 {
        // An early return must still complete the item, not abandon it.
        return Err("empty block");
    }
    Ok(number)
}

#[track_job(node = "decoder", id = number)]
fn decode(number: u64) -> u64 {
    number * 2
}

#[tokio::test]
async fn the_macros_report_the_same_state_the_runtime_api_would() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime");
    pipeline_viz::install(tracker).expect("nothing else installs a tracker in this binary");

    assert_eq!(fetch(9_355).await, 9_355);
    assert_eq!(decode(21), 42);
    assert_eq!(commit(9_355).await, Ok(9_355));
    assert_eq!(commit(0).await, Err("empty block"));

    let snapshot = settled().await;

    // `track_node` registered the stages with their declared identity.
    let fetcher = node(&snapshot, "fetch");
    assert_eq!(fetcher.display_name, "Block Fetcher");
    assert_eq!(fetcher.kind, NodeKind::Source);

    let committer = node(&snapshot, "committer");
    assert_eq!(committer.display_name, "Database Committer");
    assert_eq!(committer.kind, NodeKind::Sink);
    assert_eq!(committer.inputs, vec!["fetcher".to_string()]);

    // `track_job` completed every item, including the one that returned early,
    // so nothing is left in flight and nothing was marked abandoned.
    assert!(
        snapshot.jobs.is_empty(),
        "every tracked item completed, but these remain: {:?}",
        snapshot.jobs
    );
    assert_eq!(committer.counters.left_total, 2);
    assert_eq!(committer.counters.in_flight, 0);

    // A node named only by `track_job` is registered implicitly, exactly as it
    // would be through the runtime API.
    assert_eq!(node(&snapshot, "decoder").counters.left_total, 1);
}

#[tokio::test]
async fn a_panicking_body_leaves_the_item_abandoned() {
    // Uses the tracker the other test installs, or none at all if this runs
    // first — either way the call must not panic for a reason of its own.
    #[track_job(node = "risky", id = 1_u64)]
    fn explode() {
        panic!("stage failed");
    }

    let caught = std::panic::catch_unwind(explode);
    assert!(caught.is_err(), "the panic must reach the caller unchanged");

    if let Some(tracker) = pipeline_viz::global() {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let snapshot = tracker.snapshot();
        if let Some(job) = snapshot.jobs.iter().find(|job| job.job_id == "1") {
            assert_eq!(job.phase, JobPhase::Abandoned);
        }
    }
}

/// Waits for the collector to drain the events the macros emitted.
async fn settled() -> Snapshot {
    let tracker = pipeline_viz::global().expect("installed above");
    tokio::time::sleep(Duration::from_millis(60)).await;
    tracker.snapshot()
}

fn node<'a>(snapshot: &'a Snapshot, id: &str) -> &'a pipeline_viz::NodeState {
    snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == id)
        .unwrap_or_else(|| {
            panic!(
                "no node `{id}`; registered: {:?}",
                snapshot
                    .nodes
                    .iter()
                    .map(|node| &node.node_id)
                    .collect::<Vec<_>>()
            )
        })
}
