import { StageSim } from "./stage-sim";

/**
 * Single shared simulation instance. pipeline-stage.js reads one global
 * (`window.PipelineSim`) and React reads one module singleton, and both point
 * at this object.
 */
export const stageSim = new StageSim();
