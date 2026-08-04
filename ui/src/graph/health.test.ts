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

const abandoned = (job_id: string, entered_node_at_ms: number): JobState => ({
  job_id,
  job_type: "Block",
  current_node: "indexer",
  phase: "abandoned",
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
    expect(nodeHealth(node(1), [held("block_1", now - STALL_THRESHOLD_MS - 1)], now)).toBe(
      "stalled",
    );
  });

  it("is not stalled by an item that only just arrived", () => {
    const now = 100_000;
    expect(nodeHealth(node(1), [held("block_1", now - 100)], now)).toBe("active");
  });

  it("counts a long-running active item as stalled too", () => {
    // An item that is neither held nor moving is exactly what the user is
    // hunting, so it is not excused for lacking a stated reason.
    const now = 100_000;
    expect(nodeHealth(node(1), [active("block_1", now - STALL_THRESHOLD_MS - 1)], now)).toBe(
      "stalled",
    );
  });

  it("ignores items sitting at other nodes", () => {
    const now = 100_000;
    const elsewhere = { ...held("block_1", now - 60_000), current_node: "committer" };
    expect(nodeHealth(node(1), [elsewhere], now)).toBe("active");
  });
});

describe("oldestHeld", () => {
  const now = 100_000;

  it("returns the longest-waiting items first", () => {
    const jobs = [held("new", now - 1_000), held("old", now - 50_000), held("mid", now - 20_000)];
    expect(oldestHeld(jobs, 5).map((job) => job.job_id)).toEqual(["old", "mid", "new"]);
  });

  it("caps the list", () => {
    const jobs = Array.from({ length: 20 }, (_, index) => held(`block_${index}`, now - index * 100));
    expect(oldestHeld(jobs, 5)).toHaveLength(5);
  });

  it("ignores items that are working", () => {
    expect(oldestHeld([active("block_1", 0)], 5)).toEqual([]);
  });

  it("includes abandoned items, which are the worst case of stuck", () => {
    expect(oldestHeld([abandoned("block_1", 0)], 5).map((job) => job.job_id)).toEqual(["block_1"]);
  });
});
