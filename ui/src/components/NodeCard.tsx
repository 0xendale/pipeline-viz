import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import type { NodeState } from "../protocol/types";
import type { Health } from "../graph/health";
import type { Tape, TapeCell } from "../graph/tape";
import { formatAge } from "../format";

export interface NodeCardData extends Record<string, unknown> {
  node: NodeState;
  health: Health;
  tape: Tape;
  isSelected: boolean;
}

export type PipelineNode = Node<NodeCardData, "pipelineNode">;

/** Capacity of the well. Beyond this the count is reported instead. */
export const TAPE_CAPACITY = 34;

const EDGE: Record<Health, string> = {
  idle: "border-rule",
  active: "border-ink/50",
  stalled: "border-signal",
};

const CELL: Record<TapeCell["phase"], string> = {
  active: "bg-ink",
  held: "bg-signal",
  abandoned: "bg-fault",
};

function Cell({ cell }: { cell: TapeCell }) {
  return (
    <span
      className={`h-5 w-[6px] rounded-[1px] ${CELL[cell.phase]}`}
      title={`${cell.job_id} · ${cell.phase} · ${formatAge(cell.age_ms)} here`}
    />
  );
}

export function NodeCard({ data }: NodeProps<PipelineNode>) {
  const { node, health, tape, isSelected } = data;
  const { counters } = node;

  return (
    <div
      className={`w-[300px] border bg-panel ${EDGE[health]} ${isSelected ? "ring-1 ring-ink" : ""}`}
    >
      <Handle type="target" position={Position.Left} className="!h-2 !w-2 !border-0 !bg-rule" />

      <div className="flex items-baseline justify-between border-b border-rule px-3 py-2">
        <span className="truncate text-[13px] text-paper">{node.display_name}</span>
        <span className="engraved shrink-0">{node.kind}</span>
      </div>

      {/* The well. One cell per item actually at this stage, oldest at the
          left, so anything stuck drifts left and stays put while newer work
          streams past it on the right. Nothing here blinks: the eye finds the
          stalled cell because it stops moving, not because it flashes. */}
      <div className="px-3 pt-3">
        <div className="flex min-h-[76px] flex-wrap content-start items-start gap-[3px] overflow-hidden bg-well p-2">
          {tape.cells.length === 0 ? (
            <span className="engraved">empty</span>
          ) : (
            tape.cells.map((cell) => <Cell key={cell.job_id} cell={cell} />)
          )}
        </div>

        <div className="mt-1.5 flex justify-between">
          <span className="engraved">
            {tape.live} here
            {tape.overflow > 0 && ` · ${tape.overflow} more`}
          </span>
          <span className="engraved">{counters.queue_depth} queued</span>
        </div>
      </div>

      {tape.abandoned > 0 && (
        <div className="mt-3 flex items-baseline gap-2 border-t border-fault/40 bg-fault/10 px-3 py-1.5 text-[11px]">
          <span className="text-fault">{tape.abandoned}</span>
          <span className="engraved">dropped without completing</span>
        </div>
      )}

      <div className="mt-3 min-h-[34px] border-t border-rule px-3 py-2">
        {tape.oldestStuck ? (
          <div className="flex items-baseline gap-2 text-[11px]">
            <span
              className={tape.oldestStuck.phase === "abandoned" ? "text-fault" : "text-signal"}
            >
              {formatAge(tape.oldestStuck.age_ms)}
            </span>
            <span className="truncate text-muted">{tape.oldestStuck.job_id} not moving</span>
          </div>
        ) : (
          <div className="engraved">all moving</div>
        )}
      </div>

      <div className="grid grid-cols-2 gap-x-3 border-t border-rule px-3 py-2 text-[11px]">
        <span className="engraved">rate</span>
        <span className="text-right text-paper">{counters.throughput_per_sec.toFixed(2)}/s</span>
        <span className="engraved">p50 · p95</span>
        <span className="text-right text-paper">
          {counters.p50_ms} · {counters.p95_ms} ms
        </span>
      </div>

      <Handle type="source" position={Position.Right} className="!h-2 !w-2 !border-0 !bg-rule" />
    </div>
  );
}
