import { describe, expect, it } from "vitest";
import { formatAge, holdReason } from "./format";
import type { JobState } from "./types";

const base = {
  job_id: "block_1",
  job_type: "Block",
  current_node: "indexer",
  entered_node_at_ms: 0,
  created_at_ms: 0,
  meta: {},
};

const activeJob: JobState = { ...base, phase: "active" };
const heldJob: JobState = { ...base, phase: "held", reason: "Waiting for finality" };
const abandonedJob: JobState = { ...base, phase: "abandoned" };

describe("formatAge", () => {
  it("uses milliseconds below a second", () => {
    expect(formatAge(420)).toBe("420ms");
  });

  it("uses seconds below a minute", () => {
    expect(formatAge(4_200)).toBe("4.2s");
  });

  it("uses minutes and seconds beyond that", () => {
    expect(formatAge(125_000)).toBe("2m 5s");
  });

  it("never renders a negative age from clock skew between browser and host", () => {
    expect(formatAge(-50)).toBe("0ms");
  });
});

describe("holdReason", () => {
  it("returns the reason of a held item", () => {
    expect(holdReason(heldJob)).toBe("Waiting for finality");
  });

  it("explains an abandoned item rather than showing nothing", () => {
    expect(holdReason(abandonedJob)).toBe("Abandoned — dropped without completing");
  });

  it("returns null for an item that is working", () => {
    expect(holdReason(activeJob)).toBeNull();
  });
});
