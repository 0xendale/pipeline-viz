import { useMemo } from "react";
import {
  formatAge,
  holdReason,
  oldestHeld,
  usePipelineStore,
} from "@pipeline-viz/protocol";

const LIMIT = 5;

/**
 * The pipeline's stuck items, always visible.
 *
 * This is the question the tool exists to answer, so it does not live behind a
 * click. Selecting an entry focuses the stage holding it.
 */
export function HeldStrip({ nowMs }: { nowMs: number }) {
  const jobs = usePipelineStore((state) => state.data.jobs);
  const nodes = usePipelineStore((state) => state.data.nodes);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const stuck = useMemo(() => oldestHeld(Object.values(jobs), LIMIT), [jobs]);

  return (
    <div className="border-t border-rule bg-panel px-4 py-2">
      <div className="engraved mb-1.5">waiting longest</div>

      {stuck.length === 0 ? (
        <p className="text-[12px] text-muted">Nothing is held. Every item is moving.</p>
      ) : (
        <ul className="flex flex-wrap gap-x-6 gap-y-1">
          {stuck.map((job) => {
            const isFault = job.phase === "abandoned";
            return (
              <li key={job.job_id}>
                <button
                  type="button"
                  onClick={() => selectNode(job.current_node)}
                  className="flex items-baseline gap-3 text-left text-[12px] hover:bg-well"
                >
                  <span
                    className={`h-3 w-[5px] shrink-0 self-center rounded-[1px] ${
                      isFault ? "bg-fault" : "bg-signal"
                    }`}
                  />
                  <span className="text-paper">{job.job_id}</span>
                  <span className={isFault ? "text-fault" : "text-signal"}>
                    {formatAge(nowMs - job.entered_node_at_ms)}
                  </span>
                  <span className="engraved">
                    {nodes[job.current_node]?.display_name ?? job.current_node}
                  </span>
                  <span className="max-w-80 truncate text-muted">{holdReason(job)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
