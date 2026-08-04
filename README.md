# pipeline-viz

Live item-level visibility for Rust data pipelines.

`tracing` shows spans. `tokio-console` shows tasks. Prometheus shows counters. None of them answer the question that actually stalls a debugging session:

> Where is block 2049102 right now, and why has it not moved in forty seconds?

`pipeline-viz` answers exactly that. Declare your stages, wrap each work item in a guard, and give a reason whenever an item is parked.

## Status

**Work in progress — not yet published to crates.io.**

| Milestone | State |
|---|---|
| 1. Instrumentation API + state collector | Done |
| 2. Embedded HTTP/WebSocket server | Done |
| 3. Dashboard UI | Next |
| 4. Single-binary embedding, macros, publish | Planned |

The dashboard UI does not exist yet. Today the crate serves the event stream over a WebSocket; see [Watching the stream](#watching-the-stream).

## Usage

```toml
[dependencies]
pipeline-viz = { version = "0.1", features = ["viz"] }
```

```rust
use pipeline_viz::{NodeKind, PipelineTracker};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracker = PipelineTracker::builder()
        .bind_port(9999)
        .start_background()?;

    tracker.register_node_named("committer", "Database Committer", NodeKind::Sink, ["indexer"]);

    let mut job = tracker
        .job("committer")
        .id(2_049_102)
        .job_type("Block")
        .meta("tx_count", 142)
        .start();

    job.hold("Waiting for finality (2/12 confirmations)");
    // ... wait for confirmations ...

    job.update_reason("Writing to PostgreSQL");
    // ... write ...

    job.complete();
    Ok(())
}
```

An item that never reaches `complete()` — because a `?` returned early, say — is marked **abandoned** rather than silently vanishing. That is usually the bug you were looking for.

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

## Off by default

The crate does nothing unless the `viz` feature is enabled, and it is off by default. Without it, every call above compiles to an empty inlined body and none of the implementation's dependencies are built at all.

This is deliberate: nothing should be able to ship a listening port to production by accident. Enable `viz` in a dev profile, leave it off in release.

The claim is tested, not asserted — `tests/zero_overhead.rs` builds the default configuration and fails if `tokio`, `serde`, `axum`, or the UI embedder appear anywhere in the dependency tree.

## What it measures

Per node, all derived from the event stream:

- items in flight, and queue depth if your application reports one
- throughput (items leaving per second, 60-second rolling window)
- p50 and p95 time spent at the node

Per item: which node holds it, since when, and why it is held.

**Per-node CPU and RAM are deliberately absent.** Nodes are logical stages sharing one process and one thread pool, so those figures cannot be attributed to a single node honestly. Process-wide resource use will be reported as process-wide.

## Design notes

- The collector is a pure function of its event stream — no clock, no I/O — so the hard logic is unit-tested without a runtime, a socket, or a browser.
- In-flight counts are derived from the item map rather than incremented alongside it, so the two cannot drift apart.
- Instrumentation uses `try_send` and counts drops. It never applies backpressure to the pipeline it is watching; the dropped-event count is surfaced rather than hidden.
- Updates are coalesced on a 100ms tick. An item crossing five nodes within one tick is reported once, at the fifth.

Full design: [`docs/specs/2026-08-04-pipeline-viz-mvp-design.md`](docs/specs/2026-08-04-pipeline-viz-mvp-design.md).

## Development

```sh
cargo test --features viz
cargo test --no-default-features
cargo clippy --features viz --all-targets -- -D warnings
cargo fmt --check
```

Both feature configurations must build and pass on their own.

## License

MIT — see [LICENSE](LICENSE).
