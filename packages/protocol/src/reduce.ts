import type { JobState, NodeState, Patch, ProcessStats, Snapshot } from "./types";

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

function byNodeId(nodes: NodeState[]): Record<string, NodeState> {
  return Object.fromEntries(nodes.map((node) => [node.node_id, node]));
}

function byJobId(jobs: JobState[]): Record<string, JobState> {
  return Object.fromEntries(jobs.map((job) => [job.job_id, job]));
}

/**
 * A snapshot is complete state, so it replaces rather than merges.
 *
 * This is what makes reconnecting self-correcting: whatever the client believed
 * before is discarded, including items that were removed while it was away.
 */
export function applySnapshot(snapshot: Snapshot): PipelineData {
  return {
    nodes: byNodeId(snapshot.nodes),
    jobs: byJobId(snapshot.jobs),
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
  const nodes = { ...current.nodes, ...byNodeId(patch.nodes) };
  const jobs = { ...current.jobs, ...byJobId(patch.jobs) };
  for (const job_id of patch.removed_jobs) {
    delete jobs[job_id];
  }

  return {
    nodes,
    jobs,
    dropped_events: patch.dropped_events,
    // A patch omits the process reading when it has not changed, so the last
    // known value stands rather than the gauge blinking empty every tick.
    process: patch.process ?? current.process,
    ts_ms: patch.ts_ms,
  };
}
