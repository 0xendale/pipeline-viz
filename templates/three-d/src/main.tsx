import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import * as THREE from "three";
import "./index.css";
import App from "./App";
import { stageSim } from "./sim";

// The classic pipeline-stage.js script reads exactly one global. The React
// side imports the same singleton from sim.ts, so both halves share state.
window.THREE = THREE;
window.PipelineSim = stageSim;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
