import { useCallback, useState } from "react";
import { useLiveStream, usePipelineStore } from "@pipeline-viz/protocol";
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
  const handleBarHeight = useCallback((height: number) => setBarHeight(height), []);

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
