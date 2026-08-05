const BASE_MS = 250;

export const MAX_BACKOFF_MS = 5_000;

/** Exponential backoff, capped. Attempt 0 is the first retry. */
export function nextBackoffMs(attempt: number): number {
  return Math.min(BASE_MS * 2 ** attempt, MAX_BACKOFF_MS);
}
