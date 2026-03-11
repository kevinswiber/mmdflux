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

export type MmdsPortFace = "top" | "bottom" | "left" | "right";

export interface MmdsPort {
  face: MmdsPortFace;
  fraction: number;
  position: MmdsPosition;
  group_size: number;
}

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
  source_port?: MmdsPort;
  target_port?: MmdsPort;
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
  profiles?: string[];
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
  profiles: string[];
  defaults: NormalizedMmdsDefaults;
  nodes: NormalizedMmdsNode[];
  edges: NormalizedMmdsEdge[];
  subgraphs: NormalizedMmdsSubgraph[];
  node_by_id: Map<string, NormalizedMmdsNode>;
  subgraph_by_id: Map<string, NormalizedMmdsSubgraph>;
  subgraph_children_by_parent: Map<string, string[]>;
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
