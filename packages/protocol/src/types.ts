// Mirrors src/model.rs field for field. Names stay snake_case on purpose: the
// wire format is the contract, and a translation layer here would be one more
// place for the two models to drift apart. tests/protocol_fixtures.rs guards
// the same seam from the Rust side.

export type NodeKind = "source" | "transform" | "sink";

export type JobPhase =
  | { phase: "active" }
  | { phase: "held"; reason: string }
  | { phase: "abandoned" };

export interface NodeCounters {
  in_flight: number;
  queue_depth: number;
  left_total: number;
  throughput_per_sec: number;
  p50_ms: number;
  p95_ms: number;
}

export interface NodeState {
  node_id: string;
  display_name: string;
  kind: NodeKind;
  inputs: string[];
  counters: NodeCounters;
}

export type JobState = JobPhase & {
  job_id: string;
  job_type: string;
  current_node: string;
  entered_node_at_ms: number;
  created_at_ms: number;
  meta: Record<string, string>;
};

/** Whole-process resource use. Never per node — that cannot be measured. */
export interface ProcessStats {
  cpu_pct: number;
  ram_mb: number;
}

export interface Snapshot {
  type: "snapshot";
  ts_ms: number;
  nodes: NodeState[];
  jobs: JobState[];
  dropped_events: number;
  process: ProcessStats | null;
}

export interface Patch {
  type: "patch";
  ts_ms: number;
  nodes: NodeState[];
  jobs: JobState[];
  removed_jobs: string[];
  dropped_events: number;
  process: ProcessStats | null;
}

export type ServerMessage = Snapshot | Patch;
