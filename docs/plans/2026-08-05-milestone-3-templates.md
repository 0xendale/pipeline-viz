# Milestone 3 — Template Split: simple (done) + three-d (next)

Date: 2026-08-05
Status: three-d pending approval. simple done + green.

## Context

Repository restructured into two optional UI templates sharing one protocol package:

```
packages/protocol/   # wire types + reducer/store/hooks (tested: 34 pass)
templates/simple/    # 2D React dashboard (tested: 20 pass, typecheck/lint/build green)
templates/three-d/   # THIS PLAN — from redesign prototype, live wire data
archive/             # originals (zip, pdf), gitignored
DESIGN.md            # formal visual contract, both templates follow it
```

Rust crate unchanged; UI not embedded (deferred to milestone 4 by user decision).

## Wire contract (from src/model.rs — adapter must map to this)

| Wire | Fields |
|---|---|
| NodeState | node_id, display_name, kind (source/transform/sink), **inputs: Vec<NodeId>** (graph edges), counters {in_flight, queue_depth, left_total, throughput_per_sec, p50_ms, p95_ms} |
| JobState | job_id, job_type, current_node, phase (active / held{reason} / abandoned, flattened), entered_node_at_ms, created_at_ms, meta |
| Snapshot | ts_ms, nodes, jobs, dropped_events, process |
| Patch | ts_ms, nodes, jobs, removed_jobs, dropped_events, process |
| ServerMessage | {type:"snapshot"} | {type:"patch"} |

## StageSim adapter contract (from pipeline-stage.js usage)

`pipeline-stage.js` (copied to `templates/three-d/public/`, unmodified) requires:

- `sim.nodes[id]` → `{ inputs: string[], counters: { queue_depth, in_flight, throughput_per_sec, p50_ms } }`
- `sim.positions[id]` → `{ x, z }` (port `layout()` from prototype sim.js)
- `sim.selected` → nodeId | null; `sim.select(id)` setter
- `sim.stallThresholdMs` → 10_000 (STALL_THRESHOLD_MS from protocol)
- `sim.jobList()` → `[{ job_id, node, phase: "active"|"held"|"abandoned", mode: "travel"|"idle", queued, travel?: {from,to,start,dur} }]`
- `sim.ageOf(job, now)` → ms since `entered_node_at_ms` (use `entered_node_at_ms`; held+abandoned use it too)

## Implementation steps (main thread, no junior delegation — token budget)

### 1. Scaffold `templates/three-d/`
- `package.json`: name `@pipeline-viz/three-d`, private, dep `"@pipeline-viz/protocol": "0.0.0"` (workspace), react 19, vite 8, vitest, oxlint, typescript; scripts mirror simple (dev/build/lint/test/typecheck).
- `vite.config.ts`: port **5174**, proxy `/ws` (ws: true) + `/health` → 127.0.0.1:9999. Copy simple's vitest config (environment node, `src/**/*.test.ts`).
- `tsconfig.json` (+ node one) mirroring simple's strict config (noUncheckedIndexedAccess, verbatimModuleSyntax, no `any`/non-null/ts-ignore).
- `index.html`: dark chassis, loads `/pipeline-stage.js` + three.js from unpkg **before** module script (stage registers custom element on load).
- `src/index.css`: copy simple's `@theme` tokens verbatim (chassis/panel/well/rule/ink/signal/fault/paper/muted, font-mono) — DESIGN.md section 6 contract. Add `.engraved` micro-label + stage canvas full-viewport styles.

### 2. `src/stage-sim.ts` — typed live adapter
Pure module, unit-testable, **no three.js imports**.
- `StageSim` class implementing the StageSim contract above.
- `applySnapshot(snap: Snapshot)`: replace nodes (port layout() for positions), rebuild job list; prevNode map reset.
- `applyPatch(patch: Patch)`: update node counters, upsert/remove jobs, spawn `travel` entries when `current_node` changes (from prevNode → new, `dur` 300–600ms, start = patch.ts_ms), queue detection: index-at-node >= in_flight → `queued: true`.
- `jobList()` sorted: held/abandoned first (stage picks worst age anyway), stable order for determinism.
- `ageOf(job, now)`: `now - entered_node_at_ms`, clamp ≥ 0.
- Phase mapping: wire `active`→"active", `held{reason}`→"held", `abandoned`→"abandoned".

### 3. `src/StageCanvas.tsx` + wiring
- `useEffect`: `usePipelineStore.subscribe` → `sim.applySnapshot/applyPatch` on message; initial store snapshot applied on mount.
- Render `<pipeline-stage>` custom element; `ref` → `sim.select(id)` on node click; HUD selection state syncs via callback (controlled by stage's click event → find selected job by node → show detail rail).
- Re-render HUD on store change (subscribe → setState).

### 4. HUD (read-only, thin)
- Header: process cpu/ram (labelled process-wide), dropped_events.
- Held strip: top held/abandoned items by age, reason text (protocol `formatAge`, `holdReason`).
- Detail rail: selected node → node_id/display_name/kind, counters (throughput, p50/p95, in_flight, queue_depth), jobs there with age + reason.
- No controls (no hold/complete buttons — instrumentation is host-side; simple template has none either).

### 5. Tests
- `src/stage-sim.test.ts`: snapshot fixture → nodes/positions/jobs mapped; patch → travel spawn on node change; removed_jobs cleanup; queued classification (index >= in_flight); held reason preserved; age calc.
- Reuse fixtures from `packages/protocol/src/fixtures/` (snapshot.json, patch.json) if shape fits; else build small inline fixtures.
- Gate: `npm run three-d:test` + `three-d:typecheck` + `three-d:lint` + `three-d:build` all green.

### 6. Docs update (stale `ui/` references — verified present)
- `README.md`: milestone table row 3 → "Dashboard UI — simple + 3D templates (Done)"; remove "dashboard UI does not exist yet" paragraph (now false); replace `cd ui && npm install && npm run dev` with workspace commands; add templates table + per-template run instructions.
- `CLAUDE.md`: STRUCTURE tree `ui/` → `templates/simple|three-d` + `packages/protocol`; CODE MAP add StageSim/StageCanvas entries; COMMANDS add npm lines; NOTES milestone 3 wording.
- `DESIGN.md`: `ui/src/...` path references → `templates/simple/src/...` (created pre-migration).
- `templates/three-d/README.md`: what it is, run commands, prototype provenance.

### 7. Verification (all gates)
- `npm run simple:*` — already green, re-run to confirm no regression.
- `npm run three-d:test|typecheck|lint|build`.
- `cargo test --all-features`, `cargo clippy --all-features -- -D warnings`, `cargo fmt --check`.
- Manual smoke: `cargo run --example fake_indexer --features viz` + `npm run three-d:dev` (5174) → browser shows nodes + moving items + held reason strip + abandoned blink. If budget tight: skip Playwright, do manual curl of /health + page load check.

## Out of scope (deferred)
- UI embedding into crate (milestone 4, build.rs — not yet).
- Publishing packages/templates.
- Per-node CPU/RAM (forbidden by design).
- 3D template: no drag/drop, no editing — read-only viewer.
