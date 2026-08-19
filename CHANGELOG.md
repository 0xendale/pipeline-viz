# Changelog

All notable changes to `pipeline-viz` are recorded here. This project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The `pipeline-viz-macros` crate is versioned separately and is unchanged at
0.1.0.

## 0.1.1 — 2026-08-19

A hardening release. No public item was added, removed or changed in signature,
so upgrading is a version bump and nothing else. What did change is behaviour
that was previously wrong, and the table below says exactly what you might
notice.

### Compatibility

| Behaviour | Before | After | Kind | Opting out |
| --- | --- | --- | --- | --- |
| Generated job ids | `job_<millis>`, so two items starting in the same millisecond became **one item** | `job_<instance>_<sequence>`, unique within the process across every tracker clone | Bug fix | Pass your own `.id(...)`, which is stored byte for byte as before |
| Abandoned item records | Kept forever; a long-running process leaked one record per abandoned item | The most recent 1000 are kept, oldest evicted first and reported as a normal removal | Bug fix | `TrackerBuilder::max_retained_abandoned(n)`; pass a large `n` for the old behaviour, `0` to keep none |
| Background tasks | Outlived the tracker; the port stayed bound for the life of the process | Dropping the last handle stops the collector, sampler, server and open WebSockets and frees the port | Bug fix | Keep a handle alive, or `install()` one, for a process-lifetime dashboard |
| `is_serving()` | Fixed at whatever binding returned at startup | Reports whether the server is listening *now*, including after shutdown or runtime teardown | Bug fix | None; the old value was stale rather than useful |

None of these are breaking changes under SemVer: each replaces behaviour that
was a defect. The two worth reading twice are the abandoned-record cap, which
can now evict a record a long-running process used to keep, and task shutdown,
which now ends a dashboard that used to survive its tracker.

Shutdown is immediate: no final patch is flushed, because by the time the last
handle is gone there is nothing left to observe it.

### Fixed

- Generated job ids no longer collide under burst traffic. Ids omitted from
  `.id(...)` are now `job_<instance>_<sequence>` and are unique within a
  process across concurrent clones of a tracker.
- Retained abandoned items are bounded. The collector keeps the most recent
  `max_retained_abandoned` records (default 1000), evicting the oldest as a
  normal removal, never evicting active or held work, and never emitting a
  removal twice. The cap bounds record count, not metadata bytes.
- Background work is owned by the tracker. Dropping the last handle cancels the
  collector, the process sampler, the HTTP server and every open WebSocket, and
  releases the port. A client parked mid-send is released and sees EOF rather
  than hanging.
- `is_serving()` reports live state rather than the startup result, so a
  tracker whose runtime was destroyed no longer claims to be serving.
- The process sampler counts a full event channel as a dropped event instead of
  discarding the sample silently.

### Added

- `TrackerBuilder::max_retained_abandoned` for the abandoned-record cap.
- `examples/tokio_pipeline.rs`: a bounded-channel Tokio pipeline that can verify
  itself over its own HTTP and WebSocket endpoints with
  `PIPELINE_VIZ_EXAMPLE_MODE=self-check`.
- `scripts/verify-package.sh`: packages the crate and builds three consumers
  against the unpacked archive with `npm`, `node` and `npx` replaced by failing
  stubs, proving a published consumer needs no Node.js.
- A CI workflow covering both feature surfaces, MSRV 1.75, rustdoc, the
  protocol and dashboard test suites, package smoke on Linux and macOS, and
  SemVer checks on both root feature surfaces.

### Documentation

- Every public item is documented on both feature surfaces, with
  `missing_docs` enforced.
- The loopback-only, unauthenticated security boundary is stated in the crate
  docs and the README, along with the SSH-tunnel recipe.
- A nonzero `dropped_events` count is documented as meaning the displayed state
  may be stale, that reconnecting repeats current collector state rather than
  repairing it, and that the mitigations are channel capacity or a restart.
- `documentation = "https://docs.rs/pipeline-viz"` added to the package
  metadata.

## 0.1.0 — 2026-08-10

First release.

- `PipelineTracker`, `JobBuilder` and `JobGuard`: per-item custody with hold
  reasons, and abandonment on a dropped guard.
- A collector that owns all state and coalesces updates on a 100ms tick.
- An axum dashboard on `127.0.0.1` serving an embedded UI and a
  `Snapshot`/`Patch` WebSocket protocol.
- `#[track_node]` and `#[track_job]` behind the `macros` feature.
- Off by default: with `viz` off the whole API compiles to empty inlined bodies
  and none of the implementation's dependencies are built.
