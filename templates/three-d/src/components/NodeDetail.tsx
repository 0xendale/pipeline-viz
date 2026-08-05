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

  return (
    <aside className="pointer-events-none absolute inset-y-2 right-0 z-20 w-80">
      {node ? (
        <div className="hud-panel pointer-events-auto max-h-full overflow-y-auto rounded-md p-3">
          <div className="mb-2 flex items-baseline justify-between gap-2 border-b border-rule pb-2">
            <h2 className="truncate text-sm text-paper">{node.display_name}</h2>
            <button type="button" onClick={() => selectNode(null)} className="engraved shrink-0 hover:text-paper">close</button>
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

          <div className="engraved mb-1 mt-3">items here</div>
          <ul className="space-y-1 text-xs">
            {jobs
              .filter((job) => job.current_node === node.node_id)
              .sort((a, b) => a.entered_node_at_ms - b.entered_node_at_ms)
              .map((job) => {
                const reason = holdReason(job);
                const color =
                  job.phase === "abandoned"
                    ? "text-fault"
                    : job.phase === "held"
                      ? "text-signal"
                      : "text-ink";
                return (
                  <li key={job.job_id} className="flex items-baseline justify-between gap-2">
                    <span className={`truncate ${color}`}>
                      {job.job_id} · {job.job_type}
                    </span>
                    <span className="shrink-0 text-muted">
                      {formatAge(nowMs - job.entered_node_at_ms)}
                    </span>
                    {reason ? <span className="max-w-40 truncate text-muted">· {reason}</span> : null}
                    {job.meta.tx_count ? <span className="text-muted">tx {job.meta.tx_count}</span> : null}
                  </li>
                );
              })}
          </ul>
        </div>
      ) : (
        <div className="hud-panel pointer-events-auto rounded-md p-3">
          <p className="engraved">select a node in the scene</p>
        </div>
      )}
    </aside>
  );
}
