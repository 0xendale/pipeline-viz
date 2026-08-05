import { describe, expect, it } from "vitest";
import { tapeFor, tapePeriodSeconds } from "./tape";
import type { JobState } from "@pipeline-viz/protocol";

const at = (job_id: string, current_node: string, entered_node_at_ms: number): JobState => ({
  job_id,
  job_type: "Block",
  current_node,
  phase: "active",
  entered_node_at_ms,
  created_at_ms: entered_node_at_ms,
  meta: {},
});

const heldAt = (job_id: string, current_node: string, entered_node_at_ms: number): JobState => ({
  ...at(job_id, current_node, entered_node_at_ms),
  phase: "held",
  reason: "Waiting for finality",
});

describe("tapeFor", () => {
  const now = 100_000;

  it("shows only the items at that node", () => {
    const jobs = [at("here", "indexer", now), at("elsewhere", "committer", now)];
    expect(tapeFor(jobs, "indexer", 10, now).cells.map((c) => c.job_id)).toEqual(["here"]);
  });

  it("orders oldest first, so a stuck item drifts left and stays there", () => {
    const jobs = [
      at("newest", "indexer", now - 100),
      heldAt("stuck", "indexer", now - 90_000),
      at("middle", "indexer", now - 5_000),
    ];

    expect(tapeFor(jobs, "indexer", 10, now).cells.map((c) => c.job_id)).toEqual([
      "stuck",
      "middle",
      "newest",
    ]);
  });

  it("reports how many items it could not show", () => {
    const jobs = Array.from({ length: 30 }, (_, index) =>
      at(`block_${index}`, "indexer", now - index),
    );
    const tape = tapeFor(jobs, "indexer", 24, now);

    expect(tape.cells).toHaveLength(24);
    expect(tape.overflow).toBe(6);
    expect(tape.live).toBe(30);
  });

  it("has no overflow when everything fits", () => {
    expect(tapeFor([at("block_1", "indexer", now)], "indexer", 24, now).overflow).toBe(0);
  });

  it("carries each item's age at the node", () => {
    const tape = tapeFor([at("block_1", "indexer", now - 4_000)], "indexer", 24, now);
    expect(tape.cells[0].age_ms).toBe(4_000);
  });

  it("clamps a negative age from clock skew", () => {
    const tape = tapeFor([at("block_1", "indexer", now + 500)], "indexer", 24, now);
    expect(tape.cells[0].age_ms).toBe(0);
  });

  it("is empty at an idle node", () => {
    expect(tapeFor([], "indexer", 24, now)).toEqual({
      cells: [],
      overflow: 0,
      live: 0,
      abandoned: 0,
      oldestStuck: null,
    });
  });

  it("keeps abandoned items out of the well and counts them apart", () => {
    // Abandoned items never leave the pipeline. Left in the well they pile up
    // until a stage is a wall of dead cells and live work cannot be seen.
    const jobs = [
      at("moving", "indexer", now - 10),
      ...Array.from({ length: 40 }, (_, index) => ({
        ...at(`dead_${index}`, "indexer", now - 90_000),
        phase: "abandoned" as const,
      })),
    ];
    const tape = tapeFor(jobs, "indexer", 24, now);

    expect(tape.cells.map((cell) => cell.job_id)).toEqual(["moving"]);
    expect(tape.live).toBe(1);
    expect(tape.abandoned).toBe(40);
    expect(tape.overflow).toBe(0);
  });

  it("names the longest-waiting item that is not moving", () => {
    const jobs = [
      at("moving", "indexer", now - 200_000),
      heldAt("stuck", "indexer", now - 90_000),
      heldAt("newer", "indexer", now - 10_000),
    ];

    expect(tapeFor(jobs, "indexer", 24, now).oldestStuck?.job_id).toBe("stuck");
  });

  it("names nothing stuck when every item is moving", () => {
    expect(tapeFor([at("moving", "indexer", now)], "indexer", 24, now).oldestStuck).toBeNull();
  });
});

describe("tapePeriodSeconds", () => {
  it("gives no period to a stage moving nothing, so its tape stays still", () => {
    expect(tapePeriodSeconds(0)).toBeNull();
    expect(tapePeriodSeconds(-1)).toBeNull();
  });

  it("runs a busier stage faster", () => {
    const slow = tapePeriodSeconds(1);
    const fast = tapePeriodSeconds(50);
    expect(slow).not.toBeNull();
    expect(fast).not.toBeNull();
    expect(fast!).toBeLessThan(slow!);
  });

  it("never runs fast enough to read as a flicker", () => {
    // A dash arriving more than a few times a second stops reading as travel
    // and starts reading as a strobe, which is what this design avoids.
    expect(tapePeriodSeconds(100_000)).toBeGreaterThanOrEqual(0.4);
  });

  it("never crawls so slowly that a working stage looks stopped", () => {
    expect(tapePeriodSeconds(0.01)).toBeLessThanOrEqual(4);
  });
});
