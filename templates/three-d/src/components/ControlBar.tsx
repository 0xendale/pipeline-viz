import { useEffect, useMemo, useRef } from "react";
import { formatAge, holdReason, oldestHeld, usePipelineStore } from "@pipeline-viz/protocol";
import { selectJobsById } from "../selectors";

export type StageMode = "ambient" | "detail";

interface ControlBarProps {
  mode: StageMode;
  onModeChange: (mode: StageMode) => void;
  nowMs: number;
  onHeightChange: (height: number) => void;
}

export function ControlBar({ mode, onModeChange, nowMs, onHeightChange }: ControlBarProps) {
  const ref = useRef<HTMLDivElement>(null);
  const jobsById = usePipelineStore(selectJobsById);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const held = useMemo(() => oldestHeld(Object.values(jobsById), 5), [jobsById]);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const resize = () => onHeightChange(Math.ceil(element.getBoundingClientRect().height));
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    return () => observer.disconnect();
  }, [onHeightChange]);

  return (
    <footer ref={ref} className="pointer-events-auto absolute inset-x-0 bottom-0 z-20 border-t border-rule bg-panel/90 px-4 py-2 backdrop-blur-md">
      <div className="mb-2 flex flex-wrap items-center gap-x-5 gap-y-1 border-b border-rule pb-2 text-[11px] text-muted">
        <ModeButton active={mode === "ambient"} onClick={() => onModeChange("ambient")}>ambient</ModeButton>
        <ModeButton active={mode === "detail"} onClick={() => onModeChange("detail")}>detail</ModeButton>
        <span className="ml-auto">drag to swing · scroll to zoom · click a stage to open it</span>
      </div>
      <div className="mb-2 flex flex-wrap items-center gap-x-5 gap-y-1 border-b border-rule pb-2 text-[11px] text-muted">
        <Legend color="bg-ink">moving</Legend>
        <Legend color="bg-signal">held — stops in place, louder as it ages</Legend>
        <Legend color="bg-muted">queued — orbits outside the node</Legend>
        <Legend color="bg-fault">abandoned</Legend>
      </div>
      <div className="engraved mb-1">waiting longest</div>
      <ul className="flex min-h-4 flex-wrap gap-x-6 gap-y-1">
        {held.map((job) => {
          const reason = holdReason(job);
          const fault = job.phase === "abandoned";
          return (
            <li key={job.job_id}>
              <button type="button" onClick={() => selectNode(job.current_node)} className="flex items-baseline gap-3 text-left text-xs">
                <span className={`h-3 w-1 shrink-0 ${fault ? "bg-fault" : "bg-signal"}`} />
                <span className="text-paper">{job.job_id}</span>
                <span className={fault ? "text-fault" : "text-signal"}>{formatAge(nowMs - job.entered_node_at_ms)}</span>
                <span className="engraved">{job.current_node}</span>
                <span className="max-w-80 truncate text-muted">{reason}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </footer>
  );
}

function ModeButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: string }) {
  return <button type="button" onClick={onClick} className={`border border-rule px-2.5 py-1 uppercase tracking-[0.08em] transition-colors ${active ? "bg-ink/15 text-ink" : "text-muted hover:text-paper"}`}>{children}</button>;
}

function Legend({ color, children }: { color: string; children: string }) {
  return <span className="flex items-center gap-2"><span className={`h-2 w-2 ${color}`} />{children}</span>;
}
