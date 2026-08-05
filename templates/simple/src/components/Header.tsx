import { useMemo } from "react";
import {
  abandonedCount,
  inFlightCount,
  usePipelineStore,
} from "@pipeline-viz/protocol";

const CONNECTION = {
  connecting: { text: "connecting", tint: "bg-muted" },
  live: { text: "live", tint: "bg-ink" },
  reconnecting: { text: "reconnecting", tint: "bg-signal" },
} as const;

function Reading({ label, value, tone = "text-paper" }: {
  label: string;
  value: string;
  tone?: string;
}) {
  return (
    <div className="flex flex-col">
      <span className="engraved">{label}</span>
      <span className={`text-[13px] ${tone}`}>{value}</span>
    </div>
  );
}

export function Header() {
  const connection = usePipelineStore((state) => state.connection);
  const data = usePipelineStore((state) => state.data);
  const status = CONNECTION[connection];

  const jobs = useMemo(() => Object.values(data.jobs), [data.jobs]);
  const inFlight = inFlightCount(jobs);
  const abandoned = abandonedCount(jobs);

  return (
    <header className="flex items-center gap-8 border-b border-rule bg-panel px-4 py-2">
      <div className="flex items-center gap-2">
        <span className={`h-1.5 w-1.5 ${status.tint}`} />
        <span className="text-[13px] text-paper">pipeline-viz</span>
        <span className="engraved">{status.text}</span>
      </div>

      <Reading label="in flight" value={String(inFlight)} />

      {abandoned > 0 && (
        <Reading label="abandoned" value={String(abandoned)} tone="text-fault" />
      )}

      {data.dropped_events > 0 && (
        <Reading
          label="events dropped"
          value={String(data.dropped_events)}
          tone="text-signal"
        />
      )}

      {data.process && (
        <div className="ml-auto flex items-center gap-6">
          <Reading label="cpu" value={`${data.process.cpu_pct.toFixed(1)}%`} />
          <Reading label="ram" value={`${data.process.ram_mb.toFixed(0)} MB`} />
          {/* Labelled deliberately: nodes are logical stages sharing one
              process, so these two figures cannot be split per node. */}
          <span className="engraved max-w-24 leading-3">whole process</span>
        </div>
      )}
    </header>
  );
}
