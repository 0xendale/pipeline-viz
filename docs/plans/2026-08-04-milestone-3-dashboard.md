# Milestone 3: Dashboard UI — Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking. Implement task-by-task; each task ends with a commit and is independently reviewable.

**Goal:** A React dashboard that connects to the existing WebSocket stream and answers "where is item X, and why has it not moved?" at a glance — built and verified against `examples/fake_indexer.rs`.

**Architecture:** A Vite + React + TypeScript app in `ui/`, served by the Vite dev server during development and proxied to the Rust server on port 9999. The frontend derives nothing that Rust can compute: it holds `nodes`, `jobs`, and `process` exactly as received, and renders them. The only computation on the client is presentation — graph coordinates, health colors, and sort order — each a pure function with its own unit test. Embedding the built assets into the binary is milestone 4; this milestone stops at a working dev-server dashboard.

**Tech Stack:** Vite 7, React 19, TypeScript 5.7, `@xyflow/react` 12 (React Flow), Zustand 5, Tailwind CSS 4, Vitest 3. On the Rust side, `sysinfo` 0.33 for the one process-wide resource gauge.

## Global Constraints

From `docs/specs/2026-08-04-pipeline-viz-mvp-design.md` and `CLAUDE.md`:

- **The frontend derives nothing.** Aggregation, percentiles, throughput, and coalescing stay in `collector`. If a number needs computing from history, it is computed in Rust.
- **No per-node CPU or RAM.** One process-wide gauge in the header, labeled as whole-process. A request for per-node resource use is refused, not approximated.
- `sysinfo` is `optional = true` under the `viz` feature. `tests/zero_overhead.rs` must keep passing, and its `FORBIDDEN` list must gain `sysinfo`.
- `ui/dist/` and `ui/node_modules/` are gitignored. `ui/src/` and lockfiles are committed.
- No `unwrap()` in Rust library code. Public Rust API items need doc comments.
- Rust gate unchanged: `cargo test --features viz`, `cargo test --no-default-features`, `cargo clippy` on both with `-D warnings`, `cargo fmt --check`.
- Frontend gate: `npm run typecheck`, `npm run lint`, `npm test` — all from `ui/`.
- Commits: conventional, imperative, no trailing period, **no agent mention anywhere in commit content**.

## Deviations From the Spec, Decided Here

- **No shadcn/ui.** The spec names Tailwind + shadcn. shadcn installs a large component scaffold for what this dashboard needs (a panel, a table, a badge). Tailwind alone, with a handful of local components, keeps the surface small. Revisit if the UI grows.
- **No browser-automation tests.** Vitest covers every pure function — the reducer, layout, health, sorting, backoff — which is where correctness lives. Rendering is verified manually against `fake_indexer` in Task 7. Adding Playwright to v0.1 would cost more than it catches at this size.
- **Wire field names stay `snake_case` in TypeScript.** Mirroring the Rust names exactly removes a translation layer and one whole class of drift bug. It reads slightly against JS convention; that is the intended trade.

## File Structure

| File | Responsibility |
|---|---|
| `src/process.rs` (create) | Samples process-wide CPU and RAM; the only `sysinfo` caller |
| `src/model.rs` (modify) | Add `ProcessStats`; add `process` to `Snapshot` and `Patch` |
| `src/event.rs` (modify) | Add `Event::ProcessStats` |
| `src/collector.rs` (modify) | Store and emit process stats |
| `src/tracker.rs`, `src/noop.rs` (modify) | `enable_process_metrics` builder option |
| `tests/protocol_fixtures.rs` (create) | Golden JSON the TypeScript side parses — the anti-drift seam |
| `ui/src/protocol/types.ts` | TypeScript mirror of the wire model |
| `ui/src/store/reduce.ts` | Pure `applySnapshot` / `applyPatch` — where state correctness lives |
| `ui/src/store/store.ts` | Zustand store wrapping the reducer |
| `ui/src/net/stream.ts` | WebSocket client with reconnect backoff |
| `ui/src/graph/layout.ts` | Layered graph coordinates from `inputs` edges |
| `ui/src/graph/health.ts` | Node health and oldest-held selection |
| `ui/src/components/*.tsx` | Header, graph, node card, held strip, detail panel |

---

### Task 1: Report process-wide CPU and RAM

**Files:**
- Create: `src/process.rs`
- Modify: `Cargo.toml`, `src/lib.rs`, `src/model.rs`, `src/event.rs`, `src/collector.rs`, `src/tracker.rs`, `src/noop.rs`, `tests/zero_overhead.rs`
- Test: `src/collector.rs` (unit), `tests/public_api.rs` (end-to-end)

**Interfaces:**
- Produces:
  - `pub struct ProcessStats { pub cpu_pct: f64, pub ram_mb: f64 }`
  - `Snapshot.process: Option<ProcessStats>`, `Patch.process: Option<ProcessStats>`
  - `TrackerBuilder::enable_process_metrics(bool)` — default `true`

**Why event-driven:** the sampler emits `Event::ProcessStats` like any other event, so `collector` stays a pure function of its stream and its tests still need no clock and no `sysinfo`.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, add to `[dependencies]` and to the `viz` feature list:

```toml
sysinfo = { version = "0.33", default-features = false, features = ["system"], optional = true }
```

```toml
viz = ["dep:serde", "dep:serde_json", "dep:tokio", "dep:axum", "dep:futures-util", "dep:sysinfo"]
```

In `tests/zero_overhead.rs:12`, extend the guard — it is currently missing the crates milestone 2 added, so fix all three now:

```rust
const FORBIDDEN: &[&str] = &[
    "tokio",
    "serde",
    "serde_json",
    "futures-util",
    "axum",
    "sysinfo",
    "tokio-tungstenite",
    "rust-embed",
];
```

- [ ] **Step 2: Write the failing collector test**

In `src/collector.rs`, inside `mod tests`:

```rust
    #[test]
    fn process_stats_are_reported_and_only_resent_when_they_change() {
        let mut state = state_with_nodes();

        state.apply(Event::ProcessStats {
            cpu_pct: 12.4,
            ram_mb: 148.2,
            at_ms: 100,
        });
        let patch = state.take_patch(100).unwrap();
        let process = patch.process.expect("first sample is a change");
        assert_eq!(process.cpu_pct, 12.4);
        assert_eq!(process.ram_mb, 148.2);

        state.apply(Event::ProcessStats {
            cpu_pct: 12.4,
            ram_mb: 148.2,
            at_ms: 200,
        });
        assert!(
            state.take_patch(200).is_none(),
            "an unchanged sample is not news"
        );
    }

    #[test]
    fn snapshot_includes_the_latest_process_stats() {
        let mut state = state_with_nodes();
        state.apply(Event::ProcessStats {
            cpu_pct: 9.0,
            ram_mb: 64.0,
            at_ms: 10,
        });

        let snapshot = state.snapshot(10);
        assert_eq!(snapshot.process.map(|p| p.ram_mb), Some(64.0));
    }
```

- [ ] **Step 3: Run and confirm it fails**

Run: `cargo test --features viz --lib process`
Expected: FAIL — `no variant named 'ProcessStats' found for enum 'Event'`

- [ ] **Step 4: Extend the model**

In `src/model.rs`, add the type and wire it into both messages:

```rust
/// Resource use of the **whole process**, not of any single node.
///
/// Nodes are logical stages sharing one process and one thread pool, so a
/// per-node CPU or RAM figure cannot be attributed honestly. This is the honest
/// version of that number, and the dashboard labels it as process-wide.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "viz", derive(Serialize, Deserialize))]
pub struct ProcessStats {
    pub cpu_pct: f64,
    pub ram_mb: f64,
}
```

Add to `Snapshot` and to `Patch`, in both cases as the last field:

```rust
    /// `None` when process metrics are disabled or not yet sampled.
    pub process: Option<ProcessStats>,
```

Export it from `src/lib.rs` in the existing `pub use model::{...}` list.

- [ ] **Step 5: Extend the event**

In `src/event.rs`, add the variant and its `at_ms` arm:

```rust
    ProcessStats {
        cpu_pct: f64,
        ram_mb: f64,
        at_ms: u64,
    },
```

In `Event::at_ms`, add `| Event::ProcessStats { at_ms, .. }` to the existing match chain.

- [ ] **Step 6: Store it in the collector**

In `src/collector.rs`, add two fields to `CollectorState`:

```rust
    process: Option<ProcessStats>,
    last_patch_process: Option<ProcessStats>,
```

Add the match arm in `apply`:

```rust
            Event::ProcessStats {
                cpu_pct, ram_mb, ..
            } => {
                self.process = Some(ProcessStats { cpu_pct, ram_mb });
            }
```

In `take_patch`, treat a changed sample as a reason to emit, and include it:

```rust
        let process_changed = self.process != self.last_patch_process;
        if self.dirty_nodes.is_empty()
            && self.dirty_jobs.is_empty()
            && self.removed_jobs.is_empty()
            && !dropped_changed
            && !process_changed
        {
            return None;
        }
```

Then, before constructing the `Patch`:

```rust
        let process = if process_changed { self.process } else { None };
        self.last_patch_process = self.process;
```

and add `process,` to the `Patch` literal, and `process: self.process,` to the `Snapshot` literal in `snapshot`.

Import `ProcessStats` in the `use crate::model::{...}` line.

- [ ] **Step 7: Run and confirm it passes**

Run: `cargo test --features viz --lib`
Expected: PASS, 20 tests

- [ ] **Step 8: Write the sampler**

Create `src/process.rs`:

```rust
//! The only `sysinfo` caller in the crate.
//!
//! Reports the whole process, because that is the only resource figure that can
//! be measured honestly — see `ProcessStats`. Isolated in one file so that a
//! `sysinfo` API change is a local fix.

use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::event::Event;
use crate::runtime::now_ms;

/// CPU percentage is measured between two refreshes, so the first sample of a
/// process is always zero. One second is long enough to be meaningful and short
/// enough to feel live.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

/// Sample this process on an interval, emitting into the normal event stream.
pub(crate) fn spawn_sampler(sender: tokio::sync::mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let pid = Pid::from_u32(std::process::id());
        let mut system = System::new();
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);

        loop {
            ticker.tick().await;

            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );

            let Some(process) = system.process(pid) else {
                continue;
            };

            let event = Event::ProcessStats {
                cpu_pct: f64::from(process.cpu_usage()),
                ram_mb: process.memory() as f64 / BYTES_PER_MB,
                at_ms: now_ms(),
            };

            // Same rule as every other event: never block the pipeline. If the
            // channel is full, this sample is simply skipped.
            if sender.try_send(event).is_err() {
                continue;
            }
        }
    });
}
```

**If the `sysinfo` signatures differ from the above**, check `cargo doc -p sysinfo --open` — this crate's API moves between minor versions. The three calls that matter are `refresh_processes_specifics`, `Process::cpu_usage()` (percent, `f32`), and `Process::memory()` (bytes, `u64`). Keep all fixes inside this file.

- [ ] **Step 9: Wire it into the builder**

In `src/lib.rs`, add `#[cfg(feature = "viz")] mod process;`.

In `src/tracker.rs`, add `process_metrics: bool` to `TrackerBuilder` (default `true` in the `Default` impl), and the setter:

```rust
    /// Whether to sample process-wide CPU and RAM once a second.
    ///
    /// Reports the whole process, never a single node — see `ProcessStats`.
    pub fn enable_process_metrics(mut self, enabled: bool) -> Self {
        self.process_metrics = enabled;
        self
    }
```

In `start_background`, after the collector is spawned:

```rust
        if self.process_metrics {
            crate::process::spawn_sampler(sender.clone());
        }
```

Add the matching no-op to `src/noop.rs` on `TrackerBuilder`:

```rust
    #[inline(always)]
    pub fn enable_process_metrics(self, _enabled: bool) -> Self {
        self
    }
```

- [ ] **Step 10: Add the end-to-end test**

Append to `tests/public_api.rs`:

```rust
#[tokio::test]
async fn process_metrics_reach_the_snapshot() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime");

    // The sampler's first tick lands after one second, and CPU needs a second
    // refresh to be meaningful.
    tokio::time::sleep(Duration::from_millis(2_500)).await;

    let process = tracker
        .snapshot()
        .process
        .expect("process metrics are on by default");
    assert!(process.ram_mb > 0.0, "a running process uses memory");
    assert!(process.cpu_pct >= 0.0);
}

#[tokio::test]
async fn process_metrics_can_be_turned_off() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .enable_process_metrics(false)
        .start_background()
        .expect("started inside a runtime");

    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(tracker.snapshot().process.is_none());
}
```

- [ ] **Step 11: Verify everything and commit**

```bash
cargo test --features viz
cargo test --no-default-features
cargo clippy --features viz --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --check
git add Cargo.toml src tests
git commit -m "feat: report process-wide CPU and RAM

Sampled once a second and pushed through the normal event stream, so the
collector stays a pure function of its input. Labelled process-wide
because per-node attribution is not measurable."
```

---

### Task 2: Freeze the wire format as fixtures, then scaffold the app

**Files:**
- Create: `tests/protocol_fixtures.rs`, `ui/src/protocol/fixtures/snapshot.json`, `ui/src/protocol/fixtures/patch.json`
- Create: `ui/package.json`, `ui/tsconfig.json`, `ui/vite.config.ts`, `ui/index.html`, `ui/src/main.tsx`, `ui/src/App.tsx`, `ui/src/index.css`, `ui/src/protocol/types.ts`, `ui/src/protocol/types.test.ts`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `ui/src/protocol/types.ts` exporting `NodeState`, `JobState`, `JobPhase`, `NodeCounters`, `ProcessStats`, `Snapshot`, `Patch`, `ServerMessage`

**Why fixtures first:** the Rust and TypeScript models are two descriptions of one format, and nothing in either language stops them diverging. A committed golden file that Rust asserts it still produces and TypeScript asserts it can still parse turns a silent drift into a failing test in whichever language changed.

- [ ] **Step 1: Scaffold the app**

```bash
cd ui
npm create vite@latest . -- --template react-ts
npm install
npm install @xyflow/react zustand
npm install -D tailwindcss @tailwindcss/vite vitest
```

Delete the scaffold's demo files: `src/App.css`, `src/assets/`, and the contents of `src/App.tsx`.

- [ ] **Step 2: Configure Vite**

Replace `ui/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The dashboard talks to the Rust server on 9999. Proxying both the page and
// the socket through Vite keeps the browser on one origin, so no CORS handling
// is needed on the Rust side.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/ws": { target: "ws://127.0.0.1:9999", ws: true },
      "/health": "http://127.0.0.1:9999",
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
```

Replace `ui/src/index.css` with:

```css
@import "tailwindcss";
```

Add the scripts to `ui/package.json`:

```json
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "typecheck": "tsc --noEmit",
    "lint": "eslint .",
    "test": "vitest run"
  },
```

- [ ] **Step 3: Write the fixture generator/checker in Rust**

Create `tests/protocol_fixtures.rs`. The fixture directory is written into the app scaffolded above, so this must run after it:

```rust
//! Keeps the Rust wire format and the TypeScript model from drifting apart.
//!
//! The fixtures under `ui/src/protocol/fixtures/` are parsed by the frontend's
//! own tests. If the Rust model changes shape, this test fails; regenerate with
//! `UPDATE_FIXTURES=1 cargo test --features viz --test protocol_fixtures`.

#![cfg(feature = "viz")]

use std::collections::BTreeMap;
use std::path::Path;

use pipeline_viz::{
    JobPhase, JobState, NodeCounters, NodeKind, NodeState, Patch, ProcessStats, ServerMessage,
    Snapshot,
};

fn sample_node() -> NodeState {
    NodeState {
        node_id: "committer".into(),
        display_name: "Database Committer".into(),
        kind: NodeKind::Sink,
        inputs: vec!["validator".into()],
        counters: NodeCounters {
            in_flight: 3,
            queue_depth: 12,
            left_total: 1_402,
            throughput_per_sec: 2.49,
            p50_ms: 122,
            p95_ms: 310,
        },
    }
}

fn sample_job() -> JobState {
    JobState {
        job_id: "2049102".into(),
        job_type: "Block".into(),
        current_node: "committer".into(),
        phase: JobPhase::Held {
            reason: "Waiting for finality (2/12 confirmations)".into(),
        },
        entered_node_at_ms: 1_785_810_000_000,
        created_at_ms: 1_785_809_999_000,
        meta: BTreeMap::from([("tx_count".to_string(), "142".to_string())]),
    }
}

fn check(name: &str, value: &ServerMessage) {
    let encoded = serde_json::to_string_pretty(value).expect("serializes") + "\n";
    let path = Path::new("ui/src/protocol/fixtures").join(name);

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("creates the dir");
        std::fs::write(&path, &encoded).expect("writes the fixture");
        return;
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("missing fixture {}: {error}. Regenerate with UPDATE_FIXTURES=1", path.display())
    });

    assert_eq!(
        existing,
        encoded,
        "the wire format changed. Update ui/src/protocol/types.ts to match, then \
         regenerate with UPDATE_FIXTURES=1"
    );
}

#[test]
fn snapshot_fixture_matches_the_current_wire_format() {
    check(
        "snapshot.json",
        &ServerMessage::Snapshot(Snapshot {
            ts_ms: 1_785_810_000_000,
            nodes: vec![sample_node()],
            jobs: vec![sample_job()],
            dropped_events: 0,
            process: Some(ProcessStats {
                cpu_pct: 12.4,
                ram_mb: 148.2,
            }),
        }),
    );
}

#[test]
fn patch_fixture_matches_the_current_wire_format() {
    check(
        "patch.json",
        &ServerMessage::Patch(Patch {
            ts_ms: 1_785_810_000_100,
            nodes: vec![sample_node()],
            jobs: vec![sample_job()],
            removed_jobs: vec!["2049101".into()],
            dropped_events: 7,
            process: None,
        }),
    );
}
```

- [ ] **Step 4: Generate the fixtures and read them**

```bash
UPDATE_FIXTURES=1 cargo test --features viz --test protocol_fixtures
cargo test --features viz --test protocol_fixtures
cat ui/src/protocol/fixtures/snapshot.json
```

Expected: the second run passes without `UPDATE_FIXTURES`. Read the JSON — the field names it shows are the contract the next step mirrors.

- [ ] **Step 5: Write the TypeScript model**

Create `ui/src/protocol/types.ts`:

```ts
// Mirrors src/model.rs field for field. Names stay snake_case on purpose: the
// wire format is the contract, and a translation layer here would be one more
// place for the two models to drift apart. tests/protocol_fixtures.rs on the
// Rust side guards the other direction.

export type NodeKind = "source" | "transform" | "sink";

export type JobPhase =
  | { phase: "active" }
  | { phase: "held"; reason: string }
  | { phase: "abandoned" };

export interface NodeCounters {
  in_flight: number;
  queue_depth: number;
  left_total: number;
  throughput_per_sec: number;
  p50_ms: number;
  p95_ms: number;
}

export interface NodeState {
  node_id: string;
  display_name: string;
  kind: NodeKind;
  inputs: string[];
  counters: NodeCounters;
}

export type JobState = JobPhase & {
  job_id: string;
  job_type: string;
  current_node: string;
  entered_node_at_ms: number;
  created_at_ms: number;
  meta: Record<string, string>;
};

export interface ProcessStats {
  cpu_pct: number;
  ram_mb: number;
}

export interface Snapshot {
  type: "snapshot";
  ts_ms: number;
  nodes: NodeState[];
  jobs: JobState[];
  dropped_events: number;
  process: ProcessStats | null;
}

export interface Patch {
  type: "patch";
  ts_ms: number;
  nodes: NodeState[];
  jobs: JobState[];
  removed_jobs: string[];
  dropped_events: number;
  process: ProcessStats | null;
}

export type ServerMessage = Snapshot | Patch;
```

- [ ] **Step 6: Test the model against the Rust fixtures**

Create `ui/src/protocol/types.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import snapshotFixture from "./fixtures/snapshot.json";
import patchFixture from "./fixtures/patch.json";
import type { Patch, ServerMessage, Snapshot } from "./types";

describe("the wire model matches what Rust produces", () => {
  it("parses a snapshot", () => {
    const message = snapshotFixture as ServerMessage;
    expect(message.type).toBe("snapshot");

    const snapshot = message as Snapshot;
    expect(snapshot.nodes[0].display_name).toBe("Database Committer");
    expect(snapshot.nodes[0].counters.p95_ms).toBe(310);
    expect(snapshot.jobs[0].phase).toBe("held");
    if (snapshot.jobs[0].phase === "held") {
      expect(snapshot.jobs[0].reason).toContain("finality");
    }
    expect(snapshot.process?.cpu_pct).toBe(12.4);
  });

  it("parses a patch", () => {
    const patch = patchFixture as Patch;
    expect(patch.type).toBe("patch");
    expect(patch.removed_jobs).toEqual(["2049101"]);
    expect(patch.dropped_events).toBe(7);
    expect(patch.process).toBeNull();
  });
});
```

Set `"resolveJsonModule": true` in `ui/tsconfig.app.json` under `compilerOptions`.

- [ ] **Step 7: Run and confirm it passes**

```bash
cd ui && npm test && npm run typecheck
```
Expected: 2 tests pass, no type errors. If a field name mismatches, the fixture is right and `types.ts` is wrong.

- [ ] **Step 8: Ignore build output and commit**

Append to `.gitignore`:

```
ui/node_modules/
ui/dist/
```

```bash
git add .gitignore ui tests/protocol_fixtures.rs
git commit -m "feat: scaffold the dashboard app and freeze the wire format

Golden fixtures are generated by a Rust test and parsed by a TypeScript
test, so a change to either model fails in the language that changed."
```

---

### Task 3: The store reducer

**Files:**
- Create: `ui/src/store/reduce.ts`, `ui/src/store/reduce.test.ts`, `ui/src/store/store.ts`

**Interfaces:**
- Consumes: `Snapshot`, `Patch`, `NodeState`, `JobState` from `../protocol/types`
- Produces:
  - `interface PipelineData { nodes: Record<string, NodeState>; jobs: Record<string, JobState>; dropped_events: number; process: ProcessStats | null; ts_ms: number }`
  - `emptyPipeline(): PipelineData`
  - `applySnapshot(snapshot: Snapshot): PipelineData`
  - `applyPatch(current: PipelineData, patch: Patch): PipelineData`
  - `usePipelineStore` — Zustand store with `data`, `connection`, `applyMessage`, `setConnection`

**The rule this task encodes:** a snapshot *replaces* everything; a patch *merges* and deletes only what `removed_jobs` names. A patch is never treated as complete state.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/store/reduce.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { applyPatch, applySnapshot, emptyPipeline } from "./reduce";
import type { NodeState, JobState, Patch, Snapshot } from "../protocol/types";

const node = (node_id: string, in_flight = 0): NodeState => ({
  node_id,
  display_name: node_id,
  kind: "transform",
  inputs: [],
  counters: {
    in_flight,
    queue_depth: 0,
    left_total: 0,
    throughput_per_sec: 0,
    p50_ms: 0,
    p95_ms: 0,
  },
});

const job = (job_id: string, current_node: string): JobState => ({
  job_id,
  job_type: "Block",
  current_node,
  phase: "active",
  entered_node_at_ms: 1_000,
  created_at_ms: 1_000,
  meta: {},
});

const snapshot = (nodes: NodeState[], jobs: JobState[]): Snapshot => ({
  type: "snapshot",
  ts_ms: 1_000,
  nodes,
  jobs,
  dropped_events: 0,
  process: null,
});

const patch = (over: Partial<Patch> = {}): Patch => ({
  type: "patch",
  ts_ms: 2_000,
  nodes: [],
  jobs: [],
  removed_jobs: [],
  dropped_events: 0,
  process: null,
  ...over,
});

describe("applySnapshot", () => {
  it("replaces everything, so a reconnect cannot leave stale items behind", () => {
    const stale = applySnapshot(snapshot([node("indexer")], [job("gone", "indexer")]));
    const fresh = applySnapshot(snapshot([node("committer")], [job("block_1", "committer")]));

    expect(Object.keys(stale.jobs)).toEqual(["gone"]);
    expect(Object.keys(fresh.jobs)).toEqual(["block_1"]);
    expect(Object.keys(fresh.nodes)).toEqual(["committer"]);
  });
});

describe("applyPatch", () => {
  it("merges rather than replaces", () => {
    const base = applySnapshot(
      snapshot([node("fetcher"), node("indexer")], [job("block_1", "fetcher")]),
    );
    const next = applyPatch(base, patch({ jobs: [job("block_2", "indexer")] }));

    expect(Object.keys(next.jobs).sort()).toEqual(["block_1", "block_2"]);
    expect(Object.keys(next.nodes).sort()).toEqual(["fetcher", "indexer"]);
  });

  it("overwrites an item with its new position", () => {
    const base = applySnapshot(snapshot([node("fetcher")], [job("block_1", "fetcher")]));
    const next = applyPatch(base, patch({ jobs: [job("block_1", "committer")] }));

    expect(Object.keys(next.jobs)).toEqual(["block_1"]);
    expect(next.jobs.block_1.current_node).toBe("committer");
  });

  it("deletes only what removed_jobs names", () => {
    const base = applySnapshot(
      snapshot([node("fetcher")], [job("block_1", "fetcher"), job("block_2", "fetcher")]),
    );
    const next = applyPatch(base, patch({ removed_jobs: ["block_1"] }));

    expect(Object.keys(next.jobs)).toEqual(["block_2"]);
  });

  it("keeps the previous process reading when a patch carries none", () => {
    const base = applySnapshot({
      ...snapshot([], []),
      process: { cpu_pct: 12.4, ram_mb: 148.2 },
    });
    const next = applyPatch(base, patch({ process: null }));

    expect(next.process).toEqual({ cpu_pct: 12.4, ram_mb: 148.2 });
  });

  it("adopts a new process reading when a patch carries one", () => {
    const base = applySnapshot(snapshot([], []));
    const next = applyPatch(base, patch({ process: { cpu_pct: 30, ram_mb: 200 } }));

    expect(next.process).toEqual({ cpu_pct: 30, ram_mb: 200 });
  });

  it("does not mutate the state it was given", () => {
    const base = applySnapshot(snapshot([node("fetcher")], [job("block_1", "fetcher")]));
    const before = JSON.stringify(base);
    applyPatch(base, patch({ removed_jobs: ["block_1"] }));

    expect(JSON.stringify(base)).toBe(before);
  });

  it("starts from an empty pipeline cleanly", () => {
    const next = applyPatch(emptyPipeline(), patch({ nodes: [node("indexer")] }));
    expect(Object.keys(next.nodes)).toEqual(["indexer"]);
  });
});
```

- [ ] **Step 2: Run and confirm they fail**

```bash
cd ui && npm test
```
Expected: FAIL — `Failed to resolve import "./reduce"`

- [ ] **Step 3: Write the reducer**

Create `ui/src/store/reduce.ts`:

```ts
import type { JobState, NodeState, Patch, ProcessStats, Snapshot } from "../protocol/types";

/** Everything the dashboard renders, keyed for lookup. */
export interface PipelineData {
  nodes: Record<string, NodeState>;
  jobs: Record<string, JobState>;
  dropped_events: number;
  process: ProcessStats | null;
  ts_ms: number;
}

export function emptyPipeline(): PipelineData {
  return { nodes: {}, jobs: {}, dropped_events: 0, process: null, ts_ms: 0 };
}

function byId<T extends { [key: string]: unknown }>(items: T[], key: keyof T): Record<string, T> {
  return Object.fromEntries(items.map((item) => [String(item[key]), item]));
}

/**
 * A snapshot is complete state, so it replaces rather than merges. This is what
 * makes a reconnect self-correcting: whatever the client believed before is
 * discarded, including items that were removed while it was disconnected.
 */
export function applySnapshot(snapshot: Snapshot): PipelineData {
  return {
    nodes: byId(snapshot.nodes, "node_id"),
    jobs: byId(snapshot.jobs, "job_id"),
    dropped_events: snapshot.dropped_events,
    process: snapshot.process,
    ts_ms: snapshot.ts_ms,
  };
}

/**
 * A patch carries only what changed. Entries are whole values, so merging is an
 * overwrite per key; deletion happens only for ids named in `removed_jobs`.
 */
export function applyPatch(current: PipelineData, patch: Patch): PipelineData {
  const nodes = { ...current.nodes, ...byId(patch.nodes, "node_id") };
  const jobs = { ...current.jobs, ...byId(patch.jobs, "job_id") };
  for (const job_id of patch.removed_jobs) {
    delete jobs[job_id];
  }

  return {
    nodes,
    jobs,
    dropped_events: patch.dropped_events,
    // A patch omits the process reading when it has not changed, so the last
    // known value stands rather than the gauge blinking empty.
    process: patch.process ?? current.process,
    ts_ms: patch.ts_ms,
  };
}
```

- [ ] **Step 4: Run and confirm they pass**

```bash
cd ui && npm test
```
Expected: PASS, 10 tests

- [ ] **Step 5: Write the store**

Create `ui/src/store/store.ts`:

```ts
import { create } from "zustand";
import type { ServerMessage } from "../protocol/types";
import { applyPatch, applySnapshot, emptyPipeline, type PipelineData } from "./reduce";

export type ConnectionState = "connecting" | "live" | "reconnecting";

interface PipelineStore {
  data: PipelineData;
  connection: ConnectionState;
  selectedNode: string | null;
  applyMessage: (message: ServerMessage) => void;
  setConnection: (connection: ConnectionState) => void;
  selectNode: (node_id: string | null) => void;
}

export const usePipelineStore = create<PipelineStore>((set) => ({
  data: emptyPipeline(),
  connection: "connecting",
  selectedNode: null,
  applyMessage: (message) =>
    set((state) => ({
      data:
        message.type === "snapshot"
          ? applySnapshot(message)
          : applyPatch(state.data, message),
    })),
  setConnection: (connection) => set({ connection }),
  selectNode: (selectedNode) => set({ selectedNode }),
}));
```

- [ ] **Step 6: Commit**

```bash
git add ui/src/store
git commit -m "feat: add the dashboard state reducer

A snapshot replaces state and a patch merges it, so a reconnect is
self-correcting and a delta is never mistaken for complete state."
```

---

### Task 4: Live connection with reconnect

**Files:**
- Create: `ui/src/net/backoff.ts`, `ui/src/net/backoff.test.ts`, `ui/src/net/useLiveStream.ts`

**Interfaces:**
- Consumes: `usePipelineStore` (Task 3)
- Produces: `nextBackoffMs(attempt: number): number`, `useLiveStream(): void`

**Why reconnect is not optional here:** the Rust server deliberately closes a client that falls behind (milestone 2, Task 4). Without automatic reconnect, a slow tab goes dark permanently and the user concludes the tool is broken. Reconnecting also fetches a fresh snapshot, which is exactly the recovery the protocol was designed around.

- [ ] **Step 1: Write the failing backoff test**

Create `ui/src/net/backoff.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { MAX_BACKOFF_MS, nextBackoffMs } from "./backoff";

describe("nextBackoffMs", () => {
  it("retries almost immediately the first time", () => {
    // The most common disconnect is the server dropping a lagging client, and
    // recovery there should be invisible.
    expect(nextBackoffMs(0)).toBeLessThanOrEqual(500);
  });

  it("grows with each failed attempt", () => {
    expect(nextBackoffMs(1)).toBeGreaterThan(nextBackoffMs(0));
    expect(nextBackoffMs(4)).toBeGreaterThan(nextBackoffMs(2));
  });

  it("never exceeds the ceiling, however long the server is down", () => {
    expect(nextBackoffMs(50)).toBe(MAX_BACKOFF_MS);
    expect(nextBackoffMs(1_000)).toBe(MAX_BACKOFF_MS);
  });
});
```

- [ ] **Step 2: Run and confirm it fails**

```bash
cd ui && npm test backoff
```
Expected: FAIL — cannot resolve `./backoff`

- [ ] **Step 3: Write the backoff**

Create `ui/src/net/backoff.ts`:

```ts
const BASE_MS = 250;
export const MAX_BACKOFF_MS = 5_000;

/** Exponential backoff, capped. Attempt 0 is the first retry. */
export function nextBackoffMs(attempt: number): number {
  return Math.min(BASE_MS * 2 ** attempt, MAX_BACKOFF_MS);
}
```

- [ ] **Step 4: Run and confirm it passes**

```bash
cd ui && npm test backoff
```
Expected: PASS, 3 tests

- [ ] **Step 5: Write the connection hook**

Create `ui/src/net/useLiveStream.ts`:

```ts
import { useEffect } from "react";
import type { ServerMessage } from "../protocol/types";
import { usePipelineStore } from "../store/store";
import { nextBackoffMs } from "./backoff";

function socketUrl(): string {
  const scheme = window.location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${window.location.host}/ws`;
}

/**
 * Holds a WebSocket open for the life of the app, reconnecting when it closes.
 *
 * The server closes clients that fall behind rather than sending them a stream
 * with a gap, so a disconnect is an expected event, not an error. Reconnecting
 * gets a fresh snapshot, which is the recovery path the protocol is built for.
 */
export function useLiveStream(): void {
  const applyMessage = usePipelineStore((state) => state.applyMessage);
  const setConnection = usePipelineStore((state) => state.setConnection);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let retryTimer: number | undefined;
    let attempt = 0;
    let closed = false;

    const connect = () => {
      if (closed) return;
      socket = new WebSocket(socketUrl());

      socket.onopen = () => {
        attempt = 0;
        setConnection("live");
      };

      socket.onmessage = (event) => {
        try {
          applyMessage(JSON.parse(event.data as string) as ServerMessage);
        } catch {
          // A frame we cannot parse means a version mismatch, not a reason to
          // tear down a working connection. Skip it.
        }
      };

      socket.onclose = () => {
        if (closed) return;
        setConnection("reconnecting");
        retryTimer = window.setTimeout(connect, nextBackoffMs(attempt));
        attempt += 1;
      };

      socket.onerror = () => socket?.close();
    };

    connect();

    return () => {
      closed = true;
      window.clearTimeout(retryTimer);
      socket?.close();
    };
  }, [applyMessage, setConnection]);
}
```

- [ ] **Step 6: Commit**

```bash
git add ui/src/net
git commit -m "feat: hold the websocket open and reconnect on close

The server closes lagging clients by design, so a disconnect is routine.
Reconnecting fetches a fresh snapshot rather than resuming a gapped
stream."
```

---

### Task 5: Graph layout and node cards

**Files:**
- Create: `ui/src/graph/layout.ts`, `ui/src/graph/layout.test.ts`, `ui/src/graph/health.ts`, `ui/src/graph/health.test.ts`, `ui/src/components/NodeCard.tsx`, `ui/src/components/PipelineGraph.tsx`

**Interfaces:**
- Produces:
  - `layoutNodes(nodes: NodeState[]): Record<string, { x: number; y: number }>`
  - `type Health = "idle" | "active" | "stalled"`
  - `nodeHealth(node: NodeState, jobs: JobState[], nowMs: number): Health`
  - `STALL_THRESHOLD_MS`

**Why layout is hand-written:** the graph is a pipeline — a handful of stages with declared `inputs`. A layered left-to-right placement by longest-path rank is about twenty lines, is deterministic, and is unit-testable. A layout library would be a dependency and a black box for the same result.

- [ ] **Step 1: Write the failing layout tests**

Create `ui/src/graph/layout.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { layoutNodes } from "./layout";
import type { NodeState } from "../protocol/types";

const node = (node_id: string, inputs: string[]): NodeState => ({
  node_id,
  display_name: node_id,
  kind: "transform",
  inputs,
  counters: {
    in_flight: 0,
    queue_depth: 0,
    left_total: 0,
    throughput_per_sec: 0,
    p50_ms: 0,
    p95_ms: 0,
  },
});

describe("layoutNodes", () => {
  it("places a linear pipeline left to right in order", () => {
    const positions = layoutNodes([
      node("committer", ["validator"]),
      node("fetcher", []),
      node("validator", ["indexer"]),
      node("indexer", ["fetcher"]),
    ]);

    expect(positions.fetcher.x).toBeLessThan(positions.indexer.x);
    expect(positions.indexer.x).toBeLessThan(positions.validator.x);
    expect(positions.validator.x).toBeLessThan(positions.committer.x);
  });

  it("stacks nodes that share a rank", () => {
    const positions = layoutNodes([
      node("fetcher", []),
      node("blocks", ["fetcher"]),
      node("receipts", ["fetcher"]),
    ]);

    expect(positions.blocks.x).toBe(positions.receipts.x);
    expect(positions.blocks.y).not.toBe(positions.receipts.y);
  });

  it("ranks a node after its deepest input, not its first", () => {
    const positions = layoutNodes([
      node("fetcher", []),
      node("slow", ["fetcher"]),
      node("committer", ["fetcher", "slow"]),
    ]);

    expect(positions.committer.x).toBeGreaterThan(positions.slow.x);
  });

  it("terminates on a cycle instead of hanging", () => {
    const positions = layoutNodes([node("a", ["b"]), node("b", ["a"])]);
    expect(Object.keys(positions).sort()).toEqual(["a", "b"]);
  });

  it("ignores inputs naming nodes that do not exist", () => {
    const positions = layoutNodes([node("indexer", ["ghost"])]);
    expect(positions.indexer).toEqual({ x: 0, y: 0 });
  });
});
```

- [ ] **Step 2: Run and confirm they fail**

```bash
cd ui && npm test layout
```
Expected: FAIL — cannot resolve `./layout`

- [ ] **Step 3: Write the layout**

Create `ui/src/graph/layout.ts`:

```ts
import type { NodeState } from "../protocol/types";

export const COLUMN_WIDTH = 280;
export const ROW_HEIGHT = 170;

/**
 * Left-to-right layered placement: a node's column is one past its deepest
 * input, and nodes sharing a column are stacked in id order so the layout is
 * stable across renders.
 *
 * Pipelines are declared as a DAG, but a user can register a cycle by mistake.
 * The depth walk is bounded by the node count so a cycle produces a usable
 * layout instead of hanging the tab.
 */
export function layoutNodes(nodes: NodeState[]): Record<string, { x: number; y: number }> {
  const byId = new Map(nodes.map((node) => [node.node_id, node]));
  const ranks = new Map<string, number>();

  const rankOf = (node_id: string, seen: Set<string>): number => {
    const cached = ranks.get(node_id);
    if (cached !== undefined) return cached;
    if (seen.has(node_id)) return 0;

    const node = byId.get(node_id);
    if (!node) return 0;

    seen.add(node_id);
    const inputRanks = node.inputs
      .filter((input) => byId.has(input))
      .map((input) => rankOf(input, seen) + 1);
    seen.delete(node_id);

    const rank = inputRanks.length > 0 ? Math.max(...inputRanks) : 0;
    ranks.set(node_id, rank);
    return rank;
  };

  for (const node of nodes) rankOf(node.node_id, new Set());

  const columns = new Map<number, string[]>();
  for (const node_id of [...byId.keys()].sort()) {
    const rank = ranks.get(node_id) ?? 0;
    columns.set(rank, [...(columns.get(rank) ?? []), node_id]);
  }

  const positions: Record<string, { x: number; y: number }> = {};
  for (const [rank, ids] of columns) {
    ids.forEach((node_id, row) => {
      positions[node_id] = { x: rank * COLUMN_WIDTH, y: row * ROW_HEIGHT };
    });
  }
  return positions;
}
```

- [ ] **Step 4: Run and confirm they pass**

```bash
cd ui && npm test layout
```
Expected: PASS, 5 tests

- [ ] **Step 5: Write the failing health tests**

Create `ui/src/graph/health.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { nodeHealth, oldestHeld, STALL_THRESHOLD_MS } from "./health";
import type { JobState, NodeState } from "../protocol/types";

const node = (in_flight: number): NodeState => ({
  node_id: "indexer",
  display_name: "Indexer",
  kind: "transform",
  inputs: [],
  counters: {
    in_flight,
    queue_depth: 0,
    left_total: 0,
    throughput_per_sec: 0,
    p50_ms: 0,
    p95_ms: 0,
  },
});

const held = (job_id: string, entered_node_at_ms: number): JobState => ({
  job_id,
  job_type: "Block",
  current_node: "indexer",
  phase: "held",
  reason: "Waiting for finality",
  entered_node_at_ms,
  created_at_ms: entered_node_at_ms,
  meta: {},
});

const active = (job_id: string, entered_node_at_ms: number): JobState => ({
  job_id,
  job_type: "Block",
  current_node: "indexer",
  phase: "active",
  entered_node_at_ms,
  created_at_ms: entered_node_at_ms,
  meta: {},
});

describe("nodeHealth", () => {
  it("is idle with nothing in flight", () => {
    expect(nodeHealth(node(0), [], 10_000)).toBe("idle");
  });

  it("is active when work is moving", () => {
    expect(nodeHealth(node(1), [active("block_1", 9_000)], 10_000)).toBe("active");
  });

  it("is stalled when an item has sat past the threshold", () => {
    const now = 100_000;
    const stuck = held("block_1", now - STALL_THRESHOLD_MS - 1);
    expect(nodeHealth(node(1), [stuck], now)).toBe("stalled");
  });

  it("is not stalled by an item that only just arrived", () => {
    const now = 100_000;
    expect(nodeHealth(node(1), [held("block_1", now - 100)], now)).toBe("active");
  });

  it("counts a long-running active item as stalled too", () => {
    // An item that is neither held nor moving is exactly the case the user is
    // hunting, so it must not be excused for lacking a reason.
    const now = 100_000;
    expect(nodeHealth(node(1), [active("block_1", now - STALL_THRESHOLD_MS - 1)], now)).toBe(
      "stalled",
    );
  });
});

describe("oldestHeld", () => {
  it("returns the longest-waiting items first", () => {
    const now = 100_000;
    const jobs = [held("new", now - 1_000), held("old", now - 50_000), held("mid", now - 20_000)];

    expect(oldestHeld(jobs, 5).map((job) => job.job_id)).toEqual(["old", "mid", "new"]);
  });

  it("caps the list", () => {
    const now = 100_000;
    const jobs = Array.from({ length: 20 }, (_, index) => held(`block_${index}`, now - index * 100));
    expect(oldestHeld(jobs, 5)).toHaveLength(5);
  });

  it("ignores items that are not waiting", () => {
    expect(oldestHeld([active("block_1", 0)], 5)).toEqual([]);
  });
});
```

- [ ] **Step 6: Write health**

Create `ui/src/graph/health.ts`:

```ts
import type { JobState, NodeState } from "../protocol/types";

/**
 * How long an item may sit at one node before the node is flagged.
 *
 * Ten seconds is long enough that normal work never trips it and short enough
 * that a genuine stall is visible before the user goes looking for logs.
 */
export const STALL_THRESHOLD_MS = 10_000;

export type Health = "idle" | "active" | "stalled";

export function nodeHealth(node: NodeState, jobs: JobState[], nowMs: number): Health {
  if (node.counters.in_flight === 0) return "idle";

  const stalled = jobs.some(
    (job) =>
      job.current_node === node.node_id &&
      job.phase !== "abandoned" &&
      nowMs - job.entered_node_at_ms > STALL_THRESHOLD_MS,
  );

  return stalled ? "stalled" : "active";
}

/**
 * The longest-waiting items across the whole pipeline.
 *
 * This is the direct answer to "what is stuck right now". Node colour alone
 * requires the user to guess which node to click first; on a wide graph that is
 * the difference between seeing the answer and hunting for it.
 */
export function oldestHeld(jobs: JobState[], limit: number): JobState[] {
  return jobs
    .filter((job) => job.phase === "held" || job.phase === "abandoned")
    .sort((left, right) => left.entered_node_at_ms - right.entered_node_at_ms)
    .slice(0, limit);
}
```

- [ ] **Step 7: Run and confirm they pass**

```bash
cd ui && npm test
```
Expected: PASS, 21 tests total

- [ ] **Step 8: Write the node card**

Create `ui/src/components/NodeCard.tsx`:

```tsx
import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { NodeState } from "../protocol/types";
import type { Health } from "../graph/health";

export interface NodeCardData extends Record<string, unknown> {
  node: NodeState;
  health: Health;
  selected: boolean;
}

const BORDER: Record<Health, string> = {
  idle: "border-slate-700",
  active: "border-emerald-500",
  stalled: "border-amber-400",
};

export function NodeCard({ data }: NodeProps & { data: NodeCardData }) {
  const { node, health, selected } = data;
  const { counters } = node;

  return (
    <div
      className={`w-56 rounded-lg border-2 bg-slate-900 px-4 py-3 text-slate-100 shadow-lg ${
        BORDER[health]
      } ${selected ? "ring-2 ring-sky-400" : ""}`}
    >
      <Handle type="target" position={Position.Left} className="!bg-slate-600" />

      <div className="truncate text-sm font-semibold">{node.display_name}</div>
      <div className="mt-0.5 text-[11px] uppercase tracking-wide text-slate-500">{node.kind}</div>

      <dl className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 text-xs">
        <dt className="text-slate-400">in flight</dt>
        <dd className="text-right tabular-nums">{counters.in_flight}</dd>

        <dt className="text-slate-400">queued</dt>
        <dd className="text-right tabular-nums">{counters.queue_depth}</dd>

        <dt className="text-slate-400">per sec</dt>
        <dd className="text-right tabular-nums">{counters.throughput_per_sec.toFixed(2)}</dd>

        <dt className="text-slate-400">p50 / p95</dt>
        <dd className="text-right tabular-nums">
          {counters.p50_ms} / {counters.p95_ms} ms
        </dd>
      </dl>

      <Handle type="source" position={Position.Right} className="!bg-slate-600" />
    </div>
  );
}
```

- [ ] **Step 9: Write the graph**

Create `ui/src/components/PipelineGraph.tsx`:

```tsx
import { useMemo } from "react";
import { Background, ReactFlow, type Edge, type Node } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { usePipelineStore } from "../store/store";
import { layoutNodes } from "../graph/layout";
import { nodeHealth } from "../graph/health";
import { NodeCard, type NodeCardData } from "./NodeCard";

const NODE_TYPES = { pipelineNode: NodeCard };

export function PipelineGraph({ nowMs }: { nowMs: number }) {
  const data = usePipelineStore((state) => state.data);
  const selectedNode = usePipelineStore((state) => state.selectedNode);
  const selectNode = usePipelineStore((state) => state.selectNode);

  const nodes = useMemo(() => Object.values(data.nodes), [data.nodes]);
  const jobs = useMemo(() => Object.values(data.jobs), [data.jobs]);
  const positions = useMemo(() => layoutNodes(nodes), [nodes]);

  const flowNodes: Node<NodeCardData>[] = nodes.map((node) => ({
    id: node.node_id,
    type: "pipelineNode",
    position: positions[node.node_id] ?? { x: 0, y: 0 },
    data: {
      node,
      health: nodeHealth(node, jobs, nowMs),
      selected: selectedNode === node.node_id,
    },
  }));

  const flowEdges: Edge[] = nodes.flatMap((node) =>
    node.inputs
      .filter((input) => data.nodes[input])
      .map((input) => ({
        id: `${input}->${node.node_id}`,
        source: input,
        target: node.node_id,
        animated: false,
      })),
  );

  return (
    <ReactFlow
      nodes={flowNodes}
      edges={flowEdges}
      nodeTypes={NODE_TYPES}
      onNodeClick={(_, node) => selectNode(node.id)}
      onPaneClick={() => selectNode(null)}
      fitView
      proOptions={{ hideAttribution: false }}
    >
      <Background />
    </ReactFlow>
  );
}
```

- [ ] **Step 10: Commit**

```bash
cd ui && npm run typecheck && cd ..
git add ui/src/graph ui/src/components
git commit -m "feat: lay out the pipeline graph and colour nodes by health

Layout is a hand-written layered pass over declared inputs: deterministic,
testable, and bounded so a mis-declared cycle cannot hang the tab."
```

---

### Task 6: The held strip and the node detail panel

**Files:**
- Create: `ui/src/components/HeldStrip.tsx`, `ui/src/components/NodeDetail.tsx`, `ui/src/format.ts`, `ui/src/format.test.ts`

**Interfaces:**
- Produces: `formatAge(ms: number): string`, `holdReason(job: JobState): string | null`

- [ ] **Step 1: Write the failing formatting tests**

Create `ui/src/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { formatAge, holdReason } from "./format";
import type { JobState } from "./protocol/types";

const job = (phase: JobState["phase"], reason?: string): JobState =>
  ({
    job_id: "block_1",
    job_type: "Block",
    current_node: "indexer",
    phase,
    ...(reason === undefined ? {} : { reason }),
    entered_node_at_ms: 0,
    created_at_ms: 0,
    meta: {},
  }) as JobState;

describe("formatAge", () => {
  it("uses milliseconds below a second", () => {
    expect(formatAge(420)).toBe("420ms");
  });

  it("uses seconds below a minute", () => {
    expect(formatAge(4_200)).toBe("4.2s");
  });

  it("uses minutes and seconds beyond that", () => {
    expect(formatAge(125_000)).toBe("2m 5s");
  });

  it("never renders a negative age from clock skew", () => {
    expect(formatAge(-50)).toBe("0ms");
  });
});

describe("holdReason", () => {
  it("returns the reason of a held item", () => {
    expect(holdReason(job("held", "Waiting for finality"))).toBe("Waiting for finality");
  });

  it("explains an abandoned item rather than showing nothing", () => {
    expect(holdReason(job("abandoned"))).toBe("Abandoned — dropped without completing");
  });

  it("returns null for an item that is working", () => {
    expect(holdReason(job("active"))).toBeNull();
  });
});
```

- [ ] **Step 2: Write the formatters**

Create `ui/src/format.ts`:

```ts
import type { JobState } from "./protocol/types";

export function formatAge(ms: number): string {
  const clamped = Math.max(0, ms);
  if (clamped < 1_000) return `${Math.round(clamped)}ms`;
  if (clamped < 60_000) return `${(clamped / 1_000).toFixed(1)}s`;

  const minutes = Math.floor(clamped / 60_000);
  const seconds = Math.floor((clamped % 60_000) / 1_000);
  return `${minutes}m ${seconds}s`;
}

/** The user-facing explanation for why an item is not moving, if there is one. */
export function holdReason(job: JobState): string | null {
  if (job.phase === "held") return job.reason;
  if (job.phase === "abandoned") return "Abandoned — dropped without completing";
  return null;
}
```

- [ ] **Step 3: Run and confirm they pass**

```bash
cd ui && npm test format
```
Expected: PASS, 7 tests

- [ ] **Step 4: Write the held strip**

Create `ui/src/components/HeldStrip.tsx`:

```tsx
import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { oldestHeld } from "../graph/health";
import { formatAge, holdReason } from "../format";

const LIMIT = 5;

/**
 * The pipeline's stuck items, always visible.
 *
 * This is the question the tool exists to answer, so it does not live behind a
 * click. Selecting an entry focuses the node holding it.
 */
export function HeldStrip({ nowMs }: { nowMs: number }) {
  const jobs = usePipelineStore((state) => state.data.jobs);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const stuck = useMemo(() => oldestHeld(Object.values(jobs), LIMIT), [jobs]);

  if (stuck.length === 0) {
    return (
      <div className="border-t border-slate-800 bg-slate-950 px-4 py-2 text-xs text-slate-500">
        Nothing held. Every item is moving.
      </div>
    );
  }

  return (
    <div className="border-t border-slate-800 bg-slate-950 px-4 py-2">
      <div className="mb-1 text-[11px] uppercase tracking-wide text-slate-500">
        Longest waiting
      </div>
      <ul className="flex flex-wrap gap-2">
        {stuck.map((job) => (
          <li key={job.job_id}>
            <button
              type="button"
              onClick={() => selectNode(job.current_node)}
              className="flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-xs text-amber-100 hover:border-amber-400"
            >
              <span className="font-mono">{job.job_id}</span>
              <span className="tabular-nums text-amber-300">
                {formatAge(nowMs - job.entered_node_at_ms)}
              </span>
              <span className="max-w-64 truncate text-amber-200/80">{holdReason(job)}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

- [ ] **Step 5: Write the detail panel**

Create `ui/src/components/NodeDetail.tsx`:

```tsx
import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { formatAge, holdReason } from "../format";

export function NodeDetail({ nowMs }: { nowMs: number }) {
  const selectedNode = usePipelineStore((state) => state.selectedNode);
  const nodes = usePipelineStore((state) => state.data.nodes);
  const jobs = usePipelineStore((state) => state.data.jobs);
  const selectNode = usePipelineStore((state) => state.selectNode);

  const node = selectedNode ? nodes[selectedNode] : undefined;
  const items = useMemo(
    () =>
      Object.values(jobs)
        .filter((job) => job.current_node === selectedNode)
        .sort((left, right) => left.entered_node_at_ms - right.entered_node_at_ms),
    [jobs, selectedNode],
  );

  if (!node) return null;

  return (
    <aside className="w-96 shrink-0 overflow-y-auto border-l border-slate-800 bg-slate-950 p-4 text-slate-100">
      <div className="flex items-start justify-between">
        <div>
          <h2 className="text-base font-semibold">{node.display_name}</h2>
          <p className="font-mono text-xs text-slate-500">{node.node_id}</p>
        </div>
        <button
          type="button"
          onClick={() => selectNode(null)}
          className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800"
        >
          close
        </button>
      </div>

      <dl className="mt-4 grid grid-cols-2 gap-y-1 text-sm">
        <dt className="text-slate-400">in flight</dt>
        <dd className="text-right tabular-nums">{node.counters.in_flight}</dd>
        <dt className="text-slate-400">queue depth</dt>
        <dd className="text-right tabular-nums">{node.counters.queue_depth}</dd>
        <dt className="text-slate-400">throughput</dt>
        <dd className="text-right tabular-nums">
          {node.counters.throughput_per_sec.toFixed(2)}/s
        </dd>
        <dt className="text-slate-400">p50 time here</dt>
        <dd className="text-right tabular-nums">{node.counters.p50_ms}ms</dd>
        <dt className="text-slate-400">p95 time here</dt>
        <dd className="text-right tabular-nums">{node.counters.p95_ms}ms</dd>
        <dt className="text-slate-400">left in total</dt>
        <dd className="text-right tabular-nums">{node.counters.left_total}</dd>
      </dl>

      <h3 className="mt-6 text-[11px] uppercase tracking-wide text-slate-500">
        Items here ({items.length})
      </h3>

      {items.length === 0 ? (
        <p className="mt-2 text-xs text-slate-500">Nothing at this node right now.</p>
      ) : (
        <ul className="mt-2 space-y-2">
          {items.map((job) => {
            const reason = holdReason(job);
            return (
              <li key={job.job_id} className="rounded border border-slate-800 p-2 text-xs">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="font-mono">{job.job_id}</span>
                  <span className="tabular-nums text-slate-400">
                    {formatAge(nowMs - job.entered_node_at_ms)}
                  </span>
                </div>
                <div className="mt-1 text-slate-400">{job.job_type}</div>
                {reason && <div className="mt-1 text-amber-300">{reason}</div>}
                {Object.entries(job.meta).map(([key, value]) => (
                  <div key={key} className="mt-1 text-slate-500">
                    {key}: <span className="text-slate-300">{value}</span>
                  </div>
                ))}
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}
```

- [ ] **Step 6: Commit**

```bash
cd ui && npm test && npm run typecheck && cd ..
git add ui/src
git commit -m "feat: add the held-items strip and node detail panel

The stuck-item list stays visible rather than living behind a click,
because that is the question the dashboard exists to answer."
```

---

### Task 7: Assemble the app and verify it against the fake pipeline

**Files:**
- Create: `ui/src/components/Header.tsx`, `ui/src/useNow.ts`
- Modify: `ui/src/App.tsx`, `ui/src/main.tsx`, `ui/index.html`, `README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: everything above
- Produces: a running dashboard at `http://localhost:5173`

- [ ] **Step 1: Write the clock hook**

Ages tick even when no message arrives, so the components need a clock rather than reading `Date.now()` during render.

Create `ui/src/useNow.ts`:

```ts
import { useEffect, useState } from "react";

/**
 * A clock that advances on an interval.
 *
 * Item ages must keep counting up while a held item sits still and no patch
 * arrives, so age cannot be derived only from message timestamps.
 */
export function useNow(intervalMs = 500): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);

  return now;
}
```

- [ ] **Step 2: Write the header**

Create `ui/src/components/Header.tsx`:

```tsx
import { usePipelineStore } from "../store/store";

const CONNECTION_LABEL = {
  connecting: { text: "connecting", className: "bg-slate-600" },
  live: { text: "live", className: "bg-emerald-500" },
  reconnecting: { text: "reconnecting", className: "bg-amber-500" },
} as const;

export function Header() {
  const connection = usePipelineStore((state) => state.connection);
  const data = usePipelineStore((state) => state.data);
  const status = CONNECTION_LABEL[connection];

  return (
    <header className="flex items-center gap-6 border-b border-slate-800 bg-slate-950 px-4 py-2 text-slate-100">
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${status.className}`} />
        <span className="text-sm font-semibold">pipeline-viz</span>
        <span className="text-xs text-slate-500">{status.text}</span>
      </div>

      {data.process && (
        <div className="flex items-center gap-4 text-xs tabular-nums text-slate-400">
          {/* Labelled whole-process on purpose: these figures cannot be
              attributed to an individual node. */}
          <span>
            CPU <span className="text-slate-200">{data.process.cpu_pct.toFixed(1)}%</span>
          </span>
          <span>
            RAM <span className="text-slate-200">{data.process.ram_mb.toFixed(0)} MB</span>
          </span>
          <span className="text-slate-600">whole process</span>
        </div>
      )}

      <div className="ml-auto text-xs text-slate-500">
        {Object.keys(data.jobs).length} items in flight
        {data.dropped_events > 0 && (
          <span
            className="ml-3 text-amber-400"
            title="Events discarded because the channel was full. The pipeline was never slowed down."
          >
            {data.dropped_events} events dropped
          </span>
        )}
      </div>
    </header>
  );
}
```

- [ ] **Step 3: Assemble the app**

Replace `ui/src/App.tsx`:

```tsx
import { Header } from "./components/Header";
import { HeldStrip } from "./components/HeldStrip";
import { NodeDetail } from "./components/NodeDetail";
import { PipelineGraph } from "./components/PipelineGraph";
import { useLiveStream } from "./net/useLiveStream";
import { useNow } from "./useNow";

export default function App() {
  useLiveStream();
  const nowMs = useNow();

  return (
    <div className="flex h-screen flex-col bg-slate-900">
      <Header />
      <div className="flex min-h-0 flex-1">
        <main className="min-w-0 flex-1">
          <PipelineGraph nowMs={nowMs} />
        </main>
        <NodeDetail nowMs={nowMs} />
      </div>
      <HeldStrip nowMs={nowMs} />
    </div>
  );
}
```

Set the page title in `ui/index.html` to `pipeline-viz`, and confirm `ui/src/main.tsx` imports `./index.css`.

- [ ] **Step 4: Run it against the fake pipeline**

Two terminals:

```bash
cargo run --example fake_indexer --features viz
```

```bash
cd ui && npm run dev
```

Open `http://localhost:5173`.

Expected, and each must be confirmed by eye:
1. Four node cards appear left to right: Block Fetcher → Indexer Core → Validator → Database Committer.
2. The header dot is green and reads `live`; CPU and RAM show non-zero values within two seconds, labelled `whole process`.
3. Counters move — `in flight`, `per sec`, and `p50 / p95` all change without a page reload.
4. Within about a minute, a node border turns amber and the held strip lists an item with `Waiting for finality (n/12 confirmations)` and a rising age.
5. Clicking a node opens the panel listing the items at that node with their `tx_count` metadata.
6. Killing the `fake_indexer` process turns the header amber and it reads `reconnecting`; restarting it returns to `live` with correct state and no stale items.

Check 6 is the one that most often reveals a bug, because it exercises snapshot-replaces-state on reconnect. Do not skip it.

- [ ] **Step 5: Update the documentation**

In `README.md`: set milestone 3 to `Done` in the status table, and add after the "Watching the stream" section:

````markdown
## The dashboard

Until the UI is embedded in the binary (milestone 4), run it from source:

```sh
cargo run --example fake_indexer --features viz   # terminal one
cd ui && npm install && npm run dev               # terminal two
```

Then open `http://localhost:5173`. Vite proxies `/ws` to the Rust server on
9999, so the browser stays on one origin.

The dashboard shows the pipeline graph with live per-node counters, a
persistent list of the longest-waiting items with their hold reasons, and a
per-node drill-down. The CPU and RAM figures in the header are **whole-process**
and are labelled as such — see [What it measures](#what-it-measures).
````

In `CLAUDE.md`, add to the Testing table:

```
| `ui` | Vitest over the pure functions: reducer, layout, health, formatting. Rendering is verified manually against `fake_indexer`. |
```

and add to the commands block:

```
cd ui && npm run typecheck && npm run lint && npm test
```

- [ ] **Step 6: Full gate and commit**

```bash
cargo test --features viz
cargo test --no-default-features
cargo clippy --features viz --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --check
cd ui && npm run typecheck && npm run lint && npm test && cd ..
git add ui README.md CLAUDE.md
git commit -m "feat: assemble the dashboard and document running it

Ages advance from a local clock rather than message timestamps, so a held
item's age keeps rising while nothing arrives."
```

---

## Acceptance

Milestone 3 is done when:

1. Both Rust configurations pass tests, clippy with `-D warnings`, and `cargo fmt --check`.
2. `cd ui && npm run typecheck && npm run lint && npm test` passes — roughly 28 tests.
3. `tests/zero_overhead.rs` passes with `sysinfo`, `serde_json`, and `futures-util` added to `FORBIDDEN`.
4. `tests/protocol_fixtures.rs` passes without `UPDATE_FIXTURES`, and the frontend parses the same fixtures.
5. All six manual checks in Task 7 Step 4 confirmed by eye, including the reconnect check.
6. The header labels CPU and RAM as whole-process. No per-node resource figure appears anywhere.

## Deliberately Not In This Milestone

- `rust-embed`, `build.rs`, and serving the UI from the binary — milestone 4
- Proc-macro sugar (`#[track_node]`, `#[track_job]`) — milestone 4
- The README GIF — milestone 4
- Item animation along edges, timeline scrubbing, custom themes, dashboard auth — out of scope for v0.1 entirely
- Browser-automation tests — see the deviations section above
