import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { formatAge, holdReason } from "../format";
import type { JobState } from "../protocol/types";

const MARK: Record<JobState["phase"], string> = {
  active: "bg-ink",
  held: "bg-signal",
  abandoned: "bg-fault",
};

function Reading({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="engraved self-center">{label}</dt>
      <dd className="text-right text-[12px] text-paper">{value}</dd>
    </>
  );
}

export function NodeDetail({ nowMs }: { nowMs: number }) {
  const selectedNode = usePipelineStore((state) => state.selectedNode);
  const nodes = usePipelineStore((state) => state.data.nodes);
  const jobs = usePipelineStore((state) => state.data.jobs);
  const selectNode = usePipelineStore((state) => state.selectNode);

  const node = selectedNode ? nodes[selectedNode] : undefined;

  // Oldest first, matching the tape well: the reason to open this panel is
  // almost always to find what is not moving.
  const items = useMemo(
    () =>
      Object.values(jobs)
        .filter((job) => job.current_node === selectedNode)
        .sort((left, right) => left.entered_node_at_ms - right.entered_node_at_ms),
    [jobs, selectedNode],
  );

  if (!node) return null;

  return (
    <aside className="w-[340px] shrink-0 overflow-y-auto border-l border-rule bg-panel">
      <div className="flex items-start justify-between border-b border-rule px-4 py-3">
        <div>
          <h2 className="text-[13px] text-paper">{node.display_name}</h2>
          <p className="engraved mt-0.5">{node.node_id}</p>
        </div>
        <button
          type="button"
          onClick={() => selectNode(null)}
          className="engraved hover:text-paper"
        >
          close
        </button>
      </div>

      <dl className="grid grid-cols-2 gap-y-2 border-b border-rule px-4 py-3">
        <Reading label="in flight" value={String(node.counters.in_flight)} />
        <Reading label="queued" value={String(node.counters.queue_depth)} />
        <Reading label="rate" value={`${node.counters.throughput_per_sec.toFixed(2)}/s`} />
        <Reading label="p50 here" value={`${node.counters.p50_ms} ms`} />
        <Reading label="p95 here" value={`${node.counters.p95_ms} ms`} />
        <Reading label="left in total" value={String(node.counters.left_total)} />
      </dl>

      <div className="px-4 py-3">
        <div className="engraved mb-2">items here ({items.length})</div>

        {items.length === 0 ? (
          <p className="text-[12px] text-muted">Nothing at this stage right now.</p>
        ) : (
          <ul>
            {items.map((job) => {
              const reason = holdReason(job);
              return (
                <li key={job.job_id} className="border-t border-rule py-2 first:border-t-0">
                  <div className="flex items-baseline gap-2">
                    <span className={`h-3 w-[5px] shrink-0 self-center ${MARK[job.phase]}`} />
                    <span className="text-[12px] text-paper">{job.job_id}</span>
                    <span className="engraved">{job.job_type}</span>
                    <span className="ml-auto text-[12px] text-muted">
                      {formatAge(nowMs - job.entered_node_at_ms)}
                    </span>
                  </div>

                  {reason && (
                    <div
                      className={`mt-1 pl-[13px] text-[12px] ${
                        job.phase === "abandoned" ? "text-fault" : "text-signal"
                      }`}
                    >
                      {reason}
                    </div>
                  )}

                  {Object.entries(job.meta).map(([key, value]) => (
                    <div key={key} className="mt-1 pl-[13px] text-[11px] text-muted">
                      {key} <span className="text-paper">{value}</span>
                    </div>
                  ))}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </aside>
  );
}
