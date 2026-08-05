import { describe, expect, it } from "vitest";
import { layoutNodes } from "./layout";
import type { NodeState } from "@pipeline-viz/protocol";

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
  it("places a linear pipeline left to right in flow order", () => {
    // Deliberately unsorted: position must come from the declared edges, not
    // from the order the nodes happened to arrive in.
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

  it("terminates on a cycle instead of hanging the tab", () => {
    const positions = layoutNodes([node("a", ["b"]), node("b", ["a"])]);
    expect(Object.keys(positions).sort()).toEqual(["a", "b"]);
  });

  it("ignores inputs naming nodes that do not exist", () => {
    const positions = layoutNodes([node("indexer", ["ghost"])]);
    expect(positions.indexer).toEqual({ x: 0, y: 0 });
  });

  it("is stable across calls, so nodes do not jump between renders", () => {
    const nodes = [node("fetcher", []), node("blocks", ["fetcher"]), node("receipts", ["fetcher"])];
    expect(layoutNodes(nodes)).toEqual(layoutNodes([...nodes].reverse()));
  });
});
