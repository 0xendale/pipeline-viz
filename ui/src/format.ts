import type { JobState } from "./protocol/types";

/**
 * Ages are computed against the browser clock, which can sit slightly behind
 * the host's timestamps, so a small negative value is clamped rather than shown.
 */
export function formatAge(ms: number): string {
  const clamped = Math.max(0, ms);
  if (clamped < 1_000) return `${Math.round(clamped)}ms`;
  if (clamped < 60_000) return `${(clamped / 1_000).toFixed(1)}s`;

  const minutes = Math.floor(clamped / 60_000);
  const seconds = Math.floor((clamped % 60_000) / 1_000);
  return `${minutes}m ${seconds}s`;
}

/** Why an item is not moving, if there is a reason to show. */
export function holdReason(job: JobState): string | null {
  if (job.phase === "held") return job.reason;
  if (job.phase === "abandoned") return "Abandoned — dropped without completing";
  return null;
}
