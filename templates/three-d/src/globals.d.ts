import type { StageSim } from "./stage-sim";

declare global {
  interface Window {
    THREE: typeof import("three");
    /** Set by main.tsx; consumed by the classic pipeline-stage.js script. */
    PipelineSim: StageSim;
  }
}

export {};
