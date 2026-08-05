import { abandonedCount, inFlightCount, usePipelineStore } from "@pipeline-viz/protocol";

export function Header() {
  const connection = usePipelineStore((state) => state.connection);
  const process = usePipelineStore((state) => state.data.process);
  const dropped = usePipelineStore((state) => state.data.dropped_events);
  const data = usePipelineStore((state) => state.data);
  const jobs = Object.values(data.jobs);
  const inFlight = inFlightCount(jobs);
  const abandoned = abandonedCount(jobs);
  const queued = Object.values(data.nodes).reduce(
    (total, node) => total + node.counters.queue_depth,
    0,
  );

  // The connection word sits in the title, not as a bare dot floating between
  // groups: a stalled socket and a stalled pipeline look identical otherwise.
  const dot = {
    connecting: "bg-muted",
    live: "bg-ink",
    reconnecting: "bg-signal animate-pulse",
  }[connection];

  return (
    <header className="flex h-11 shrink-0 items-center gap-8 border-b border-rule bg-panel px-4">
      <div className="flex items-center gap-2">
        <span className={`h-1.5 w-1.5 ${dot}`} />
        <span className="text-[13px] text-paper">pipeline-viz</span>
        <span className={`engraved ${connection === "live" ? "" : "text-signal"}`}>
          {connection} · cyber-physical
        </span>
      </div>

      <div className="flex items-center gap-8">
        <Metric label="in flight" value={inFlight} />
        <Metric label="abandoned" value={abandoned} tone="text-fault" />
        <Metric label="queued" value={queued} tone="text-signal" />
      </div>

      <div className="ml-auto flex items-center gap-6">
        {process ? (
          <>
            <span className="engraved">
              cpu <span className="text-paper">{process.cpu_pct.toFixed(1)}%</span>
            </span>
            <span className="engraved">
              ram <span className="text-paper">{process.ram_mb.toFixed(0)} MB</span>
            </span>
          </>
        ) : null}
        {/* Silent at zero. A non-zero count means the dashboard is missing events,
            which is a fault, not a routine gauge. */}
        {dropped > 0 ? (
          <span className="engraved" title="Events dropped because the channel was full">
            dropped <span className="text-fault">{dropped}</span>
          </span>
        ) : null}
      </div>
    </header>
  );
}

function Metric({ label, value, tone = "text-paper" }: { label: string; value: number; tone?: string }) {
  return (
    <div className="flex flex-col">
      <span className="engraved">{label}</span>
      <span className={`text-[13px] ${tone}`}>{value}</span>
    </div>
  );
}
