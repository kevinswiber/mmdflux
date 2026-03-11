import type {
  MmdsArrow,
  MmdsDirection,
  MmdsDocument,
  MmdsEdgeStroke,
  MmdsPort,
  MmdsPortFace,
  MmdsPosition,
  MmdsSize,
  NormalizedMmdsDocument,
  NormalizedMmdsEdge,
  NormalizedMmdsNode,
  NormalizedMmdsSubgraph,
} from "./types.js";
import { assertValidMmdsDocument } from "./validate.js";

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

const PORT_FACES = new Set<MmdsPortFace>(["top", "bottom", "left", "right"]);

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

function normalizePort(value: unknown): MmdsPort | undefined {
  if (!value || typeof value !== "object") return undefined;
  const maybe = value as Record<string, unknown>;
  const face =
    typeof maybe.face === "string" && PORT_FACES.has(maybe.face as MmdsPortFace)
      ? (maybe.face as MmdsPortFace)
      : undefined;
  const fraction = asFiniteNumber(maybe.fraction);
  const position = normalizePosition(maybe.position);
  const group_size = asFiniteNumber(maybe.group_size);
  if (!face || fraction === undefined || !position || group_size === undefined)
    return undefined;
  return { face, fraction, position, group_size: Math.round(group_size) };
}

export function normalizeMmds(doc: MmdsDocument): NormalizedMmdsDocument {
  assertValidMmdsDocument(doc);

  const profiles = Array.isArray(doc.profiles)
    ? doc.profiles.filter((value): value is string => typeof value === "string")
    : [];

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
      source_port: normalizePort(edge.source_port),
      target_port: normalizePort(edge.target_port),
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
    profiles,
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
