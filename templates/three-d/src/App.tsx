import { useCallback, useEffect, useState } from "react";
import { useLiveStream, usePipelineStore } from "@pipeline-viz/protocol";
import { orderNodeIds } from "./stage-sim";
import { ControlBar, type StageMode } from "./components/ControlBar";
import { Header } from "./components/Header";
import { NodeDetail } from "./components/NodeDetail";
import { StageCanvas } from "./components/StageCanvas";
import { StageRail } from "./components/StageRail";
import { useNow } from "./useNow";

export default function App() {
  useLiveStream();
  const nowMs = useNow(500);
  const selected = usePipelineStore((state) => state.selectedNode);
  const [mode, setMode] = useState<StageMode>("ambient");
  const [barHeight, setBarHeight] = useState(130);
  // Second line of defence against camera shake: the stage re-fits whenever
  // pad-bottom changes, so ignore the sub-pixel jitter a ResizeObserver reports
  // for text that reflows by a hairline.
  const handleBarHeight = useCallback(
    (height: number) => setBarHeight((current) => (Math.abs(current - height) < 6 ? current : height)),
    [],
  );

  // Keyboard is the fastest route to "look at that stage": Esc drops the panel,
  // 1-9 walk the pipeline in flow order, and A/D swap the two stage modes.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const store = usePipelineStore.getState();
      if (event.key === "Escape") {
        store.selectNode(null);
        return;
      }
      if (event.key === "a" || event.key === "d") {
        setMode(event.key === "a" ? "ambient" : "detail");
        return;
      }
      const index = Number(event.key);
      if (!Number.isInteger(index) || index < 1 || index > 9) return;
      const ordered = orderNodeIds(Object.values(store.data.nodes));
      const nodeId = ordered[index - 1];
      if (nodeId) store.selectNode(nodeId === store.selectedNode ? null : nodeId);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col bg-chassis">
      <Header />
      <div className="relative min-h-0 flex-1">
        <StageCanvas mode={mode} selected={selected !== null} barHeight={barHeight} />
        <div className="pointer-events-none absolute inset-0 z-10 bg-[repeating-linear-gradient(to_bottom,rgba(0,0,0,0.13)_0px,rgba(0,0,0,0.13)_1px,transparent_1px,transparent_3px)] opacity-55 mix-blend-multiply" />
        <div className="pointer-events-none absolute inset-0 z-10 bg-[radial-gradient(ellipse_at_center,transparent_45%,rgba(10,9,8,0.75)_100%)]" />
        <StageRail />
        <NodeDetail nowMs={nowMs} />
        <ControlBar mode={mode} onModeChange={setMode} nowMs={nowMs} onHeightChange={handleBarHeight} />
      </div>
    </div>
  );
}
