//! Exercises the dashboard server the way a browser does: a real port, a real
//! socket, real JSON. The collector's logic is tested elsewhere; these tests
//! cover transport.

#![cfg(feature = "viz")]

use std::time::Duration;

use futures_util::StreamExt;
use pipeline_viz::PipelineTracker;
use tokio_tungstenite::tungstenite::Message;

fn tracker() -> PipelineTracker {
    PipelineTracker::builder()
        // Port 0 asks the OS for a free port, so tests never collide.
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime")
}

#[tokio::test]
async fn the_bound_port_is_reported_back() {
    let tracker = tracker();

    assert!(tracker.is_serving());
    assert_ne!(tracker.port(), 0, "port(0) must resolve to a real port");

    let body = http_get(&format!("http://127.0.0.1:{}/health", tracker.port())).await;
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn a_busy_port_disables_the_dashboard_without_failing() {
    let squatter = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let taken = squatter.local_addr().unwrap().port();

    let tracker = PipelineTracker::builder()
        .bind_port(taken)
        .start_background()
        .expect("a busy port is not a startup failure");

    assert!(!tracker.is_serving());

    // The pipeline keeps working with no dashboard.
    tracker.job("indexer").id("block_1").start().complete();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(tracker.snapshot().nodes.len(), 1);
}

/// Minimal HTTP GET over a raw socket, so the tests need no HTTP client crate.
async fn http_get(url: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let rest = url.strip_prefix("http://").expect("http url");
    let (authority, path) = rest.split_once('/').expect("path present");
    let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
    stream
        .write_all(
            format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Connect a WebSocket client and return the stream.
async fn connect(
    tracker: &PipelineTracker,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://127.0.0.1:{}/ws", tracker.port());
    let (stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connect");
    stream
}

async fn next_json(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> serde_json::Value {
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a message arrives within two seconds")
        .expect("the stream is open")
        .expect("the frame is valid");

    match message {
        Message::Text(text) => serde_json::from_str(&text).expect("valid JSON"),
        other => panic!("expected a text frame, got {other:?}"),
    }
}

#[tokio::test]
async fn a_connecting_client_receives_full_state_first() {
    let tracker = tracker();
    tracker.register_node_named(
        "committer",
        "Database Committer",
        pipeline_viz::NodeKind::Sink,
        ["indexer"],
    );
    let mut job = tracker
        .job("committer")
        .id("block_1")
        .job_type("Block")
        .start();
    job.hold("Waiting for finality");
    tokio::time::sleep(Duration::from_millis(60)).await;

    let mut stream = connect(&tracker).await;
    let first = next_json(&mut stream).await;

    assert_eq!(first["type"], "snapshot");
    assert_eq!(first["nodes"][0]["display_name"], "Database Committer");
    assert_eq!(first["jobs"][0]["job_id"], "block_1");
    assert_eq!(first["jobs"][0]["phase"], "held");
    assert_eq!(first["jobs"][0]["reason"], "Waiting for finality");
}

#[tokio::test]
async fn changes_after_the_snapshot_arrive_as_patches() {
    let tracker = tracker();
    let mut stream = connect(&tracker).await;

    let first = next_json(&mut stream).await;
    assert_eq!(first["type"], "snapshot");
    assert_eq!(first["jobs"].as_array().unwrap().len(), 0);

    let mut job = tracker
        .job("indexer")
        .id("block_5")
        .job_type("Block")
        .start();
    job.hold("Decoding receipts");

    let patch = next_json(&mut stream).await;
    assert_eq!(patch["type"], "patch");
    assert_eq!(patch["jobs"][0]["job_id"], "block_5");
    assert_eq!(patch["jobs"][0]["phase"], "held");
    assert_eq!(patch["jobs"][0]["reason"], "Decoding receipts");
}

#[tokio::test]
async fn a_completed_item_is_reported_as_a_removal() {
    let tracker = tracker();
    let mut stream = connect(&tracker).await;
    assert_eq!(next_json(&mut stream).await["type"], "snapshot");

    tracker.job("indexer").id("block_6").start().complete();

    // The enter and the completion coalesce into a single tick, so the item
    // is only ever reported as removed.
    let patch = next_json(&mut stream).await;
    assert_eq!(patch["type"], "patch");
    assert_eq!(patch["removed_jobs"][0], "block_6");
}

#[tokio::test]
async fn two_clients_both_receive_the_same_patches() {
    let tracker = tracker();
    let mut first_client = connect(&tracker).await;
    let mut second_client = connect(&tracker).await;
    assert_eq!(next_json(&mut first_client).await["type"], "snapshot");
    assert_eq!(next_json(&mut second_client).await["type"], "snapshot");

    tracker.job("indexer").id("block_8").start();

    assert_eq!(
        next_json(&mut first_client).await["jobs"][0]["job_id"],
        "block_8"
    );
    assert_eq!(
        next_json(&mut second_client).await["jobs"][0]["job_id"],
        "block_8"
    );
}

#[tokio::test]
async fn a_client_that_stops_reading_is_disconnected_rather_than_desynchronized() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(1))
        .start_background()
        .expect("started inside a runtime");

    let mut stream = connect(&tracker).await;
    assert_eq!(next_json(&mut stream).await["type"], "snapshot");

    // Never read again, while generating far more patch data than the kernel's
    // socket buffers can hold. This loop occupies the task, so nothing drains
    // the client side until it returns; bulky metadata makes the server's send
    // side block quickly, after which the 256-message broadcast buffer
    // overruns. With small items the buffers can absorb the whole flood and no
    // lag ever occurs.
    for index in 0..30_000_u64 {
        tracker
            .job("indexer")
            .id(index)
            .meta("payload", "x".repeat(1024))
            .start();
        if index % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    // Drain until the server closes the connection. It must close rather than
    // continue with a gap.
    let closed = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(frame) = stream.next().await {
            if frame.is_err() || matches!(frame, Ok(Message::Close(_))) {
                return true;
            }
        }
        true
    })
    .await
    .expect("the server closes the connection rather than hanging");

    assert!(closed);
}
