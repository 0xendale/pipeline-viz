import { useMemo } from "react";
import { Background, ReactFlow, type Edge } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { usePipelineStore } from "../store/store";
import { layoutNodes } from "../graph/layout";
import { nodeHealth } from "../graph/health";
import { NodeCard, type PipelineNode } from "./NodeCard";

const NODE_TYPES = { pipelineNode: NodeCard };

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
      isSelected: selectedNode === node.node_id,
    },
  }));

  // Edges come straight from declared inputs. Unknown sources are dropped
  // rather than rendered as dangling, which React Flow would reject anyway.
  const flowEdges: Edge[] = nodes.flatMap((node) =>
    node.inputs
      .filter((input) => data.nodes[input])
      .map((input) => ({
        id: `${input}->${node.node_id}`,
        source: input,
        target: node.node_id,
      })),
  );

  return (
    <ReactFlow
      nodes={flowNodes}
      edges={flowEdges}
      nodeTypes={NODE_TYPES}
      onNodeClick={(_, node) => selectNode(node.id)}
      onPaneClick={() => selectNode(null)}
      fitView
      colorMode="dark"
    >
      <Background />
    </ReactFlow>
  );
}
