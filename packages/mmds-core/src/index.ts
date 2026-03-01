export type MmdsDirection = "TD" | "BT" | "LR" | "RL";
export type MmdsGeometryLevel = "layout" | "routed";

export type MmdsEdgeStroke = "solid" | "dotted" | "thick" | "invisible";
export type MmdsArrow =
  | "none"
  | "normal"
  | "cross"
  | "circle"
  | "open_triangle"
  | "diamond"
  | "open_diamond";

export interface MmdsPosition {
  x: number;
  y: number;
}

export interface MmdsSize {
  width: number;
  height: number;
}

export interface MmdsBounds {
  width: number;
  height: number;
}

export interface MmdsMetadata {
  diagram_type?: string;
  direction?: MmdsDirection;
  bounds?: MmdsBounds;
  [key: string]: unknown;
}

export interface MmdsNode {
  id: string;
  label: string;
  shape?: string;
  parent?: string;
  position: MmdsPosition;
  size: MmdsSize;
}

export interface MmdsEdge {
  id: string;
  source: string;
  target: string;
  from_subgraph?: string;
  to_subgraph?: string;
  label?: string;
  stroke?: MmdsEdgeStroke;
  arrow_start?: MmdsArrow;
  arrow_end?: MmdsArrow;
  minlen?: number;
  path?: [number, number][];
  label_position?: MmdsPosition;
  is_backward?: boolean;
}

export interface MmdsSubgraph {
  id: string;
  title?: string;
  children: string[];
  parent?: string;
  direction?: MmdsDirection;
  bounds?: MmdsBounds;
}

export interface MmdsDefaults {
  node?: {
    shape?: string;
  };
  edge?: {
    stroke?: MmdsEdgeStroke;
    arrow_start?: MmdsArrow;
    arrow_end?: MmdsArrow;
    minlen?: number;
  };
}

export interface MmdsDocument {
  version?: number;
  defaults?: MmdsDefaults;
  geometry_level?: MmdsGeometryLevel;
  metadata?: MmdsMetadata;
  nodes: MmdsNode[];
  edges: MmdsEdge[];
  subgraphs?: MmdsSubgraph[];
  extensions?: Record<string, unknown>;
}

export interface NormalizedMmdsDefaults {
  node: {
    shape: string;
  };
  edge: {
    stroke: MmdsEdgeStroke;
    arrow_start: MmdsArrow;
    arrow_end: MmdsArrow;
    minlen: number;
  };
}

export interface NormalizedMmdsNode extends MmdsNode {
  shape: string;
}

export interface NormalizedMmdsEdge extends MmdsEdge {
  stroke: MmdsEdgeStroke;
  arrow_start: MmdsArrow;
  arrow_end: MmdsArrow;
  minlen: number;
}

export interface NormalizedMmdsSubgraph extends MmdsSubgraph {
  children: string[];
}

export interface NormalizedMmdsDocument
  extends Omit<MmdsDocument, "defaults" | "nodes" | "edges" | "subgraphs"> {
  defaults: NormalizedMmdsDefaults;
  nodes: NormalizedMmdsNode[];
  edges: NormalizedMmdsEdge[];
  subgraphs: NormalizedMmdsSubgraph[];
  node_by_id: Map<string, NormalizedMmdsNode>;
  subgraph_by_id: Map<string, NormalizedMmdsSubgraph>;
  subgraph_children_by_parent: Map<string, string[]>;
}

const DEFAULT_NODE_SHAPE = "rectangle";
const DEFAULT_EDGE_STROKE: MmdsEdgeStroke = "solid";
const DEFAULT_ARROW_START: MmdsArrow = "none";
const DEFAULT_ARROW_END: MmdsArrow = "normal";
const DEFAULT_MINLEN = 1;

const EDGE_STROKES = new Set<MmdsEdgeStroke>([
  "solid",
  "dotted",
  "thick",
  "invisible",
]);

const EDGE_ARROWS = new Set<MmdsArrow>([
  "none",
  "normal",
  "cross",
  "circle",
  "open_triangle",
  "diamond",
  "open_diamond",
]);

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function asFiniteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

function asEdgeStroke(value: unknown): MmdsEdgeStroke | undefined {
  return typeof value === "string" && EDGE_STROKES.has(value as MmdsEdgeStroke)
    ? (value as MmdsEdgeStroke)
    : undefined;
}

function asArrow(value: unknown): MmdsArrow | undefined {
  return typeof value === "string" && EDGE_ARROWS.has(value as MmdsArrow)
    ? (value as MmdsArrow)
    : undefined;
}

function normalizePath(value: unknown): [number, number][] | undefined {
  if (!Array.isArray(value)) return undefined;

  const out: [number, number][] = [];
  for (const point of value) {
    if (!Array.isArray(point) || point.length !== 2) return undefined;
    const x = asFiniteNumber(point[0]);
    const y = asFiniteNumber(point[1]);
    if (x === undefined || y === undefined) return undefined;
    out.push([x, y]);
  }
  return out.length > 0 ? out : undefined;
}

function normalizePosition(value: unknown): MmdsPosition | undefined {
  if (!value || typeof value !== "object") return undefined;
  const maybe = value as Record<string, unknown>;
  const x = asFiniteNumber(maybe.x);
  const y = asFiniteNumber(maybe.y);
  if (x === undefined || y === undefined) return undefined;
  return { x, y };
}

function normalizeSize(value: unknown): MmdsSize | undefined {
  if (!value || typeof value !== "object") return undefined;
  const maybe = value as Record<string, unknown>;
  const width = asFiniteNumber(maybe.width);
  const height = asFiniteNumber(maybe.height);
  if (width === undefined || height === undefined) return undefined;
  return { width, height };
}

function assertDocumentShape(doc: MmdsDocument): void {
  if (!doc || typeof doc !== "object") {
    throw new Error("MMDS document must be an object");
  }
  if (!Array.isArray(doc.nodes)) {
    throw new Error("MMDS document must include a nodes array");
  }
  if (!Array.isArray(doc.edges)) {
    throw new Error("MMDS document must include an edges array");
  }
}

export function normalizeMmds(doc: MmdsDocument): NormalizedMmdsDocument {
  assertDocumentShape(doc);

  const defaultNodeShape =
    asString(doc.defaults?.node?.shape) ?? DEFAULT_NODE_SHAPE;
  const defaultEdgeStroke =
    asEdgeStroke(doc.defaults?.edge?.stroke) ?? DEFAULT_EDGE_STROKE;
  const defaultArrowStart =
    asArrow(doc.defaults?.edge?.arrow_start) ?? DEFAULT_ARROW_START;
  const defaultArrowEnd =
    asArrow(doc.defaults?.edge?.arrow_end) ?? DEFAULT_ARROW_END;
  const defaultMinlen =
    asFiniteNumber(doc.defaults?.edge?.minlen) ?? DEFAULT_MINLEN;

  const nodes: NormalizedMmdsNode[] = doc.nodes.map((node, index) => {
    const id = asString(node.id);
    const label = asString(node.label);
    const position = normalizePosition(node.position);
    const size = normalizeSize(node.size);

    if (!id)
      throw new Error(`MMDS node at index ${index} is missing a string id`);
    if (label === undefined) {
      throw new Error(`MMDS node '${id}' is missing a string label`);
    }
    if (!position) {
      throw new Error(`MMDS node '${id}' is missing a numeric position`);
    }
    if (!size) {
      throw new Error(`MMDS node '${id}' is missing a numeric size`);
    }

    return {
      id,
      label,
      shape: asString(node.shape) ?? defaultNodeShape,
      parent: asString(node.parent),
      position,
      size,
    };
  });

  const edges: NormalizedMmdsEdge[] = doc.edges.map((edge, index) => {
    const id = asString(edge.id);
    const source = asString(edge.source);
    const target = asString(edge.target);

    if (!id)
      throw new Error(`MMDS edge at index ${index} is missing a string id`);
    if (!source)
      throw new Error(`MMDS edge '${id}' is missing a string source`);
    if (!target)
      throw new Error(`MMDS edge '${id}' is missing a string target`);

    return {
      id,
      source,
      target,
      from_subgraph: asString(edge.from_subgraph),
      to_subgraph: asString(edge.to_subgraph),
      label: asString(edge.label),
      stroke: asEdgeStroke(edge.stroke) ?? defaultEdgeStroke,
      arrow_start: asArrow(edge.arrow_start) ?? defaultArrowStart,
      arrow_end: asArrow(edge.arrow_end) ?? defaultArrowEnd,
      minlen: asFiniteNumber(edge.minlen) ?? defaultMinlen,
      path: normalizePath(edge.path),
      label_position: normalizePosition(edge.label_position),
      is_backward:
        typeof edge.is_backward === "boolean" ? edge.is_backward : undefined,
    };
  });

  const subgraphs: NormalizedMmdsSubgraph[] = Array.isArray(doc.subgraphs)
    ? doc.subgraphs.map((subgraph, index) => {
        const id = asString(subgraph.id);
        if (!id) {
          throw new Error(
            `MMDS subgraph at index ${index} is missing a string id`,
          );
        }

        const children = Array.isArray(subgraph.children)
          ? subgraph.children.filter(
              (value): value is string => typeof value === "string",
            )
          : [];

        return {
          id,
          title: asString(subgraph.title),
          children,
          parent: asString(subgraph.parent),
          direction: asString(subgraph.direction) as MmdsDirection | undefined,
          bounds:
            subgraph.bounds && typeof subgraph.bounds === "object"
              ? {
                  width: asFiniteNumber(subgraph.bounds.width) ?? 0,
                  height: asFiniteNumber(subgraph.bounds.height) ?? 0,
                }
              : undefined,
        };
      })
    : [];

  const nodeById = new Map<string, NormalizedMmdsNode>();
  for (const node of nodes) {
    nodeById.set(node.id, node);
  }

  const subgraphById = new Map<string, NormalizedMmdsSubgraph>();
  for (const subgraph of subgraphs) {
    subgraphById.set(subgraph.id, subgraph);
  }

  const subgraphChildrenByParent = new Map<string, string[]>();
  for (const subgraph of subgraphs) {
    const parent = subgraph.parent;
    if (!parent) continue;
    const bucket = subgraphChildrenByParent.get(parent) ?? [];
    bucket.push(subgraph.id);
    subgraphChildrenByParent.set(parent, bucket);
  }

  return {
    version: doc.version,
    geometry_level: doc.geometry_level,
    metadata: doc.metadata,
    extensions: doc.extensions,
    defaults: {
      node: {
        shape: defaultNodeShape,
      },
      edge: {
        stroke: defaultEdgeStroke,
        arrow_start: defaultArrowStart,
        arrow_end: defaultArrowEnd,
        minlen: defaultMinlen,
      },
    },
    nodes,
    edges,
    subgraphs,
    node_by_id: nodeById,
    subgraph_by_id: subgraphById,
    subgraph_children_by_parent: subgraphChildrenByParent,
  };
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

export type MmdsEndpointKind = "node" | "subgraph";

export interface MmdsEdgeEndpointTarget {
  kind: MmdsEndpointKind;
  id: string;
  node_id: string;
  subgraph_id?: string;
}

export interface MmdsEdgeEndpointTargets {
  from: MmdsEdgeEndpointTarget;
  to: MmdsEdgeEndpointTarget;
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
