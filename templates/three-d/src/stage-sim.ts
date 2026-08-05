import type { JobState, NodeState, PipelineData } from "@pipeline-viz/protocol";
import { STALL_THRESHOLD_MS } from "@pipeline-viz/protocol";

/**
 * The shape pipeline-stage.js reads off `window.PipelineSim`. The stage is a
 * classic script with no type knowledge, so these are the exact field names it
 * greps for — do not rename casually.
 */
export interface StageJob {
  job_id: string;
  node: string;
  phase: "active" | "held" | "abandoned";
  /** "travel" = between two nodes on a curve; "idle" = anchored at a node. */
  mode: "travel" | "idle";
  /** True when backpressure parks the item outside the node's orbit. */
  queued: boolean;
  /** Wire clock, in ms. The stage ages anchored items against this. */
  entered_node_at_ms: number;
  travel?: { from: string; to: string; start: number; dur: number };
}

export interface StageNode {
  display_name: string;
  kind: NodeState["kind"];
  inputs: string[];
  counters: {
    queue_depth: number;
    in_flight: number;
    throughput_per_sec: number;
    p50_ms: number;
  };
}

interface Position {
  x: number;
  z: number;
}

/** Grid layout, ported from the prototype sim.js (which ported layout.ts).
    Columns are graph ranks (longest input chain); rows are order within a
    column. Scaled from px to world units. */
const COLUMN_WIDTH = 430;
const ROW_HEIGHT = 300;
const SCALE = 1 / 68;
const TRAVEL_DUR_MS = 400;

function layout(nodes: NodeState[]): Record<string, Position> {
  const byId = new Map(nodes.map((node) => [node.node_id, node]));
  const ranks = new Map<string, number>();

  const rankOf = (id: string, path: Set<string>): number => {
    const cached = ranks.get(id);
    if (cached !== undefined) return cached;
    const node = byId.get(id);
    if (!node || path.has(id)) return 0;
    path.add(id);
    let deepest = 0;
    for (const input of node.inputs) {
      if (!byId.has(input)) continue;
      deepest = Math.max(deepest, rankOf(input, path) + 1);
    }
    path.delete(id);
    ranks.set(id, deepest);
    return deepest;
  };

  const ids = [...byId.keys()].sort();
  for (const id of ids) rankOf(id, new Set());

  const columns = new Map<number, string[]>();
  for (const id of ids) {
    const rank = ranks.get(id) ?? 0;
    const column = columns.get(rank) ?? [];
    column.push(id);
    columns.set(rank, column);
  }

  const positions: Record<string, Position> = {};
  for (const [rank, column] of columns) {
    column.forEach((id, row) => {
      positions[id] = { x: rank * COLUMN_WIDTH * SCALE, z: row * ROW_HEIGHT * SCALE };
    });
  }
  return positions;
}

export function orderNodeIds(nodes: NodeState[]): string[] {
  const byId = new Map(nodes.map((node) => [node.node_id, node]));
  const ranks = new Map<string, number>();

  const rankOf = (id: string, path: Set<string>): number => {
    const cached = ranks.get(id);
    if (cached !== undefined) return cached;
    const node = byId.get(id);
    if (!node || path.has(id)) return 0;
    path.add(id);
    const rank = node.inputs.reduce(
      (deepest, input) => (byId.has(input) ? Math.max(deepest, rankOf(input, path) + 1) : deepest),
      0,
    );
    path.delete(id);
    ranks.set(id, rank);
    return rank;
  };

  for (const id of byId.keys()) rankOf(id, new Set());
  return [...byId.keys()].sort(
    (left, right) => (ranks.get(left) ?? 0) - (ranks.get(right) ?? 0) || left.localeCompare(right),
  );
}

function toStagePhase(job: JobState): StageJob["phase"] {
  return job.phase;
}

/**
 * Live bridge between the protocol store and the WebGL stage.
 *
 * Pure data in, pure data out: no DOM, no three.js, no store import — tests
 * drive it with wire-shaped fixtures and assert the stage-shaped output. The
 * React side calls `ingest` on every store change and wires `onSelect` back.
 */
export class StageSim {
  nodes: Record<string, StageNode> = {};
  positions: Record<string, Position> = {};
  selected: string | null = null;
  stallThresholdMs = STALL_THRESHOLD_MS;
  /** Called by the stage's click handler; wired to the store by React. */
  onSelect: ((nodeId: string | null) => void) | null = null;

  private jobs = new Map<string, StageJob>();
  /** Latest wire timestamp — the age baseline for anchored items. */
  private tsMs = 0;

  select(nodeId: string | null): void {
    this.selected = nodeId;
    this.onSelect?.(nodeId);
  }

  jobList(): StageJob[] {
    return [...this.jobs.values()].sort((a, b) => (a.job_id < b.job_id ? -1 : 1));
  }

  /** Age in ms. Anchored items age against the wire clock (the stage's own
      `now` argument is page-relative, so it is ignored); traveling items age
      against their travel start, which the stage measures on its own clock. */
  ageOf(job: StageJob, now: number): number {
    if (job.travel) return Math.max(0, now - job.travel.start);
    return Math.max(0, this.tsMs - job.entered_node_at_ms);
  }

  /** Replace everything from a snapshot, or fold in a patch — the caller has
      already reduced either into a single PipelineData, so this method only
      diffs jobs for travel animation and updates node tables. */
  ingest(data: PipelineData): void {
    this.tsMs = data.ts_ms;

    this.nodes = Object.fromEntries(
      Object.values(data.nodes).map((node) => [
        node.node_id,
        {
          display_name: node.display_name,
          kind: node.kind,
          inputs: node.inputs,
          counters: {
            queue_depth: node.counters.queue_depth,
            in_flight: node.counters.in_flight,
            throughput_per_sec: node.counters.throughput_per_sec,
            p50_ms: node.counters.p50_ms,
          },
        },
      ]),
    );
    this.positions = layout(Object.values(data.nodes));

    const wireJobs = Object.values(data.jobs);
    const now = performance.now();
    const next = new Map<string, StageJob>();
    for (const wire of wireJobs) {
      next.set(wire.job_id, this.toStageJob(wire, now));
    }
    this.jobs = next;

    // Backpressure made explicit: at a node, items beyond the reported
    // in-flight capacity (oldest first) are queued and orbit outside.
    const perNode = new Map<string, StageJob[]>();
    for (const job of this.jobs.values()) {
      if (job.mode === "travel" || job.phase === "abandoned") continue;
      const list = perNode.get(job.node) ?? [];
      list.push(job);
      perNode.set(job.node, list);
    }
    for (const [nodeId, list] of perNode) {
      const inFlight = this.nodes[nodeId]?.counters.in_flight ?? 0;
      list
        .sort((a, b) => a.entered_node_at_ms - b.entered_node_at_ms)
        .forEach((job, index) => {
          job.queued = index >= inFlight;
        });
    }
  }

  private toStageJob(wire: JobState, now: number): StageJob {
    const previous = this.jobs.get(wire.job_id);
    const moved = previous !== undefined && previous.node !== wire.current_node;
    const travel = previous?.travel;
    const travelDone =
      travel !== undefined && now - travel.start > travel.dur && !moved;

    const job: StageJob = {
      job_id: wire.job_id,
      node: wire.current_node,
      phase: toStagePhase(wire),
      mode: "idle",
      queued: false,
      entered_node_at_ms: wire.entered_node_at_ms,
    };

    if (moved) {
      job.mode = "travel";
      job.travel = {
        from: previous!.node,
        to: wire.current_node,
        start: now,
        dur: TRAVEL_DUR_MS,
      };
    } else if (travel !== undefined && !travelDone) {
      // Same node, travel still in flight — keep gliding.
      job.mode = "travel";
      job.travel = travel;
    }
    return job;
  }
}
