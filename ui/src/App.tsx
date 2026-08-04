import { Header } from "./components/Header";
import { HeldStrip } from "./components/HeldStrip";
import { NodeDetail } from "./components/NodeDetail";
import { PipelineGraph } from "./components/PipelineGraph";
import { useLiveStream } from "./net/useLiveStream";
import { useNow } from "./useNow";

export default function App() {
  useLiveStream();
  const nowMs = useNow();

  return (
    <div className="flex h-screen flex-col bg-slate-900">
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
