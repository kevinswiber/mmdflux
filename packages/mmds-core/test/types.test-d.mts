import {
  getExtension,
  MMDS_NODE_STYLE_NAMESPACE,
  MMDS_NODE_STYLE_PROFILE,
  MMDS_TEXT_MEASUREMENTS_NAMESPACE,
  MMDS_TEXT_MEASUREMENTS_PROFILE,
  MMDS_TEXT_METRICS_NAMESPACE,
  MMDS_TEXT_METRICS_PROFILE,
} from "../src/extensions.js";
import type {
  MmdsArrowHead,
  MmdsDefaultTextStyle,
  MmdsDocument,
  MmdsEdge,
  MmdsEdgeLabelSide,
  MmdsEdgeLabelStyleEntry,
  MmdsLayoutTextMetrics,
  MmdsMetadata,
  MmdsMetadataDiagnostics,
  MmdsNodeStyleEntry,
  MmdsNodeStyleExtension,
  MmdsRect,
  MmdsSubgraph,
  MmdsSubgraphStyleEntry,
  MmdsTextMeasurementLineWidth,
  MmdsTextMeasurementScalarWidth,
  MmdsTextMeasurementStyle,
  MmdsTextMeasurementsExtension,
  MmdsTextMeasurementsProfileRef,
  MmdsTextMetricsExtension,
  MmdsTextMetricsProfile,
  MmdsUnfitLabelOverlapDiagnostic,
  NormalizedMmdsNode,
  NormalizedMmdsSubgraph,
} from "../src/types.js";

const _good: MmdsDocument = { nodes: [], edges: [] };
void _good;

// @ts-expect-error nodes must be an array, not a number
const _bad: MmdsDocument = { nodes: 1, edges: [] };
void _bad;

const _goodArrowHead: MmdsArrowHead = "none";
void _goodArrowHead;

// @ts-expect-error "open" is not a valid MmdsArrowHead literal
const _badArrowHead: MmdsArrowHead = "open";
void _badArrowHead;

const _nodeStyleEntry: MmdsNodeStyleEntry = {
  fill: "#fff",
  stroke: "#000",
  classNames: ["foo"],
};
void _nodeStyleEntry;

const _edgeLabelStyleEntry: MmdsEdgeLabelStyleEntry = {
  "font-size": "14",
};
void _edgeLabelStyleEntry;

const _subgraphStyleEntry: MmdsSubgraphStyleEntry = {
  rx: "8",
  ry: "12",
  classNames: ["bar"],
};
void _subgraphStyleEntry;

const _nodeStyleExtension: MmdsNodeStyleExtension = {
  nodes: { A: _nodeStyleEntry },
  edges: { e0: _edgeLabelStyleEntry },
  subgraphs: { sg1: _subgraphStyleEntry },
};
void _nodeStyleExtension;

const _genericNodeStyleExtension = getExtension<MmdsNodeStyleExtension>(
  { extensions: { [MMDS_NODE_STYLE_NAMESPACE]: _nodeStyleExtension } },
  MMDS_NODE_STYLE_NAMESPACE,
);
void _genericNodeStyleExtension;

const _nodeStyleNamespace: "org.mmdflux.node-style.v1" =
  MMDS_NODE_STYLE_NAMESPACE;
void _nodeStyleNamespace;

const _nodeStyleProfile: "mmdflux-node-style-v1" = MMDS_NODE_STYLE_PROFILE;
void _nodeStyleProfile;

// @ts-expect-error wrong field name
const _badNodeStyleEntry: MmdsNodeStyleEntry = { fil: "#fff" };
void _badNodeStyleEntry;

const _textMetricsProfile: MmdsTextMetricsProfile = {
  id: "mmdflux-sans-v1",
  source: "recorded",
  version: 1,
};
void _textMetricsProfile;

const _defaultTextStyle: MmdsDefaultTextStyle = {
  "font-family": '"trebuchet ms", verdana, arial, sans-serif',
  "font-size": 16,
  "font-style": "normal",
  "font-weight": "400",
  "line-height": 24,
};
void _defaultTextStyle;

const _layoutTextMetrics: MmdsLayoutTextMetrics = {
  "node-padding-x": 15,
  "node-padding-y": 15,
  "label-padding-x": 4,
  "label-padding-y": 2,
  "edge-label-max-width": 200,
};
void _layoutTextMetrics;

const _textMetricsExtension: MmdsTextMetricsExtension = {
  metricsProfile: _textMetricsProfile,
  defaultTextStyle: _defaultTextStyle,
  layoutText: _layoutTextMetrics,
};
void _textMetricsExtension;

const _textMeasurementsProfileRef: MmdsTextMeasurementsProfileRef = {
  id: "mmdflux-browser-canvas-v1",
  source: "dynamic",
  version: 1,
};
void _textMeasurementsProfileRef;

const _textMeasurementStyle: MmdsTextMeasurementStyle = {
  id: "s0",
  fontFamily: "Verdana",
  fontSize: 16,
  fontStyle: "normal",
  fontWeight: "400",
  lineHeight: 24,
  cssFont: "normal 400 16px Verdana",
};
void _textMeasurementStyle;

const _textMeasurementLineWidth: MmdsTextMeasurementLineWidth = {
  style: "s0",
  text: "Alpha",
  width: 42.5,
};
void _textMeasurementLineWidth;

const _textMeasurementScalarWidth: MmdsTextMeasurementScalarWidth = {
  style: "s0",
  text: "A",
  width: 8.5,
};
void _textMeasurementScalarWidth;

const _textMeasurementsExtension: MmdsTextMeasurementsExtension = {
  profileRef: _textMeasurementsProfileRef,
  textStyles: [_textMeasurementStyle],
  lineWidths: [_textMeasurementLineWidth],
  scalarWidths: [_textMeasurementScalarWidth],
};
void _textMeasurementsExtension;

const _textMetricsNamespace: "org.mmdflux.text-metrics.v1" =
  MMDS_TEXT_METRICS_NAMESPACE;
void _textMetricsNamespace;

const _textMetricsProfileName: "mmdflux-text-metrics-v1" =
  MMDS_TEXT_METRICS_PROFILE;
void _textMetricsProfileName;

const _textMeasurementsNamespace: "org.mmdflux.text-measurements.v1" =
  MMDS_TEXT_MEASUREMENTS_NAMESPACE;
void _textMeasurementsNamespace;

const _textMeasurementsProfileName: "mmdflux-text-measurements-v1" =
  MMDS_TEXT_MEASUREMENTS_PROFILE;
void _textMeasurementsProfileName;

const _badTextMeasurementsProfileRef: MmdsTextMeasurementsProfileRef = {
  id: "mmdflux-sans-v1",
  // @ts-expect-error text measurement sidecars must reference a dynamic profile
  source: "recorded",
  version: 1,
};
void _badTextMeasurementsProfileRef;

const _edgeLabelSide: MmdsEdgeLabelSide = "above";
void _edgeLabelSide;

const _edgeLabelRect: MmdsRect = { x: 5, y: 5, width: 8, height: 4 };
void _edgeLabelRect;

const _edgeWithLabelGeometry: MmdsEdge = {
  id: "e0",
  source: "A",
  target: "B",
  label_side: _edgeLabelSide,
  label_rect: _edgeLabelRect,
};
void _edgeWithLabelGeometry;

// @ts-expect-error label_side only accepts schema-defined tokens
const _badEdgeLabelSide: MmdsEdgeLabelSide = "sideways";
void _badEdgeLabelSide;

function _readTypedMetadata(metadata: MmdsMetadata): void {
  const engine: string | undefined = metadata.engine;
  const diagnostics: MmdsMetadataDiagnostics | undefined = metadata.diagnostics;
  void engine;
  void diagnostics;
}
void _readTypedMetadata;

const _unfitLabelOverlapDiagnostic: MmdsUnfitLabelOverlapDiagnostic = {
  edge_id: "e0",
  label: "yes",
  gap_pixels: 12,
  label_span_pixels: 30,
  attempted_side: "above",
};
void _unfitLabelOverlapDiagnostic;

const _metadataDiagnostics: MmdsMetadataDiagnostics = {
  unfit_label_overlaps: [_unfitLabelOverlapDiagnostic],
};
void _metadataDiagnostics;

const _badUnfitLabelOverlapDiagnostic: MmdsUnfitLabelOverlapDiagnostic = {
  edge_id: "e0",
  label: "yes",
  gap_pixels: 12,
  label_span_pixels: 30,
  // @ts-expect-error attempted_side must be a schema-defined side
  attempted_side: "diagonal",
};
void _badUnfitLabelOverlapDiagnostic;

const _subgraphWithConcurrentRegions: MmdsSubgraph = {
  id: "sg_state",
  children: [],
  concurrent_regions: ["fork_top", "fork_bottom"],
};
void _subgraphWithConcurrentRegions;

function _readNormalizedClassNames(
  node: NormalizedMmdsNode,
  subgraph: NormalizedMmdsSubgraph,
): void {
  const nodeClasses: string[] | undefined = node.classNames;
  const subgraphClasses: string[] | undefined = subgraph.classNames;
  void nodeClasses;
  void subgraphClasses;
}
void _readNormalizedClassNames;
