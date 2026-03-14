import type {
  MmdsEdge,
  MmdsEdgeEndpointTargets,
  MmdsNode,
  MmdsSubgraph,
} from "./types.js";

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

export function collectSubgraphAncestorIds(
  subgraphId: string,
  subgraphById: ReadonlyMap<string, Pick<MmdsSubgraph, "parent">>,
): string[] {
  const ancestors: string[] = [];
  const visited = new Set<string>();

  let current = subgraphById.get(subgraphId)?.parent;
  while (current && !visited.has(current)) {
    ancestors.push(current);
    visited.add(current);
    current = subgraphById.get(current)?.parent;
  }

  return ancestors;
}

export function collectNodeAncestorSubgraphIds(
  node: Pick<MmdsNode, "parent">,
  subgraphById: ReadonlyMap<string, Pick<MmdsSubgraph, "parent">>,
): string[] {
  if (!node.parent) return [];
  return [
    node.parent,
    ...collectSubgraphAncestorIds(node.parent, subgraphById),
  ];
}

export function collectDescendantSubgraphIds(
  subgraphId: string,
  subgraphs: Iterable<Pick<MmdsSubgraph, "id" | "parent">>,
): string[] {
  const byParent = new Map<string, string[]>();
  for (const subgraph of subgraphs) {
    if (!subgraph.parent) continue;
    const list = byParent.get(subgraph.parent) ?? [];
    list.push(subgraph.id);
    byParent.set(subgraph.parent, list);
  }

  const descendants: string[] = [];
  const visited = new Set<string>();
  const queue = [...(byParent.get(subgraphId) ?? [])];

  while (queue.length > 0) {
    const current = queue.shift();
    if (!current || visited.has(current)) continue;
    visited.add(current);
    descendants.push(current);

    const children = byParent.get(current) ?? [];
    for (const child of children) {
      if (!visited.has(child)) queue.push(child);
    }
  }

  return descendants;
}

export function collectSubgraphDescendantNodeIds(
  subgraphId: string,
  nodes: Iterable<Pick<MmdsNode, "id" | "parent">>,
  subgraphs: Iterable<Pick<MmdsSubgraph, "id" | "parent">>,
): string[] {
  const related = new Set<string>([
    subgraphId,
    ...collectDescendantSubgraphIds(subgraphId, subgraphs),
  ]);

  const nodeIds: string[] = [];
  for (const node of nodes) {
    if (node.parent && related.has(node.parent)) {
      nodeIds.push(node.id);
    }
  }

  return nodeIds;
}

export function edgeEndpointTargets(
  edge: Pick<MmdsEdge, "source" | "target" | "from_subgraph" | "to_subgraph">,
): MmdsEdgeEndpointTargets {
  const fromSubgraph = asString(edge.from_subgraph);
  const toSubgraph = asString(edge.to_subgraph);

  return {
    from: {
      kind: fromSubgraph ? "subgraph" : "node",
      id: fromSubgraph ?? edge.source,
      node_id: edge.source,
      subgraph_id: fromSubgraph,
    },
    to: {
      kind: toSubgraph ? "subgraph" : "node",
      id: toSubgraph ?? edge.target,
      node_id: edge.target,
      subgraph_id: toSubgraph,
    },
  };
}
