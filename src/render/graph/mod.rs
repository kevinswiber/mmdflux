//! Render-only graph-family emission APIs.
//!
//! `render::graph` exposes narrow, geometry-based rendering entrypoints for
//! callers that already have `GraphGeometry` or `RoutedGraphGeometry`.
//! Solve orchestration remains owned by the runtime facade and graph engines.
//!
//! Low-level text drawing lives under [`crate::render::graph::text`].
//!
//! Internally, graph render emission consumes graph-owned float and grid
//! geometry helpers exposed through `crate::graph`.

pub(crate) mod svg;
pub mod text;

pub use self::svg::SvgRenderOptions;
use crate::format::{OutputFormat, RoutingStyle, TextColorMode};
use crate::graph::direction_policy::build_node_directions;
use crate::graph::geometry::{GraphGeometry, LayoutEdge, RoutedGraphGeometry, SelfEdgeGeometry};
use crate::graph::routing::{self, EdgeRouting};
use crate::graph::{Direction, Graph};
use crate::simplification::PathSimplification;

pub(crate) fn edge_routing_from_style(routing_style: RoutingStyle) -> EdgeRouting {
    match routing_style {
        RoutingStyle::Direct => EdgeRouting::DirectRoute,
        RoutingStyle::Polyline => EdgeRouting::PolylineRoute,
        RoutingStyle::Orthogonal => EdgeRouting::OrthogonalRoute,
    }
}

/// Public text render options for render-only geometry emission.
#[derive(Debug, Clone)]
pub struct TextRenderOptions {
    pub output_format: OutputFormat,
    pub text_color_mode: TextColorMode,
    pub routing_style: RoutingStyle,
    pub cluster_ranksep: Option<f64>,
    pub padding: Option<usize>,
    #[allow(dead_code)]
    pub path_simplification: PathSimplification,
}

impl Default for TextRenderOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Text,
            text_color_mode: TextColorMode::Plain,
            routing_style: RoutingStyle::Orthogonal,
            cluster_ranksep: None,
            padding: None,
            path_simplification: PathSimplification::default(),
        }
    }
}

/// Render SVG directly from precomputed graph geometry.
pub fn render_svg_from_geometry(
    diagram: &Graph,
    geometry: &GraphGeometry,
    options: &SvgRenderOptions,
) -> String {
    render_svg_from_geometry_with_routing(
        diagram,
        geometry,
        options,
        edge_routing_from_style(options.routing_style),
    )
}

/// Render SVG directly from precomputed routed graph geometry.
///
/// Routed geometry owns the edge path topology, so SVG emission uses the
/// provided routed paths directly instead of generating routes from style.
pub fn render_svg_from_routed_geometry(
    diagram: &Graph,
    routed: &RoutedGraphGeometry,
    options: &SvgRenderOptions,
) -> String {
    let geometry = geometry_for_routed_svg(diagram, routed);
    render_svg_from_geometry_with_routing(diagram, &geometry, options, EdgeRouting::EngineProvided)
}

pub(crate) fn render_svg_from_geometry_with_routing(
    diagram: &Graph,
    geometry: &GraphGeometry,
    options: &SvgRenderOptions,
    edge_routing: EdgeRouting,
) -> String {
    svg::render_svg_from_geometry(diagram, options, geometry, edge_routing)
}

fn geometry_for_routed_svg(diagram: &Graph, routed: &RoutedGraphGeometry) -> GraphGeometry {
    GraphGeometry {
        nodes: routed.nodes.clone(),
        edges: routed
            .edges
            .iter()
            .map(|edge| LayoutEdge {
                index: edge.index,
                from: edge.from.clone(),
                to: edge.to.clone(),
                waypoints: vec![],
                label_position: edge.label_position,
                label_side: edge.label_side,
                from_subgraph: edge.from_subgraph.clone(),
                to_subgraph: edge.to_subgraph.clone(),
                layout_path_hint: Some(edge.path.clone()),
                preserve_orthogonal_topology: edge.preserve_orthogonal_topology,
            })
            .collect(),
        subgraphs: routed.subgraphs.clone(),
        self_edges: routed
            .self_edges
            .iter()
            .map(|edge| SelfEdgeGeometry {
                node_id: edge.node_id.clone(),
                edge_index: edge.edge_index,
                points: edge.path.clone(),
            })
            .collect(),
        direction: routed.direction,
        node_directions: build_node_directions(diagram),
        bounds: routed.bounds,
        reversed_edges: routed
            .edges
            .iter()
            .filter(|edge| edge.is_backward)
            .map(|edge| edge.index)
            .collect(),
        engine_hints: None,
        grid_projection: None,
        rerouted_edges: std::collections::HashSet::new(),
        enhanced_backward_routing: false,
    }
}

/// Render text or ASCII directly from precomputed graph geometry.
pub fn render_text_from_geometry(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    options: &TextRenderOptions,
) -> String {
    let routed_owned;
    let routed = match routed {
        Some(routed) => routed,
        None => {
            routed_owned = routing::route_graph_geometry(
                diagram,
                geometry,
                edge_routing_from_style(options.routing_style),
            );
            &routed_owned
        }
    };
    let config = layout_config_for_diagram(diagram, options);
    let layout = crate::graph::grid::geometry_to_grid_layout_with_routed(
        diagram,
        geometry,
        Some(routed),
        &config,
    );
    text::render_text_from_grid_layout(diagram, &layout, options)
}

/// Render a diagram to the configured output format.
///
/// # Example
///
/// ```ignore
/// use mmdflux::graph::geometry::{
///     EngineHints, FRect, GraphGeometry, LayeredHints, LayoutEdge, PositionedNode,
/// };
/// use mmdflux::render::graph::{render_text_from_geometry, TextRenderOptions};
/// use mmdflux::{Diagram, Direction, Edge, Node, Shape};
/// use std::collections::{HashMap, HashSet};
///
/// let mut diagram = Diagram::new(Direction::LeftRight);
/// diagram.add_node(Node::new("A"));
/// diagram.add_node(Node::new("B"));
/// diagram.add_edge(Edge::new("A", "B"));
///
/// let geometry = GraphGeometry {
///     nodes: HashMap::from([
///         (
///             "A".to_string(),
///             PositionedNode {
///                 id: "A".to_string(),
///                 rect: FRect::new(0.0, 0.0, 9.0, 3.0),
///                 shape: Shape::Rectangle,
///                 label: "A".to_string(),
///                 parent: None,
///             },
///         ),
///         (
///             "B".to_string(),
///             PositionedNode {
///                 id: "B".to_string(),
///                 rect: FRect::new(20.0, 0.0, 9.0, 3.0),
///                 shape: Shape::Rectangle,
///                 label: "B".to_string(),
///                 parent: None,
///             },
///         ),
///     ]),
///     edges: vec![LayoutEdge {
///         index: 0,
///         from: "A".to_string(),
///         to: "B".to_string(),
///         waypoints: vec![],
///         label_position: None,
///         label_side: None,
///         from_subgraph: None,
///         to_subgraph: None,
///         layout_path_hint: None,
///         preserve_orthogonal_topology: false,
///     }],
///     subgraphs: HashMap::new(),
///     self_edges: vec![],
///     direction: Direction::LeftRight,
///     node_directions: HashMap::from([
///         ("A".to_string(), Direction::LeftRight),
///         ("B".to_string(), Direction::LeftRight),
///     ]),
///     bounds: FRect::new(0.0, 0.0, 30.0, 6.0),
///     reversed_edges: vec![],
///     engine_hints: Some(EngineHints::Layered(LayeredHints {
///         node_ranks: HashMap::from([
///             ("A".to_string(), 0),
///             ("B".to_string(), 1),
///         ]),
///         rank_to_position: HashMap::from([
///             (0, (0.0, 3.0)),
///             (1, (20.0, 23.0)),
///         ]),
///         edge_waypoints: HashMap::new(),
///         label_positions: HashMap::new(),
///     })),
///     grid_projection: None,
///     rerouted_edges: HashSet::new(),
///     enhanced_backward_routing: false,
/// };
///
/// let ascii = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());
/// ```
pub(crate) fn layout_config_for_diagram(
    diagram: &Graph,
    options: &TextRenderOptions,
) -> crate::graph::grid::GridLayoutConfig {
    let mut config = crate::graph::grid::GridLayoutConfig::default();

    let max_label_len = diagram
        .edges
        .iter()
        .filter_map(|e| e.label.as_ref())
        .map(|label| {
            label
                .split('\n')
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    match diagram.direction {
        Direction::LeftRight | Direction::RightLeft => {
            config.h_spacing = config.h_spacing.max(max_label_len + 4);
        }
        Direction::TopDown | Direction::BottomTop => {
            if max_label_len > 0 {
                let (has_branching, left_len, right_len) = branching_label_info(diagram);
                if has_branching {
                    config.v_spacing = config.v_spacing.max(5);
                    config.h_spacing = config.h_spacing.max(left_len.max(right_len) + 4);
                    config.left_label_margin = left_len;
                    config.right_label_margin = right_len;
                } else {
                    config.v_spacing = config.v_spacing.max(3);
                }
            }
        }
    }

    if diagram.has_subgraphs() {
        let max_depth = diagram
            .subgraphs
            .keys()
            .map(|id| diagram.subgraph_depth(id))
            .max()
            .unwrap_or(0);
        if max_depth > 0 {
            config.padding += max_depth * 2;
        }
    }

    if let Some(cluster_ranksep) = options.cluster_ranksep {
        config.cluster_rank_sep = cluster_ranksep;
    }
    if let Some(padding) = options.padding {
        config.padding = padding;
    }

    config
}

fn branching_label_info(diagram: &Graph) -> (bool, usize, usize) {
    let mut labeled_edges_per_source: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for edge in &diagram.edges {
        if let Some(ref label) = edge.label {
            labeled_edges_per_source
                .entry(&edge.from)
                .or_default()
                .push(label);
        }
    }

    let mut has_branching = false;
    let mut max_left = 0;
    let mut max_right = 0;

    for labels in labeled_edges_per_source.values() {
        if labels.len() >= 2 {
            has_branching = true;
            max_left = max_left.max(labels[0].chars().count());
            max_right = max_right.max(
                labels[1..]
                    .iter()
                    .map(|l| l.chars().count())
                    .max()
                    .unwrap_or(0),
            );
        }
    }

    (has_branching, max_left, max_right)
}

// RenderConfig conversion tests live in runtime/config.rs.
