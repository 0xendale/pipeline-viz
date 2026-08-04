import { useMemo } from "react";
import { ReactFlow } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { usePipelineStore } from "../store/store";
import { layoutNodes } from "../graph/layout";
import { nodeHealth } from "../graph/health";
import { tapeFor, tapePeriodSeconds } from "../graph/tape";
import { NodeCard, TAPE_CAPACITY, type PipelineNode } from "./NodeCard";
import { TapeEdgeLine, type TapeEdge } from "./TapeEdge";

const NODE_TYPES = { pipelineNode: NodeCard };
const EDGE_TYPES = { tape: TapeEdgeLine };

export function PipelineGraph({ nowMs }: { nowMs: number }) {
  const data = usePipelineStore((state) => state.data);
  const selectedNode = usePipelineStore((state) => state.selectedNode);
  const selectNode = usePipelineStore((state) => state.selectNode);

  const nodes = useMemo(() => Object.values(data.nodes), [data.nodes]);
  const jobs = useMemo(() => Object.values(data.jobs), [data.jobs]);
  const positions = useMemo(() => layoutNodes(nodes), [nodes]);

  const flowNodes: PipelineNode[] = nodes.map((node) => ({
    id: node.node_id,
    type: "pipelineNode",
    position: positions[node.node_id] ?? { x: 0, y: 0 },
    data: {
      node,
      health: nodeHealth(node, jobs, nowMs),
      tape: tapeFor(jobs, node.node_id, TAPE_CAPACITY, nowMs),
      isSelected: selectedNode === node.node_id,
    },
  }));

  // Edges come straight from declared inputs; unknown sources are dropped
  // rather than drawn dangling. Each tape runs at its source stage's rate.
  const flowEdges: TapeEdge[] = nodes.flatMap((node) =>
    node.inputs
      .filter((input) => data.nodes[input])
      .map((input) => ({
        id: `${input}->${node.node_id}`,
        source: input,
        target: node.node_id,
        type: "tape" as const,
        data: {
          periodSeconds: tapePeriodSeconds(data.nodes[input].counters.throughput_per_sec),
        },
      })),
  );

  return (
    <ReactFlow
      nodes={flowNodes}
      edges={flowEdges}
      nodeTypes={NODE_TYPES}
      edgeTypes={EDGE_TYPES}
      onNodeClick={(_, node) => selectNode(node.id)}
      onPaneClick={() => selectNode(null)}
      fitView
      fitViewOptions={{ padding: 0.25 }}
    />
  );
}
