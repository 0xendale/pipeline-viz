import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import type { NodeState } from "../protocol/types";
import type { Health } from "../graph/health";

export interface NodeCardData extends Record<string, unknown> {
  node: NodeState;
  health: Health;
  isSelected: boolean;
}

export type PipelineNode = Node<NodeCardData, "pipelineNode">;

const BORDER: Record<Health, string> = {
  idle: "border-slate-700",
  active: "border-emerald-500",
  stalled: "border-amber-400",
};

export function NodeCard({ data }: NodeProps<PipelineNode>) {
  const { node, health, isSelected } = data;
  const { counters } = node;

  return (
    <div
      className={`w-56 rounded-lg border-2 bg-slate-900 px-4 py-3 text-slate-100 shadow-lg ${
        BORDER[health]
      } ${isSelected ? "ring-2 ring-sky-400" : ""}`}
    >
      <Handle type="target" position={Position.Left} className="!bg-slate-600" />

      <div className="truncate text-sm font-semibold">{node.display_name}</div>
      <div className="mt-0.5 text-[11px] uppercase tracking-wide text-slate-500">{node.kind}</div>

      <dl className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 text-xs">
        <dt className="text-slate-400">in flight</dt>
        <dd className="text-right tabular-nums">{counters.in_flight}</dd>

        <dt className="text-slate-400">queued</dt>
        <dd className="text-right tabular-nums">{counters.queue_depth}</dd>

        <dt className="text-slate-400">per sec</dt>
        <dd className="text-right tabular-nums">{counters.throughput_per_sec.toFixed(2)}</dd>

        <dt className="text-slate-400">p50 / p95</dt>
        <dd className="text-right tabular-nums">
          {counters.p50_ms} / {counters.p95_ms} ms
        </dd>
      </dl>

      <Handle type="source" position={Position.Right} className="!bg-slate-600" />
    </div>
  );
}
