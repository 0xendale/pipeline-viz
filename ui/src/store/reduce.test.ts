import { describe, expect, it } from "vitest";
import { applyPatch, applySnapshot, emptyPipeline } from "./reduce";
import type { JobState, NodeState, Patch, Snapshot } from "../protocol/types";

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
