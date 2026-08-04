# pipeline-viz — MVP Design (v0.1)

**Date:** 2026-08-04
**Status:** Approved design, pre-implementation
**Supersedes:** `pipeline-viz-architecture.pdf` (v1.0.0 proposal) for v0.1 scope

## 1. Problem & Positioning

Rust data pipelines (blockchain indexers, ETL jobs, event-driven services) lose visibility at the level that matters most during debugging: **individual work items**. Existing tools answer adjacent questions — `tracing` shows spans, `tokio-console` shows tasks, Prometheus shows counters — but none answer "where is block 2049102 right now, and why has it not moved in 40 seconds?"

`pipeline-viz` is a drop-in Rust crate that answers exactly that question. A user adds the dependency, adds three lines to `main`, opens `localhost:9999`, and sees their pipeline topology with live per-item custody and hold reasons.

**Primary audience:** Rust developers discovering the crate on crates.io. The MVP therefore optimizes for time-to-first-wow — `cargo add`, three lines, browser — over depth of features.

**The wedge is item-level custody plus hold reasons.** Every scope decision below defends that wedge and cuts anything that does not serve it.

## 2. Approach

The MVP is a **vertical slice shipped alongside a demo example**. One crate, one embedded UI, plus `examples/fake_indexer.rs` — a synthetic four-stage pipeline that stalls items deliberately. That example serves three purposes: it drives UI development before any real integration exists, it acts as the manual smoke test, and it produces the README GIF.

Two alternatives were considered and rejected for v0.1:

- **Protocol-first** (publish the event schema, ship the UI separately): cleaner boundaries and enables third-party UIs, but leaves nothing to `cargo add` and immediately see. Revisit at v0.3 if demand appears. The WebSocket JSON seam already makes this a cheap later split.
- **The full PDF proposal** (item animation along edges, macros plus functions plus per-node `sysinfo`, ring-buffer replay, 16ms batching): estimated 2–3 months, with high odds of stalling before reaching a first user.

## 3. Architecture

A single crate, `pipeline-viz`, with three internal modules and one dependency direction:

```
user code ──calls──> api (Tracker, Node, JobGuard, macros)
                       │ emits Event
                       v
                     collector (bounded mpsc, background task, owns all state)
                       │ broadcasts snapshots + patches
                       v
                     server (axum: GET / embedded UI, GET /ws)
```

**Module responsibilities and boundaries:**

| Module | Does | Depends on | Testable by |
|---|---|---|---|
| `api` | Public surface: `Tracker`, node registration, `JobGuard`, proc-macro sugar. Serializes user calls into `Event` values and sends them. Holds no state. | mpsc sender only | Asserting emitted events |
| `collector` | Owns `PipelineState`. Consumes events, applies them, computes rolling counters, emits coalesced patches on a tick. | Nothing external | Feeding synthetic event streams, asserting state |
| `server` | axum HTTP for embedded UI assets, WebSocket endpoint. Subscribes to collector broadcasts. Never touches pipeline state directly. | collector's subscribe handle | Connecting a WS client, asserting message sequence |

`collector` is the single owner of state, which keeps the hard logic (aggregation, percentiles, coalescing) in Rust where it is unit-testable, and keeps the frontend a pure renderer.

### 3.1 Abandoned-item detection

`JobGuard` implements `Drop`. An item whose guard goes out of scope without an explicit `complete()` is marked **abandoned** rather than silently disappearing. This catches the `?`-early-return bug class — precisely the situation where items really do vanish from a pipeline — and it comes free from the guard pattern.

### 3.2 Feature gating

The crate ships with `default-features = false`. Users opt in with `features = ["viz"]`, normally in a dev-only profile.

With `viz` off: every `api` function compiles to an `#[inline(always)]` empty body, and `collector`/`server` are not compiled at all. All heavy dependencies (axum, tokio-tungstenite, rust-embed, sysinfo) are declared `optional = true` behind the feature.

Off-by-default is deliberate: an OSS crate must not let a user accidentally ship a binary that opens a listening port in production. The cost is that a first-time user who forgets the feature flag sees nothing, which is mitigated in the README quickstart and by a one-line log message on startup.

## 4. Data Model & Wire Protocol

The server holds one authoritative state object, not an event log:

```
PipelineState {
    nodes: HashMap<NodeId, NodeState>,   // display_name, kind, inputs, counters
    jobs:  HashMap<JobId,  JobState>,    // current_node, state, since_ms, hold_reason, meta
}
```

Two message kinds travel over the WebSocket:

- **`Snapshot`** — the complete state. Sent once, on connect. A newly opened browser tab is immediately correct with no replay logic and no ring buffer of historical events.
- **`Patch`** — coalesced deltas, flushed on a 100ms tick.

### 4.1 Coalescing

Patches are coalesced, not queued. An item that traverses five nodes within one tick produces **one** patch entry reflecting its final position, not five. This keeps outbound message volume flat as pipeline throughput rises, which dissolves the throttling problem rather than solving it.

The tick is 100ms rather than the 16ms in the original proposal. Queue depths and hold states do not benefit from 60Hz updates, and 100ms reduces message volume roughly sixfold.

### 4.2 Counters

`throughput` and `p50`/`p95` time-in-node are computed inside `collector` over a rolling 60-second window and shipped inside `Patch`. The frontend performs no aggregation.

### 4.3 Backpressure

The event channel is bounded at 4096. When full, the event is **dropped**, a `dropped_events` counter increments, and that counter is displayed in the dashboard header.

**The visualizer must never block or slow the host pipeline.** This is the one non-negotiable constraint. A visible drop counter is the honest tradeoff: the user sees that data was lost rather than experiencing unexplained latency in their application.

## 5. Dashboard

Three zones on one screen, no client-side routing.

**Header** — process-wide CPU and RAM (labeled explicitly as whole-process), uptime, connection status, `dropped_events`.

**Graph** (center, React Flow) — one box per node showing display name, in-flight count, queue depth, and throughput per second. Border color encodes health: grey idle, green active, **amber when the node holds an item older than a threshold**. Edges are static; no per-item dots animate along them.

**Oldest-held strip** (under the graph, always visible) — the top five stuck items across the whole pipeline, each clickable to focus its node. This is the direct answer to the wedge question. Amber node borders alone require the user to already guess which node to click; on a twelve-node graph the strip is what makes the answer readable at a glance.

**Drill-down panel** (right, opens on node click) — that node's counters, p50/p95 time-in-node, and a table of its current items: id, age, state, hold reason.

### 5.1 Frontend state

A single Zustand store with one reducer that applies `Patch` messages. React Flow node data reads directly from the store. No derived state is held locally — the Rust collector is the single source of truth.

### 5.2 Build and embedding

Vite builds to `dist/`, which `rust-embed` packs into the binary at compile time. `dist/` is gitignored. `build.rs` runs the UI build only when feature `viz` is enabled, and the published crate ships prebuilt assets, so end users never need Node.js installed.

## 6. Per-Node Resource Metrics

The original proposal specifies per-node CPU and RAM via `sysinfo`. **This is not measurable.** Nodes are logical pipeline stages sharing a single process and a single Tokio thread pool; `sysinfo` reports process-wide figures only. Attributing a specific CPU percentage to one node would be fabricated data.

v0.1 therefore reports:

- **Per node:** queue depth, in-flight count, throughput per second, p50/p95 time-in-node — all directly measurable from the event stream.
- **Process-wide:** a single CPU/RAM gauge in the header, labeled as whole-process.

## 7. Instrumentation API

Both a plain-function API and proc-macro sugar ship in v0.1.

The runtime API is the foundation:

```rust
let tracker = PipelineTracker::builder().bind_port(9999).start_background()?;
tracker.register_node("committer", NodeKind::Sink, &["indexer"]);

let mut job = tracker.job("committer").id(block_num).job_type("Block");
job.hold("Waiting for finality");
job.update_reason("Writing to PostgreSQL");
job.complete();
```

Macros (`#[track_node]`, `#[track_job]`) are **thin sugar that expands to exactly these runtime calls** — no separate logic path, no independent state handling. This bounds the maintenance cost of the second crate and guarantees both surfaces cannot diverge in behavior.

## 8. Error Handling

Visualizer failures never propagate into user code:

| Failure | Behavior |
|---|---|
| Port already bound | Log a warning; pipeline runs normally, no dashboard |
| WebSocket client disconnects | Drop that subscriber; no other effect |
| Event channel full | Drop event, increment `dropped_events`, surface in header |
| Panic inside collector task | Tracker goes inert; host pipeline unaffected |

Every failure mode degrades to "no dashboard", never to "broken application".

## 9. Testing Strategy

`collector` is a pure function of its event stream, so the majority of tests need no I/O.

- **Unit (collector):** feed synthetic event sequences, assert resulting `PipelineState`. Covers patch coalescing, p50/p95 computation, abandoned-on-drop marking, and channel-full drop accounting.
- **Integration (server):** spawn a tracker, connect a WebSocket client, assert the `Snapshot`-then-`Patch` message sequence.
- **Example as smoke test:** `examples/fake_indexer.rs` runs a four-stage pipeline with deliberate stalls and random failures.
- **Zero-overhead compile test:** build the crate without feature `viz` and assert that axum and tokio-tungstenite are absent from the resulting dependency tree. This verifies the zero-overhead claim rather than asserting it in prose.

## 10. Out of Scope for v0.1

Explicitly excluded, to be reconsidered only after real user feedback:

- Multi-process or remote pipeline aggregation
- Persistence, history storage, or timeline scrubbing
- Authentication or access control on the dashboard
- Item-level animation along graph edges
- Per-node OS-level CPU/RAM attribution
- Event replay for late-joining clients beyond the initial snapshot
- Custom theming
- A Svelte Flow alternative frontend

## 11. Milestones

Each milestone is independently runnable.

1. **`api` + `collector` + unit tests, no server.** Proves the state machine in isolation.
2. **axum + WebSocket + `Snapshot`/`Patch`.** Verifiable with `websocat` before any frontend exists.
3. **React UI built against `examples/fake_indexer.rs`.**
4. **`rust-embed`, feature gating, proc-macro sugar, README GIF, publish to crates.io.**
