import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { oldestHeld } from "../graph/health";
import { formatAge, holdReason } from "../format";

const LIMIT = 5;

/**
 * The pipeline's stuck items, always visible.
 *
 * This is the question the tool exists to answer, so it does not live behind a
 * click. Selecting an entry focuses the node holding it.
 */
export function HeldStrip({ nowMs }: { nowMs: number }) {
  const jobs = usePipelineStore((state) => state.data.jobs);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const stuck = useMemo(() => oldestHeld(Object.values(jobs), LIMIT), [jobs]);

  if (stuck.length === 0) {
    return (
      <div className="border-t border-slate-800 bg-slate-950 px-4 py-2 text-xs text-slate-500">
        Nothing held. Every item is moving.
      </div>
    );
  }

  return (
    <div className="border-t border-slate-800 bg-slate-950 px-4 py-2">
      <div className="mb-1 text-[11px] uppercase tracking-wide text-slate-500">Longest waiting</div>
      <ul className="flex flex-wrap gap-2">
        {stuck.map((job) => (
          <li key={job.job_id}>
            <button
              type="button"
              onClick={() => selectNode(job.current_node)}
              className="flex items-center gap-2 rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-xs text-amber-100 hover:border-amber-400"
            >
              <span className="font-mono">{job.job_id}</span>
              <span className="tabular-nums text-amber-300">
                {formatAge(nowMs - job.entered_node_at_ms)}
              </span>
              <span className="max-w-64 truncate text-amber-200/80">{holdReason(job)}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
