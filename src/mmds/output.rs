//! MMDS output contract and serialization helpers.
//!
//! Produces structured JSON from graph-family geometry with two levels:
//! - `layout`: Node geometry + edge topology/semantics (no edge paths).
//! - `routed`: Everything from layout + routed edge paths and bounds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::engines::graph::EngineAlgorithmId;
use crate::errors::RenderError;
use crate::graph::attachment::EdgePort;
use crate::graph::geometry::{GraphGeometry, PositionedNode, RoutedGraphGeometry};
use crate::graph::projection::{GridProjection, OverrideSubgraphProjection};
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::style::NodeStyle;
use crate::graph::{Arrow, Direction, GeometryLevel, Graph, Shape, Stroke};
use crate::simplification::PathSimplification;

pub const MMDS_CORE_PROFILE: &str = "mmds-core-v1";
pub const MMDS_SVG_PROFILE: &str = "mmdflux-svg-v1";
pub const MMDS_TEXT_PROFILE: &str = "mmdflux-text-v1";
pub const MMDS_NODE_STYLE_PROFILE: &str = "mmdflux-node-style-v1";
pub const MMDS_TEXT_EXTENSION_NAMESPACE: &str = "org.mmdflux.render.text.v1";
pub const MMDS_NODE_STYLE_EXTENSION_NAMESPACE: &str = "org.mmdflux.node-style.v1";
pub const SUPPORTED_MMDS_PROFILES: &[&str] = &[
    MMDS_CORE_PROFILE,
    MMDS_SVG_PROFILE,
    MMDS_TEXT_PROFILE,
    MMDS_NODE_STYLE_PROFILE,
];

/// Serialize a graph-family diagram to MMDS JSON at layout level.
///
/// Uses `GraphGeometry` for node positions and `Diagram` for edge semantics.
/// Edge paths are excluded at layout level.
#[cfg(test)]
pub(crate) fn to_mmds_layout(diagram: &Graph, geometry: &GraphGeometry) -> String {
    to_mmds_layout_typed("flowchart", diagram, geometry)
}

/// Serialize a graph-family diagram to MMDS JSON at layout level with explicit type.
#[cfg(test)]
pub(crate) fn to_mmds_layout_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
) -> String {
    render_mmds_output(
        diagram_type,
        diagram,
        geometry,
        None,
        PathSimplification::None,
        None,
    )
}

/// Serialize a graph-family diagram to MMDS JSON at routed level.
///
/// Includes everything from layout level plus routed edge paths and
/// subgraph bounds.
#[cfg(test)]
pub(crate) fn to_mmds_routed(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: &RoutedGraphGeometry,
) -> String {
    to_mmds_routed_typed("flowchart", diagram, geometry, routed)
}

/// Serialize a graph-family diagram to MMDS JSON at routed level with explicit type.
#[cfg(test)]
pub(crate) fn to_mmds_routed_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: &RoutedGraphGeometry,
) -> String {
    render_mmds_output(
        diagram_type,
        diagram,
        geometry,
        Some(routed),
        PathSimplification::None,
        None,
    )
}

/// Serialize a diagram to MMDS JSON at the specified geometry level.
#[cfg(test)]
pub(crate) fn to_mmds_json(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<EngineAlgorithmId>,
) -> Result<String, RenderError> {
    to_mmds_json_typed(
        "flowchart",
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
    )
}

/// Serialize a diagram to MMDS JSON at the specified geometry level with explicit type.
pub(crate) fn to_mmds_json_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<EngineAlgorithmId>,
) -> Result<String, RenderError> {
    match level {
        GeometryLevel::Layout => Ok(render_mmds_output(
            diagram_type,
            diagram,
            geometry,
            None,
            path_simplification,
            engine_id,
        )),
        GeometryLevel::Routed => routed
            .ok_or_else(|| RenderError {
                message: "routed MMDS output requested but routed geometry was not provided"
                    .to_string(),
            })
            .map(|routed| {
                render_mmds_output(
                    diagram_type,
                    diagram,
                    geometry,
                    Some(routed),
                    path_simplification,
                    engine_id,
                )
            }),
    }
}

pub(crate) fn to_mmds_json_typed_with_routing(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<EngineAlgorithmId>,
) -> Result<String, RenderError> {
    let routed_owned = (routed.is_none() && matches!(level, GeometryLevel::Routed))
        .then(|| route_graph_geometry(diagram, geometry, EdgeRouting::OrthogonalRoute));
    let routed = routed.or(routed_owned.as_ref());

    to_mmds_json_typed(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
    )
}

fn render_mmds_output(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    path_simplification: PathSimplification,
    engine_id: Option<EngineAlgorithmId>,
) -> String {
    let output = build_mmds_output(
        diagram_type,
        diagram,
        geometry,
        routed,
        path_simplification,
        engine_id,
    );
    serialize_mmds_output(&output)
}

fn serialize_mmds_output(output: &MmdsOutput) -> String {
    serde_json::to_string_pretty(output).expect("MMDS serialization should not fail")
}

fn edge_port_to_mmds(port: &EdgePort) -> MmdsPort {
    MmdsPort {
        face: port.face.as_str().to_string(),
        fraction: port.fraction,
        position: MmdsPosition {
            x: port.position.x,
            y: port.position.y,
        },
        group_size: port.group_size,
    }
}

fn build_mmds_output(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    path_simplification: PathSimplification,
    engine_id: Option<EngineAlgorithmId>,
) -> MmdsOutput {
    let level = if routed.is_some() { "routed" } else { "layout" };
    let styled_nodes = collect_styled_nodes(diagram);

    // At routed level, use the recomputed routed bounds (which cover all
    // routed edge paths) instead of stale layout bounds.
    let effective_bounds = routed.map_or(geometry.bounds, |r| r.bounds);
    let metadata = MmdsMetadata {
        diagram_type: diagram_type.to_string(),
        direction: direction_str(diagram.direction).to_string(),
        bounds: MmdsBounds {
            width: effective_bounds.width,
            height: effective_bounds.height,
        },
        engine: engine_id.map(|id| id.to_string()),
    };

    // Build nodes from geometry (float positions)
    let mut nodes: Vec<MmdsNode> = geometry.nodes.values().map(mmds_node).collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // Build edges
    let edges: Vec<MmdsEdge> = diagram
        .edges
        .iter()
        .enumerate()
        .map(|(i, edge)| {
            let mut mmds_edge = MmdsEdge {
                id: format!("e{i}"),
                source: edge.from.clone(),
                target: edge.to.clone(),
                from_subgraph: edge.from_subgraph.clone(),
                to_subgraph: edge.to_subgraph.clone(),
                label: edge.label.clone(),
                stroke: stroke_str(edge.stroke).to_string(),
                arrow_start: arrow_str(edge.arrow_start).to_string(),
                arrow_end: arrow_str(edge.arrow_end).to_string(),
                minlen: edge.minlen,
                path: None,
                label_position: None,
                is_backward: None,
                source_port: None,
                target_port: None,
            };

            // Add routed fields only at routed level
            if let Some(routed) = routed
                && let Some(re) = routed.edges.iter().find(|e| e.index == i)
            {
                let full_path: Vec<[f64; 2]> = re.path.iter().map(|p| [p.x, p.y]).collect();
                mmds_edge.path = Some(
                    path_simplification
                        .simplify_with_coords(&full_path, |point| (point[0], point[1])),
                );
                mmds_edge.label_position =
                    re.label_position.map(|p| MmdsPosition { x: p.x, y: p.y });
                mmds_edge.is_backward = Some(re.is_backward);
                mmds_edge.source_port = re.source_port.as_ref().map(edge_port_to_mmds);
                mmds_edge.target_port = re.target_port.as_ref().map(edge_port_to_mmds);
            }

            mmds_edge
        })
        .collect();

    // Build subgraphs
    let mut subgraphs: Vec<MmdsSubgraph> = diagram
        .subgraphs
        .values()
        .map(|sg| {
            let direct_children: Vec<String> = sg
                .nodes
                .iter()
                .filter(|node_id| {
                    diagram
                        .nodes
                        .get(*node_id)
                        .and_then(|n| n.parent.as_deref())
                        == Some(&sg.id)
                })
                .cloned()
                .collect();

            let bounds = routed.and_then(|r| {
                r.subgraphs.get(&sg.id).map(|sg_geom| MmdsBounds {
                    width: sg_geom.rect.width,
                    height: sg_geom.rect.height,
                })
            });

            MmdsSubgraph {
                id: sg.id.clone(),
                title: sg.title.clone(),
                children: direct_children,
                parent: sg.parent.clone(),
                direction: sg.dir.map(|d| direction_str(d).to_string()),
                bounds,
            }
        })
        .collect();
    subgraphs.sort_by(|a, b| a.id.cmp(&b.id));

    let mut profiles = Vec::new();
    let mut extensions = BTreeMap::new();
    if let Some(grid_projection) = &geometry.grid_projection {
        push_profile(&mut profiles, MMDS_CORE_PROFILE);
        push_profile(&mut profiles, MMDS_TEXT_PROFILE);
        extensions.insert(
            MMDS_TEXT_EXTENSION_NAMESPACE.to_string(),
            grid_projection_extension(grid_projection),
        );
    }
    if !styled_nodes.is_empty() {
        push_profile(&mut profiles, MMDS_CORE_PROFILE);
        push_profile(&mut profiles, MMDS_NODE_STYLE_PROFILE);
        extensions.insert(
            MMDS_NODE_STYLE_EXTENSION_NAMESPACE.to_string(),
            node_style_extension(styled_nodes),
        );
    }

    MmdsOutput {
        version: 1,
        profiles,
        extensions,
        defaults: MmdsDefaults::default(),
        geometry_level: level.to_string(),
        metadata,
        nodes,
        edges,
        subgraphs,
    }
}

fn collect_styled_nodes(diagram: &Graph) -> BTreeMap<String, NodeStyle> {
    diagram
        .nodes
        .iter()
        .filter(|(_, node)| !node.style.is_empty())
        .map(|(node_id, node)| (node_id.clone(), node.style.clone()))
        .collect()
}

fn push_profile(profiles: &mut Vec<String>, profile: &str) {
    if !profiles.iter().any(|existing| existing == profile) {
        profiles.push(profile.to_string());
    }
}

fn grid_projection_extension(grid_projection: &GridProjection) -> Map<String, Value> {
    let mut extension = Map::new();
    extension.insert(
        "projection".to_string(),
        Value::Object(serialize_grid_projection(grid_projection)),
    );
    extension
}

fn serialize_grid_projection(grid_projection: &GridProjection) -> Map<String, Value> {
    let mut projection = Map::new();
    projection.insert(
        "node_ranks".to_string(),
        Value::Object(
            grid_projection
                .node_ranks
                .iter()
                .map(|(node_id, rank)| (node_id.clone(), Value::Number(Number::from(*rank))))
                .collect(),
        ),
    );
    projection.insert(
        "edge_waypoints".to_string(),
        Value::Object(
            grid_projection
                .edge_waypoints
                .iter()
                .map(|(edge_idx, waypoints)| {
                    (
                        edge_idx.to_string(),
                        Value::Array(
                            waypoints
                                .iter()
                                .map(|(point, rank)| ranked_point_value(*point, *rank))
                                .collect(),
                        ),
                    )
                })
                .collect(),
        ),
    );
    projection.insert(
        "label_positions".to_string(),
        Value::Object(
            grid_projection
                .label_positions
                .iter()
                .map(|(edge_idx, (point, rank))| {
                    (edge_idx.to_string(), ranked_point_value(*point, *rank))
                })
                .collect(),
        ),
    );
    if !grid_projection.override_subgraphs.is_empty() {
        projection.insert(
            "override_subgraphs".to_string(),
            Value::Object(
                grid_projection
                    .override_subgraphs
                    .iter()
                    .map(|(subgraph_id, projection)| {
                        (
                            subgraph_id.clone(),
                            Value::Object(serialize_override_subgraph_projection(projection)),
                        )
                    })
                    .collect(),
            ),
        );
    }
    projection
}

fn ranked_point_value(point: crate::graph::geometry::FPoint, rank: i32) -> Value {
    let mut value = Map::new();
    value.insert(
        "x".to_string(),
        Value::Number(Number::from_f64(point.x).expect("grid projection x should be finite")),
    );
    value.insert(
        "y".to_string(),
        Value::Number(Number::from_f64(point.y).expect("grid projection y should be finite")),
    );
    value.insert("rank".to_string(), Value::Number(Number::from(rank)));
    Value::Object(value)
}

fn serialize_override_subgraph_projection(
    projection: &OverrideSubgraphProjection,
) -> Map<String, Value> {
    serialize_rect_map(&projection.nodes)
}

fn serialize_rect_map(
    rects: &std::collections::HashMap<String, crate::graph::geometry::FRect>,
) -> Map<String, Value> {
    rects
        .iter()
        .map(|(node_id, rect)| (node_id.clone(), rect_value(*rect)))
        .collect()
}

fn rect_value(rect: crate::graph::geometry::FRect) -> Value {
    let mut value = Map::new();
    value.insert(
        "x".to_string(),
        Value::Number(Number::from_f64(rect.x).expect("subgraph projection x should be finite")),
    );
    value.insert(
        "y".to_string(),
        Value::Number(Number::from_f64(rect.y).expect("subgraph projection y should be finite")),
    );
    value.insert(
        "width".to_string(),
        Value::Number(
            Number::from_f64(rect.width).expect("subgraph projection width should be finite"),
        ),
    );
    value.insert(
        "height".to_string(),
        Value::Number(
            Number::from_f64(rect.height).expect("subgraph projection height should be finite"),
        ),
    );
    Value::Object(value)
}

fn node_style_extension(styled_nodes: BTreeMap<String, NodeStyle>) -> Map<String, Value> {
    let nodes = styled_nodes
        .iter()
        .map(|(node_id, style)| {
            (
                node_id.clone(),
                Value::Object(serialize_node_style_extension(style)),
            )
        })
        .collect();
    let mut extension = Map::new();
    extension.insert("nodes".to_string(), Value::Object(nodes));
    extension
}

fn serialize_node_style_extension(style: &NodeStyle) -> Map<String, Value> {
    let mut payload = Map::new();
    if let Some(fill) = &style.fill {
        payload.insert("fill".to_string(), Value::String(fill.raw().to_string()));
    }
    if let Some(stroke) = &style.stroke {
        payload.insert(
            "stroke".to_string(),
            Value::String(stroke.raw().to_string()),
        );
    }
    if let Some(color) = &style.color {
        payload.insert("color".to_string(), Value::String(color.raw().to_string()));
    }
    payload
}

fn mmds_node(pn: &PositionedNode) -> MmdsNode {
    MmdsNode {
        id: pn.id.clone(),
        label: pn.label.clone(),
        shape: shape_str(pn.shape).to_string(),
        parent: pn.parent.clone(),
        position: MmdsPosition {
            x: pn.rect.x + pn.rect.width / 2.0,
            y: pn.rect.y + pn.rect.height / 2.0,
        },
        size: MmdsSize {
            width: pn.rect.width,
            height: pn.rect.height,
        },
    }
}

fn direction_str(dir: Direction) -> &'static str {
    match dir {
        Direction::TopDown => "TD",
        Direction::BottomTop => "BT",
        Direction::LeftRight => "LR",
        Direction::RightLeft => "RL",
    }
}

fn shape_str(shape: Shape) -> &'static str {
    match shape {
        Shape::Rectangle => "rectangle",
        Shape::Round => "round",
        Shape::Stadium => "stadium",
        Shape::Subroutine => "subroutine",
        Shape::Cylinder => "cylinder",
        Shape::Document => "document",
        Shape::Documents => "documents",
        Shape::TaggedDocument => "tagged_document",
        Shape::Card => "card",
        Shape::TaggedRect => "tagged_rect",
        Shape::Diamond => "diamond",
        Shape::Hexagon => "hexagon",
        Shape::Trapezoid => "trapezoid",
        Shape::InvTrapezoid => "inv_trapezoid",
        Shape::Parallelogram => "parallelogram",
        Shape::InvParallelogram => "inv_parallelogram",
        Shape::ManualInput => "manual_input",
        Shape::Asymmetric => "asymmetric",
        Shape::Circle => "circle",
        Shape::DoubleCircle => "double_circle",
        Shape::SmallCircle => "small_circle",
        Shape::FramedCircle => "framed_circle",
        Shape::CrossedCircle => "crossed_circle",
        Shape::TextBlock => "text_block",
        Shape::ForkJoin => "fork_join",
    }
}

fn stroke_str(stroke: Stroke) -> &'static str {
    match stroke {
        Stroke::Solid => "solid",
        Stroke::Dotted => "dotted",
        Stroke::Thick => "thick",
        Stroke::Invisible => "invisible",
    }
}

fn arrow_str(arrow: Arrow) -> &'static str {
    match arrow {
        Arrow::Normal => "normal",
        Arrow::None => "none",
        Arrow::Cross => "cross",
        Arrow::Circle => "circle",
        Arrow::OpenTriangle => "open_triangle",
        Arrow::Diamond => "diamond",
        Arrow::OpenDiamond => "open_diamond",
    }
}

// ---------------------------------------------------------------------------
// MMDS data types
// ---------------------------------------------------------------------------

/// Top-level MMDS output envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsOutput {
    /// Schema version (1 for MMDS).
    pub version: u32,
    /// Optional behavior bundle declarations for capability negotiation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
    /// Optional namespaced extension payloads keyed by versioned namespace IDs.
    ///
    /// Key format is governed by schema/docs (for example:
    /// `org.mmdflux.render.svg.v1`), while values stay renderer-specific.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Map<String, Value>>,
    /// Document-level default values for omitted node/edge fields.
    pub defaults: MmdsDefaults,
    /// Geometry level: "layout" or "routed".
    pub geometry_level: String,
    /// Diagram metadata.
    pub metadata: MmdsMetadata,
    /// Node inventory with positions.
    pub nodes: Vec<MmdsNode>,
    /// Edge inventory (topology at layout, paths at routed).
    pub edges: Vec<MmdsEdge>,
    /// Subgraph inventory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subgraphs: Vec<MmdsSubgraph>,
}

/// Default values for omitted fields in nodes and edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MmdsDefaults {
    pub node: MmdsNodeDefaults,
    pub edge: MmdsEdgeDefaults,
}

/// Node-level default values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsNodeDefaults {
    #[serde(default = "default_node_shape")]
    pub shape: String,
}

impl Default for MmdsNodeDefaults {
    fn default() -> Self {
        Self {
            shape: default_node_shape(),
        }
    }
}

/// Edge-level default values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsEdgeDefaults {
    #[serde(default = "default_stroke")]
    pub stroke: String,
    #[serde(default = "default_arrow_start")]
    pub arrow_start: String,
    #[serde(default = "default_arrow_end")]
    pub arrow_end: String,
    #[serde(default = "default_minlen")]
    pub minlen: i32,
}

impl Default for MmdsEdgeDefaults {
    fn default() -> Self {
        Self {
            stroke: default_stroke(),
            arrow_start: default_arrow_start(),
            arrow_end: default_arrow_end(),
            minlen: default_minlen(),
        }
    }
}

/// Diagram-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsMetadata {
    /// Diagram type (e.g., "flowchart", "class").
    pub diagram_type: String,
    /// Layout direction: "TD", "BT", "LR", or "RL".
    pub direction: String,
    /// Overall diagram bounds in MMDS layout space.
    pub bounds: MmdsBounds,
    /// Engine+algorithm identifier that produced this output (e.g., "flux-layered").
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub engine: Option<String>,
}

/// Bounding box dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsBounds {
    pub width: f64,
    pub height: f64,
}

/// A node in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsNode {
    /// Node identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Shape name (snake_case).
    #[serde(
        default = "default_node_shape",
        skip_serializing_if = "is_default_node_shape"
    )]
    pub shape: String,
    /// Parent subgraph ID, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub parent: Option<String>,
    /// Center position in layout float space.
    pub position: MmdsPosition,
    /// Node dimensions.
    pub size: MmdsSize,
}

/// Float-precision position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsPosition {
    pub x: f64,
    pub y: f64,
}

/// Float-precision dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsSize {
    pub width: f64,
    pub height: f64,
}

/// Port attachment metadata for an edge endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsPort {
    /// Which face of the node the edge attaches to ("top", "bottom", "left", "right").
    pub face: String,
    /// Position within the face (0.0 = start, 1.0 = end).
    pub fraction: f64,
    /// Absolute position of the attachment point.
    pub position: MmdsPosition,
    /// How many edges share this face on this node.
    pub group_size: usize,
}

/// An edge in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsEdge {
    /// Deterministic edge identifier ("e{declaration_index}").
    pub id: String,
    /// Source node ID.
    pub source: String,
    /// Target node ID.
    pub target: String,
    /// Original source subgraph ID when this edge targeted a subgraph as source.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub from_subgraph: Option<String>,
    /// Original target subgraph ID when this edge targeted a subgraph as target.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub to_subgraph: Option<String>,
    /// Edge label, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub label: Option<String>,
    /// Stroke style.
    #[serde(default = "default_stroke", skip_serializing_if = "is_default_stroke")]
    pub stroke: String,
    /// Arrow at source end.
    #[serde(
        default = "default_arrow_start",
        skip_serializing_if = "is_default_arrow_start"
    )]
    pub arrow_start: String,
    /// Arrow at target end.
    #[serde(
        default = "default_arrow_end",
        skip_serializing_if = "is_default_arrow_end"
    )]
    pub arrow_end: String,
    /// Minimum rank separation.
    #[serde(default = "default_minlen", skip_serializing_if = "is_default_minlen")]
    pub minlen: i32,
    /// Routed edge path (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub path: Option<Vec<[f64; 2]>>,
    /// Label center position (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub label_position: Option<MmdsPosition>,
    /// Whether edge flows backward (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub is_backward: Option<bool>,
    /// Source-side port attachment (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub source_port: Option<MmdsPort>,
    /// Target-side port attachment (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub target_port: Option<MmdsPort>,
}

/// A subgraph in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmdsSubgraph {
    /// Subgraph identifier.
    pub id: String,
    /// Display title.
    pub title: String,
    /// IDs of nodes directly in this subgraph.
    pub children: Vec<String>,
    /// Parent subgraph ID, if nested.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub parent: Option<String>,
    /// Subgraph direction override ("TD", "BT", "LR", "RL"), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub direction: Option<String>,
    /// Subgraph bounding box (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub bounds: Option<MmdsBounds>,
}

fn default_node_shape() -> String {
    "rectangle".to_string()
}

fn default_stroke() -> String {
    "solid".to_string()
}

fn default_arrow_start() -> String {
    "none".to_string()
}

fn default_arrow_end() -> String {
    "normal".to_string()
}

fn default_minlen() -> i32 {
    1
}

fn is_default_node_shape(value: &String) -> bool {
    value == "rectangle"
}

fn is_default_stroke(value: &String) -> bool {
    value == "solid"
}

fn is_default_arrow_start(value: &String) -> bool {
    value == "none"
}

fn is_default_arrow_end(value: &String) -> bool {
    value == "normal"
}

fn is_default_minlen(value: &i32) -> bool {
    *value == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::engines::graph::EngineConfig;
    use crate::engines::graph::algorithms::layered::{MeasurementMode, run_layered_layout};
    use crate::graph::routing::{EdgeRouting, route_graph_geometry};
    use crate::mermaid::parse_flowchart;

    fn layout_geometry(input: &str) -> (Graph, GraphGeometry) {
        let fc = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&fc);
        let config = EngineConfig::Layered(
            crate::engines::graph::algorithms::layered::LayoutConfig::default(),
        );
        let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config).unwrap();
        (diagram, geom)
    }

    fn routed_geometry(diagram: &Graph, geometry: &GraphGeometry) -> RoutedGraphGeometry {
        route_graph_geometry(diagram, geometry, EdgeRouting::PolylineRoute)
    }

    #[test]
    fn layout_json_has_version_and_level() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.version, 1);
        assert_eq!(output.geometry_level, "layout");
    }

    #[test]
    fn layout_json_has_metadata() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.defaults.node.shape, "rectangle");
        assert_eq!(output.defaults.edge.stroke, "solid");
        assert_eq!(output.defaults.edge.arrow_start, "none");
        assert_eq!(output.defaults.edge.arrow_end, "normal");
        assert_eq!(output.defaults.edge.minlen, 1);
        assert_eq!(output.metadata.diagram_type, "flowchart");
        assert_eq!(output.metadata.direction, "TD");
        assert!(output.metadata.bounds.width > 0.0);
        assert!(output.metadata.bounds.height > 0.0);
    }

    #[test]
    fn layout_json_has_nodes_with_positions() {
        let (diagram, geom) = layout_geometry("graph TD\nA[Start]-->B[End]");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        assert_eq!(output.nodes.len(), 2);
        let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
        assert_eq!(node_a.label, "Start");
        assert_eq!(node_a.shape, "rectangle");
        assert!(node_a.size.width > 0.0);
        assert!(node_a.size.height > 0.0);
    }

    #[test]
    fn layout_json_edges_have_no_paths() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_layout(&diagram, &geom);

        // Layout-level: no edge geometry fields
        assert!(!json.contains("\"path\""));
        assert!(!json.contains("\"label_position\""));
        assert!(!json.contains("\"is_backward\""));

        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.edges.len(), 1);
        assert_eq!(output.edges[0].source, "A");
        assert_eq!(output.edges[0].target, "B");
        assert!(output.edges[0].path.is_none());
    }

    #[test]
    fn layout_json_edge_semantics() {
        let (diagram, geom) = layout_geometry("graph TD\nA-.label.->B");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        let edge = &output.edges[0];
        assert_eq!(edge.id, "e0");
        assert_eq!(edge.stroke, "dotted");
        assert_eq!(edge.label, Some("label".to_string()));
        assert_eq!(edge.arrow_end, "normal");
        assert_eq!(edge.minlen, 1);
    }

    #[test]
    fn layout_omits_default_edge_fields() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let edge = &value["edges"][0];
        assert!(edge.get("stroke").is_none());
        assert!(edge.get("arrow_start").is_none());
        assert!(edge.get("arrow_end").is_none());
        assert!(edge.get("minlen").is_none());
    }

    #[test]
    fn layout_keeps_non_default_edge_fields() {
        let (diagram, geom) = layout_geometry("graph TD\nA -.-> B\nC --x D\nE ----> F");
        let json = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let edges = value["edges"].as_array().unwrap();
        assert_eq!(edges[0]["stroke"], "dotted");
        assert_eq!(edges[1]["arrow_end"], "cross");
        assert!(edges[2]["minlen"].as_i64().unwrap() > 1);
    }

    #[test]
    fn layout_omits_default_node_shape() {
        let (diagram, geom) = layout_geometry("graph TD\nA[Rect]\nB(Round)");
        let json = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let nodes = value["nodes"].as_array().unwrap();
        assert!(nodes[0].get("shape").is_none());
        assert_eq!(nodes[1]["shape"], "round");
    }

    #[test]
    fn layout_omits_empty_subgraphs_key() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("subgraphs").is_none());
    }

    #[test]
    fn layout_deserializes_with_defaults() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let json = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.nodes[0].shape, "rectangle");
        assert_eq!(output.edges[0].stroke, "solid");
        assert_eq!(output.edges[0].arrow_start, "none");
        assert_eq!(output.edges[0].arrow_end, "normal");
        assert_eq!(output.edges[0].minlen, 1);
        assert!(output.subgraphs.is_empty());
    }

    #[test]
    fn layout_json_subgraphs() {
        let (diagram, geom) = layout_geometry("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        assert_eq!(output.subgraphs.len(), 1);
        assert_eq!(output.subgraphs[0].id, "sg1");
        assert_eq!(output.subgraphs[0].title, "Group");
        assert_eq!(output.subgraphs[0].direction, None);
        assert!(output.subgraphs[0].bounds.is_none());
    }

    #[test]
    fn layout_json_subgraph_direction_override() {
        let (diagram, geom) =
            layout_geometry("graph TD\nsubgraph sg1[Group]\ndirection LR\nA-->B\nend");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.subgraphs[0].direction.as_deref(), Some("LR"));
    }

    #[test]
    fn layout_json_nodes_sorted_by_id() {
        let (diagram, geom) = layout_geometry("graph TD\nC-->B\nB-->A");
        let json = to_mmds_layout(&diagram, &geom);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        let ids: Vec<&str> = output.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["A", "B", "C"]);
    }

    #[test]
    fn layout_json_direction_variants() {
        for (dir_str, expected) in [("TD", "TD"), ("LR", "LR"), ("BT", "BT"), ("RL", "RL")] {
            let input = format!("graph {dir_str}\nA-->B");
            let (diagram, geom) = layout_geometry(&input);
            let json = to_mmds_layout(&diagram, &geom);
            let output: MmdsOutput = serde_json::from_str(&json).unwrap();
            assert_eq!(output.metadata.direction, expected);
        }
    }

    #[test]
    fn routed_json_has_version_and_level() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let routed = routed_geometry(&diagram, &geom);
        let json = to_mmds_routed(&diagram, &geom, &routed);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.version, 1);
        assert_eq!(output.geometry_level, "routed");
    }

    #[test]
    fn routed_json_includes_edge_paths() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let routed = routed_geometry(&diagram, &geom);
        let json = to_mmds_routed(&diagram, &geom, &routed);

        assert!(json.contains("\"path\""));

        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        let edge = &output.edges[0];
        assert!(edge.path.is_some());
        assert!(edge.path.as_ref().unwrap().len() >= 2);
        assert!(edge.is_backward.is_some());
    }

    #[test]
    fn routed_json_includes_metadata_bounds() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let routed = routed_geometry(&diagram, &geom);
        let json = to_mmds_routed(&diagram, &geom, &routed);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        let bounds = &output.metadata.bounds;
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn routed_json_subgraph_bounds() {
        let (diagram, geom) = layout_geometry("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
        let routed = routed_geometry(&diagram, &geom);
        let json = to_mmds_routed(&diagram, &geom, &routed);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();

        let sg = &output.subgraphs[0];
        assert!(sg.bounds.is_some());
        let bounds = sg.bounds.as_ref().unwrap();
        assert!(bounds.width > 0.0);
    }

    #[test]
    fn to_mmds_json_dispatches_by_level() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let routed = routed_geometry(&diagram, &geom);

        let layout_json = to_mmds_json(
            &diagram,
            &geom,
            Some(&routed),
            GeometryLevel::Layout,
            PathSimplification::None,
            None,
        )
        .unwrap();
        assert!(!layout_json.contains("\"path\""));

        let routed_json = to_mmds_json(
            &diagram,
            &geom,
            Some(&routed),
            GeometryLevel::Routed,
            PathSimplification::None,
            None,
        )
        .unwrap();
        assert!(routed_json.contains("\"path\""));
    }

    #[test]
    fn mmds_port_serializes_correctly() {
        let port = MmdsPort {
            face: "bottom".to_string(),
            fraction: 0.5,
            position: MmdsPosition { x: 50.0, y: 35.0 },
            group_size: 1,
        };
        let json = serde_json::to_string(&port).unwrap();
        assert!(json.contains("\"face\":\"bottom\""));
        assert!(json.contains("\"fraction\":0.5"));
        assert!(json.contains("\"group_size\":1"));
    }

    #[test]
    fn mmds_edge_source_port_none_omitted_from_json() {
        let edge = MmdsEdge {
            id: "e0".into(),
            source: "A".into(),
            target: "B".into(),
            from_subgraph: None,
            to_subgraph: None,
            label: None,
            stroke: "solid".into(),
            arrow_start: "none".into(),
            arrow_end: "normal".into(),
            minlen: 1,
            path: None,
            label_position: None,
            is_backward: None,
            source_port: None,
            target_port: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        assert!(!json.contains("source_port"));
        assert!(!json.contains("target_port"));
    }

    #[test]
    fn mmds_edge_source_port_round_trips() {
        let port = MmdsPort {
            face: "right".to_string(),
            fraction: 0.3,
            position: MmdsPosition { x: 100.0, y: 30.0 },
            group_size: 2,
        };
        let edge = MmdsEdge {
            id: "e0".into(),
            source: "A".into(),
            target: "B".into(),
            from_subgraph: None,
            to_subgraph: None,
            label: None,
            stroke: "solid".into(),
            arrow_start: "none".into(),
            arrow_end: "normal".into(),
            minlen: 1,
            path: None,
            label_position: None,
            is_backward: None,
            source_port: Some(port),
            target_port: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: MmdsEdge = serde_json::from_str(&json).unwrap();
        let sp = deserialized.source_port.unwrap();
        assert_eq!(sp.face, "right");
        assert!((sp.fraction - 0.3).abs() < 1e-9);
        assert!((sp.position.x - 100.0).abs() < 1e-9);
        assert_eq!(sp.group_size, 2);
        assert!(deserialized.target_port.is_none());
    }

    #[test]
    fn mmds_edge_deserializes_without_ports() {
        let json = r#"{
            "id": "e0",
            "source": "A",
            "target": "B"
        }"#;
        let edge: MmdsEdge = serde_json::from_str(json).unwrap();
        assert!(edge.source_port.is_none());
        assert!(edge.target_port.is_none());
    }

    #[test]
    fn routed_json_includes_port_metadata() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let routed = routed_geometry(&diagram, &geom);
        let json = to_mmds_routed(&diagram, &geom, &routed);
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        let edge = &output.edges[0];
        // A simple TD A-->B should have port metadata at routed level
        assert!(edge.source_port.is_some());
        assert!(edge.target_port.is_some());
        let sp = edge.source_port.as_ref().unwrap();
        let tp = edge.target_port.as_ref().unwrap();
        assert_eq!(sp.face, "bottom");
        assert_eq!(tp.face, "top");
        assert_eq!(sp.group_size, 1);
        assert_eq!(tp.group_size, 1);
    }

    #[test]
    fn to_mmds_json_routed_requires_routed_geometry() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B");
        let err = to_mmds_json(
            &diagram,
            &geom,
            None,
            GeometryLevel::Routed,
            PathSimplification::None,
            None,
        )
        .unwrap_err();
        assert!(err.message.contains("routed MMDS output requested"));
    }

    #[test]
    fn routed_mmds_metadata_uses_routed_bounds_not_layout_bounds() {
        let (diagram, geom) = layout_geometry("graph TD\nA-->B\nB-->C\nC-->A");
        let routed = routed_geometry(&diagram, &geom);

        let routed_output: MmdsOutput =
            serde_json::from_str(&to_mmds_routed(&diagram, &geom, &routed)).unwrap();

        // The MMDS routed bounds must always match the routed geometry bounds.
        assert!(
            (routed_output.metadata.bounds.width - routed.bounds.width).abs() < 0.001,
            "routed MMDS metadata.bounds.width should match routed geometry bounds.width; \
             mmds={:.2}, routed_geom={:.2}",
            routed_output.metadata.bounds.width,
            routed.bounds.width
        );
        assert!(
            (routed_output.metadata.bounds.height - routed.bounds.height).abs() < 0.001,
            "routed MMDS metadata.bounds.height should match routed geometry bounds.height; \
             mmds={:.2}, routed_geom={:.2}",
            routed_output.metadata.bounds.height,
            routed.bounds.height
        );
    }
}
