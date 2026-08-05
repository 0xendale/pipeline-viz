// Public surface of @pipeline-viz/protocol. Templates import from this barrel
// only — never from individual source files — so the package can reorganize
// internally without touching either template.

export type {
  JobPhase,
  JobState,
  NodeCounters,
  NodeKind,
  NodeState,
  Patch,
  ProcessStats,
  ServerMessage,
  Snapshot,
} from "./types";

export { applyPatch, applySnapshot, emptyPipeline, type PipelineData } from "./reduce";

export { usePipelineStore, type ConnectionState } from "./store";

export { useLiveStream } from "./useLiveStream";

export { MAX_BACKOFF_MS, nextBackoffMs } from "./backoff";

export {
  STALL_THRESHOLD_MS,
  abandonedCount,
  inFlightCount,
  nodeHealth,
  oldestHeld,
  type Health,
} from "./health";

export { formatAge, holdReason } from "./format";
