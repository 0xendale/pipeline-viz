# pipeline-viz

Live item-level visibility for Rust data pipelines.

`tracing` shows spans. `tokio-console` shows tasks. Prometheus shows counters. None of them answer the question that actually stalls a debugging session:

> Where is block 2049102 right now, and why has it not moved in forty seconds?

`pipeline-viz` answers exactly that. Declare your stages, wrap each work item in a guard, and give a reason whenever an item is parked.

![The three-d dashboard: four pipeline stages in 3D, with a stage rail on the left and a strip of the longest-waiting items along the bottom](docs/images/three-d-overview.png)

Add the dependency, add three lines to `main`, open `localhost:9999`. The dashboard is compiled into your binary — there is no static directory to deploy and no Node.js on the machine that runs it.

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

## Attribute macros

Enable the `macros` feature and the same instrumentation becomes two annotations:

```toml
[dependencies]
pipeline-viz = { version = "0.1", features = ["viz", "macros"] }
```

```rust
use pipeline_viz::{track_job, track_node};

#[track_node(id = "committer", kind = Sink, name = "Database Committer", inputs = ["indexer"])]
#[track_job(node = "committer", id = number, job_type = "Block", meta(tx_count = 142))]
async fn commit(number: u64) -> Result<(), Error> {
    // ... write to PostgreSQL ...
    Ok(())
}
```

`track_node` registers the stage on the function's first call. `track_job` opens a guard for the body and completes it when the body returns — including an early `return` or a `?` that yielded an `Err`. A panic drops the guard instead, which is what marks the item abandoned.

Annotated functions have nowhere to receive a tracker handle, so both macros read one installed at startup:

```rust
let tracker = PipelineTracker::builder().bind_port(9999).start_background()?;
pipeline_viz::install(tracker)?;
```

They expand to exactly the runtime calls shown above and hold no state of their own, so the two surfaces cannot drift apart. With nothing installed — or with `viz` off — the generated calls do nothing.

## Watching the stream

The event stream is also readable directly:

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

## The dashboard

It is already in your binary. Start your program and open the port you bound:

```sh
cargo run --example fake_indexer --features viz
open http://localhost:9999
```

The dashboard shows the pipeline graph with live per-node counters, a persistent
strip of the longest-waiting items with their hold reasons, and a per-node
drill-down listing what is sitting there and why. Stages turn amber when an item
has been there for more than ten seconds.

![The dashboard with a stage opened: its counters and percentiles on the right, each item at that stage listed with its age, one of them abandoned](docs/images/three-d-dashboard.png)

Each stage takes a form from its kind — a portal for a source, a prism for a
transform, an archive rack for a sink — and items are physical objects on it:
teal moving, amber held, red abandoned. Queued items orbit outside the stage.
The rail on the left names the stall directly, so a stuck stage is legible
without opening anything.

![The pipeline in 3D: six stages laid out along their edges, the sink ringed amber because items are stalled there](docs/images/three-d-scene.png)

Click a stage or press `1`-`9` to open it, `Esc` to close, `a` / `d` to switch
between the ambient and detail stage modes. Drag to swing the camera, scroll to
zoom.

The CPU and RAM figures in the header are **whole-process** and labelled as
such — see [What it measures](#what-it-measures).

### Working on the UI

The dashboard is `templates/three-d`, built by `build.rs` and embedded with
`rust-embed`; a second template, `templates/simple`, is a dense 2D alternative
kept as a development target. Both run against a live producer with hot reload:

```sh
cargo run --example fake_indexer --features viz   # terminal one
npm install && npm run three-d:dev                # terminal two, or simple:dev
```

Then open `http://localhost:5174` (`simple` uses 5173). Vite proxies `/ws` to
the Rust server on 9999, so the browser stays on a single origin.

Building the crate with `viz` on runs the Vite build automatically. On a machine
without Node.js, populate `assets/` once and set
`PIPELINE_VIZ_SKIP_UI_BUILD=1`.

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
cargo test --features viz,macros
cargo test --no-default-features --features macros
cargo clippy --features viz,macros --all-targets -- -D warnings
cargo clippy --no-default-features --features macros --all-targets -- -D warnings
cargo fmt --all --check
npm run protocol:test && npm run protocol:typecheck
npm run simple:test && npm run simple:typecheck && npm run simple:lint && npm run simple:build
npm run three-d:test && npm run three-d:typecheck && npm run three-d:lint && npm run three-d:build
```

Both feature configurations must build and pass on their own.

## License

MIT — see [LICENSE](LICENSE).
