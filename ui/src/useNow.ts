import { useEffect, useState } from "react";

/**
 * A clock that advances on an interval.
 *
 * Item ages must keep rising while a held item sits still and no patch arrives,
 * so age cannot be derived from message timestamps alone.
 */
export function useNow(intervalMs = 500): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);

  return now;
}
