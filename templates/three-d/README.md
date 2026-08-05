# three-d template

Immersive WebGL dashboard built on the `Pipeline visualization system redesign`
prototype: the `<pipeline-stage>` custom element (three.js) renders nodes as
portals, prisms and archives, work items as moving entities, and makes
backpressure visible as orbiting queues.

The prototype's fake `sim.js` is replaced by a typed adapter
(`src/stage-sim.ts`) that feeds the stage from the real WebSocket protocol:
same `Snapshot` then coalesced `Patch` stream as the simple template, rendered
through `@pipeline-viz/protocol`.

## Run

Rust producer in one terminal:

```sh
cargo run --example fake_indexer --features viz
```

Dashboard in another:

```sh
npm install
npm run dev
```

Vite proxies `/ws` and `/health` to `127.0.0.1:9999`. Dev port is **5174** so
the two templates can run side by side.

## Notes

- `pipeline-stage.js` is the prototype scene engine, unmodified, served from
  `public/`. It is a classic script with no type knowledge; the adapter
  contract it reads lives in `src/stage-sim.ts` and is covered by unit tests.
- three.js is bundled as an npm dependency and assigned to `window.THREE` for
  the classic stage script; production no longer depends on unpkg availability.
- `sim.js` is not loaded: it fabricates jobs and exposes mutation controls.
  Its graph-rank layout and stage archetype mapping are ported into the typed
  live adapter and the stage rail.
- `support.js` is the Design Canvas runtime for `.dc.html` files, not a
  production dependency of this Vite template.
- `Pipeline Viz 3D.dc.html` is the visual source for the header metrics, stage
  rail, detail panel, ambient/detail switch, legend, scanline/vignette layers,
  and waiting-longest strip. Prototype scenario buttons stay excluded because
  production instrumentation is host-side and read-only.
- `Pipeline Viz Current.dc.html` remains the 2D reference; its implementation
  lives in `templates/simple/`.
- The HUD is live Snapshot/Patch data through `@pipeline-viz/protocol`; no
  browser simulation is allowed on this surface.
