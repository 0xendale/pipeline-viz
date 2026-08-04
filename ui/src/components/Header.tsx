import { useMemo } from "react";
import { usePipelineStore } from "../store/store";
import { abandonedCount, inFlightCount } from "../graph/health";

const CONNECTION = {
  connecting: { text: "connecting", dot: "bg-slate-600" },
  live: { text: "live", dot: "bg-emerald-500" },
  reconnecting: { text: "reconnecting", dot: "bg-amber-500" },
} as const;

export function Header() {
  const connection = usePipelineStore((state) => state.connection);
  const data = usePipelineStore((state) => state.data);
  const status = CONNECTION[connection];

  const jobs = useMemo(() => Object.values(data.jobs), [data.jobs]);
  const inFlight = inFlightCount(jobs);
  const abandoned = abandonedCount(jobs);

  return (
    <header className="flex items-center gap-6 border-b border-slate-800 bg-slate-950 px-4 py-2 text-slate-100">
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${status.dot}`} />
        <span className="text-sm font-semibold">pipeline-viz</span>
        <span className="text-xs text-slate-500">{status.text}</span>
      </div>

      {data.process && (
        <div className="flex items-center gap-4 text-xs tabular-nums text-slate-400">
          {/* Labelled whole-process deliberately: nodes are logical stages
              sharing one process, so these cannot be attributed per node. */}
          <span>
            CPU <span className="text-slate-200">{data.process.cpu_pct.toFixed(1)}%</span>
          </span>
          <span>
            RAM <span className="text-slate-200">{data.process.ram_mb.toFixed(0)} MB</span>
          </span>
          <span className="text-slate-600">whole process</span>
        </div>
      )}

      <div className="ml-auto text-xs text-slate-500">
        {inFlight} in flight
        {abandoned > 0 && (
          <span
            className="ml-3 text-rose-400"
            title="Items whose guard was dropped without completing — usually an early return through ?."
          >
            {abandoned} abandoned
          </span>
        )}
        {data.dropped_events > 0 && (
          <span
            className="ml-3 text-amber-400"
            title="Events discarded because the channel was full. The pipeline was never slowed down."
          >
            {data.dropped_events} events dropped
          </span>
        )}
      </div>
    </header>
  );
}
