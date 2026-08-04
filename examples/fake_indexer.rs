//! A synthetic blockchain indexer that stalls items on purpose.
//!
//! Run it, then watch the stream:
//!
//! ```sh
//! cargo run --example fake_indexer --features viz
//! websocat ws://127.0.0.1:9999/ws
//! ```

use std::time::Duration;

use pipeline_viz::{NodeKind, PipelineTracker};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracker = PipelineTracker::builder()
        .bind_port(9999)
        .start_background()?;

    tracker.register_node_named(
        "fetcher",
        "Block Fetcher",
        NodeKind::Source,
        [] as [&str; 0],
    );
    tracker.register_node_named("indexer", "Indexer Core", NodeKind::Transform, ["fetcher"]);
    tracker.register_node_named("validator", "Validator", NodeKind::Transform, ["indexer"]);
    tracker.register_node_named(
        "committer",
        "Database Committer",
        NodeKind::Sink,
        ["validator"],
    );

    println!(
        "pipeline-viz listening on http://127.0.0.1:{}",
        tracker.port()
    );
    println!("stream: websocat ws://127.0.0.1:{}/ws", tracker.port());

    for block_number in 2_049_100_u64.. {
        let tracker = tracker.clone();
        tokio::spawn(async move {
            process_block(tracker, block_number).await;
        });
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    Ok(())
}

async fn process_block(tracker: PipelineTracker, block_number: u64) {
    let mut job = tracker
        .job("fetcher")
        .id(block_number)
        .job_type("Block")
        .meta("tx_count", block_number % 200)
        .start();

    tokio::time::sleep(Duration::from_millis(120)).await;

    job.move_to("indexer");
    tokio::time::sleep(Duration::from_millis(300)).await;

    job.move_to("validator");

    // Every seventh block stalls on finality, so there is always something
    // held to look at.
    if block_number % 7 == 0 {
        for confirmations in 2..12 {
            job.update_reason(&format!(
                "Waiting for finality ({confirmations}/12 confirmations)"
            ));
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        job.resume();
    }

    job.move_to("committer");
    job.update_reason("Writing to PostgreSQL");
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Every eleventh block returns early, so the abandoned state is visible
    // without having to write a bug on purpose.
    if block_number % 11 == 0 {
        return;
    }

    job.complete();
}
