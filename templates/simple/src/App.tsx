import { Header } from "./components/Header";
import { HeldStrip } from "./components/HeldStrip";
import { NodeDetail } from "./components/NodeDetail";
import { PipelineGraph } from "./components/PipelineGraph";
import { useLiveStream } from "@pipeline-viz/protocol";
import { useNow } from "./useNow";

export default function App() {
  useLiveStream();

  // 500ms is fast enough that a rising age reads as continuous and slow enough
  // that it never reads as a blink.
  const nowMs = useNow(500);

  return (
    <div className="flex h-full flex-col bg-chassis">
      <Header />
      <div className="flex min-h-0 flex-1">
        <main className="min-w-0 flex-1">
          <PipelineGraph nowMs={nowMs} />
        </main>
        <NodeDetail nowMs={nowMs} />
      </div>
      <HeldStrip nowMs={nowMs} />
    </div>
  );
}
