//! Exercises the crate the way a user does: build a tracker, track items, read
//! state back. The collector's own logic is unit-tested in `src/collector.rs`;
//! these tests cover the wiring between the public API and that logic.

#![cfg(feature = "viz")]

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Duration;

use pipeline_viz::{JobPhase, NodeKind, PipelineTracker, Snapshot};

/// Give the collector task a chance to drain the channel and tick.
async fn settle(tracker: &PipelineTracker) -> Snapshot {
    tokio::time::sleep(Duration::from_millis(60)).await;
    tracker.snapshot()
}

fn tracker() -> PipelineTracker {
    PipelineTracker::builder()
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime")
}

#[tokio::test]
async fn explicit_job_ids_remain_unrestricted_and_unchanged() {
    // Given
    let tracker = tracker();

    // When
    let prefix_shaped = tracker.job("indexer").id("job_1_1").start();
    let domain_shaped = tracker.job("indexer").id("block_42").start();

    // Then
    assert_eq!(prefix_shaped.id(), "job_1_1");
    assert_eq!(domain_shaped.id(), "block_42");
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_job_ids_are_unique_across_concurrent_clones() {
    // Given
    let tracker = tracker();
    let (sender, receiver) = mpsc::channel();

    // When
    std::thread::scope(|scope| {
        for _ in 0..100 {
            let tracker = tracker.clone();
            let sender = sender.clone();
            scope.spawn(move || {
                for _ in 0..100 {
                    let job = tracker.job("indexer").start();
                    sender.send(job.id().to_string()).unwrap();
                    job.complete();
                }
            });
        }
    });
    drop(sender);
    let ids: Vec<_> = receiver.into_iter().collect();

    // Then
    assert_eq!(ids.len(), 10_000);
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 10_000);
    assert!(ids.iter().all(|id| {
        let mut parts = id.split('_');
        matches!(
            (parts.next(), parts.next(), parts.next(), parts.next()),
            (Some("job"), Some(instance), Some(sequence), None)
                if instance.parse::<u64>().is_ok() && sequence.parse::<u64>().is_ok()
        )
    }));
}

#[tokio::test]
async fn generated_job_ids_use_distinct_tracker_prefixes() {
    // Given
    let first_tracker = tracker();
    let second_tracker = tracker();

    // When
    let first = first_tracker.job("indexer").start();
    let second = second_tracker.job("indexer").start();
    let first_prefix = first.id().rsplit_once('_').unwrap().0;
    let second_prefix = second.id().rsplit_once('_').unwrap().0;

    // Then
    assert_ne!(first_prefix, second_prefix);
}

#[tokio::test]
async fn an_item_held_with_a_reason_is_visible_with_that_reason() {
    let tracker = tracker();
    tracker.register_node_named(
        "committer",
        "Database Committer",
        NodeKind::Sink,
        ["indexer"],
    );

    let mut job = tracker
        .job("committer")
        .id(2_049_102)
        .job_type("Block")
        .meta("tx_count", 142)
        .start();
    job.hold("Waiting for finality (2/12 confirmations)");

    let snapshot = settle(&tracker).await;
    let tracked = &snapshot.jobs[0];

    assert_eq!(tracked.job_id, "2049102");
    assert_eq!(tracked.job_type, "Block");
    assert_eq!(tracked.current_node, "committer");
    assert_eq!(
        tracked.meta.get("tx_count").map(String::as_str),
        Some("142")
    );
    assert_eq!(
        tracked.phase,
        JobPhase::Held {
            reason: "Waiting for finality (2/12 confirmations)".into()
        }
    );

    let committer = &snapshot.nodes[0];
    assert_eq!(committer.display_name, "Database Committer");
    assert_eq!(committer.counters.in_flight, 1);
}

#[tokio::test]
async fn completing_an_item_removes_it_from_the_pipeline() {
    let tracker = tracker();

    let job = tracker.job("committer").id("block_1").start();
    job.complete();

    let snapshot = settle(&tracker).await;
    assert!(snapshot.jobs.is_empty());
    assert_eq!(snapshot.nodes[0].counters.left_total, 1);
    assert_eq!(snapshot.nodes[0].counters.in_flight, 0);
}

#[tokio::test]
async fn an_item_dropped_on_an_early_return_shows_up_as_abandoned() {
    let tracker = tracker();

    // The bug this exists to catch: `?` returns before `complete()` is reached.
    fn process(tracker: &PipelineTracker) -> Result<(), &'static str> {
        let mut job = tracker.job("indexer").id("block_7").start();
        job.hold("Decoding receipts");
        Err("decode failed")?;
        job.complete();
        Ok(())
    }

    assert!(process(&tracker).is_err());

    let snapshot = settle(&tracker).await;
    assert_eq!(snapshot.jobs.len(), 1, "the item must not silently vanish");
    assert_eq!(snapshot.jobs[0].job_id, "block_7");
    assert_eq!(snapshot.jobs[0].phase, JobPhase::Abandoned);
}

#[tokio::test]
async fn an_item_moving_between_nodes_carries_its_identity() {
    let tracker = tracker();

    let mut job = tracker
        .job("fetcher")
        .id("block_9")
        .job_type("Block")
        .start();
    job.move_to("indexer");
    job.move_to("committer");

    let snapshot = settle(&tracker).await;
    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.jobs[0].current_node, "committer");
    assert_eq!(
        snapshot.jobs[0].job_type, "Block",
        "job_type set at the first node survives later moves"
    );

    let in_flight: u32 = snapshot.nodes.iter().map(|n| n.counters.in_flight).sum();
    assert_eq!(in_flight, 1, "an item is at exactly one node");
}

#[tokio::test]
async fn a_full_channel_drops_events_instead_of_blocking_the_pipeline() {
    let tracker = PipelineTracker::builder()
        .channel_capacity(1)
        .tick(Duration::from_secs(3_600))
        .start_background()
        .expect("started inside a runtime");

    // A tight loop on a current-thread runtime never yields, so the collector
    // cannot drain. Every one of these calls must still return immediately.
    for index in 0..10_000 {
        tracker.job("indexer").id(index).start().complete();
    }

    assert!(
        tracker.dropped_events() > 0,
        "the channel should have overflowed"
    );

    let snapshot = settle(&tracker).await;
    assert!(
        snapshot.dropped_events > 0,
        "the loss must be reported to the dashboard, not hidden"
    );
}

#[tokio::test]
async fn queue_depth_reported_by_the_host_is_surfaced() {
    let tracker = tracker();
    tracker.register_node("indexer", NodeKind::Transform, ["fetcher"]);
    tracker.report_queue_depth("indexer", 12);

    let snapshot = settle(&tracker).await;
    let indexer = snapshot
        .nodes
        .iter()
        .find(|n| n.node_id == "indexer")
        .expect("indexer registered");
    assert_eq!(indexer.counters.queue_depth, 12);
}

#[test]
fn starting_outside_a_runtime_fails_without_panicking() {
    let error = PipelineTracker::builder().start_background().unwrap_err();
    assert!(error.to_string().contains("Tokio runtime"));
}

#[tokio::test]
async fn wire_format_is_stable() {
    let tracker = tracker();
    let mut job = tracker
        .job("committer")
        .id("block_1")
        .job_type("Block")
        .start();
    job.hold("Waiting for finality");

    let snapshot = settle(&tracker).await;
    let json = serde_json::to_value(pipeline_viz::ServerMessage::Snapshot(snapshot)).unwrap();

    assert_eq!(json["type"], "snapshot");
    let job = &json["jobs"][0];
    assert_eq!(job["job_id"], "block_1");
    assert_eq!(job["current_node"], "committer");
    assert_eq!(job["phase"], "held");
    assert_eq!(job["reason"], "Waiting for finality");
}

#[tokio::test]
async fn process_metrics_reach_the_snapshot() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime");

    // The sampler's first tick lands after one second, and CPU needs a second
    // refresh before it means anything.
    tokio::time::sleep(Duration::from_millis(2_500)).await;

    let process = tracker
        .snapshot()
        .process
        .expect("process metrics are on by default");
    assert!(process.ram_mb > 0.0, "a running process uses memory");
    assert!(process.cpu_pct >= 0.0);
}

#[tokio::test]
async fn process_metrics_can_be_turned_off() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .enable_process_metrics(false)
        .start_background()
        .expect("started inside a runtime");

    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(tracker.snapshot().process.is_none());
}
