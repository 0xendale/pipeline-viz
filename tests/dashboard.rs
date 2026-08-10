//! The dashboard has to come out of the binary, not off the developer's disk.
//!
//! These tests are the check that `build.rs` ran, that `rust-embed` packed what
//! it produced, and that the routes a browser actually requests are wired up.

#![cfg(feature = "viz")]

use std::time::Duration;

use pipeline_viz::PipelineTracker;

fn tracker() -> PipelineTracker {
    PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime")
}

#[tokio::test]
async fn the_root_serves_the_embedded_dashboard_document() {
    let tracker = tracker();
    let response = get(tracker.port(), "/").await;

    assert!(
        response.status.starts_with("HTTP/1.1 200"),
        "unexpected status: {}",
        response.status
    );
    assert!(
        response.header("content-type").contains("text/html"),
        "index.html must be served as HTML, got {:?}",
        response.header("content-type")
    );
    assert!(
        response.body.contains("<div id=\"root\"></div>"),
        "the served document is not the built dashboard: {}",
        &response.body[..response.body.len().min(200)]
    );
    // A cached document would pin a browser to a stale bundle hash forever.
    assert_eq!(response.header("cache-control"), "no-cache");
}

#[tokio::test]
async fn the_document_references_bundle_files_that_are_also_embedded() {
    let tracker = tracker();
    let document = get(tracker.port(), "/").await.body;

    let referenced = bundle_references(&document);
    assert!(
        !referenced.is_empty(),
        "the dashboard document references no bundle assets: {document}"
    );

    for path in referenced {
        let response = get(tracker.port(), &path).await;
        assert!(
            response.status.starts_with("HTTP/1.1 200"),
            "{path} is referenced by index.html but not embedded ({})",
            response.status
        );
        assert!(
            response.header("cache-control").contains("immutable"),
            "{path} is content-hashed and should be cached immutably"
        );
    }
}

#[tokio::test]
async fn an_unknown_route_falls_back_to_the_document_but_a_missing_bundle_file_does_not() {
    let tracker = tracker();

    // A reloaded client-side route must still load the dashboard.
    let route = get(tracker.port(), "/node/committer").await;
    assert!(route.status.starts_with("HTTP/1.1 200"));
    assert!(route.header("content-type").contains("text/html"));

    // A genuinely missing bundle file must not be answered with HTML, which
    // would surface as an unreadable syntax error in the browser console.
    let missing = get(tracker.port(), "/assets/does-not-exist.js").await;
    assert!(
        missing.status.starts_with("HTTP/1.1 404"),
        "expected 404, got {}",
        missing.status
    );
}

#[tokio::test]
async fn the_api_routes_still_win_over_the_asset_fallback() {
    let tracker = tracker();
    assert_eq!(get(tracker.port(), "/health").await.body.trim(), "ok");
}

/// Paths of `assets/…` files referenced by `src`/`href` attributes.
fn bundle_references(document: &str) -> Vec<String> {
    let mut found = Vec::new();
    for attribute in ["src=\"", "href=\""] {
        let mut rest = document;
        while let Some(start) = rest.find(attribute) {
            rest = &rest[start + attribute.len()..];
            let Some(end) = rest.find('"') else { break };
            let value = &rest[..end];
            if value.starts_with("/assets/") {
                found.push(value.to_string());
            }
            rest = &rest[end..];
        }
    }
    found
}

struct HttpResponse {
    status: String,
    headers: String,
    body: String,
}

impl HttpResponse {
    /// Header value, lowercased, or an empty string when absent.
    fn header(&self, name: &str) -> String {
        self.headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_ascii_lowercase())
            })
            .unwrap_or_default()
    }
}

/// Minimal HTTP GET over a raw socket, so the tests need no HTTP client crate.
async fn get(port: u16, path: &str) -> HttpResponse {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("the dashboard is listening");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .expect("request written");

    // Bundle files are binary-ish; read bytes and lossily decode for matching.
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("response read");
    let response = String::from_utf8_lossy(&raw).into_owned();

    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    let (status, headers) = head.split_once("\r\n").unwrap_or((head, ""));

    HttpResponse {
        status: status.to_string(),
        headers: headers.to_string(),
        body: body.to_string(),
    }
}
