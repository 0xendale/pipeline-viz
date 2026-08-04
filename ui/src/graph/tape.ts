import type { JobState } from "../protocol/types";

/** One item, as it appears in a node's tape well. */
export interface TapeCell {
  job_id: string;
  phase: JobState["phase"];
  /** How long the item has been at this node, for the tooltip. */
  age_ms: number;
}

export interface Tape {
  /** Live items at this node — active or held — oldest first. */
  cells: TapeCell[];
  /** Live items beyond what the well can show. */
  overflow: number;
  /** Live items in total, shown or not. Matches the node's in_flight counter. */
  live: number;
  /** Items dropped without completing. Reported apart from the well. */
  abandoned: number;
  /** The longest-waiting item that is not moving, if there is one. */
  oldestStuck: TapeCell | null;
}

/**
 * The items sitting at one node, oldest first.
 *
 * Oldest-first is not an arbitrary sort. An item's position in the well is its
 * waiting order, so anything stuck drifts to the left and stays there while
 * newer work streams past it on the right. The eye finds the stalled cell
 * without anything having to blink to attract it.
 */
export function tapeFor(
  jobs: JobState[],
  node_id: string,
  capacity: number,
  nowMs: number,
): Tape {
  const present = jobs
    .filter((job) => job.current_node === node_id)
    .sort((left, right) => left.entered_node_at_ms - right.entered_node_at_ms);

  const toCell = (job: JobState): TapeCell => ({
    job_id: job.job_id,
    phase: job.phase,
    age_ms: Math.max(0, nowMs - job.entered_node_at_ms),
  });

  // Abandoned items never leave, so leaving them in the well lets a single bad
  // stage fill with dead cells until live work is invisible. They are a fault
  // condition, not throughput, and get counted separately instead.
  const live = present.filter((job) => job.phase !== "abandoned");
  const stuck = present.find((job) => job.phase !== "active");

  return {
    cells: live.slice(0, capacity).map(toCell),
    overflow: Math.max(0, live.length - capacity),
    live: live.length,
    abandoned: present.length - live.length,
    oldestStuck: stuck ? toCell(stuck) : null,
  };
}

/**
 * Seconds for one dash to travel one tape segment, from measured throughput.
 *
 * Faster stage, faster tape. A stage moving nothing gets `null`, which the edge
 * renders as a still tape rather than as motion with no work behind it.
 */
export function tapePeriodSeconds(throughputPerSec: number): number | null {
  if (throughputPerSec <= 0) return null;

  // One dash per item would strobe at high throughput and crawl at low
  // throughput, so the rate is compressed: readable as "faster" or "slower"
  // without ever becoming a flicker.
  const period = 2 / Math.sqrt(throughputPerSec);
  return Math.min(4, Math.max(0.4, Number(period.toFixed(2))));
}
