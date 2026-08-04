//! Exercises the dashboard server the way a browser does: a real port, a real
//! socket, real JSON. The collector's logic is tested elsewhere; these tests
//! cover transport.

#![cfg(feature = "viz")]

use std::time::Duration;

use pipeline_viz::PipelineTracker;

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
