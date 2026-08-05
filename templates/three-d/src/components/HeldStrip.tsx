import { formatAge, holdReason, oldestHeld, usePipelineStore } from "@pipeline-viz/protocol";
import { selectJobsById } from "../selectors";

export function HeldStrip({ nowMs }: { nowMs: number }) {
  const jobsById = usePipelineStore(selectJobsById);
  const jobs = Object.values(jobsById);
  const held = oldestHeld(jobs, 8);

  if (held.length === 0) {
    return (
      <footer className="h-10 shrink-0 border-t border-rule bg-panel px-4">
        <span className="engraved leading-10">no held items</span>
      </footer>
    );
  }

  return (
    <footer className="flex h-10 shrink-0 items-center gap-6 overflow-x-auto border-t border-rule bg-panel px-4">
      <span className="engraved shrink-0">held</span>
      {held.map((job) => {
        const reason = holdReason(job);
        const fault = job.phase === "abandoned";
        return (
          <span
            key={job.job_id}
            className="flex shrink-0 items-center gap-2 text-xs"
            title={reason ?? undefined}
          >
            <span className={fault ? "text-fault" : "text-signal"}>
              {job.job_id} · {job.current_node}
            </span>
            <span className="text-muted">{formatAge(nowMs - job.entered_node_at_ms)}</span>
            {reason ? <span className="max-w-64 truncate text-muted">{reason}</span> : null}
          </span>
        );
      })}
    </footer>
  );
}
