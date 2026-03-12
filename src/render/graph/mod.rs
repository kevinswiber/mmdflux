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
pub(crate) mod svg_metrics;
pub mod text;

use self::svg_metrics::{DEFAULT_FONT_FAMILY, DEFAULT_PROPORTIONAL_FONT_SIZE};
use crate::graph::direction_policy::build_node_directions;
use crate::graph::geometry::{GraphGeometry, LayoutEdge, RoutedGraphGeometry, SelfEdgeGeometry};
use crate::graph::routing::{self, EdgeRouting};
use crate::graph::{Diagram, Direction};
use crate::{
    Curve, EdgePreset, EngineId, OutputFormat, PathSimplification, RenderConfig, RoutingStyle,
    TextColorMode,
};

/// Engine defaults for SVG style (routing + curve).
///
/// When no preset or explicit style is specified, these engine-specific defaults
/// preserve the pre-Phase-7 rendering behaviour.
fn engine_style_defaults(engine: Option<EngineId>) -> (RoutingStyle, Curve) {
    match engine {
        Some(EngineId::Mermaid) => (RoutingStyle::Polyline, Curve::Basis),
        _ => (RoutingStyle::Orthogonal, Curve::Basis),
    }
}

pub(crate) fn edge_routing_from_style(routing_style: RoutingStyle) -> EdgeRouting {
    match routing_style {
        RoutingStyle::Direct => EdgeRouting::DirectRoute,
        RoutingStyle::Polyline => EdgeRouting::PolylineRoute,
        RoutingStyle::Orthogonal => EdgeRouting::OrthogonalRoute,
    }
}

/// Public SVG render options for render-only geometry emission.
#[derive(Debug, Clone)]
pub struct SvgRenderOptions {
    pub scale: f64,
    pub font_family: String,
    pub font_size: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub routing_style: RoutingStyle,
    pub curve: Curve,
    pub edge_radius: f64,
    pub diagram_padding: f64,
    pub path_simplification: PathSimplification,
}

impl Default for SvgRenderOptions {
    fn default() -> Self {
        let font_size = DEFAULT_PROPORTIONAL_FONT_SIZE;
        Self {
            scale: 1.0,
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size,
            node_padding_x: 15.0,
            node_padding_y: 15.0,
            routing_style: RoutingStyle::Orthogonal,
            curve: Curve::Basis,
            edge_radius: 5.0,
            diagram_padding: 8.0,
            path_simplification: PathSimplification::default(),
        }
    }
}

impl From<&RenderConfig> for SvgRenderOptions {
    fn from(config: &RenderConfig) -> Self {
        let mut svg = Self::default();
        if let Some(scale) = config.svg_scale {
            svg.scale = scale;
        }
        if let Some(padding_x) = config.svg_node_padding_x {
            svg.node_padding_x = padding_x;
        }
        if let Some(padding_y) = config.svg_node_padding_y {
            svg.node_padding_y = padding_y;
        }
        if let Some(radius) = config.edge_radius {
            svg.edge_radius = radius;
        }
        if let Some(padding) = config.svg_diagram_padding {
            svg.diagram_padding = padding;
        }

        let engine_id = config.layout_engine.map(|id| id.engine());
        let (def_routing, def_curve) = engine_style_defaults(engine_id);
        let (preset_routing, preset_curve) = config
            .edge_preset
            .map(EdgePreset::expand)
            .unwrap_or((def_routing, def_curve));

        svg.routing_style = config.routing_style.unwrap_or(preset_routing);
        svg.curve = config.curve.unwrap_or(preset_curve);
        svg.path_simplification = config.path_simplification;
        svg
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

impl From<&RenderConfig> for TextRenderOptions {
    fn from(config: &RenderConfig) -> Self {
        Self {
            output_format: OutputFormat::Text,
            text_color_mode: config.text_color_mode,
            routing_style: config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0))
                .unwrap_or(RoutingStyle::Orthogonal),
            cluster_ranksep: config.cluster_ranksep,
            padding: config.padding,
            path_simplification: config.path_simplification,
        }
    }
}

/// Render SVG directly from precomputed graph geometry.
pub fn render_svg_from_geometry(
    diagram: &Diagram,
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
    diagram: &Diagram,
    routed: &RoutedGraphGeometry,
    options: &SvgRenderOptions,
) -> String {
    let geometry = geometry_for_routed_svg(diagram, routed);
    render_svg_from_geometry_with_routing(diagram, &geometry, options, EdgeRouting::EngineProvided)
}

pub(crate) fn render_svg_from_geometry_with_routing(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    options: &SvgRenderOptions,
    edge_routing: EdgeRouting,
) -> String {
    svg::render_svg_from_geometry(diagram, options, geometry, edge_routing)
}

fn geometry_for_routed_svg(diagram: &Diagram, routed: &RoutedGraphGeometry) -> GraphGeometry {
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
    diagram: &Diagram,
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
/// ```
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
    diagram: &Diagram,
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

fn branching_label_info(diagram: &Diagram) -> (bool, usize, usize) {
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

#[cfg(test)]
mod tests {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;
    use crate::runtime::test_support_tests::render_text_diagram;

    #[test]
    fn test_render_with_subgraph_produces_borders() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        let output = render_text_diagram(&diagram);

        assert!(
            output.contains('┌') || output.contains('+'),
            "output should contain top-left corner: {output}"
        );
        assert!(
            output.contains('┘') || output.contains('+'),
            "output should contain bottom-right corner: {output}"
        );
        assert!(
            output.contains("Group"),
            "output should contain title: {output}"
        );
    }

    #[test]
    fn test_render_simple_diagram_unchanged() {
        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        let output = render_text_diagram(&diagram);

        assert!(
            output.contains('A'),
            "output should contain node A: {output}"
        );
        assert!(
            output.contains('B'),
            "output should contain node B: {output}"
        );
    }
}
