//! The dashboard's background work belongs to the tracker, not to the process.
//!
//! Dropping the last `PipelineTracker` must stop the collector, the sampler,
//! the HTTP server and every open WebSocket, and must release the port. A
//! long-lived host process that starts and stops several pipelines cannot be
//! allowed to accumulate listeners.

#![cfg(feature = "viz")]

use std::time::Duration;

use futures_util::StreamExt;
use pipeline_viz::PipelineTracker;

fn tracker() -> PipelineTracker {
    PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime")
}

async fn health_ok(port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).await.is_err() {
        return false;
    }
    response.contains("ok")
}

/// Poll a condition until it holds, or fail after two seconds. Shutdown is
/// asynchronous by nature; asserting on a fixed sleep would be flaky.
async fn eventually(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{label} did not happen within two seconds");
}

async fn eventually_no_health(port: u16) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !health_ok(port).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the dashboard was still answering two seconds after the last handle dropped");
}

async fn connect(
    port: u16,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (stream, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("ws connect");
    stream
}

#[tokio::test]
async fn the_last_handle_dropping_stops_the_dashboard() {
    let tracker = tracker();
    let port = tracker.port();
    assert!(health_ok(port).await, "the dashboard answers while alive");

    drop(tracker);

    eventually_no_health(port).await;
}

#[tokio::test]
async fn a_clone_keeps_the_dashboard_alive() {
    let tracker = tracker();
    let port = tracker.port();
    let clone = tracker.clone();

    drop(tracker);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(health_ok(port).await, "a live clone still owns the service");
    assert!(clone.is_serving());

    drop(clone);
    eventually_no_health(port).await;
}

#[tokio::test]
async fn a_live_builder_or_guard_keeps_the_dashboard_alive() {
    let tracker = tracker();
    let port = tracker.port();
    let builder = tracker.job("indexer").id("block_1");
    let guard = tracker.job("indexer").id("block_2").start();

    drop(tracker);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(health_ok(port).await, "a builder and a guard hold handles");

    drop(builder);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(health_ok(port).await, "the guard still holds a handle");

    drop(guard);
    eventually_no_health(port).await;
}

#[tokio::test]
async fn dropping_before_the_tasks_are_polled_does_not_hang() {
    let finished = tokio::time::timeout(Duration::from_secs(5), async {
        let tracker = tracker();
        let port = tracker.port();
        // No await between start and drop: the spawned tasks have not run yet.
        drop(tracker);
        eventually_no_health(port).await;
    })
    .await;

    assert!(finished.is_ok(), "an immediate drop must not block");
}

#[tokio::test]
async fn an_open_websocket_is_closed_when_the_last_handle_drops() {
    let tracker = tracker();
    let mut stream = connect(tracker.port()).await;

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a snapshot arrives")
        .expect("the stream is open")
        .expect("the frame is valid");
    assert!(first.to_text().expect("text frame").contains("snapshot"));

    drop(tracker);

    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = stream.next().await {
            if frame.is_err() {
                return;
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the open socket must end, not hang");
}

#[tokio::test]
async fn a_blocked_client_still_gets_eof_when_the_last_handle_drops() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(1))
        .start_background()
        .expect("started inside a runtime");
    let mut stream = connect(tracker.port()).await;
    let _snapshot = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a snapshot arrives");

    // Flood the socket without reading it, so the server's send is parked on a
    // full kernel buffer. Cancellation has to unblock that send, not wait on it.
    for index in 0..5_000_u64 {
        tracker
            .job("indexer")
            .id(index)
            .meta("payload", "x".repeat(4096))
            .start();
        if index % 200 == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    drop(tracker);

    let ended = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(frame) = stream.next().await {
            if frame.is_err() {
                return;
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "a blocked client must reach EOF");
}

#[tokio::test]
async fn the_port_is_released_for_rebinding() {
    // Nothing ever connects, so no socket enters TIME_WAIT and a failure to
    // rebind can only mean the listener itself is still open.
    let tracker = tracker();
    let port = tracker.port();
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
        "the tracker holds the port while alive"
    );

    drop(tracker);

    let mut rebound = None;
    eventually("the port was released", || {
        rebound = std::net::TcpListener::bind(("127.0.0.1", port)).ok();
        rebound.is_some()
    })
    .await;
}

#[tokio::test]
async fn the_sampler_does_not_keep_the_dashboard_alive() {
    for process_metrics in [true, false] {
        let tracker = PipelineTracker::builder()
            .bind_port(0)
            .tick(Duration::from_millis(10))
            .enable_process_metrics(process_metrics)
            .start_background()
            .expect("started inside a runtime");
        let port = tracker.port();
        assert!(health_ok(port).await);

        drop(tracker);
        eventually_no_health(port).await;
    }
}

#[test]
fn destroying_the_runtime_clears_the_serving_status() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let tracker = runtime.block_on(async { tracker() });
    assert!(tracker.is_serving());

    // The tracker outlives its runtime. Its tasks are gone, so it is no longer
    // serving anything and must not claim otherwise.
    drop(runtime);

    assert!(!tracker.is_serving());
}
