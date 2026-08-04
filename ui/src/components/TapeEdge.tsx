import { BaseEdge, getBezierPath, type Edge, type EdgeProps } from "@xyflow/react";

export interface TapeEdgeData extends Record<string, unknown> {
  /** Seconds per dash cycle, or null when the source stage is moving nothing. */
  periodSeconds: number | null;
}

export type TapeEdge = Edge<TapeEdgeData, "tape">;

/**
 * The tape running between two stages.
 *
 * It advances continuously at a rate set by the source stage's measured
 * throughput, and holds still when that stage is moving nothing. It is not a
 * per-item animation: the wire protocol has no in-transit state, so a dash is
 * a rate made visible, never a claim about where a particular item is.
 */
export function TapeEdgeLine({
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
}: EdgeProps<TapeEdge>) {
  const [path] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });

  const period = data?.periodSeconds ?? null;

  return (
    <BaseEdge
      path={path}
      className={period === null ? "tape-still" : "tape-flow"}
      style={{
        stroke: period === null ? "var(--color-rule)" : "var(--color-ink)",
        strokeWidth: 1.5,
        opacity: period === null ? 1 : 0.75,
        ...(period === null ? {} : { ["--tape-period" as string]: `${period}s` }),
      }}
    />
  );
}
