//! A real Tokio pipeline: three stages joined by bounded channels.
//!
//! Unlike `fake_indexer`, which spawns a task per item to make the dashboard
//! busy, this is shaped the way production code usually is — a producer, two
//! worker stages, `tokio::sync::mpsc` between them, backpressure included. It
//! is finite: it processes nine blocks and exits.
//!
//! Two modes, selected by one environment variable:
//!
//! ```sh
//! # Watch it in a browser (the default).
//! cargo run --example tokio_pipeline --features viz
//!
//! # Prove it works, with no browser and no human.
//! cargo build --example tokio_pipeline --features viz
//! PIPELINE_VIZ_EXAMPLE_MODE=self-check target/debug/examples/tokio_pipeline
//! ```
//!
//! Self-check binds an ephemeral port, connects to its own `/health` and `/ws`,
//! and asserts over the wire that the named hold reason, the abandoned item and
//! the completions all arrived. It exits non-zero if any of them did not.

use std::error::Error;
use std::time::Duration;

use futures_util::StreamExt;
use pipeline_viz::{JobGuard, NodeKind, PipelineTracker};
use tokio::sync::mpsc;

/// The block that stalls on finality. Its hold reason is what self-check reads
/// back off the wire, so it is spelled out in one place.
const STALLED_BLOCK: u64 = 42;
const STALL_REASON: &str = "Waiting for finality (2/12 confirmations)";

/// The block whose handler returns early, leaving the item abandoned.
const ABANDONED_BLOCK: u64 = 45;

/// The blocks to process. Small, finite, and deterministic.
const BLOCKS: std::ops::RangeInclusive<u64> = 40..=48;

/// Bounded, and deliberately small: a full channel is the interesting case.
const CHANNEL_CAPACITY: usize = 4;

/// Self-check gives the whole run this long, end to end.
const SELF_CHECK_BUDGET: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    match std::env::var("PIPELINE_VIZ_EXAMPLE_MODE").as_deref() {
        Ok("self-check") => self_check().await,
        Ok("interactive") | Err(_) => interactive().await,
        Ok(other) => Err(format!(
            "PIPELINE_VIZ_EXAMPLE_MODE must be `interactive` or `self-check`, got `{other}`"
        )
        .into()),
    }
}

/// Serve on the usual port, run the pipeline slowly enough to watch, then exit.
async fn interactive() -> Result<(), Box<dyn Error>> {
    let tracker = PipelineTracker::builder()
        .bind_port(9999)
        .start_background()?;
    register_nodes(&tracker);

    println!("READY port={} mode=interactive", tracker.port());
    println!("dashboard: http://127.0.0.1:{}", tracker.port());
    println!("stream:    ws://127.0.0.1:{}/ws", tracker.port());

    run_pipeline(&tracker, Duration::from_millis(250)).await;

    println!("pipeline finished; dropping the tracker stops the dashboard");
    Ok(())
}

/// Run the same pipeline against an ephemeral port and check it over HTTP and
/// the WebSocket, with no browser and no human involved.
async fn self_check() -> Result<(), Box<dyn Error>> {
    // One budget for the entire run. A hang is a failure, not a hung CI job.
    match tokio::time::timeout(SELF_CHECK_BUDGET, run_self_check()).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "self-check did not finish within {}s",
            SELF_CHECK_BUDGET.as_secs()
        )
        .into()),
    }
}

async fn run_self_check() -> Result<(), Box<dyn Error>> {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()?;
    register_nodes(&tracker);

    let port = tracker.port();
    if !tracker.is_serving() {
        return Err("the dashboard did not bind an ephemeral port".into());
    }
    println!("READY port={port} mode=self-check");

    let health = http_get(port, "/health").await?;
    if health.trim() != "ok" {
        return Err(format!("GET /health returned {health:?}, expected \"ok\"").into());
    }

    // Connect before the pipeline starts, so every transition arrives as a
    // patch rather than having to be inferred from the opening snapshot.
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .map_err(|error| format!("could not open the dashboard WebSocket: {error}"))?;

    let first = next_message(&mut socket).await?;
    if first["type"] != "snapshot" {
        return Err(format!("expected a snapshot first, got {}", first["type"]).into());
    }

    let pipeline = {
        let tracker = tracker.clone();
        tokio::spawn(async move { run_pipeline(&tracker, Duration::from_millis(40)).await })
    };

    let mut observed = Observed::default();
    while !observed.complete() {
        let message = next_message(&mut socket).await?;
        observed.absorb(&message);
    }

    pipeline.await?;

    println!(
        "self-check: held      {} reason={STALL_REASON:?}",
        block_id(STALLED_BLOCK)
    );
    println!("self-check: abandoned {}", block_id(ABANDONED_BLOCK));
    println!("self-check: completed {} items", observed.completed);
    println!("self-check: dropped_events={}", tracker.dropped_events());

    // Dropping the last handle must end the socket rather than leave it open.
    drop(tracker);
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(frame) = socket.next().await {
            if frame.is_err() {
                return;
            }
        }
    })
    .await;
    if closed.is_err() {
        return Err("the WebSocket stayed open after the tracker was dropped".into());
    }
    println!("self-check: the dashboard stopped with the tracker");

    println!("SELF-CHECK OK");
    Ok(())
}

/// What the self-check is waiting to see on the wire.
#[derive(Default)]
struct Observed {
    held: bool,
    abandoned: bool,
    completed: usize,
}

impl Observed {
    fn complete(&self) -> bool {
        // Every block except the stalled one and the abandoned one completes.
        let expected = BLOCKS.count() - 2;
        self.held && self.abandoned && self.completed >= expected
    }

    fn absorb(&mut self, message: &serde_json::Value) {
        for job in message["jobs"].as_array().into_iter().flatten() {
            let id = job["job_id"].as_str().unwrap_or_default();
            if id == block_id(STALLED_BLOCK)
                && job["phase"] == "held"
                && job["reason"] == STALL_REASON
            {
                self.held = true;
            }
            if id == block_id(ABANDONED_BLOCK) && job["phase"] == "abandoned" {
                self.abandoned = true;
            }
        }
        for removed in message["removed_jobs"].as_array().into_iter().flatten() {
            let id = removed.as_str().unwrap_or_default();
            if id != block_id(ABANDONED_BLOCK) {
                self.completed += 1;
            }
        }
    }
}

fn block_id(number: u64) -> String {
    format!("block_{number}")
}

fn register_nodes(tracker: &PipelineTracker) {
    tracker.register_node_named(
        "fetcher",
        "Block Fetcher",
        NodeKind::Source,
        [] as [&str; 0],
    );
    tracker.register_node_named("parser", "Receipt Parser", NodeKind::Transform, ["fetcher"]);
    tracker.register_node_named(
        "committer",
        "Database Committer",
        NodeKind::Sink,
        ["parser"],
    );
}

/// One item travelling between stages, carrying its own guard.
///
/// This is the shape that makes the dashboard useful: custody of the item and
/// custody of its visualization are the same thing, so an item that is dropped
/// on the floor is an item the dashboard shows as abandoned.
struct Block {
    number: u64,
    guard: JobGuard,
}

/// Producer -> parser -> committer, joined by bounded channels.
async fn run_pipeline(tracker: &PipelineTracker, unit: Duration) {
    let (to_parser, parser_inbox) = mpsc::channel::<Block>(CHANNEL_CAPACITY);
    let (to_committer, committer_inbox) = mpsc::channel::<Block>(CHANNEL_CAPACITY);

    let parser = tokio::spawn(parse_stage(
        tracker.clone(),
        parser_inbox,
        to_committer,
        unit,
    ));
    let committer = tokio::spawn(commit_stage(tracker.clone(), committer_inbox, unit));

    for number in BLOCKS {
        let guard = tracker
            .job("fetcher")
            .id(block_id(number))
            .job_type("Block")
            .meta("tx_count", number * 3)
            .start();

        // A bounded channel is the whole reason a queue depth exists to report.
        tracker.report_queue_depth("parser", queued(&to_parser));
        if to_parser.send(Block { number, guard }).await.is_err() {
            break;
        }
        tokio::time::sleep(unit).await;
    }
    drop(to_parser);

    let _ = parser.await;
    let _ = committer.await;
}

async fn parse_stage(
    tracker: PipelineTracker,
    mut inbox: mpsc::Receiver<Block>,
    outbox: mpsc::Sender<Block>,
    unit: Duration,
) {
    while let Some(mut block) = inbox.recv().await {
        block.guard.move_to("parser");
        block.guard.meta("parsed_by", "receipt-parser");
        tokio::time::sleep(unit).await;

        tracker.report_queue_depth("committer", queued(&outbox));
        if outbox.send(block).await.is_err() {
            return;
        }
    }
}

async fn commit_stage(_tracker: PipelineTracker, mut inbox: mpsc::Receiver<Block>, unit: Duration) {
    while let Some(mut block) = inbox.recv().await {
        block.guard.move_to("committer");

        if block.number == STALLED_BLOCK {
            // Parked with a reason, which is the question this crate exists to
            // answer: not "where is it" but "why has it not moved".
            block.guard.hold(STALL_REASON);
            tokio::time::sleep(unit * 4).await;
            block
                .guard
                .update_reason("Waiting for finality (11/12 confirmations)");
            tokio::time::sleep(unit * 2).await;
            block.guard.resume();
        }

        if block.number == ABANDONED_BLOCK {
            // The bug this crate is for: an early return that drops the item.
            // The guard goes with it, so the dashboard says so.
            continue;
        }

        tokio::time::sleep(unit).await;
        block.guard.complete();
    }
}

/// How many items are sitting in a bounded channel right now.
fn queued(sender: &mpsc::Sender<Block>) -> u32 {
    (sender.max_capacity() - sender.capacity()) as u32
}

/// Minimal HTTP GET over a raw socket, so the example needs no HTTP client.
async fn http_get(port: u16, path: &str) -> Result<String, Box<dyn Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;

    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string())
}

async fn next_message(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<serde_json::Value, Box<dyn Error>> {
    loop {
        let frame = socket
            .next()
            .await
            .ok_or("the dashboard closed the WebSocket before the run finished")??;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
            return Ok(serde_json::from_str(&text)?);
        }
    }
}
