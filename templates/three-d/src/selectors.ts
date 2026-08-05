import type { PipelineData } from "@pipeline-viz/protocol";

export interface PipelineStateSnapshot {
  data: PipelineData;
}

export function selectJobsById(state: PipelineStateSnapshot): PipelineData["jobs"] {
  return state.data.jobs;
}
