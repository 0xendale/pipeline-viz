import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { formatAge, holdReason } from "../format";

export function NodeDetail({ nowMs }: { nowMs: number }) {
  const selectedNode = usePipelineStore((state) => state.selectedNode);
  const nodes = usePipelineStore((state) => state.data.nodes);
  const jobs = usePipelineStore((state) => state.data.jobs);
  const selectNode = usePipelineStore((state) => state.selectNode);

  const node = selectedNode ? nodes[selectedNode] : undefined;

  // Oldest first: the reason to open this panel is usually to find what is not
  // moving, not to read the most recent arrival.
  const items = useMemo(
    () =>
      Object.values(jobs)
        .filter((job) => job.current_node === selectedNode)
        .sort((left, right) => left.entered_node_at_ms - right.entered_node_at_ms),
    [jobs, selectedNode],
  );

  if (!node) return null;

  return (
    <aside className="w-96 shrink-0 overflow-y-auto border-l border-slate-800 bg-slate-950 p-4 text-slate-100">
      <div className="flex items-start justify-between">
        <div>
          <h2 className="text-base font-semibold">{node.display_name}</h2>
          <p className="font-mono text-xs text-slate-500">{node.node_id}</p>
        </div>
        <button
          type="button"
          onClick={() => selectNode(null)}
          className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-800"
        >
          close
        </button>
      </div>

      <dl className="mt-4 grid grid-cols-2 gap-y-1 text-sm">
        <dt className="text-slate-400">in flight</dt>
        <dd className="text-right tabular-nums">{node.counters.in_flight}</dd>
        <dt className="text-slate-400">queue depth</dt>
        <dd className="text-right tabular-nums">{node.counters.queue_depth}</dd>
        <dt className="text-slate-400">throughput</dt>
        <dd className="text-right tabular-nums">
          {node.counters.throughput_per_sec.toFixed(2)}/s
        </dd>
        <dt className="text-slate-400">p50 time here</dt>
        <dd className="text-right tabular-nums">{node.counters.p50_ms}ms</dd>
        <dt className="text-slate-400">p95 time here</dt>
        <dd className="text-right tabular-nums">{node.counters.p95_ms}ms</dd>
        <dt className="text-slate-400">left in total</dt>
        <dd className="text-right tabular-nums">{node.counters.left_total}</dd>
      </dl>

      <h3 className="mt-6 text-[11px] uppercase tracking-wide text-slate-500">
        Items here ({items.length})
      </h3>

      {items.length === 0 ? (
        <p className="mt-2 text-xs text-slate-500">Nothing at this node right now.</p>
      ) : (
        <ul className="mt-2 space-y-2">
          {items.map((job) => {
            const reason = holdReason(job);
            return (
              <li key={job.job_id} className="rounded border border-slate-800 p-2 text-xs">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="font-mono">{job.job_id}</span>
                  <span className="tabular-nums text-slate-400">
                    {formatAge(nowMs - job.entered_node_at_ms)}
                  </span>
                </div>
                <div className="mt-1 text-slate-400">{job.job_type}</div>
                {reason && <div className="mt-1 text-amber-300">{reason}</div>}
                {Object.entries(job.meta).map(([key, value]) => (
                  <div key={key} className="mt-1 text-slate-500">
                    {key}: <span className="text-slate-300">{value}</span>
                  </div>
                ))}
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}
