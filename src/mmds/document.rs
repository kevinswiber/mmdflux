//! MMDS document contract and serialization helpers.
//!
//! Produces structured JSON from graph-family geometry with two levels:
//! - `layout`: Node geometry + edge topology/semantics (no edge paths).
//! - `routed`: Everything from layout + routed edge paths and bounds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::errors::RenderError;
use crate::graph::attachment::{EdgePort, PortFace};
use crate::graph::geometry::{
    EdgeLabelSide, GraphGeometry, PositionedNode, RoutedEdgeGeometry, RoutedGraphGeometry,
};
use crate::graph::measure::{
    TextMeasurementCache, TextMetricsProfileDescriptor, TextMetricsProvider,
    default_proportional_text_metrics,
};
use crate::graph::projection::{GridProjection, OverrideSubgraphProjection};
use crate::graph::routing::{
    EdgeRouting, route_graph_geometry, route_graph_geometry_with_provider,
};
use crate::graph::style::{EdgeStyle, NodeStyle};
use crate::graph::{Arrow, Direction, GeometryLevel, Graph, Shape, Stroke};
use crate::simplification::PathSimplification;

pub const CORE_PROFILE: &str = "mmds-core-v1";
pub const SVG_PROFILE: &str = "mmdflux-svg-v1";
pub const TEXT_PROFILE: &str = "mmdflux-text-v1";
pub const NODE_STYLE_PROFILE: &str = "mmdflux-node-style-v1";
pub const TEXT_METRICS_PROFILE: &str = "mmdflux-text-metrics-v1";
pub const TEXT_MEASUREMENTS_PROFILE: &str = "mmdflux-text-measurements-v1";
pub const TEXT_EXTENSION_NAMESPACE: &str = "org.mmdflux.render.text.v1";
pub const NODE_STYLE_EXTENSION_NAMESPACE: &str = "org.mmdflux.node-style.v1";
pub const TEXT_METRICS_EXTENSION_NAMESPACE: &str = "org.mmdflux.text-metrics.v1";
pub const TEXT_MEASUREMENTS_EXTENSION_NAMESPACE: &str = "org.mmdflux.text-measurements.v1";
pub const SUPPORTED_PROFILES: &[&str] = &[
    CORE_PROFILE,
    SVG_PROFILE,
    TEXT_PROFILE,
    NODE_STYLE_PROFILE,
    TEXT_METRICS_PROFILE,
    TEXT_MEASUREMENTS_PROFILE,
];

/// Serialize a graph-family diagram to MMDS JSON at layout level.
///
/// Uses `GraphGeometry` for node positions and `Diagram` for edge semantics.
/// Edge paths are excluded at layout level.
#[cfg(test)]
pub(crate) fn to_layout(diagram: &Graph, geometry: &GraphGeometry) -> String {
    to_layout_typed("flowchart", diagram, geometry)
}

/// Serialize a graph-family diagram to MMDS JSON at layout level with explicit type.
#[cfg(test)]
pub(crate) fn to_layout_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
) -> String {
    render_document_json(
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
pub(crate) fn to_routed(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: &RoutedGraphGeometry,
) -> String {
    to_routed_typed("flowchart", diagram, geometry, routed)
}

/// Serialize a graph-family diagram to MMDS JSON at routed level with explicit type.
#[cfg(test)]
pub(crate) fn to_routed_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: &RoutedGraphGeometry,
) -> String {
    render_document_json(
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
pub(crate) fn to_json(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> Result<String, RenderError> {
    to_json_typed(
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
#[cfg(test)]
pub(crate) fn to_json_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> Result<String, RenderError> {
    to_document_typed(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
    )
    .map(|document| serialize_document(&document))
}

#[cfg(test)]
pub(crate) fn to_document_typed(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> Result<Document, RenderError> {
    to_document_typed_with_text_metrics(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
        None,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn to_document_typed_with_text_metrics(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
    text_metrics_descriptor: Option<&TextMetricsProfileDescriptor>,
) -> Result<Document, RenderError> {
    to_document_typed_with_text_metrics_and_measurements(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
        text_metrics_descriptor,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn to_document_typed_with_text_metrics_and_measurements(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
    text_metrics_descriptor: Option<&TextMetricsProfileDescriptor>,
    text_measurements: Option<&TextMeasurementCache>,
) -> Result<Document, RenderError> {
    let options = DocumentBuildOptions {
        path_simplification,
        engine_id,
        text_metrics_descriptor,
        text_measurements,
    };
    match level {
        GeometryLevel::Layout => Ok(build_document(
            diagram_type,
            diagram,
            geometry,
            None,
            options,
        )),
        GeometryLevel::Routed => routed
            .ok_or_else(|| RenderError {
                message: "routed MMDS output requested but routed geometry was not provided"
                    .to_string(),
            })
            .map(|routed| build_document(diagram_type, diagram, geometry, Some(routed), options)),
    }
}

/// Serialize a graph-family diagram to MMDS JSON with fallback routing.
#[doc(hidden)]
#[deprecated(
    note = "use materialize_diagram plus serde_json serialization for JSON output, or render_document for replay"
)]
pub fn to_json_typed_with_routing(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> Result<String, RenderError> {
    to_document_typed_with_routing(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
    )
    .map(|document| serialize_document(&document))
}

pub(crate) fn to_document_typed_with_routing(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> Result<Document, RenderError> {
    to_document_typed_with_routing_and_text_metrics(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn to_document_typed_with_routing_and_text_metrics(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
    text_metrics_descriptor: Option<&TextMetricsProfileDescriptor>,
    text_metrics_provider: Option<&dyn TextMetricsProvider>,
    text_measurements: Option<&TextMeasurementCache>,
) -> Result<Document, RenderError> {
    let routed_owned = (routed.is_none() && matches!(level, GeometryLevel::Routed)).then(|| {
        match text_metrics_provider {
            Some(metrics) => route_graph_geometry_with_provider(
                diagram,
                geometry,
                EdgeRouting::OrthogonalRoute,
                metrics,
            ),
            None => {
                // Static MMDS fallback routing for legacy adapter callers that
                // request routed output without already carrying resolved metrics.
                let metrics = default_proportional_text_metrics();
                route_graph_geometry(diagram, geometry, EdgeRouting::OrthogonalRoute, &metrics)
            }
        }
    });
    let routed = routed.or(routed_owned.as_ref());

    to_document_typed_with_text_metrics_and_measurements(
        diagram_type,
        diagram,
        geometry,
        routed,
        level,
        path_simplification,
        engine_id,
        text_metrics_descriptor,
        text_measurements,
    )
}

#[cfg(test)]
fn render_document_json(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    path_simplification: PathSimplification,
    engine_id: Option<&str>,
) -> String {
    let document = build_document(
        diagram_type,
        diagram,
        geometry,
        routed,
        DocumentBuildOptions {
            path_simplification,
            engine_id,
            text_metrics_descriptor: None,
            text_measurements: None,
        },
    );
    serialize_document(&document)
}

fn serialize_document(document: &Document) -> String {
    serde_json::to_string_pretty(document).expect("MMDS serialization should not fail")
}

fn edge_port_to_mmds(port: &EdgePort) -> Port {
    Port {
        face: port.face,
        fraction: port.fraction,
        position: Position {
            x: port.position.x,
            y: port.position.y,
        },
        group_size: port.group_size,
    }
}

/// Resolve `label_side` for an MMDS edge using the fallback chain:
///   `label_geometry.side` → `layout_edge.label_side`
///
/// Returns `None` when no side has been assigned at either layer.
fn mmds_edge_label_side(
    layout_edge: Option<&crate::graph::geometry::LayoutEdge>,
) -> Option<EdgeLabelSide> {
    let le = layout_edge?;
    le.label_geometry.as_ref().map(|g| g.side).or(le.label_side)
}

/// Resolve `label_rect` for an MMDS edge (routed level only).
///
/// Returns `None` at layout level or when no label geometry exists.
fn mmds_edge_label_rect(routed_edge: Option<&RoutedEdgeGeometry>, is_routed: bool) -> Option<Rect> {
    if !is_routed {
        return None;
    }
    let re = routed_edge?;
    re.label_geometry.as_ref().map(|g| Rect {
        x: g.rect.x,
        y: g.rect.y,
        width: g.rect.width,
        height: g.rect.height,
    })
}

#[derive(Clone, Copy)]
struct DocumentBuildOptions<'a> {
    path_simplification: PathSimplification,
    engine_id: Option<&'a str>,
    text_metrics_descriptor: Option<&'a TextMetricsProfileDescriptor>,
    text_measurements: Option<&'a TextMeasurementCache>,
}

fn build_document(
    diagram_type: &str,
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    options: DocumentBuildOptions<'_>,
) -> Document {
    let level = if routed.is_some() {
        GeometryLevel::Routed
    } else {
        GeometryLevel::Layout
    };
    let styled_nodes = collect_styled_nodes(diagram);
    let styled_edges = collect_styled_edges(diagram);
    let styled_subgraphs = collect_styled_subgraphs(diagram);

    // At routed level, use the recomputed routed bounds (which cover all
    // routed edge paths) instead of stale layout bounds.
    let effective_bounds = routed.map_or(geometry.bounds, |r| r.bounds);
    let metadata = Metadata {
        diagram_type: diagram_type.to_string(),
        direction: diagram.direction,
        bounds: Bounds {
            width: effective_bounds.width,
            height: effective_bounds.height,
        },
        engine: options.engine_id.map(|id| id.to_string()),
    };

    // Build nodes from geometry (float positions)
    let mut nodes: Vec<Node> = geometry.nodes.values().map(node).collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // Build edges
    let is_routed = routed.is_some();
    let edges: Vec<Edge> = diagram
        .edges
        .iter()
        .enumerate()
        .map(|(i, edge)| {
            let layout_edge = geometry.edges.iter().find(|le| le.index == i);
            let routed_edge = routed.and_then(|r| r.edges.iter().find(|e| e.index == i));

            let mut mmds_edge = Edge {
                id: format!("e{i}"),
                source: edge.from.clone(),
                target: edge.to.clone(),
                from_subgraph: edge.from_subgraph.clone(),
                to_subgraph: edge.to_subgraph.clone(),
                label: edge.label.clone(),
                stroke: edge.stroke,
                arrow_start: edge.arrow_start,
                arrow_end: edge.arrow_end,
                minlen: edge.minlen,
                path: None,
                label_position: None,
                is_backward: None,
                source_port: None,
                target_port: None,
                label_side: mmds_edge_label_side(layout_edge),
                label_rect: mmds_edge_label_rect(routed_edge, is_routed),
            };

            // Add routed fields only at routed level
            if let Some(routed) = routed {
                if let Some(re) = routed_edge {
                    let full_path: Vec<[f64; 2]> = re.path.iter().map(|p| [p.x, p.y]).collect();
                    mmds_edge.path = Some(
                        options
                            .path_simplification
                            .simplify_with_coords(&full_path, |point| (point[0], point[1])),
                    );
                    mmds_edge.label_position =
                        re.label_position.map(|p| Position { x: p.x, y: p.y });
                    mmds_edge.is_backward = Some(re.is_backward);
                    mmds_edge.source_port = re.source_port.as_ref().map(edge_port_to_mmds);
                    mmds_edge.target_port = re.target_port.as_ref().map(edge_port_to_mmds);
                } else if let Some(self_edge) = routed.self_edges.iter().find(|e| e.edge_index == i)
                {
                    let full_path: Vec<[f64; 2]> =
                        self_edge.path.iter().map(|p| [p.x, p.y]).collect();
                    mmds_edge.path = Some(
                        options
                            .path_simplification
                            .simplify_with_coords(&full_path, |point| (point[0], point[1])),
                    );
                }
            }

            mmds_edge
        })
        .collect();

    // Build subgraphs
    let mut subgraphs: Vec<Subgraph> = diagram
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
                r.subgraphs.get(&sg.id).map(|sg_geom| Bounds {
                    width: sg_geom.rect.width,
                    height: sg_geom.rect.height,
                })
            });

            Subgraph {
                id: sg.id.clone(),
                title: sg.title.clone(),
                children: direct_children,
                parent: sg.parent.clone(),
                direction: sg.dir,
                bounds,
                invisible: sg.invisible,
                concurrent_regions: sg.concurrent_regions.clone(),
            }
        })
        .collect();
    subgraphs.sort_by(|a, b| a.id.cmp(&b.id));

    let mut profiles = Vec::new();
    let mut extensions = BTreeMap::new();
    if let Some(grid_projection) = &geometry.grid_projection {
        push_profile(&mut profiles, CORE_PROFILE);
        push_profile(&mut profiles, TEXT_PROFILE);
        extensions.insert(
            TEXT_EXTENSION_NAMESPACE.to_string(),
            grid_projection_extension(grid_projection),
        );
    }
    if !styled_nodes.is_empty() || !styled_edges.is_empty() || !styled_subgraphs.is_empty() {
        push_profile(&mut profiles, CORE_PROFILE);
        push_profile(&mut profiles, NODE_STYLE_PROFILE);
        extensions.insert(
            NODE_STYLE_EXTENSION_NAMESPACE.to_string(),
            style_extension(styled_nodes, styled_edges, styled_subgraphs),
        );
    }
    if let Some(text_metrics_descriptor) = options.text_metrics_descriptor {
        push_profile(&mut profiles, CORE_PROFILE);
        push_profile(&mut profiles, TEXT_METRICS_PROFILE);
        extensions.insert(
            TEXT_METRICS_EXTENSION_NAMESPACE.to_string(),
            text_metrics_extension(text_metrics_descriptor),
        );
        if let Some(text_measurements) = options.text_measurements {
            push_profile(&mut profiles, TEXT_MEASUREMENTS_PROFILE);
            extensions.insert(
                TEXT_MEASUREMENTS_EXTENSION_NAMESPACE.to_string(),
                text_measurements_extension(text_metrics_descriptor, text_measurements),
            );
        }
    }

    Document {
        version: 1,
        profiles,
        extensions,
        defaults: Defaults::default(),
        geometry_level: level,
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

fn collect_styled_edges(diagram: &Graph) -> BTreeMap<String, EdgeStyle> {
    diagram
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| !edge.style.is_empty())
        .map(|(index, edge)| (format!("e{index}"), edge.style.clone()))
        .collect()
}

#[derive(Debug, Default, Clone)]
struct SubgraphStyleEntry {
    style: NodeStyle,
    class_names: Vec<String>,
}

fn collect_styled_subgraphs(diagram: &Graph) -> BTreeMap<String, SubgraphStyleEntry> {
    diagram
        .subgraphs
        .iter()
        .filter(|(_, subgraph)| !subgraph.style.is_empty() || !subgraph.class_names.is_empty())
        .map(|(subgraph_id, subgraph)| {
            (
                subgraph_id.clone(),
                SubgraphStyleEntry {
                    style: subgraph.style.clone(),
                    class_names: subgraph.class_names.clone(),
                },
            )
        })
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

fn style_extension(
    styled_nodes: BTreeMap<String, NodeStyle>,
    styled_edges: BTreeMap<String, EdgeStyle>,
    styled_subgraphs: BTreeMap<String, SubgraphStyleEntry>,
) -> Map<String, Value> {
    let mut extension = Map::new();
    if !styled_nodes.is_empty() {
        let nodes = styled_nodes
            .iter()
            .map(|(node_id, style)| {
                (
                    node_id.clone(),
                    Value::Object(serialize_node_style_extension(style)),
                )
            })
            .collect();
        extension.insert("nodes".to_string(), Value::Object(nodes));
    }
    if !styled_edges.is_empty() {
        let edges = styled_edges
            .iter()
            .map(|(edge_id, style)| {
                (
                    edge_id.clone(),
                    Value::Object(serialize_edge_style_extension(style)),
                )
            })
            .collect();
        extension.insert("edges".to_string(), Value::Object(edges));
    }
    if !styled_subgraphs.is_empty() {
        let subgraphs = styled_subgraphs
            .iter()
            .map(|(subgraph_id, entry)| {
                let mut payload = serialize_node_style_extension(&entry.style);
                if !entry.class_names.is_empty() {
                    payload.insert(
                        "classNames".to_string(),
                        Value::Array(
                            entry
                                .class_names
                                .iter()
                                .map(|name| Value::String(name.clone()))
                                .collect(),
                        ),
                    );
                }
                (subgraph_id.clone(), Value::Object(payload))
            })
            .collect();
        extension.insert("subgraphs".to_string(), Value::Object(subgraphs));
    }
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
    if let Some(v) = &style.font_family {
        payload.insert("font-family".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_size {
        payload.insert("font-size".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_style {
        payload.insert("font-style".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_weight {
        payload.insert("font-weight".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.stroke_width {
        payload.insert("stroke-width".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.stroke_dasharray {
        payload.insert("stroke-dasharray".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.rx {
        payload.insert("rx".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.ry {
        payload.insert("ry".to_string(), Value::String(v.clone()));
    }
    payload
}

fn serialize_edge_style_extension(style: &EdgeStyle) -> Map<String, Value> {
    let mut payload = Map::new();
    if let Some(v) = &style.stroke_width {
        payload.insert("stroke-width".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_family {
        payload.insert("font-family".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_size {
        payload.insert("font-size".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_style {
        payload.insert("font-style".to_string(), Value::String(v.clone()));
    }
    if let Some(v) = &style.font_weight {
        payload.insert("font-weight".to_string(), Value::String(v.clone()));
    }
    payload
}

fn text_metrics_extension(descriptor: &TextMetricsProfileDescriptor) -> Map<String, Value> {
    let mut metrics_profile = Map::new();
    metrics_profile.insert(
        "id".to_string(),
        Value::String(descriptor.profile_id.clone()),
    );
    metrics_profile.insert(
        "source".to_string(),
        Value::String(descriptor.source.clone()),
    );
    metrics_profile.insert(
        "version".to_string(),
        Value::Number(Number::from(descriptor.version)),
    );

    let mut default_text_style = Map::new();
    default_text_style.insert(
        "font-family".to_string(),
        Value::String(descriptor.default_text_style.font_family.clone()),
    );
    default_text_style.insert(
        "font-size".to_string(),
        finite_number_value(descriptor.default_text_style.font_size, "font-size"),
    );
    default_text_style.insert(
        "font-style".to_string(),
        Value::String(descriptor.default_text_style.font_style.clone()),
    );
    default_text_style.insert(
        "font-weight".to_string(),
        Value::String(descriptor.default_text_style.font_weight.clone()),
    );
    default_text_style.insert(
        "line-height".to_string(),
        finite_number_value(descriptor.default_text_style.line_height, "line-height"),
    );

    let mut layout_text = Map::new();
    layout_text.insert(
        "node-padding-x".to_string(),
        finite_number_value(descriptor.layout_text.node_padding_x, "node-padding-x"),
    );
    layout_text.insert(
        "node-padding-y".to_string(),
        finite_number_value(descriptor.layout_text.node_padding_y, "node-padding-y"),
    );
    layout_text.insert(
        "label-padding-x".to_string(),
        finite_number_value(descriptor.layout_text.label_padding_x, "label-padding-x"),
    );
    layout_text.insert(
        "label-padding-y".to_string(),
        finite_number_value(descriptor.layout_text.label_padding_y, "label-padding-y"),
    );
    layout_text.insert(
        "edge-label-max-width".to_string(),
        descriptor
            .layout_text
            .edge_label_max_width
            .map(|value| finite_number_value(value, "edge-label-max-width"))
            .unwrap_or(Value::Null),
    );

    let mut extension = Map::new();
    extension.insert("metricsProfile".to_string(), Value::Object(metrics_profile));
    extension.insert(
        "defaultTextStyle".to_string(),
        Value::Object(default_text_style),
    );
    extension.insert("layoutText".to_string(), Value::Object(layout_text));
    extension
}

fn text_measurements_extension(
    descriptor: &TextMetricsProfileDescriptor,
    measurements: &TextMeasurementCache,
) -> Map<String, Value> {
    let mut profile_ref = Map::new();
    profile_ref.insert(
        "id".to_string(),
        Value::String(descriptor.profile_id.clone()),
    );
    profile_ref.insert(
        "source".to_string(),
        Value::String(descriptor.source.clone()),
    );
    profile_ref.insert(
        "version".to_string(),
        Value::Number(Number::from(descriptor.version)),
    );

    let text_styles = measurements
        .text_styles
        .values()
        .map(|style| {
            let mut entry = Map::new();
            entry.insert("id".to_string(), Value::String(style.id.clone()));
            entry.insert(
                "fontFamily".to_string(),
                Value::String(style.style.font_family.clone()),
            );
            entry.insert(
                "fontSize".to_string(),
                finite_number_value(style.style.font_size_px(), "textStyles.fontSize"),
            );
            entry.insert(
                "fontStyle".to_string(),
                Value::String(style.style.font_style.clone()),
            );
            entry.insert(
                "fontWeight".to_string(),
                Value::String(style.style.font_weight.clone()),
            );
            entry.insert(
                "lineHeight".to_string(),
                finite_number_value(style.style.line_height_px(), "textStyles.lineHeight"),
            );
            entry.insert("cssFont".to_string(), Value::String(style.css_font.clone()));
            Value::Object(entry)
        })
        .collect();
    let line_widths = measurements
        .line_widths
        .iter()
        .map(|((style_id, text), width)| {
            let mut entry = Map::new();
            entry.insert("style".to_string(), Value::String(style_id.clone()));
            entry.insert("text".to_string(), Value::String(text.clone()));
            entry.insert(
                "width".to_string(),
                finite_number_value(*width, "lineWidths.width"),
            );
            Value::Object(entry)
        })
        .collect();
    let scalar_widths = measurements
        .scalar_widths
        .iter()
        .map(|((style_id, ch), width)| {
            let mut entry = Map::new();
            entry.insert("style".to_string(), Value::String(style_id.clone()));
            entry.insert("text".to_string(), Value::String(ch.to_string()));
            entry.insert(
                "width".to_string(),
                finite_number_value(*width, "scalarWidths.width"),
            );
            Value::Object(entry)
        })
        .collect();

    let mut extension = Map::new();
    extension.insert("profileRef".to_string(), Value::Object(profile_ref));
    extension.insert("textStyles".to_string(), Value::Array(text_styles));
    extension.insert("lineWidths".to_string(), Value::Array(line_widths));
    extension.insert("scalarWidths".to_string(), Value::Array(scalar_widths));
    extension
}

fn finite_number_value(value: f64, context: &str) -> Value {
    Value::Number(
        Number::from_f64(value).unwrap_or_else(|| {
            panic!("MMDS text metrics extension value {context} should be finite")
        }),
    )
}

fn node(pn: &PositionedNode) -> Node {
    Node {
        id: pn.id.clone(),
        label: pn.label.clone(),
        shape: pn.shape,
        parent: pn.parent.clone(),
        position: Position {
            x: pn.rect.x + pn.rect.width / 2.0,
            y: pn.rect.y + pn.rect.height / 2.0,
        },
        size: Size {
            width: pn.rect.width,
            height: pn.rect.height,
        },
    }
}

// ---------------------------------------------------------------------------
// MMDS data types
// ---------------------------------------------------------------------------

/// Top-level graph-family MMDS document envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
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
    pub defaults: Defaults,
    /// Geometry level: `layout` or `routed`.
    pub geometry_level: GeometryLevel,
    /// Diagram metadata.
    pub metadata: Metadata,
    /// Node inventory with positions.
    pub nodes: Vec<Node>,
    /// Edge inventory (topology at layout, paths at routed).
    pub edges: Vec<Edge>,
    /// Subgraph inventory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subgraphs: Vec<Subgraph>,
}

/// Legacy name for [`Document`].
///
/// New code should use `mmds::Document`. The alias remains available so
/// existing adapter code that imports `mmds::Output` continues to compile.
#[doc(hidden)]
#[deprecated(note = "use mmds::Document instead")]
pub type Output = Document;

/// Default values for omitted fields in nodes and edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Defaults {
    pub node: NodeDefaults,
    pub edge: EdgeDefaults,
}

/// Node-level default values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDefaults {
    #[serde(default = "default_node_shape")]
    pub shape: Shape,
}

impl Default for NodeDefaults {
    fn default() -> Self {
        Self {
            shape: default_node_shape(),
        }
    }
}

/// Edge-level default values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDefaults {
    #[serde(default = "default_stroke")]
    pub stroke: Stroke,
    #[serde(default = "default_arrow_start")]
    pub arrow_start: Arrow,
    #[serde(default = "default_arrow_end")]
    pub arrow_end: Arrow,
    #[serde(default = "default_minlen")]
    pub minlen: i32,
}

impl Default for EdgeDefaults {
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
pub struct Metadata {
    /// Diagram type (e.g., "flowchart", "class").
    pub diagram_type: String,
    /// Layout direction: `TD`, `BT`, `LR`, or `RL`.
    pub direction: Direction,
    /// Overall diagram bounds in MMDS layout space.
    pub bounds: Bounds,
    /// Engine+algorithm identifier that produced this output (e.g., "flux-layered").
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub engine: Option<String>,
}

/// Bounding box dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub width: f64,
    pub height: f64,
}

/// Axis-aligned rectangle with position and dimensions.
///
/// Used for edge `label_rect` (routed level only) and sequence-style rect
/// payloads. Coordinates are in unitless MMDS coordinate space; `x`/`y` denote
/// the top-left corner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A node in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Node identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Node shape.
    #[serde(
        default = "default_node_shape",
        skip_serializing_if = "is_default_node_shape"
    )]
    pub shape: Shape,
    /// Parent subgraph ID, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub parent: Option<String>,
    /// Center position in layout float space.
    pub position: Position,
    /// Node dimensions.
    pub size: Size,
}

/// Float-precision position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Float-precision dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Port attachment metadata for an edge endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    /// Which face of the node the edge attaches to.
    pub face: PortFace,
    /// Position within the face (0.0 = start, 1.0 = end).
    pub fraction: f64,
    /// Absolute position of the attachment point.
    pub position: Position,
    /// How many edges share this face on this node.
    pub group_size: usize,
}

/// An edge in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
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
    pub stroke: Stroke,
    /// Arrow at source end.
    #[serde(
        default = "default_arrow_start",
        skip_serializing_if = "is_default_arrow_start"
    )]
    pub arrow_start: Arrow,
    /// Arrow at target end.
    #[serde(
        default = "default_arrow_end",
        skip_serializing_if = "is_default_arrow_end"
    )]
    pub arrow_end: Arrow,
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
    pub label_position: Option<Position>,
    /// Whether edge flows backward (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub is_backward: Option<bool>,
    /// Source-side port attachment (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub source_port: Option<Port>,
    /// Target-side port attachment (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub target_port: Option<Port>,
    /// Label side relative to the edge: `"above"`, `"below"`, or `"center"`.
    ///
    /// Appears at both `layout` and `routed` geometry levels when the engine
    /// has assigned a side. `None` means the engine made no assignment.
    /// Populated from `EdgeLabelGeometry` when label-side assignment runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub label_side: Option<EdgeLabelSide>,
    /// Padded label rectangle in MMDS coordinate space (routed level only).
    ///
    /// Includes `label_padding_x` / `label_padding_y` padding on each side.
    /// Consumers that need the unpadded rectangle can subtract the padding
    /// constants advertised in the profile. Populated from
    /// `EdgeLabelGeometry` when routed label geometry is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub label_rect: Option<Rect>,
}

/// A subgraph in MMDS output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subgraph {
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
    /// Subgraph direction override, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub direction: Option<Direction>,
    /// Subgraph bounding box (routed level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Invisible subgraph (participates in layout, renders no border).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invisible: bool,
    /// IDs of child subgraphs that are concurrent regions (from `--` dividers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concurrent_regions: Vec<String>,
}

fn default_node_shape() -> Shape {
    Shape::Rectangle
}

fn default_stroke() -> Stroke {
    Stroke::Solid
}

fn default_arrow_start() -> Arrow {
    Arrow::None
}

fn default_arrow_end() -> Arrow {
    Arrow::Normal
}

fn default_minlen() -> i32 {
    1
}

fn is_default_node_shape(value: &Shape) -> bool {
    *value == Shape::Rectangle
}

fn is_default_stroke(value: &Stroke) -> bool {
    *value == Stroke::Solid
}

fn is_default_arrow_start(value: &Arrow) -> bool {
    *value == Arrow::None
}

fn is_default_arrow_end(value: &Arrow) -> bool {
    *value == Arrow::Normal
}

fn is_default_minlen(value: &i32) -> bool {
    *value == 1
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::graph::geometry::LayoutEdge;
    use crate::graph::space::{FPoint, FRect};
    use crate::graph::{Edge as GraphEdge, Node as GraphNode};
    use crate::internal_tests::stub_metrics::WideMProvider;

    fn labeled_graph_geometry() -> (Graph, GraphGeometry) {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(GraphNode::new("A"));
        diagram.add_node(GraphNode::new("B"));
        diagram.add_edge(GraphEdge::new("A", "B").with_label("mmmm"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".to_string(),
            PositionedNode {
                id: "A".to_string(),
                rect: FRect::new(40.0, 20.0, 50.0, 30.0),
                shape: Shape::Rectangle,
                label: "A".to_string(),
                parent: None,
            },
        );
        nodes.insert(
            "B".to_string(),
            PositionedNode {
                id: "B".to_string(),
                rect: FRect::new(40.0, 110.0, 50.0, 30.0),
                shape: Shape::Rectangle,
                label: "B".to_string(),
                parent: None,
            },
        );

        let geometry = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".to_string(),
                to: "B".to_string(),
                waypoints: vec![],
                label_position: Some(FPoint::new(65.0, 80.0)),
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: Some(vec![FPoint::new(65.0, 50.0), FPoint::new(65.0, 110.0)]),
                preserve_orthogonal_topology: false,
                label_geometry: None,
                effective_wrapped_lines: None,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 130.0, 160.0),
            reversed_edges: vec![],
            engine_hints: None,
            grid_projection: None,
            rerouted_edges: HashSet::new(),
            enhanced_backward_routing: false,
        };

        (diagram, geometry)
    }

    #[test]
    fn mmds_fallback_routing_uses_supplied_provider_when_available() {
        let provider = WideMProvider;
        let (diagram, geometry) = labeled_graph_geometry();

        let document = to_document_typed_with_routing_and_text_metrics(
            "flowchart",
            &diagram,
            &geometry,
            None,
            GeometryLevel::Routed,
            PathSimplification::None,
            None,
            None,
            Some(&provider),
            None,
        )
        .expect("fallback routed document should serialize");

        let label_rect = document.edges[0]
            .label_rect
            .as_ref()
            .expect("fallback routing should populate label_rect");
        assert!(
            label_rect.width > 160.0,
            "label_rect width should come from supplied provider, got {}",
            label_rect.width
        );
    }

    #[test]
    fn mmds_port_serializes_correctly() {
        let port = Port {
            face: PortFace::Bottom,
            fraction: 0.5,
            position: Position { x: 50.0, y: 35.0 },
            group_size: 1,
        };
        let json = serde_json::to_string(&port).unwrap();
        assert!(json.contains("\"face\":\"bottom\""));
        assert!(json.contains("\"fraction\":0.5"));
        assert!(json.contains("\"group_size\":1"));
    }

    #[test]
    fn mmds_edge_source_port_none_omitted_from_json() {
        let edge = Edge {
            id: "e0".into(),
            source: "A".into(),
            target: "B".into(),
            from_subgraph: None,
            to_subgraph: None,
            label: None,
            stroke: Stroke::Solid,
            arrow_start: Arrow::None,
            arrow_end: Arrow::Normal,
            minlen: 1,
            path: None,
            label_position: None,
            is_backward: None,
            source_port: None,
            target_port: None,
            label_side: None,
            label_rect: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        assert!(!json.contains("source_port"));
        assert!(!json.contains("target_port"));
    }

    #[test]
    fn mmds_edge_source_port_round_trips() {
        let port = Port {
            face: PortFace::Right,
            fraction: 0.3,
            position: Position { x: 100.0, y: 30.0 },
            group_size: 2,
        };
        let edge = Edge {
            id: "e0".into(),
            source: "A".into(),
            target: "B".into(),
            from_subgraph: None,
            to_subgraph: None,
            label: None,
            stroke: Stroke::Solid,
            arrow_start: Arrow::None,
            arrow_end: Arrow::Normal,
            minlen: 1,
            path: None,
            label_position: None,
            is_backward: None,
            source_port: Some(port),
            target_port: None,
            label_side: None,
            label_rect: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: Edge = serde_json::from_str(&json).unwrap();
        let sp = deserialized.source_port.unwrap();
        assert_eq!(sp.face, PortFace::Right);
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
        let edge: Edge = serde_json::from_str(json).unwrap();
        assert!(edge.source_port.is_none());
        assert!(edge.target_port.is_none());
    }
}
