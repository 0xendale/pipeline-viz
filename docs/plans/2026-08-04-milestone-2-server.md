# Milestone 2: Embedded HTTP/WebSocket Server — Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking. Implement task-by-task; each task ends with a commit and is independently reviewable.

**Goal:** Serve the collector's `Snapshot` and `Patch` messages over a WebSocket on a port the tracker binds at startup, verifiable with `websocat` before any frontend exists.

**Architecture:** A new `server` module holds an axum `Router` with three routes. It never touches `CollectorState` directly — it calls `CollectorHandle::snapshot()` once per client and forwards broadcast messages thereafter. The listener is bound **synchronously inside `start_background`** so the caller learns the real port immediately (important for `port(0)` in tests) and so a bind failure degrades to "no dashboard" rather than surfacing later from a detached task.

**Tech Stack:** axum 0.8 (`ws`, `http1`, `tokio` features), `futures-util` for the WebSocket sink/stream split, `serde_json` for the wire encoding. All behind the existing `viz` feature.

## Global Constraints

Copied from `docs/specs/2026-08-04-pipeline-viz-mvp-design.md` and `CLAUDE.md`:

- Rust edition 2021. `rustfmt` defaults, no custom config.
- Every new dependency is `optional = true` and listed under `viz = [...]`. Nothing new may appear in the default dependency tree.
- `tests/zero_overhead.rs` already lists `axum` in `FORBIDDEN`; it must keep passing.
- `collector` remains the sole owner of `PipelineState`. `server` subscribes; it never mutates.
- Every failure degrades to "no dashboard", never to a broken host application. No `unwrap()` in library code.
- Public API items need doc comments.
- Both configurations must pass on their own: `cargo test --features viz` and `cargo test --no-default-features`, plus `cargo clippy` on both with `-D warnings`, plus `cargo fmt --check`.
- Commits: conventional (`feat:`, `fix:`, `test:`, `docs:`), imperative, no trailing period, no agent mention anywhere in commit content.

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Add axum, futures-util, serde_json as optional deps under `viz`; add `tokio-tungstenite` and `futures-util` as dev-deps for the test client |
| `src/server.rs` (create) | Router, route handlers, per-client WebSocket loop. Knows nothing about how state is computed |
| `src/runtime.rs` (modify) | Expose `CollectorHandle::subscribe()` (drop the `#[allow(dead_code)]`) |
| `src/tracker.rs` (modify) | Bind the listener in `start_background`, spawn the server, report the real port |
| `src/lib.rs` (modify) | Declare `mod server` under the `viz` feature |
| `tests/server.rs` (create) | End-to-end: real port, real WebSocket client, real JSON |
| `examples/fake_indexer.rs` (create) | A four-stage pipeline that stalls items on purpose — the manual `websocat` verification vehicle |

**Note on the example:** the spec assigns `examples/fake_indexer.rs` to milestone 3. It is pulled forward into Task 5 here because milestone 2's own acceptance criterion is "verifiable with `websocat`", and that needs something producing events. It stays in place for milestone 3.

---

### Task 1: Bind the listener and serve a health route

**Files:**
- Modify: `Cargo.toml`
- Create: `src/server.rs`
- Modify: `src/lib.rs`, `src/tracker.rs`, `src/runtime.rs`
- Test: `tests/server.rs`

**Interfaces:**
- Consumes: `CollectorHandle` (`src/runtime.rs`), `TrackerBuilder::start_background` (`src/tracker.rs`)
- Produces:
  - `pub(crate) fn serve(listener: std::net::TcpListener, collector: CollectorHandle)` — spawns the server task
  - `PipelineTracker::port() -> u16` now returns the **actually bound** port
  - `PipelineTracker::is_serving() -> bool`

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml`:

```toml
[dependencies]
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
tokio = { version = "1", features = ["sync", "rt", "time", "macros", "net"], optional = true }
axum = { version = "0.8", default-features = false, features = ["ws", "http1", "tokio"], optional = true }
futures-util = { version = "0.3", default-features = false, features = ["sink"], optional = true }

[dev-dependencies]
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "net"] }
tokio-tungstenite = "0.24"
futures-util = "0.3"

[features]
default = []
viz = ["dep:serde", "dep:serde_json", "dep:tokio", "dep:axum", "dep:futures-util"]
```

- [ ] **Step 2: Write the failing test**

Create `tests/server.rs`:

```rust
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

/// Minimal HTTP GET over a raw socket, so the tests need no HTTP client crate.
async fn http_get(url: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let rest = url.strip_prefix("http://").expect("http url");
    let (authority, path) = rest.split_once('/').expect("path present");
    let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
    stream
        .write_all(format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").as_bytes())
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
```

- [ ] **Step 3: Run it and confirm it fails**

Run: `cargo test --features viz --test server`
Expected: FAIL — `no method named 'is_serving' found for struct 'PipelineTracker'`

- [ ] **Step 4: Write the server module**

Create `src/server.rs`:

```rust
//! Serves the dashboard. Subscribes to collector output; never mutates state.

use axum::routing::get;
use axum::Router;

use crate::runtime::CollectorHandle;

/// Shared with every request handler.
#[derive(Clone, Debug)]
pub(crate) struct ServerState {
    pub(crate) collector: CollectorHandle,
}

/// Take over an already-bound listener and serve until the process exits.
///
/// The listener is bound by the caller so that a bind failure is reported
/// synchronously, and so the real port is known before this task starts.
pub(crate) fn serve(listener: std::net::TcpListener, collector: CollectorHandle) {
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("pipeline-viz: dashboard disabled ({error})");
                return;
            }
        };

        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/", get(index))
            .with_state(ServerState { collector });

        // A serving failure means no dashboard. It must never take the host
        // pipeline down with it.
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("pipeline-viz: dashboard stopped ({error})");
        }
    });
}

/// Placeholder until the dashboard UI is embedded in milestone 4.
async fn index() -> &'static str {
    "pipeline-viz is running. The dashboard UI is not built yet; connect to /ws for the event stream."
}
```

- [ ] **Step 5: Declare the module**

In `src/lib.rs`, alongside the other `viz`-gated modules:

```rust
#[cfg(feature = "viz")]
mod server;
```

- [ ] **Step 6: Bind in the builder and report the real port**

In `src/runtime.rs`, delete the `#[allow(dead_code)]` attribute above `subscribe` — it now has a caller.

In `src/tracker.rs`, replace the body of `start_background` and the `port` accessor:

```rust
    /// Start the collector and the dashboard server on the current Tokio runtime.
    ///
    /// Binding happens here rather than inside the spawned task so the caller
    /// learns the real port immediately, and so a busy port is reported as a
    /// warning rather than vanishing into a detached task. A bind failure
    /// leaves the tracker fully functional with no dashboard.
    pub fn start_background(self) -> Result<PipelineTracker, Error> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(Error::NoRuntime);
        }

        let (sender, receiver) = mpsc::channel(self.channel_capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let collector = spawn_collector(receiver, Arc::clone(&dropped), self.tick);

        let listener = std::net::TcpListener::bind(("127.0.0.1", self.port));
        let (bound_port, serving) = match listener {
            Ok(listener) => {
                let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(self.port);
                match listener.set_nonblocking(true) {
                    Ok(()) => {
                        crate::server::serve(listener, collector.clone());
                        (port, true)
                    }
                    Err(error) => {
                        eprintln!("pipeline-viz: dashboard disabled ({error})");
                        (self.port, false)
                    }
                }
            }
            Err(error) => {
                eprintln!(
                    "pipeline-viz: dashboard disabled, port {} unavailable ({error})",
                    self.port
                );
                (self.port, false)
            }
        };

        Ok(PipelineTracker {
            inner: Arc::new(TrackerInner {
                sender,
                dropped,
                collector,
                port: bound_port,
                serving,
            }),
        })
    }
```

Add `serving: bool` to `TrackerInner`, and add the accessor next to `port`:

```rust
    /// Whether the dashboard server is actually listening.
    ///
    /// False when the port was unavailable. The tracker still works; there is
    /// simply nothing to connect a browser to.
    pub fn is_serving(&self) -> bool {
        self.inner.serving
    }
```

Add the matching no-op in `src/noop.rs`, next to `port`:

```rust
    #[inline(always)]
    pub fn is_serving(&self) -> bool {
        false
    }
```

- [ ] **Step 7: Run the test and confirm it passes**

Run: `cargo test --features viz --test server`
Expected: PASS, 1 test

- [ ] **Step 8: Confirm a busy port degrades instead of failing**

Add to `tests/server.rs`:

```rust
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
```

Run: `cargo test --features viz --test server`
Expected: PASS, 2 tests

- [ ] **Step 9: Verify both configurations and commit**

```bash
cargo test --features viz
cargo test --no-default-features
cargo clippy --features viz --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --check
git add Cargo.toml src/server.rs src/lib.rs src/tracker.rs src/runtime.rs src/noop.rs tests/server.rs
git commit -m "feat: bind the dashboard listener and serve a health route

Bind in start_background rather than in the spawned task so the caller
learns the real port and a busy port degrades to a warning."
```

---

### Task 2: Send a snapshot when a client connects

**Files:**
- Modify: `src/server.rs`
- Test: `tests/server.rs`

**Interfaces:**
- Consumes: `CollectorHandle::snapshot() -> Snapshot`, `ServerState` (Task 1)
- Produces: `GET /ws`, whose first frame is always `{"type":"snapshot",...}`

- [ ] **Step 1: Write the failing test**

Append to `tests/server.rs`:

```rust
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

/// Connect a WebSocket client and return the stream.
async fn connect(
    tracker: &PipelineTracker,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://127.0.0.1:{}/ws", tracker.port());
    let (stream, _) = tokio_tungstenite::connect_async(url).await.expect("ws connect");
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
    let mut job = tracker.job("committer").id("block_1").job_type("Block").start();
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
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test --features viz --test server a_connecting_client`
Expected: FAIL — the connection is refused or returns 404, since `/ws` does not exist

- [ ] **Step 3: Add the route and the connection handler**

In `src/server.rs`, add the imports and the handler, and register the route:

```rust
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::SinkExt;

use crate::model::ServerMessage;
```

Add to the router in `serve`, before `.with_state(...)`:

```rust
            .route("/ws", get(websocket_upgrade))
```

And the handlers:

```rust
async fn websocket_upgrade(
    upgrade: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    upgrade.on_upgrade(move |socket| client_loop(socket, state))
}

/// One task per connected dashboard client.
async fn client_loop(mut socket: WebSocket, state: ServerState) {
    let snapshot = state.collector.snapshot();

    let Ok(encoded) = serde_json::to_string(&ServerMessage::Snapshot(snapshot)) else {
        return;
    };
    // A send failure means the client is gone, which needs no handling
    // beyond ending this task.
    let _ = socket.send(Message::Text(encoded.into())).await;
}
```

Note on axum 0.8: `Message::Text` holds a `Utf8Bytes`, so a `String` needs `.into()`. If the compiler reports a type mismatch here, that conversion is the cause.

- [ ] **Step 4: Run the test and confirm it passes**

Run: `cargo test --features viz --test server a_connecting_client`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/server.rs tests/server.rs
git commit -m "feat: send a full snapshot on websocket connect

Full state on connect is what lets the protocol carry no event replay:
a newly opened tab is correct immediately."
```

---

### Task 3: Stream patches after the snapshot

**Files:**
- Modify: `src/server.rs`
- Test: `tests/server.rs`

**Interfaces:**
- Consumes: `CollectorHandle::subscribe() -> broadcast::Receiver<Arc<ServerMessage>>`
- Produces: a `/ws` stream of `{"type":"patch",...}` frames after the snapshot

**Ordering hazard this task must solve:** if the handler subscribes *after* taking the snapshot, patches emitted in between are lost. If it subscribes *before* and forwards everything, a patch older than the snapshot can arrive after it and roll state backwards, because patch entries carry whole values rather than increments. The fix: subscribe first, snapshot second, then discard any buffered patch whose `ts_ms` is older than the snapshot's.

- [ ] **Step 1: Write the failing test**

Append to `tests/server.rs`:

```rust
#[tokio::test]
async fn changes_after_the_snapshot_arrive_as_patches() {
    let tracker = tracker();
    let mut stream = connect(&tracker).await;

    let first = next_json(&mut stream).await;
    assert_eq!(first["type"], "snapshot");
    assert_eq!(first["jobs"].as_array().unwrap().len(), 0);

    let mut job = tracker.job("indexer").id("block_5").job_type("Block").start();
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

    assert_eq!(next_json(&mut first_client).await["jobs"][0]["job_id"], "block_8");
    assert_eq!(next_json(&mut second_client).await["jobs"][0]["job_id"], "block_8");
}
```

- [ ] **Step 2: Run and confirm they fail**

Run: `cargo test --features viz --test server`
Expected: FAIL — the three new tests time out at `next_json`, since nothing is sent after the snapshot

- [ ] **Step 3: Subscribe before snapshotting, then forward**

Replace `client_loop` in `src/server.rs`:

```rust
/// One task per connected dashboard client.
///
/// Subscription happens *before* the snapshot is taken, so no patch emitted
/// during setup is lost. The cost is that the buffer may hold patches older
/// than the snapshot; those are discarded by timestamp, because patch entries
/// carry whole values and replaying an old one would roll state backwards.
async fn client_loop(mut socket: WebSocket, state: ServerState) {
    let mut patches = state.collector.subscribe();
    let snapshot = state.collector.snapshot();
    let snapshot_ts = snapshot.ts_ms;

    let Ok(encoded) = serde_json::to_string(&ServerMessage::Snapshot(snapshot)) else {
        return;
    };
    if socket.send(Message::Text(encoded.into())).await.is_err() {
        return;
    }

    loop {
        match patches.recv().await {
            Ok(message) => {
                if let ServerMessage::Patch(patch) = message.as_ref() {
                    if patch.ts_ms < snapshot_ts {
                        continue;
                    }
                }
                let Ok(encoded) = serde_json::to_string(message.as_ref()) else {
                    continue;
                };
                if socket.send(Message::Text(encoded.into())).await.is_err() {
                    return;
                }
            }
            // Handled in Task 4.
            Err(_) => return,
        }
    }
}
```

- [ ] **Step 4: Run and confirm they pass**

Run: `cargo test --features viz --test server`
Expected: PASS, 6 tests

- [ ] **Step 5: Commit**

```bash
git add src/server.rs tests/server.rs
git commit -m "feat: stream coalesced patches to connected clients

Subscribe before snapshotting so no patch is lost during setup, then
drop buffered patches older than the snapshot: patch entries carry whole
values, so replaying an old one would roll state backwards."
```

---

### Task 4: Disconnect a client that falls too far behind

**Files:**
- Modify: `src/server.rs`
- Test: `tests/server.rs`

**Interfaces:**
- Consumes: `broadcast::error::RecvError::{Lagged, Closed}`
- Produces: no new API — a behavioral guarantee that a slow client cannot pin memory or receive a gapped stream

**Why this matters:** a client that stops reading fills its broadcast buffer. Continuing to send after a `Lagged` error would deliver a stream with an invisible hole, and the client's state would be silently wrong. Closing the socket forces a reconnect, which gets a fresh snapshot — correct by construction.

- [ ] **Step 1: Write the failing test**

Append to `tests/server.rs`:

```rust
#[tokio::test]
async fn a_client_that_stops_reading_is_disconnected_rather_than_desynchronized() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(1))
        .start_background()
        .expect("started inside a runtime");

    let mut stream = connect(&tracker).await;
    assert_eq!(next_json(&mut stream).await["type"], "snapshot");

    // Never read again, while generating far more patches than the broadcast
    // buffer (256) can hold.
    for index in 0..5_000 {
        tracker.job("indexer").id(index).start();
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
```

- [ ] **Step 2: Run and confirm it fails**

Run: `cargo test --features viz --test server a_client_that_stops_reading`
Expected: FAIL — the test times out, because the current `Err(_) => return` path drops the task without closing the socket cleanly, and lag is not distinguished from closure

- [ ] **Step 3: Handle lag explicitly**

In `src/server.rs`, replace the `Err(_) => return` arm:

```rust
            // The client stopped reading and missed messages. Closing forces a
            // reconnect, which gets a fresh snapshot; continuing would deliver
            // a stream with an invisible gap and leave the client silently wrong.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            // The collector shut down; the host pipeline is finished with us.
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
```

- [ ] **Step 4: Run and confirm it passes**

Run: `cargo test --features viz --test server`
Expected: PASS, 7 tests

- [ ] **Step 5: Commit**

```bash
git add src/server.rs tests/server.rs
git commit -m "fix: close the socket when a client falls behind

Continuing after a broadcast lag would deliver a stream with an
invisible gap. A reconnect gets a fresh snapshot instead."
```

---

### Task 5: Add the demo pipeline and document the stream

**Files:**
- Create: `examples/fake_indexer.rs`
- Modify: `Cargo.toml`, `README.md`

**Interfaces:**
- Consumes: the whole public API
- Produces: `cargo run --example fake_indexer --features viz`, a four-stage pipeline serving a live stream on port 9999

- [ ] **Step 1: Declare the example**

In `Cargo.toml`:

```toml
[[example]]
name = "fake_indexer"
required-features = ["viz"]
```

- [ ] **Step 2: Write the example**

Create `examples/fake_indexer.rs`:

```rust
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

    tracker.register_node_named("fetcher", "Block Fetcher", NodeKind::Source, [] as [&str; 0]);
    tracker.register_node_named("indexer", "Indexer Core", NodeKind::Transform, ["fetcher"]);
    tracker.register_node_named("validator", "Validator", NodeKind::Transform, ["indexer"]);
    tracker.register_node_named("committer", "Database Committer", NodeKind::Sink, ["validator"]);

    println!("pipeline-viz listening on http://127.0.0.1:{}", tracker.port());
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
```

- [ ] **Step 3: Run it and watch the stream by hand**

```sh
cargo run --example fake_indexer --features viz
```

In a second terminal:

```sh
websocat ws://127.0.0.1:9999/ws | head -5
```

Expected: one `{"type":"snapshot",...}` frame with four nodes, then a stream of `{"type":"patch",...}` frames. Within a minute, at least one item sits in `"phase":"held"` with a finality reason, and at least one shows `"phase":"abandoned"`.

If `websocat` is not installed: `brew install websocat`.

- [ ] **Step 4: Document the stream in the README**

In `README.md`, change the milestone table row for milestone 2 to `Done`, and add this section directly after the `## Usage` section:

````markdown
## Watching the stream

Until the dashboard UI lands, the event stream is readable directly:

```sh
cargo run --example fake_indexer --features viz
websocat ws://127.0.0.1:9999/ws
```

The first frame is the complete state:

```json
{"type":"snapshot","ts_ms":1785810000000,"nodes":[...],"jobs":[...],"dropped_events":0}
```

Every frame after it is a coalesced delta covering only what changed:

```json
{"type":"patch","ts_ms":1785810000100,"nodes":[...],"jobs":[...],"removed_jobs":["block_1"],"dropped_events":0}
```

A client that stops reading is disconnected rather than sent a stream with a
gap in it; reconnecting gets a fresh snapshot.
````

- [ ] **Step 5: Verify everything and commit**

```bash
cargo test --features viz
cargo test --no-default-features
cargo clippy --features viz --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --check
git add Cargo.toml examples/fake_indexer.rs README.md
git commit -m "feat: add the fake_indexer example and document the stream

A four-stage pipeline that holds and abandons items on purpose, so the
websocket stream can be verified by hand before the UI exists."
```

---

## Acceptance

Milestone 2 is done when all of the following hold:

1. `cargo test --features viz` and `cargo test --no-default-features` both pass.
2. `cargo clippy` on both configurations passes with `-D warnings`; `cargo fmt --check` is clean.
3. `tests/zero_overhead.rs` still passes — `axum`, `serde_json`, and `futures-util` must not appear in the default dependency tree.
4. `cargo run --example fake_indexer --features viz` serves a stream that `websocat` can read, beginning with a snapshot.
5. A busy port produces a warning and a working tracker, not an error.

## Deliberately Not In This Milestone

- Embedded UI assets and `rust-embed` — milestone 4
- The React dashboard — milestone 3
- Process-wide CPU and RAM in the header — needs `sysinfo`, lands with the UI that displays it
- Authentication, TLS, and binding to anything other than `127.0.0.1`
- Any client-to-server message: the WebSocket is one-directional in v0.1
