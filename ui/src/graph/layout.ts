import type { NodeState } from "../protocol/types";

export const COLUMN_WIDTH = 280;
export const ROW_HEIGHT = 170;

export interface Position {
  x: number;
  y: number;
}

/**
 * Left-to-right layered placement: a node's column is one past its deepest
 * input, and nodes sharing a column are stacked in id order so the layout is
 * stable across renders rather than jumping as messages arrive.
 *
 * Pipelines are declared as a DAG, but a user can register a cycle by mistake.
 * The depth walk carries the path it is on and stops when it meets itself, so a
 * cycle produces a usable layout instead of hanging the tab.
 */
export function layoutNodes(nodes: NodeState[]): Record<string, Position> {
  const byId = new Map(nodes.map((node) => [node.node_id, node]));
  const ranks = new Map<string, number>();

  const rankOf = (node_id: string, path: Set<string>): number => {
    const cached = ranks.get(node_id);
    if (cached !== undefined) return cached;
    if (path.has(node_id)) return 0;

    const node = byId.get(node_id);
    if (!node) return 0;

    path.add(node_id);
    const inputRanks = node.inputs
      .filter((input) => byId.has(input))
      .map((input) => rankOf(input, path) + 1);
    path.delete(node_id);

    const rank = inputRanks.length > 0 ? Math.max(...inputRanks) : 0;
    ranks.set(node_id, rank);
    return rank;
  };

  const sortedIds = [...byId.keys()].sort();
  for (const node_id of sortedIds) rankOf(node_id, new Set());

  const columns = new Map<number, string[]>();
  for (const node_id of sortedIds) {
    const rank = ranks.get(node_id) ?? 0;
    columns.set(rank, [...(columns.get(rank) ?? []), node_id]);
  }

  const positions: Record<string, Position> = {};
  for (const [rank, ids] of columns) {
    ids.forEach((node_id, row) => {
      positions[node_id] = { x: rank * COLUMN_WIDTH, y: row * ROW_HEIGHT };
    });
  }
  return positions;
}
