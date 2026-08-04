import { describe, expect, it } from "vitest";
import { MAX_BACKOFF_MS, nextBackoffMs } from "./backoff";

describe("nextBackoffMs", () => {
  it("retries almost immediately the first time", () => {
    // The most common disconnect is the server dropping a lagging client, and
    // recovery from that should be invisible.
    expect(nextBackoffMs(0)).toBeLessThanOrEqual(500);
  });

  it("grows with each failed attempt", () => {
    expect(nextBackoffMs(1)).toBeGreaterThan(nextBackoffMs(0));
    expect(nextBackoffMs(4)).toBeGreaterThan(nextBackoffMs(2));
  });

  it("never exceeds the ceiling, however long the server is down", () => {
    expect(nextBackoffMs(50)).toBe(MAX_BACKOFF_MS);
    expect(nextBackoffMs(1_000)).toBe(MAX_BACKOFF_MS);
  });
});
