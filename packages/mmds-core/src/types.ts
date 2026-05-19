export type MmdsDirection = "TD" | "BT" | "LR" | "RL";
export type MmdsGeometryLevel = "layout" | "routed";

export type MmdsEdgeStroke =
  | "solid"
  | "dashed"
  | "dotted"
  | "thick"
  | "invisible";
export type MmdsArrow =
  | "none"
  | "normal"
  | "cross"
  | "circle"
  | "open_triangle"
  | "diamond"
  | "open_diamond";

export type MmdsPortFace = "top" | "bottom" | "left" | "right";
export type MmdsEdgeLabelSide = "above" | "below" | "center";

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

export interface MmdsUnfitLabelOverlapDiagnostic {
  edge_id: string;
  label: string;
  gap_pixels: number;
  label_span_pixels: number;
  attempted_side: MmdsEdgeLabelSide;
}

export interface MmdsMetadataDiagnostics {
  unfit_label_overlaps?: MmdsUnfitLabelOverlapDiagnostic[];
}

export interface MmdsMetadata {
  diagram_type?: string;
  direction?: MmdsDirection;
  bounds?: MmdsBounds;
  engine?: string;
  diagnostics?: MmdsMetadataDiagnostics;
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
  label_side?: MmdsEdgeLabelSide;
  /** @remarks Routed-level only. */
  label_rect?: MmdsRect;
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
  invisible?: boolean;
  concurrent_regions?: string[];
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
  // Sequence (timeline-family) fields
  participants?: MmdsParticipant[];
  messages?: MmdsMessage[];
  notes?: MmdsNote[];
  activations?: MmdsActivation[];
  blocks?: MmdsBlock[];
  participant_boxes?: MmdsParticipantBox[];
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
  classNames?: string[];
}

export interface NormalizedMmdsEdge extends MmdsEdge {
  stroke: MmdsEdgeStroke;
  arrow_start: MmdsArrow;
  arrow_end: MmdsArrow;
  minlen: number;
}

export interface NormalizedMmdsSubgraph extends MmdsSubgraph {
  children: string[];
  classNames?: string[];
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

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

export interface MmdsNodeStyleEntry {
  fill?: string;
  stroke?: string;
  color?: string;
  "font-family"?: string;
  "font-size"?: string;
  "font-style"?: string;
  "font-weight"?: string;
  "stroke-width"?: string;
  "stroke-dasharray"?: string;
  rx?: string;
  ry?: string;
  classNames?: string[];
}

export type MmdsSubgraphStyleEntry = MmdsNodeStyleEntry;

export interface MmdsEdgeLabelStyleEntry {
  "stroke-width"?: string;
  "font-family"?: string;
  "font-size"?: string;
  "font-style"?: string;
  "font-weight"?: string;
}

export interface MmdsNodeStyleExtension {
  nodes?: Record<string, MmdsNodeStyleEntry>;
  edges?: Record<string, MmdsEdgeLabelStyleEntry>;
  subgraphs?: Record<string, MmdsSubgraphStyleEntry>;
}

export type MmdsTextMetricsSource = "heuristic" | "recorded" | "dynamic";

export interface MmdsTextMetricsProfile {
  id: string;
  source: MmdsTextMetricsSource;
  version: number;
}

export interface MmdsDefaultTextStyle {
  "font-family": string;
  "font-size": number;
  "font-style": string;
  "font-weight": string;
  "line-height": number;
}

export interface MmdsLayoutTextMetrics {
  "node-padding-x": number;
  "node-padding-y": number;
  "label-padding-x": number;
  "label-padding-y": number;
  "edge-label-max-width": number | null;
}

export interface MmdsTextMetricsExtension {
  metricsProfile: MmdsTextMetricsProfile;
  defaultTextStyle: MmdsDefaultTextStyle;
  layoutText: MmdsLayoutTextMetrics;
}

export interface MmdsTextMeasurementsProfileRef {
  id: string;
  source: "dynamic";
  version: number;
}

export interface MmdsTextMeasurementStyle {
  id: string;
  fontFamily: string;
  fontSize: number;
  fontStyle: string;
  fontWeight: string;
  lineHeight: number;
  cssFont: string;
}

export interface MmdsTextMeasurementLineWidth {
  style: string;
  text: string;
  width: number;
}

export interface MmdsTextMeasurementScalarWidth {
  style: string;
  text: string;
  width: number;
}

export interface MmdsTextMeasurementsExtension {
  profileRef: MmdsTextMeasurementsProfileRef;
  textStyles: MmdsTextMeasurementStyle[];
  lineWidths: MmdsTextMeasurementLineWidth[];
  scalarWidths: MmdsTextMeasurementScalarWidth[];
}

// ---------------------------------------------------------------------------
// Sequence (timeline-family) types
// ---------------------------------------------------------------------------

export type MmdsParticipantKind = "participant" | "actor";
export type MmdsLineStyle = "solid" | "dashed";
export type MmdsArrowHead = "filled" | "none" | "cross" | "async";
export type MmdsNotePlacement = "left_of" | "right_of" | "over";
export type MmdsBlockKind =
  | "loop"
  | "alt"
  | "opt"
  | "par"
  | "critical"
  | "break"
  | "rect";
export type MmdsBlockDividerKind = "else" | "and" | "option";

export interface MmdsRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface MmdsParticipant {
  id: string;
  label: string;
  kind: MmdsParticipantKind;
  position: MmdsPosition;
  size: MmdsSize;
  lifeline_x: number;
}

export interface MmdsMessage {
  id: string;
  from: number;
  to: number;
  line_style: MmdsLineStyle;
  arrow_head: MmdsArrowHead;
  text: string;
  y: number;
}

export interface MmdsNote {
  placement: MmdsNotePlacement;
  participants: number[];
  text: string;
  position: MmdsPosition;
  size: MmdsSize;
}

export interface MmdsActivation {
  participant: number;
  y_start: number;
  y_end: number;
  depth: number;
}

export interface MmdsBlockDivider {
  y: number;
  kind: MmdsBlockDividerKind;
  label: string;
}

export interface MmdsBlock {
  kind: MmdsBlockKind;
  label: string;
  rect: MmdsRect;
  dividers?: MmdsBlockDivider[];
}

export interface MmdsParticipantBox {
  label?: string;
  color?: string;
  participants: number[];
  rect: MmdsRect;
}

// ---------------------------------------------------------------------------
// Edge endpoint types
// ---------------------------------------------------------------------------

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
