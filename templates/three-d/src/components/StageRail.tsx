import { useMemo } from "react";
import { STALL_THRESHOLD_MS, formatAge, usePipelineStore } from "@pipeline-viz/protocol";
import { selectJobsById } from "../selectors";
import { orderNodeIds } from "../stage-sim";

const ARCHETYPE = { source: "portal", transform: "prism", sink: "archive" } as const;

export function StageRail() {
  const nodes = usePipelineStore((state) => state.data.nodes);
  const jobsById = usePipelineStore(selectJobsById);
  const selected = usePipelineStore((state) => state.selectedNode);
  const selectNode = usePipelineStore((state) => state.selectNode);
  const jobs = Object.values(jobsById);

  const stages = useMemo(
    () =>
      orderNodeIds(Object.values(nodes)).flatMap((nodeId) => {
        const node = nodes[nodeId];
        if (!node) return [];
        const atNode = jobs.filter((job) => job.current_node === node.node_id);
        const stalled = atNode.filter(
          (job) =>
            job.phase !== "abandoned" &&
            Date.now() - job.entered_node_at_ms > STALL_THRESHOLD_MS,
        );
        const abandoned = atNode.some((job) => job.phase === "abandoned");
        // The oldest thing sitting here is the whole question this dashboard
        // answers, so the rail carries it rather than hiding it behind a click.
        const oldest = stalled.reduce<number>(
          (worst, job) => Math.max(worst, Date.now() - job.entered_node_at_ms),
          0,
        );
        return [{
          ...node,
          here: node.counters.in_flight,
          oldest,
          abandoned,
          tint: abandoned ? "border-fault" : stalled.length > 0 ? "border-signal" : "border-ink/50",
          queueTint: node.counters.queue_depth > 3 ? "text-signal" : "text-muted",
        }];
      }),
    [jobsById, nodes],
  );

  return (
    <nav className="pointer-events-auto absolute left-4 top-5 z-10 flex w-[208px] flex-col gap-2" aria-label="Pipeline stages">
      <span className="engraved">stages</span>
      {stages.map((stage, index) => (
        <button
          key={stage.node_id}
          type="button"
          onClick={() => selectNode(selected === stage.node_id ? null : stage.node_id)}
          aria-pressed={selected === stage.node_id}
          title={`${stage.display_name} — press ${index + 1}`}
          className={`flex flex-col gap-1 border border-rule border-l-2 bg-panel/85 px-2 py-1.5 text-left transition-colors hover:bg-well ${stage.tint} ${selected === stage.node_id ? "ring-1 ring-ink" : ""}`}
        >
          <span className="flex items-start justify-between gap-2">
            {/* Wrapped, not truncated: the stage name is how a user matches this
                row to the label floating over the scene. */}
            <span className="text-[12px] leading-tight text-paper">{stage.display_name}</span>
            <span className="engraved shrink-0 pt-px">{ARCHETYPE[stage.kind]}</span>
          </span>
          <span className="flex items-baseline gap-2 text-[10px] uppercase tracking-[0.1em] text-muted">
            <span className="text-paper/80">{stage.here} here</span>
            <span className={stage.queueTint}>{stage.counters.queue_depth} q</span>
            <span className="ml-auto text-paper">{stage.counters.throughput_per_sec.toFixed(2)}/s</span>
          </span>
          {stage.oldest > 0 || stage.abandoned ? (
            <span className={`text-[10px] uppercase tracking-[0.1em] ${stage.abandoned ? "text-fault" : "text-signal"}`}>
              {stage.abandoned ? "abandoned here" : `stuck ${formatAge(stage.oldest)}`}
            </span>
          ) : null}
        </button>
      ))}
    </nav>
  );
}
