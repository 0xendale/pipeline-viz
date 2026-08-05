# DESIGN.md — pipeline-viz design system contract

Pre-implementation contract for the dashboard UI. Every color, size, spacing,
and motion decision in the templates must trace to a token or rule here. Two
templates share this system: `templates/simple/` (the operational 2D dashboard
that ships today) and `templates/three-d/` (a 3D scene reading the same
protocol). They are one brand at two levels of immersion, not two brands.

## 0. Research Log

| Lane | Deliverable |
|---|---|
| Existing UI extraction | Tokens and patterns lifted from `templates/simple/src/index.css` (`@theme` palette, `.engraved`, tape motion, focus ring) and `templates/simple/src/components/{Header,NodeCard,TapeEdge,HeldStrip,NodeDetail,PipelineGraph}.tsx` (border-led depth, well/strip composition, 10-13px mono scale). Thresholds from `packages/protocol/src/health.ts` (STALL_THRESHOLD_MS = 10s). |
| Redesign reference audit | Untracked prototype `Pipeline visualization system redesign/`: `Pipeline Viz 3D.dc.html` (HUD chrome), `pipeline-stage.js` (scene: portal/prism/archive archetypes, grid floor, emissive escalation, orbit camera, perf caps), `sim.js` (mock data — NOT the wire protocol), `github.md` (screen map). Extracted: scene grammar, material mapping of the same palette, escalation levels, camera rules, perf budget. The prototype's sim is a visual stand-in; production three-d renders real Snapshot/Patch output like everything else. |
| Embedded references | Not run: the existing UI is itself the Layer B reference (warm industrial instrument), and the 3D prototype is the concrete second reference. No external brand system applies. |
| Lazyweb / Imagen | Not run: both concrete references already exist in-repo; generating mood material would add noise, not signal. |

## 1. Design Intent

The dashboard is an **instrument, not a dashboard**: a machine that runs work
items past the operator. Three commitments follow, and both templates honor
them:

1. **Stillness is the signal.** Steady work never flashes, pulses, or
   celebrates. The eye finds a stuck item because it *stopped moving*, not
   because something blinked. Any animation on healthy flow is slop.
2. **One color means one thing.** Three signal colors, three meanings, no
   reuse (section 2). Color is never decoration.
3. **Depth comes from borders and wells, not shadows.** The chassis is flat;
   structure is engraved into it with 1px rules and recessed wells. Elevation
   shadows, glows-as-chrome, and rounded-card SaaS language are out of system.

The 3D template is the same instrument read as a physical scene: the graphite
chassis becomes a floor, the border language becomes wireframe edges, the
signal colors become emissive materials. It extends section 7's mapping; it
never introduces a second palette, typeface, or accent.

## 2. Color System

Source of truth: `templates/simple/src/index.css` `@theme`. Hex values below are normative;
three-d maps the same values into materials (section 7).

### Surface ramp (warm graphite)

| Token | Value | Role |
|---|---|---|
| `chassis` | `#1c1b19` | App background, graph canvas floor |
| `panel` | `#232220` | Cards, header, strips, detail panel |
| `well` | `#2a2825` | Recessed inset holding live cells (node well); hover wash on panels |
| `rule` | `#383530` | All 1px borders/dividers; idle node border |

### Signal colors (each means exactly one thing)

| Token | Value | Meaning (only this) |
|---|---|---|
| `ink` | `#7fd1c1` | Movement / active: tape dashes, active cells, selected ring, links, focus ring, node border at 50% when flowing |
| `signal` | `#e8a33d` | Held with a stated reason: held cells, stalled node border, hold ages, warn markers |
| `fault` | `#c25b4a` | Abandoned (dropped without completing): abandoned cells/rows, fault strip, abandoned counter |

### Text

| Token | Value | Role |
|---|---|---|
| `paper` | `#e6e1d8` | Primary readings, names, values |
| `muted` | `#8a857c` | Engraved micro-labels, secondary metadata, queued state, hints |

### Tints and functional variants

- Tinted wells for stateful strips: `signal` or `fault` at ~10% opacity over
  `panel`, with a 40%-opacity border in the same hue (pattern from
  `NodeCard`'s abandoned strip).
- 3D scene void may sit one step darker than `chassis` (`#141311` fog,
  `#201f1d`/`#1d1c1a` dark metal) — these are *shading of the same graphite*,
  not new hues.
- Queued items read as `muted` in both templates; queue depth escalates to
  `signal` only when it crosses the attention rule of the view (2D: stage
  rail tint above 3 queued; 3D: widening orbit ring).
- No purple, no blue, no green-as-success. `ink` is mint and it means motion,
  not success. Nothing celebrates completion.

## 3. Typography

One family everywhere, both templates, including 3D canvas labels:

```
--font-mono: ui-monospace, "SF Mono", "JetBrains Mono", "Fira Code", Menlo, monospace;
font-variant-ligatures: none;   /* ligatures read as decoration on an instrument */
```

Scale (px) — four sizes, no others without a new token:

| Token | Size / style | Use |
|---|---|---|
| `engraved` | 10px, uppercase, `letter-spacing: 0.14em`, `muted` | Micro-labels: kinds, counter names, section headers, hints. Reads as engraving on the chassis. Implemented as the `.engraved` class. |
| `reading-sm` | 11px | Secondary values: hold reasons, stuck rows, legend, footer stats |
| `reading` | 12px | Item ids, detail-panel values, list rows |
| `name` | 13px | Node display names, header wordmark, metric values in header |

Rules:

- Font weight stays at 400/500. Hierarchy comes from the paper/muted split
  and the engraved treatment, never from bold text.
- Numbers are content; labels are engraved. A value is never set in
  `engraved` (the 3D HUD's header already models this: engraved label above,
  13px paper value below).
- Letter-spacing is a label device only (`0.14em`, or `0.08-0.1em` for 11px
  uppercase controls). Body text and numbers are never tracked out.
- 3D node labels render the same way via canvas sprites: 40px `name` in
  `paper`, 24px engraved `kind` sublabel in `muted`, ~6px tracking.

## 4. Spacing, Layout, Breakpoints

Spacing scale is the Tailwind 4px base; observed system values:

| Token | Value | Use |
|---|---|---|
| `gap-cell` | 3px | Between tape cells in a well; between legend swatches and text is 7px |
| `pad-inset` | 8px (`p-2`) | Inside wells and tight strips |
| `pad-panel` | 12px 16px (`px-3 py-2` / `px-4 py-3`) | Panel headers, card sections, strip rows |
| `gap-row` | 8px | Vertical rhythm inside list rows and dl grids |
| `gap-section` | 12-16px | Between card sections; header item separation is 24-32px |

Fixed chrome dimensions (both templates):

| Element | Size |
|---|---|
| Header height | 44px |
| Detail panel width | 340px, right-docked |
| Stage rail width | 184px, left, below header |
| Node card width | 300px (2D graph) |
| Tape cell | 6px x 20px, `border-radius: 1px`; held-strip tick 5px x 12px |

Radii: effectively square. `1px` on item cells/ticks only; panels, cards,
buttons, and wells are square-cornered. Focus ring: 2px `ink`, offset 2px.

Breakpoints — honest v0.1 posture:

- The instrument is **desktop-first, dense, single-screen**. Target
  1280px+; fully usable at 1024px.
- Below 1024px the 2D template keeps its structure but allows horizontal
  scroll of the graph canvas; the detail panel overlays instead of docking.
- Below 768px is **accepted debt** (section 8): no dedicated mobile layout in
  v0.1. The 3D template is pointer-and-wheel and makes no touch claims in
  v0.1 beyond not crashing.
- Three-d HUD insets (`pad-left/right/top/bottom`) are measured from the
  rendered chrome, not hardcoded, so the scene recenters when panels open.

## 5. Component Primitives & States

Every screen in both templates composes these seven primitives. States listed
are the complete set; do not invent new ones without extending this file.

### 5.1 Status signal

The atomic state mark: a small square/tick (no icons, no emoji) in `ink`,
`signal`, `fault`, or `muted`.

- States: `moving` (ink), `held` (signal), `abandoned` (fault),
  `queued` (muted), `idle/empty` (muted engraved text, no mark).
- Forms: 6px header liveness square; 6x20 tape cell; 5x12 strip tick;
  8px legend swatch. In 3D: the emissive material of an item orb.
- Never blinks. Escalation is by *stopping* and by color, not animation.

### 5.2 Metric reading

Engraved label + paper value, stacked (header) or in a 2-col `dl` (detail
panel, node card footer).

- States: normal (paper value); attention (value in `signal` — e.g. queued
  total); fault (value in `fault` — abandoned count).
- Whole-process CPU/RAM are always labeled `whole process` — see section 8,
  fabricated per-node resources are forbidden.

### 5.3 Pipeline node / stage

2D: `NodeCard` — 300px panel, border color carries health
(`idle` rule / `active` ink-50 / `stalled` signal), name + engraved kind
header, recessed well of tape cells, abandoned strip, oldest-stuck row,
rate + p50/p95 footer. Selected: 1px `ink` ring.

3D: archetype machine on the grid (section 7) with canvas label, invisible
hit sphere, selection ring (flat torus on the floor), warn cone above the
node at escalation level >= 2.

- States (both): `idle`, `active`, `stalled` (any item > 10s, border/accent
  `signal`), `faulted` (any abandoned present, accent `fault`).

### 5.4 Held strip

Persistent answer to "what is stuck right now": the longest-waiting items,
held before abandoned, oldest first, capped (2D shows the ranked list; the
reference shows 5).

- Row = status tick + job id + age (in state color) + engraved node name +
  truncated reason. Click selects the node. `min-height` keeps the strip
  present when empty — the instrument never collapses its answer row.

### 5.5 Detail panel

Right-docked 340px, `panel` at ~90% opacity over the canvas, `rule` left
border: node name + engraved `id · kind · archetype` header, close control,
2-col metric grid (in flight, queued, rate, p50, p95, left total), then
`items here (n)` list — tick, id, engraved type, age, reason in state color,
one meta line (`tx_count`). Identical content model in both templates; only
the canvas behind it changes.

### 5.6 Graph / stage canvas

2D: React Flow canvas on `chassis`, nodes from `graph/layout.ts`, edges as
`TapeEdge` — dashed tape advancing at a per-edge period derived from measured
throughput (`--tape-period`, default 2s, linear, continuous); still tape on
idle edges (2px dash, no motion).

3D: `<pipeline-stage>` scene per section 7.

- Rule for both: the canvas renders positions and counts from the protocol;
  layout math (`layout.ts`, `health.ts`, `tape.ts`) is pure and unit-tested.
  No per-transition animation beyond the tape's steady advance (edge-travel
  animation is v0.1 debt, section 8).

### 5.7 Template shell

The chrome wrapping the canvas: header (wordmark + liveness square, in
flight / abandoned / queued, whole-process CPU/RAM), template content,
bottom bar (legend + held strip in both templates; scenario buttons exist
only in the prototype and are not product surface). Shell is `panel` at
82-90% opacity with `backdrop-filter: blur(6px)` over the canvas, `rule`
borders, 8-12px vertical padding. Template switching (simple / three-d) is a
shell-level concern; primitives 5.1-5.6 are shared.

## 6. Motion

Motion serves meaning; steady state is still.

| Motion | Spec | Meaning |
|---|---|---|
| Tape advance | `stroke-dashoffset` to -16, `var(--tape-period)` (default 2s), `linear infinite` | Work flowing on this edge; period set from measured throughput |
| Still tape | dash `2 10`, no animation | Edge alive, nothing moving |
| 3D item orbits | active items drift/spin; held items stop in place; scale pulse amplitude 0 / 0.05 / 0.12 / 0.22 by escalation level | A stalled item simply stops — that IS the alert |
| 3D fault flicker | only at level 3 (abandoned or >60s): emissive/wireframe flicker `sin(t*8..9)` | Fault-level escalation; the only flash in the system |
| 3D camera | ambient sweep: slow yaw oscillation ±0.22 rad around broadside; detail mode: 0.035 rad/s; fit-ease `dt*3.2` lerp on panel open | Orientation, never spectacle |
| HUD in | `opacity 0 -> 1` once on mount | Chrome appearing |

Hard rules:

- `prefers-reduced-motion: reduce` kills tape animation and all 3D ambient
  motion (orbit drift, pulses, sweeps); state color and layout remain.
- Animate `transform`, `opacity`, `stroke-dashoffset`, and 3D uniforms only.
  Never animate layout properties.
- No transitions on hover that imply interactivity where there is none.
  Hover = `well`-grade background lightening or muted -> paper text; that is
  the entire hover vocabulary.

## 7. Depth & the three-d Template

The 3D scene is the same instrument read as a cyber-physical floor. This
mapping is normative — it is what makes three-d an extension, not a reskin:

| 2D token/pattern | 3D expression |
|---|---|
| `chassis` floor | `WebGLRenderer` clear color `#1c1b19`; exponential fog `#141311`, density tied to camera distance so pulling back never greys the scene |
| Border-led depth | Wireframe `EdgesGeometry` on every solid: `ink` at 50% for live edges, `rule` at 70% for structure. No shadows (shadowMap off), no postprocessing |
| Signal colors | Emissive materials: active `ink` (intensity ~1.6), held `signal` (~1.8), fault `c25b4a` (~1.6 + wireframe flicker), queued `muted` (~0.9). Dark diffuse bases so emissive carries the state |
| NodeCard | Archetype by `NodeKind`: **portal** (source — plinth + three counter-rotating torus rings + fins + core disc), **prism** (transform — hex base + glass octahedron + 3 orbiting shards + base ring; fins ride up as p50 climbs), **archive** (sink — 5x5 tile matrix whose wave lights on commit + stepped wire frames + corner slabs) |
| Tape cell well | Item orbs (0.17 box) held in a ring at the node; queued items orbit *outside* the node on a ring that widens with queue depth — backpressure made physical |
| Tape edge | Quadratic bezier arc between nodes, `ink` at 16% opacity; items in transit travel the arc (prototype behavior; see debt) |
| Stall escalation | Levels from item age: 0 = fine; 1 = >10s (STALL_THRESHOLD_MS); 2 = >30s (warn cone appears, node accent -> signal); 3 = >60s or abandoned (accent -> fault, loud flicker). Same thresholds as 2D health |
| Engraved label | Canvas-sprite node label: name + kind, mono, paper/muted |
| Selection | Click hit-sphere -> floor selection ring in current accent; detail panel opens; camera fit eases, never cuts |

Camera contract: drag swings yaw (±), wheel zooms (0.45-2.2), click picks a
stage. The whole pipeline stays framed — selection lights a stage, it does
not fly the camera away from the line. A pipeline is a line; the camera
sweeps a limited arc around broadside so every stage stays visible.

Performance contract (part of the design, not engineering trivia): shared
geometry/materials, pooled item meshes, pixel ratio capped (1.35 ambient /
2 detail), one hemisphere + one directional light, render loop paused when
the tab is hidden. Long background runs are the use case.

## 8. Live-Data Truth, Accessibility, Accepted Debt

### Live-data truth (non-negotiable)

- The UI **renders Snapshot/Patch output**. Snapshot on connect, coalesced
  patches on the 100ms tick, reconnect = fresh snapshot. Aggregation,
  percentiles, throughput, and coalescing happen in the Rust collector;
  the browser derives layout and health from the stream and nothing else.
- **No fabricated data.** Per-node CPU/RAM do not exist and are never shown;
  the header's CPU/RAM are whole-process and labeled `whole process`.
- The three-d prototype's `sim.js` is reference-only. Production three-d
  consumes the same `@pipeline-viz/protocol` stream as simple. If a number is not
  in the protocol, it is not on screen.
- `dropped_events` is surfaced, not hidden (instrument honesty).

### Accessibility constraints

- Signal colors are never the sole carrier of state: position (held strip),
  text (reason, age), and shape (border/tick/escalation level) always
  co-carry it. Mint/amber/terracotta on graphite were chosen distinct in
  hue and lightness; verify contrast at implementation (paper on chassis
  exceeds 12:1; signal/fault used for text only at >= 11px on panel/chassis
  — check against 4.5:1 and adjust lightness, never hue).
- Focus visible everywhere: 2px `ink` outline, 2px offset; canvas picking in
  3D has a keyboard equivalent via the stage rail (buttons, real focus).
- Reduced motion per section 6. No flashing content except the level-3 fault
  flicker, which must respect reduced-motion (static fault wireframe
  instead) and stays under 3 flashes/sec.
- All controls are real buttons with engraved or 11px labels; hit targets
  >= 24px in the dense chrome.

### Accepted debt (explicit, scoped)

1. **No edge-travel animation in v0.1** (2D): edges show tape flow; items do
   not animate node-to-node. The 3D prototype travels orbs along arcs —
   adopting that in production three-d is a separate decision, not assumed.
2. **WebGL fallback**: if WebGL is unavailable or context creation fails,
   three-d degrades to the simple template with an engraved notice. No
   software-rendered 3D, no broken canvas.
3. **No-embed packaging**: the UI runs from source (`vite dev` / build)
   until milestone 4 lands `rust-embed`; nothing in this system may depend
   on dev-server-only behavior.
4. **No sub-768px layout** in v0.1 (section 4).
5. **Custom theming** is out of scope; this file is the only theme.
