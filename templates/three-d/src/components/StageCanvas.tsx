import { useEffect, useRef, useState } from "react";
import { usePipelineStore } from "@pipeline-viz/protocol";
import { stageSim } from "../sim";

/**
 * Mounts the <pipeline-stage> custom element (registered by the classic
 * pipeline-stage.js script) and feeds it the reduced protocol data on every
 * store change. Selection flows both ways: the stage's click handler is wired
 * into the store, and store-driven selection (future: HUD links) is mirrored
 * into the stage.
 */
export function StageCanvas({ mode, selected, barHeight }: { mode: "ambient" | "detail"; selected: boolean; barHeight: number }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLElement | null>(null);
  const [mounted, setMounted] = useState(false);
  const hasTopology = usePipelineStore((state) => Object.keys(state.data.nodes).length > 0);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !hasTopology) return;

    stageSim.ingest(usePipelineStore.getState().data);
    const stage = document.createElement("pipeline-stage");
    stageRef.current = stage;
    host.appendChild(stage);
    setMounted(true);

    return () => {
      stage.remove();
      stageRef.current = null;
    };
  }, [hasTopology]);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    stage.setAttribute("mode", mode);
    stage.setAttribute("pad-left", "216");
    stage.setAttribute("pad-right", selected ? "340" : "0");
    stage.setAttribute("pad-top", "8");
    stage.setAttribute("pad-bottom", String(barHeight));
  }, [barHeight, mode, selected, mounted]);

  useEffect(() => {
    if (!mounted) return;

    stageSim.onSelect = (nodeId) => {
      usePipelineStore.getState().selectNode(nodeId);
    };

    const unsubscribe = usePipelineStore.subscribe((state, previous) => {
      if (state.data !== previous.data) stageSim.ingest(state.data);
      if (state.selectedNode !== stageSim.selected) stageSim.selected = state.selectedNode;
    });

    stageSim.ingest(usePipelineStore.getState().data);

    return () => {
      unsubscribe();
      stageSim.onSelect = null;
    };
  }, [mounted]);

  return (
    <div ref={hostRef} className="h-full w-full">
      {!hasTopology ? <div className="engraved p-6">waiting for pipeline topology</div> : null}
    </div>
  );
}
