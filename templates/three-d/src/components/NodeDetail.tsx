import { formatAge, holdReason, usePipelineStore } from "@pipeline-viz/protocol";
import { selectJobsById } from "../selectors";

const KIND_LABEL: Record<string, string> = {
  source: "source",
  transform: "transform",
  sink: "sink",
};

export function NodeDetail({ nowMs }: { nowMs: number }) {
  const nodes = usePipelineStore((state) => state.data.nodes);
  const jobsById = usePipelineStore(selectJobsById);
  const selected = usePipelineStore((state) => state.selectedNode);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const jobs = Object.values(jobsById);

  const node = selected ? nodes[selected] : undefined;
  // Oldest first: the top of this list is the answer to "what is stuck here?".
  const here = jobs
    .filter((job) => job.current_node === selected)
    .sort((a, b) => a.entered_node_at_ms - b.entered_node_at_ms);

  // Nothing selected means nothing to say. An empty panel would occupy the right
  // edge of the scene permanently and pay for itself with no reading.
  if (!node) return null;

  return (
    <aside className="pointer-events-none absolute inset-y-2 right-2 z-20 w-80">
      <div className="hud-panel pointer-events-auto max-h-full overflow-y-auto rounded-md p-3">
          <div className="mb-2 flex items-baseline justify-between gap-2 border-b border-rule pb-2">
            <h2 className="truncate text-sm text-paper">{node.display_name}</h2>
            <button type="button" onClick={() => selectNode(null)} className="engraved shrink-0 hover:text-paper" title="Close (Esc)">close</button>
          </div>
          <div className="engraved mb-3">{node.node_id} · {KIND_LABEL[node.kind]}</div>

          <dl className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs">
            <dt className="engraved">throughput</dt>
            <dd className="text-right text-paper">
              {node.counters.throughput_per_sec.toFixed(1)}/s
            </dd>
            <dt className="engraved">p50</dt>
            <dd className="text-right text-paper">{formatAge(node.counters.p50_ms)}</dd>
            <dt className="engraved">p95</dt>
            <dd className="text-right text-paper">{formatAge(node.counters.p95_ms)}</dd>
            <dt className="engraved">in flight</dt>
            <dd className="text-right text-paper">{node.counters.in_flight}</dd>
            <dt className="engraved">queue</dt>
            <dd className="text-right text-paper">{node.counters.queue_depth}</dd>
            <dt className="engraved">left total</dt>
            <dd className="text-right text-paper">{node.counters.left_total}</dd>
          </dl>

          <div className="engraved mb-1 mt-3">items here ({here.length})</div>
          {here.length === 0 ? <p className="text-xs text-muted">nothing here right now</p> : null}
          <ul className="space-y-1.5 text-xs">
            {here.map((job) => {
                const reason = holdReason(job);
                const color =
                  job.phase === "abandoned"
                    ? "text-fault"
                    : job.phase === "held"
                      ? "text-signal"
                      : "text-ink";
                // Two lines, not one. The id and the age are the identity; the
                // reason is a sentence and gets its own line rather than being
                // squeezed into an ellipsis next to three other fields.
                return (
                  <li key={job.job_id} className="border-b border-rule/50 pb-1 last:border-0">
                    <div className="flex items-baseline justify-between gap-2">
                      <span className={color}>{job.job_id}</span>
                      <span className="engraved ml-auto shrink-0">{job.job_type}</span>
                      <span className={`shrink-0 tabular-nums ${color}`}>
                        {formatAge(nowMs - job.entered_node_at_ms)}
                      </span>
                    </div>
                    {reason ? <div className="text-muted">{reason}</div> : null}
                    {job.meta.tx_count ? (
                      <div className="engraved">tx {job.meta.tx_count}</div>
                    ) : null}
                  </li>
                );
              })}
          </ul>
      </div>
    </aside>
  );
}
