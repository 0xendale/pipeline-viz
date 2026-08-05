import { describe, expect, it } from "vitest";
import { emptyPipeline } from "@pipeline-viz/protocol";
import { selectJobsById } from "./selectors";

describe("three-d store selectors", () => {
  it("returns cached jobs reference for unchanged state", () => {
    const data = emptyPipeline();
    const state = { data };

    expect(selectJobsById(state)).toBe(selectJobsById(state));
  });
});
