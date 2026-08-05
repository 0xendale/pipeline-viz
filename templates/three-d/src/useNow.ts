import { useEffect, useState } from "react";

/**
 * Re-render clock, in wire epoch ms (Date.now), so ages computed against
 * `entered_node_at_ms` are meaningful.
 */
export function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);
  return now;
}
