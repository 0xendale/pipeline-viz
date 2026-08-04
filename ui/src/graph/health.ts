import type { JobState, NodeState } from "../protocol/types";

/**
 * How long an item may sit at one node before the node is flagged.
 *
 * Ten seconds is long enough that ordinary work never trips it, and short
 * enough that a genuine stall is visible before the user goes hunting in logs.
 */
export const STALL_THRESHOLD_MS = 10_000;

export type Health = "idle" | "active" | "stalled";

export function nodeHealth(node: NodeState, jobs: JobState[], nowMs: number): Health {
  if (node.counters.in_flight === 0) return "idle";

  const stalled = jobs.some(
    (job) =>
      job.current_node === node.node_id &&
      job.phase !== "abandoned" &&
      nowMs - job.entered_node_at_ms > STALL_THRESHOLD_MS,
  );

  return stalled ? "stalled" : "active";
}

/**
 * The longest-waiting items across the whole pipeline.
 *
 * This is the direct answer to "what is stuck right now". Node colour alone
 * makes the user guess which node to open first; on a wide graph that is the
 * difference between seeing the answer and hunting for it.
 *
 * Held items rank ahead of abandoned ones regardless of age. Abandoned items
 * never leave the pipeline, so sorting on age alone lets them accumulate and
 * permanently occupy every slot — which is exactly when the strip stops
 * reporting anything live. They still appear once the live holds run out.
 */
export function oldestHeld(jobs: JobState[], limit: number): JobState[] {
  const priority = (job: JobState) => (job.phase === "held" ? 0 : 1);

  return jobs
    .filter((job) => job.phase === "held" || job.phase === "abandoned")
    .sort((left, right) => {
      const byPhase = priority(left) - priority(right);
      return byPhase !== 0 ? byPhase : left.entered_node_at_ms - right.entered_node_at_ms;
    })
    .slice(0, limit);
}

/** Items in flight, excluding abandoned ones — those are no longer moving. */
export function inFlightCount(jobs: JobState[]): number {
  return jobs.filter((job) => job.phase !== "abandoned").length;
}

/** Items dropped without completing. Almost always a bug in the host pipeline. */
export function abandonedCount(jobs: JobState[]): number {
  return jobs.filter((job) => job.phase === "abandoned").length;
}
