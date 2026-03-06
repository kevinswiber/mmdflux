//! MMDS hydration and validation.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use serde_json::{Map, Value};

use crate::diagram::EdgeRouting;
use crate::diagrams::flowchart::geometry::{
    FPoint, FRect, GraphGeometry, LayoutEdge, PositionedNode, RoutedGraphGeometry,
    SelfEdgeGeometry, SubgraphGeometry,
};
use crate::diagrams::flowchart::render::route_policy::build_node_directions;
use crate::diagrams::flowchart::routing::route_graph_geometry;
use crate::graph::{Arrow, Diagram, Direction, Edge, Node, Shape, Stroke, Subgraph};
use crate::mmds::{
    MMDS_NODE_STYLE_EXTENSION_NAMESPACE, MMDS_NODE_STYLE_PROFILE, MmdsEdge, MmdsOutput,
};
use crate::style::{ColorToken, NodeStyle};

const SUPPORTED_MMDS_PROFILES: &[&str] = &[
    "mmds-core-v1",
    "mmdflux-svg-v1",
    "mmdflux-text-v1",
    MMDS_NODE_STYLE_PROFILE,
];

/// Placeholder hydration entrypoint for future MMDS input work.
pub fn stub_hydrate() {}

/// Parse MMDS JSON input into the typed output envelope.
///
/// Unlike a plain deserialize, this expands omitted node/edge fields using
/// the top-level `defaults` block before constructing `MmdsOutput`.
pub fn parse_mmds_input(input: &str) -> Result<MmdsOutput, MmdsParseError> {
    let mut value: Value = serde_json::from_str(input).map_err(|err| MmdsParseError {
        message: format!("MMDS parse error: {err}"),
    })?;

    expand_defaults_in_value(&mut value)?;

    serde_json::from_value::<MmdsOutput>(value).map_err(|err| MmdsParseError {
        message: format!("MMDS parse error: {err}"),
    })
}

/// Hydrate a graph `Diagram` from MMDS JSON text.
pub fn from_mmds_str(input: &str) -> Result<Diagram, MmdsHydrationError> {
    let output = parse_mmds_input(input).map_err(|err| MmdsHydrationError::Parse {
        message: err.to_string(),
    })?;
    from_mmds_output(&output)
}

/// Result of profile capability evaluation for a parsed MMDS payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmdsProfileNegotiation {
    /// Profiles recognized by the current runtime.
    pub supported: Vec<String>,
    /// Profiles declared by payload but unknown to this runtime.
    pub unknown: Vec<String>,
}

/// Evaluate declared profiles against runtime-known profile vocabulary.
///
/// This helper is advisory. Hydration remains permissive with unknown profiles.
pub fn evaluate_mmds_profiles(input: &str) -> Result<MmdsProfileNegotiation, MmdsParseError> {
    let output = parse_mmds_input(input)?;
    Ok(evaluate_mmds_profiles_for_output(&output))
}

/// Evaluate declared profiles for an already-parsed MMDS payload.
pub fn evaluate_mmds_profiles_for_output(output: &MmdsOutput) -> MmdsProfileNegotiation {
    let mut supported = Vec::new();
    let mut unknown = Vec::new();
    let mut seen_supported = std::collections::HashSet::new();
    let mut seen_unknown = std::collections::HashSet::new();

    for profile in &output.profiles {
        if is_supported_profile(profile) {
            if seen_supported.insert(profile.clone()) {
                supported.push(profile.clone());
            }
            continue;
        }

        if seen_unknown.insert(profile.clone()) {
            unknown.push(profile.clone());
        }
    }

    MmdsProfileNegotiation { supported, unknown }
}

/// Hydrate a graph `Diagram` from a parsed MMDS envelope.
pub fn from_mmds_output(output: &MmdsOutput) -> Result<Diagram, MmdsHydrationError> {
    validate_output(output)?;

    let direction = parse_direction(&output.metadata.direction).ok_or_else(|| {
        MmdsHydrationError::InvalidDirection {
            context: "metadata.direction".to_string(),
            value: output.metadata.direction.clone(),
        }
    })?;
    let mut diagram = Diagram::new(direction);

    for (index, subgraph) in output.subgraphs.iter().enumerate() {
        if subgraph.id.trim().is_empty() {
            return Err(MmdsHydrationError::MissingSubgraphId { index });
        }
        let dir = if let Some(direction) = &subgraph.direction {
            Some(parse_direction(direction).ok_or_else(|| {
                MmdsHydrationError::InvalidDirection {
                    context: format!("subgraph {} direction", subgraph.id),
                    value: direction.clone(),
                }
            })?)
        } else {
            None
        };
        diagram.subgraphs.insert(
            subgraph.id.clone(),
            Subgraph {
                id: subgraph.id.clone(),
                title: subgraph.title.clone(),
                nodes: subgraph.children.clone(),
                parent: subgraph.parent.clone(),
                dir,
            },
        );
        diagram.subgraph_order.push(subgraph.id.clone());
    }

    for (index, node) in output.nodes.iter().enumerate() {
        if node.id.trim().is_empty() {
            return Err(MmdsHydrationError::MissingNodeId { index });
        }
        let shape = parse_shape(&node.shape).ok_or_else(|| MmdsHydrationError::InvalidShape {
            node_id: node.id.clone(),
            value: node.shape.clone(),
        })?;

        let mut hydrated = Node::new(node.id.clone())
            .with_label(node.label.clone())
            .with_shape(shape);
        hydrated.parent = node.parent.clone();
        diagram.add_node(hydrated);
    }

    hydrate_node_style_extension(&mut diagram, &output.extensions);

    for node in diagram.nodes.values() {
        if let Some(parent) = &node.parent
            && !diagram.subgraphs.contains_key(parent)
        {
            return Err(MmdsHydrationError::DanglingNodeParent {
                node_id: node.id.clone(),
                parent: parent.clone(),
            });
        }
    }

    for subgraph in diagram.subgraphs.values() {
        if let Some(parent) = &subgraph.parent
            && !diagram.subgraphs.contains_key(parent)
        {
            return Err(MmdsHydrationError::DanglingSubgraphParent {
                subgraph_id: subgraph.id.clone(),
                parent: parent.clone(),
            });
        }

        for child in &subgraph.nodes {
            if !diagram.nodes.contains_key(child) {
                return Err(MmdsHydrationError::DanglingSubgraphChild {
                    subgraph_id: subgraph.id.clone(),
                    child: child.clone(),
                });
            }
        }
    }

    for subgraph_id in diagram.subgraphs.keys() {
        let mut seen = std::collections::HashSet::new();
        let mut current = subgraph_id.as_str();
        while let Some(parent) = diagram
            .subgraphs
            .get(current)
            .and_then(|subgraph| subgraph.parent.as_deref())
        {
            if !seen.insert(current) {
                return Err(MmdsHydrationError::CyclicSubgraphParentChain {
                    subgraph_id: subgraph_id.clone(),
                });
            }
            current = parent;
        }
    }

    reconstruct_compound_membership(&mut diagram);

    let edges = sorted_output_edges(output);

    for (index, edge) in edges {
        if edge.id.trim().is_empty() {
            return Err(MmdsHydrationError::MissingEdgeId { index });
        }
        if edge.source.trim().is_empty() {
            return Err(MmdsHydrationError::MissingEdgeSource {
                edge_id: edge.id.clone(),
            });
        }
        if edge.target.trim().is_empty() {
            return Err(MmdsHydrationError::MissingEdgeTarget {
                edge_id: edge.id.clone(),
            });
        }

        if !diagram.nodes.contains_key(&edge.source) {
            return Err(MmdsHydrationError::DanglingEdgeSource {
                edge_id: edge.id.clone(),
                source: edge.source.clone(),
            });
        }
        if !diagram.nodes.contains_key(&edge.target) {
            return Err(MmdsHydrationError::DanglingEdgeTarget {
                edge_id: edge.id.clone(),
                target: edge.target.clone(),
            });
        }
        if let Some(from_subgraph) = &edge.from_subgraph
            && !diagram.subgraphs.contains_key(from_subgraph)
        {
            return Err(MmdsHydrationError::DanglingEdgeFromSubgraphIntent {
                edge_id: edge.id.clone(),
                subgraph: from_subgraph.clone(),
            });
        }
        if let Some(to_subgraph) = &edge.to_subgraph
            && !diagram.subgraphs.contains_key(to_subgraph)
        {
            return Err(MmdsHydrationError::DanglingEdgeToSubgraphIntent {
                edge_id: edge.id.clone(),
                subgraph: to_subgraph.clone(),
            });
        }

        let stroke =
            parse_stroke(&edge.stroke).ok_or_else(|| MmdsHydrationError::InvalidStroke {
                edge_id: edge.id.clone(),
                value: edge.stroke.clone(),
            })?;
        let arrow_start =
            parse_arrow(&edge.arrow_start).ok_or_else(|| MmdsHydrationError::InvalidArrow {
                edge_id: edge.id.clone(),
                endpoint: "start".to_string(),
                value: edge.arrow_start.clone(),
            })?;
        let arrow_end =
            parse_arrow(&edge.arrow_end).ok_or_else(|| MmdsHydrationError::InvalidArrow {
                edge_id: edge.id.clone(),
                endpoint: "end".to_string(),
                value: edge.arrow_end.clone(),
            })?;

        let mut hydrated = Edge::new(edge.source.clone(), edge.target.clone())
            .with_stroke(stroke)
            .with_arrows(arrow_start, arrow_end)
            .with_minlen(edge.minlen);
        if let Some(label) = &edge.label {
            hydrated = hydrated.with_label(label.clone());
        }
        hydrated.from_subgraph = edge.from_subgraph.clone();
        hydrated.to_subgraph = edge.to_subgraph.clone();
        diagram.add_edge(hydrated);
    }

    Ok(diagram)
}

fn hydrate_node_style_extension(
    diagram: &mut Diagram,
    extensions: &std::collections::BTreeMap<String, Map<String, Value>>,
) {
    let Some(extension) = extensions.get(MMDS_NODE_STYLE_EXTENSION_NAMESPACE) else {
        return;
    };
    let Some(nodes) = extension.get("nodes").and_then(Value::as_object) else {
        return;
    };

    for (node_id, raw_style) in nodes {
        let Some(style_object) = raw_style.as_object() else {
            continue;
        };
        let Some(node) = diagram.nodes.get_mut(node_id) else {
            continue;
        };

        let style = parse_node_style_extension(style_object);
        if !style.is_empty() {
            node.style = node.style.merge(&style);
        }
    }
}

fn parse_node_style_extension(style_object: &Map<String, Value>) -> NodeStyle {
    NodeStyle {
        fill: parse_node_style_color(style_object, "fill"),
        stroke: parse_node_style_color(style_object, "stroke"),
        color: parse_node_style_color(style_object, "color"),
    }
}

fn parse_node_style_color(style_object: &Map<String, Value>, key: &str) -> Option<ColorToken> {
    let raw = style_object.get(key)?.as_str()?;
    ColorToken::parse(raw).ok()
}

fn reconstruct_compound_membership(diagram: &mut Diagram) {
    let mut node_ids: Vec<&String> = diagram.nodes.keys().collect();
    node_ids.sort();

    let memberships: Vec<(String, Vec<String>)> = node_ids
        .into_iter()
        .map(|node_id| {
            let mut ancestors = Vec::new();
            let mut current = diagram
                .nodes
                .get(node_id)
                .and_then(|node| node.parent.as_deref());
            while let Some(parent) = current {
                ancestors.push(parent.to_string());
                current = diagram
                    .subgraphs
                    .get(parent)
                    .and_then(|subgraph| subgraph.parent.as_deref());
            }
            (node_id.clone(), ancestors)
        })
        .collect();

    for subgraph in diagram.subgraphs.values_mut() {
        subgraph.nodes.clear();
    }

    for (node_id, ancestors) in memberships {
        for subgraph_id in ancestors {
            if let Some(subgraph) = diagram.subgraphs.get_mut(&subgraph_id) {
                subgraph.nodes.push(node_id.clone());
            }
        }
    }
}

/// Hydrate graph geometry IR from MMDS JSON text.
pub fn hydrate_graph_geometry_from_mmds(input: &str) -> Result<GraphGeometry, MmdsHydrationError> {
    let output = parse_mmds_input(input).map_err(|err| MmdsHydrationError::Parse {
        message: err.to_string(),
    })?;
    hydrate_graph_geometry_from_output(&output)
}

/// Hydrate graph geometry IR from parsed MMDS output.
pub fn hydrate_graph_geometry_from_output(
    output: &MmdsOutput,
) -> Result<GraphGeometry, MmdsHydrationError> {
    let (_, geometry) = hydrate_geometry_parts(output)?;
    Ok(geometry)
}

/// Hydrate graph geometry IR from parsed MMDS output, using a pre-built diagram.
pub fn hydrate_graph_geometry_from_output_with_diagram(
    output: &MmdsOutput,
    diagram: &Diagram,
) -> Result<GraphGeometry, MmdsHydrationError> {
    validate_output(output)?;
    build_graph_geometry(output, diagram)
}

/// Hydrate routed geometry IR from MMDS JSON text.
pub fn hydrate_routed_geometry_from_mmds(
    input: &str,
) -> Result<RoutedGraphGeometry, MmdsHydrationError> {
    let output = parse_mmds_input(input).map_err(|err| MmdsHydrationError::Parse {
        message: err.to_string(),
    })?;
    hydrate_routed_geometry_from_output(&output)
}

/// Hydrate routed geometry IR from parsed MMDS output.
pub fn hydrate_routed_geometry_from_output(
    output: &MmdsOutput,
) -> Result<RoutedGraphGeometry, MmdsHydrationError> {
    let (diagram, geometry) = hydrate_geometry_parts(output)?;
    let edge_routing = if output.geometry_level == "routed" {
        EdgeRouting::EngineProvided
    } else {
        EdgeRouting::PolylineRoute
    };
    Ok(route_graph_geometry(&diagram, &geometry, edge_routing))
}

fn hydrate_geometry_parts(
    output: &MmdsOutput,
) -> Result<(Diagram, GraphGeometry), MmdsHydrationError> {
    let diagram = from_mmds_output(output)?;
    let geometry = build_graph_geometry(output, &diagram)?;
    Ok((diagram, geometry))
}

fn build_graph_geometry(
    output: &MmdsOutput,
    diagram: &Diagram,
) -> Result<GraphGeometry, MmdsHydrationError> {
    let nodes = build_positioned_nodes(output, diagram)?;
    let (edges, self_edges, reversed_edges) = build_layout_edges(output);
    let subgraphs = build_subgraph_geometry(output, diagram, &nodes);

    Ok(GraphGeometry {
        nodes,
        edges,
        subgraphs,
        self_edges,
        direction: diagram.direction,
        node_directions: build_node_directions(diagram),
        bounds: FRect::new(
            0.0,
            0.0,
            output.metadata.bounds.width,
            output.metadata.bounds.height,
        ),
        reversed_edges,
        engine_hints: None,
        rerouted_edges: std::collections::HashSet::new(),
        enhanced_backward_routing: false,
    })
}

fn build_positioned_nodes(
    output: &MmdsOutput,
    diagram: &Diagram,
) -> Result<HashMap<String, PositionedNode>, MmdsHydrationError> {
    output
        .nodes
        .iter()
        .map(|node| {
            let hydrated = diagram.nodes.get(&node.id).ok_or_else(|| {
                MmdsHydrationError::MissingGeometryNode {
                    node_id: node.id.clone(),
                }
            })?;
            // MMDS position is node center; FRect uses top-left origin
            let left = node.position.x - node.size.width / 2.0;
            let top = node.position.y - node.size.height / 2.0;
            Ok((
                node.id.clone(),
                PositionedNode {
                    id: node.id.clone(),
                    rect: FRect::new(left, top, node.size.width, node.size.height),
                    shape: hydrated.shape,
                    label: hydrated.label.clone(),
                    parent: hydrated.parent.clone(),
                },
            ))
        })
        .collect()
}

fn build_layout_edges(output: &MmdsOutput) -> (Vec<LayoutEdge>, Vec<SelfEdgeGeometry>, Vec<usize>) {
    let routed_level = output.geometry_level == "routed";
    let edges = sorted_output_edges(output);

    let mut layout_edges = Vec::with_capacity(edges.len());
    let mut self_edges = Vec::new();
    let mut reversed_edges = Vec::new();

    for (index, (_, edge)) in edges.into_iter().enumerate() {
        let mut path = routed_level
            .then(|| parse_path_points(edge.path.as_deref()))
            .flatten();
        if edge.source == edge.target
            && let Some(points) = path.take()
        {
            self_edges.push(SelfEdgeGeometry {
                node_id: edge.source.clone(),
                edge_index: index,
                points,
            });
        }

        if routed_level && edge.is_backward.unwrap_or(false) {
            reversed_edges.push(index);
        }

        let label_position = if routed_level {
            edge.label_position
                .as_ref()
                .map(|position| FPoint::new(position.x, position.y))
        } else {
            None
        };

        layout_edges.push(LayoutEdge {
            index,
            from: edge.source.clone(),
            to: edge.target.clone(),
            waypoints: Vec::new(),
            label_position,
            label_side: None,
            from_subgraph: edge.from_subgraph.clone(),
            to_subgraph: edge.to_subgraph.clone(),
            layout_path_hint: path,
        });
    }

    (layout_edges, self_edges, reversed_edges)
}

fn sorted_output_edges(output: &MmdsOutput) -> Vec<(usize, &MmdsEdge)> {
    let mut edges: Vec<(usize, &MmdsEdge)> = output.edges.iter().enumerate().collect();
    edges.sort_by(|(left_index, left), (right_index, right)| {
        compare_edge_ids(&left.id, &right.id).then(left_index.cmp(right_index))
    });
    edges
}

fn parse_path_points(path: Option<&[[f64; 2]]>) -> Option<Vec<FPoint>> {
    path.map(|points| points.iter().map(|[x, y]| FPoint::new(*x, *y)).collect())
}

fn build_subgraph_geometry(
    output: &MmdsOutput,
    diagram: &Diagram,
    nodes: &HashMap<String, PositionedNode>,
) -> HashMap<String, SubgraphGeometry> {
    output
        .subgraphs
        .iter()
        .map(|subgraph| {
            let (center_x, center_y, fallback_width, fallback_height) =
                derive_subgraph_center_and_extent(&subgraph.id, diagram, nodes);

            let width = subgraph
                .bounds
                .as_ref()
                .map_or(fallback_width, |bounds| bounds.width);
            let height = subgraph
                .bounds
                .as_ref()
                .map_or(fallback_height, |bounds| bounds.height);

            (
                subgraph.id.clone(),
                SubgraphGeometry {
                    id: subgraph.id.clone(),
                    rect: FRect::new(center_x, center_y, width, height),
                    title: subgraph.title.clone(),
                    depth: diagram.subgraph_depth(&subgraph.id),
                },
            )
        })
        .collect()
}

fn derive_subgraph_center_and_extent(
    subgraph_id: &str,
    diagram: &Diagram,
    nodes: &HashMap<String, PositionedNode>,
) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for node in diagram.nodes.values() {
        if !node_is_within_subgraph(node, subgraph_id, diagram) {
            continue;
        }
        let Some(placed) = nodes.get(&node.id) else {
            continue;
        };
        let left = placed.rect.x - placed.rect.width / 2.0;
        let right = placed.rect.x + placed.rect.width / 2.0;
        let top = placed.rect.y - placed.rect.height / 2.0;
        let bottom = placed.rect.y + placed.rect.height / 2.0;
        min_x = min_x.min(left);
        max_x = max_x.max(right);
        min_y = min_y.min(top);
        max_y = max_y.max(bottom);
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        let width = (max_x - min_x).max(0.0);
        let height = (max_y - min_y).max(0.0);
        let center_x = min_x + width / 2.0;
        let center_y = min_y + height / 2.0;
        (center_x, center_y, width, height)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    }
}

fn node_is_within_subgraph(node: &Node, subgraph_id: &str, diagram: &Diagram) -> bool {
    let mut current = node.parent.as_deref();
    while let Some(parent) = current {
        if parent == subgraph_id {
            return true;
        }
        current = diagram
            .subgraphs
            .get(parent)
            .and_then(|subgraph| subgraph.parent.as_deref());
    }
    false
}

fn validate_output(output: &MmdsOutput) -> Result<(), MmdsHydrationError> {
    if output.version != 1 {
        return Err(MmdsHydrationError::UnsupportedVersion {
            version: output.version,
        });
    }

    if !matches!(output.geometry_level.as_str(), "layout" | "routed") {
        return Err(MmdsHydrationError::InvalidGeometryLevel {
            value: output.geometry_level.clone(),
        });
    }

    if !matches!(output.metadata.diagram_type.as_str(), "flowchart" | "class") {
        return Err(MmdsHydrationError::UnsupportedDiagramType {
            value: output.metadata.diagram_type.clone(),
        });
    }

    Ok(())
}

fn is_supported_profile(profile: &str) -> bool {
    SUPPORTED_MMDS_PROFILES.contains(&profile)
}

fn compare_edge_ids(left: &str, right: &str) -> Ordering {
    let left_number = parse_edge_index(left);
    let right_number = parse_edge_index(right);

    match (left_number, right_number) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

fn parse_edge_index(value: &str) -> Option<u64> {
    value.strip_prefix('e')?.parse::<u64>().ok()
}

fn expand_defaults_in_value(value: &mut Value) -> Result<(), MmdsParseError> {
    let root = value.as_object_mut().ok_or_else(|| MmdsParseError {
        message: "MMDS parse error: top-level JSON value must be an object".to_string(),
    })?;

    let node_shape = default_string(
        root,
        &["defaults", "node", "shape"],
        Value::String("rectangle".to_string()),
    );
    let edge_stroke = default_string(
        root,
        &["defaults", "edge", "stroke"],
        Value::String("solid".to_string()),
    );
    let edge_arrow_start = default_string(
        root,
        &["defaults", "edge", "arrow_start"],
        Value::String("none".to_string()),
    );
    let edge_arrow_end = default_string(
        root,
        &["defaults", "edge", "arrow_end"],
        Value::String("normal".to_string()),
    );
    let edge_minlen = default_number(root, &["defaults", "edge", "minlen"], Value::from(1));

    if let Some(nodes) = root.get_mut("nodes").and_then(Value::as_array_mut) {
        for node in nodes {
            if let Some(node_obj) = node.as_object_mut() {
                node_obj
                    .entry("shape".to_string())
                    .or_insert_with(|| node_shape.clone());
            }
        }
    }

    if let Some(edges) = root.get_mut("edges").and_then(Value::as_array_mut) {
        for edge in edges {
            if let Some(edge_obj) = edge.as_object_mut() {
                edge_obj
                    .entry("stroke".to_string())
                    .or_insert_with(|| edge_stroke.clone());
                edge_obj
                    .entry("arrow_start".to_string())
                    .or_insert_with(|| edge_arrow_start.clone());
                edge_obj
                    .entry("arrow_end".to_string())
                    .or_insert_with(|| edge_arrow_end.clone());
                edge_obj
                    .entry("minlen".to_string())
                    .or_insert_with(|| edge_minlen.clone());
            }
        }
    }

    Ok(())
}

fn default_string(root: &Map<String, Value>, path: &[&str], fallback: Value) -> Value {
    traverse_value(root, path).cloned().unwrap_or(fallback)
}

fn default_number(root: &Map<String, Value>, path: &[&str], fallback: Value) -> Value {
    traverse_value(root, path).cloned().unwrap_or(fallback)
}

fn traverse_value<'a>(root: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = root.get(*first)?;
    for key in rest {
        current = current.get(*key)?;
    }
    Some(current)
}

fn parse_direction(value: &str) -> Option<Direction> {
    match value {
        "TD" => Some(Direction::TopDown),
        "BT" => Some(Direction::BottomTop),
        "LR" => Some(Direction::LeftRight),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

fn parse_shape(value: &str) -> Option<Shape> {
    match value {
        "rectangle" => Some(Shape::Rectangle),
        "round" => Some(Shape::Round),
        "stadium" => Some(Shape::Stadium),
        "subroutine" => Some(Shape::Subroutine),
        "cylinder" => Some(Shape::Cylinder),
        "document" => Some(Shape::Document),
        "documents" => Some(Shape::Documents),
        "tagged_document" => Some(Shape::TaggedDocument),
        "card" => Some(Shape::Card),
        "tagged_rect" => Some(Shape::TaggedRect),
        "diamond" => Some(Shape::Diamond),
        "hexagon" => Some(Shape::Hexagon),
        "trapezoid" => Some(Shape::Trapezoid),
        "inv_trapezoid" => Some(Shape::InvTrapezoid),
        "parallelogram" => Some(Shape::Parallelogram),
        "inv_parallelogram" => Some(Shape::InvParallelogram),
        "manual_input" => Some(Shape::ManualInput),
        "asymmetric" => Some(Shape::Asymmetric),
        "circle" => Some(Shape::Circle),
        "double_circle" => Some(Shape::DoubleCircle),
        "small_circle" => Some(Shape::SmallCircle),
        "framed_circle" => Some(Shape::FramedCircle),
        "crossed_circle" => Some(Shape::CrossedCircle),
        "text_block" => Some(Shape::TextBlock),
        "fork_join" => Some(Shape::ForkJoin),
        _ => None,
    }
}

fn parse_stroke(value: &str) -> Option<Stroke> {
    match value {
        "solid" => Some(Stroke::Solid),
        "dotted" => Some(Stroke::Dotted),
        "thick" => Some(Stroke::Thick),
        "invisible" => Some(Stroke::Invisible),
        _ => None,
    }
}

fn parse_arrow(value: &str) -> Option<Arrow> {
    match value {
        "normal" => Some(Arrow::Normal),
        "none" => Some(Arrow::None),
        "cross" => Some(Arrow::Cross),
        "circle" => Some(Arrow::Circle),
        "open_triangle" => Some(Arrow::OpenTriangle),
        "diamond" => Some(Arrow::Diamond),
        "open_diamond" => Some(Arrow::OpenDiamond),
        _ => None,
    }
}

/// Parse-time error for MMDS input.
#[derive(Debug, Clone)]
pub struct MmdsParseError {
    message: String,
}

impl fmt::Display for MmdsParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for MmdsParseError {}

/// MMDS hydration and validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MmdsHydrationError {
    Parse {
        message: String,
    },
    UnsupportedVersion {
        version: u32,
    },
    UnsupportedDiagramType {
        value: String,
    },
    InvalidGeometryLevel {
        value: String,
    },
    InvalidDirection {
        context: String,
        value: String,
    },
    InvalidShape {
        node_id: String,
        value: String,
    },
    InvalidStroke {
        edge_id: String,
        value: String,
    },
    InvalidArrow {
        edge_id: String,
        endpoint: String,
        value: String,
    },
    MissingNodeId {
        index: usize,
    },
    MissingGeometryNode {
        node_id: String,
    },
    MissingSubgraphId {
        index: usize,
    },
    MissingEdgeId {
        index: usize,
    },
    MissingEdgeSource {
        edge_id: String,
    },
    MissingEdgeTarget {
        edge_id: String,
    },
    DanglingEdgeSource {
        edge_id: String,
        source: String,
    },
    DanglingEdgeTarget {
        edge_id: String,
        target: String,
    },
    DanglingEdgeFromSubgraphIntent {
        edge_id: String,
        subgraph: String,
    },
    DanglingEdgeToSubgraphIntent {
        edge_id: String,
        subgraph: String,
    },
    DanglingNodeParent {
        node_id: String,
        parent: String,
    },
    DanglingSubgraphParent {
        subgraph_id: String,
        parent: String,
    },
    DanglingSubgraphChild {
        subgraph_id: String,
        child: String,
    },
    CyclicSubgraphParentChain {
        subgraph_id: String,
    },
}

impl fmt::Display for MmdsHydrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MmdsHydrationError::Parse { message } => write!(f, "{message}"),
            MmdsHydrationError::UnsupportedVersion { version } => {
                write!(f, "MMDS validation error: unsupported version {version}")
            }
            MmdsHydrationError::UnsupportedDiagramType { value } => {
                write!(
                    f,
                    "MMDS validation error: unsupported diagram_type '{value}'"
                )
            }
            MmdsHydrationError::InvalidGeometryLevel { value } => {
                write!(f, "MMDS validation error: invalid geometry_level '{value}'")
            }
            MmdsHydrationError::InvalidDirection { context, value } => {
                write!(
                    f,
                    "MMDS validation error: invalid direction '{value}' for {context}"
                )
            }
            MmdsHydrationError::InvalidShape { node_id, value } => write!(
                f,
                "MMDS validation error: node {node_id} has invalid shape '{value}'"
            ),
            MmdsHydrationError::InvalidStroke { edge_id, value } => write!(
                f,
                "MMDS validation error: edge {edge_id} has invalid stroke '{value}'"
            ),
            MmdsHydrationError::InvalidArrow {
                edge_id,
                endpoint,
                value,
            } => write!(
                f,
                "MMDS validation error: edge {edge_id} has invalid {endpoint} arrow '{value}'"
            ),
            MmdsHydrationError::MissingNodeId { index } => {
                write!(
                    f,
                    "MMDS validation error: node at index {index} is missing id"
                )
            }
            MmdsHydrationError::MissingGeometryNode { node_id } => write!(
                f,
                "MMDS validation error: geometry node '{node_id}' not found"
            ),
            MmdsHydrationError::MissingSubgraphId { index } => write!(
                f,
                "MMDS validation error: subgraph at index {index} is missing id"
            ),
            MmdsHydrationError::MissingEdgeId { index } => {
                write!(
                    f,
                    "MMDS validation error: edge at index {index} is missing id"
                )
            }
            MmdsHydrationError::MissingEdgeSource { edge_id } => {
                write!(f, "MMDS validation error: edge {edge_id} is missing source")
            }
            MmdsHydrationError::MissingEdgeTarget { edge_id } => {
                write!(f, "MMDS validation error: edge {edge_id} is missing target")
            }
            MmdsHydrationError::DanglingEdgeSource { edge_id, source } => write!(
                f,
                "MMDS validation error: edge {edge_id} source '{source}' not found"
            ),
            MmdsHydrationError::DanglingEdgeTarget { edge_id, target } => write!(
                f,
                "MMDS validation error: edge {edge_id} target '{target}' not found"
            ),
            MmdsHydrationError::DanglingEdgeFromSubgraphIntent { edge_id, subgraph } => write!(
                f,
                "MMDS validation error: edge {edge_id} from_subgraph '{subgraph}' not found"
            ),
            MmdsHydrationError::DanglingEdgeToSubgraphIntent { edge_id, subgraph } => write!(
                f,
                "MMDS validation error: edge {edge_id} to_subgraph '{subgraph}' not found"
            ),
            MmdsHydrationError::DanglingNodeParent { node_id, parent } => write!(
                f,
                "MMDS validation error: node {node_id} parent subgraph '{parent}' not found"
            ),
            MmdsHydrationError::DanglingSubgraphParent {
                subgraph_id,
                parent,
            } => write!(
                f,
                "MMDS validation error: subgraph {subgraph_id} parent '{parent}' not found"
            ),
            MmdsHydrationError::DanglingSubgraphChild { subgraph_id, child } => write!(
                f,
                "MMDS validation error: subgraph {subgraph_id} child '{child}' not found"
            ),
            MmdsHydrationError::CyclicSubgraphParentChain { subgraph_id } => write!(
                f,
                "MMDS validation error: cyclic subgraph parent chain detected at '{subgraph_id}'"
            ),
        }
    }
}

impl Error for MmdsHydrationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstruct_compound_membership_includes_descendants_for_ancestors() {
        let mut diagram = Diagram::new(Direction::TopDown);

        diagram.subgraphs.insert(
            "outer".to_string(),
            Subgraph {
                id: "outer".to_string(),
                title: "Outer".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );
        diagram.subgraphs.insert(
            "inner".to_string(),
            Subgraph {
                id: "inner".to_string(),
                title: "Inner".to_string(),
                nodes: vec!["B".to_string()],
                parent: Some("outer".to_string()),
                dir: None,
            },
        );

        let mut a = Node::new("A");
        a.parent = Some("outer".to_string());
        diagram.add_node(a);

        let mut b = Node::new("B");
        b.parent = Some("inner".to_string());
        diagram.add_node(b);

        reconstruct_compound_membership(&mut diagram);

        assert_eq!(
            diagram.subgraphs["outer"].nodes,
            vec!["A".to_string(), "B".to_string()]
        );
        assert_eq!(diagram.subgraphs["inner"].nodes, vec!["B".to_string()]);
    }
}
