//! Render-only graph-family emission APIs.
//!
//! `render::graph` exposes narrow, geometry-based rendering entrypoints for
//! callers that already have `GraphGeometry` or `RoutedGraphGeometry`.
//! Solve orchestration remains owned by the runtime facade and graph engines.

pub(crate) mod backends;
pub(crate) mod backward_policy;
pub(crate) mod layout_building;
pub(crate) mod route_policy;
pub(crate) mod routing;
pub(crate) mod svg;
pub(crate) mod svg_metrics;
pub(crate) mod svg_router;
pub(crate) mod text_adapter;
pub(crate) mod text_edge;
pub(crate) mod text_layout;
pub(crate) mod text_router;
pub(crate) mod text_routing_core;
pub(crate) mod text_shape;
pub(crate) mod text_subgraph;
pub(crate) mod text_types;

use self::svg_metrics::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE};
use self::text_edge::render_all_edges_with_labels;
use self::text_router::{RoutedEdge, Segment, route_all_edges};
use self::text_shape::render_node;
use self::text_types::{GridLayoutConfig, Layout, SubgraphBounds};
use crate::engines::graph::EdgeRouting;
use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::{Diagram, Direction};
use crate::render::primitives::canvas::{Cell, Connections};
use crate::render::{Canvas, CharSet};
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
        let font_size = DEFAULT_FONT_SIZE;
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

pub(crate) fn render_svg_from_geometry_with_routing(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    options: &SvgRenderOptions,
    edge_routing: EdgeRouting,
) -> String {
    svg::render_svg_from_geometry(diagram, options, geometry, edge_routing)
}

/// Render text or ASCII directly from precomputed graph geometry.
pub fn render_text_from_geometry(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    options: &TextRenderOptions,
) -> String {
    let routed_geometry = match routed {
        Some(routed) => routed.clone(),
        None => routing::route_graph_geometry(
            diagram,
            geometry,
            edge_routing_from_style(options.routing_style),
        ),
    };
    let config = layout_config_for_diagram(diagram, options);
    let layout = text_adapter::geometry_to_text_layout_with_routed(
        diagram,
        geometry,
        Some(&routed_geometry),
        &config,
    );
    render_text_from_layout(diagram, &layout, options)
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
pub(crate) fn render_text_from_layout(
    diagram: &Diagram,
    layout: &Layout,
    options: &TextRenderOptions,
) -> String {
    let charset = match options.output_format {
        OutputFormat::Ascii => CharSet::ascii(),
        _ => CharSet::unicode(),
    };

    let mut canvas = Canvas::new(layout.width, layout.height);

    if !layout.subgraph_bounds.is_empty() {
        text_subgraph::render_subgraph_borders(&mut canvas, &layout.subgraph_bounds, &charset);
    }

    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for node_id in node_keys {
        let node = &diagram.nodes[node_id];
        if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
            render_node(&mut canvas, node, x, y, &charset, diagram.direction);
        }
    }

    let routed_edges = route_all_edges(&diagram.edges, layout, diagram.direction);
    render_all_edges_with_labels(
        &mut canvas,
        &routed_edges,
        &charset,
        diagram.direction,
        &layout.edge_label_positions,
    );

    apply_subgraph_border_junctions(
        &mut canvas,
        &layout.subgraph_bounds,
        &routed_edges,
        &charset,
    );

    if options.text_color_mode.uses_ansi() {
        canvas.to_ansi_string()
    } else {
        canvas.to_string()
    }
}

pub(crate) fn layout_config_for_diagram(
    diagram: &Diagram,
    options: &TextRenderOptions,
) -> GridLayoutConfig {
    let mut config = GridLayoutConfig::default();

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

fn apply_subgraph_border_junctions(
    canvas: &mut Canvas,
    subgraph_bounds: &std::collections::HashMap<String, SubgraphBounds>,
    routed_edges: &[RoutedEdge],
    charset: &CharSet,
) {
    if subgraph_bounds.is_empty() || routed_edges.is_empty() {
        return;
    }

    let should_skip_title_cell =
        |cell: &Cell| cell.is_subgraph_title && cell.ch != charset.horizontal && cell.ch != ' ';
    let conns_all = Connections {
        up: true,
        down: true,
        left: true,
        right: true,
    };

    for bounds in subgraph_bounds.values() {
        if bounds.width < 2 || bounds.height < 2 {
            continue;
        }

        let left = bounds.x;
        let right = bounds.x.saturating_add(bounds.width.saturating_sub(1));
        let top = bounds.y;
        let bottom = bounds.y.saturating_add(bounds.height.saturating_sub(1));

        for routed in routed_edges {
            for segment in &routed.segments {
                match *segment {
                    Segment::Vertical { x, y_start, y_end } => {
                        let (y_min, y_max) = if y_start <= y_end {
                            (y_start, y_end)
                        } else {
                            (y_end, y_start)
                        };
                        if x > left && x < right {
                            if y_min < top
                                && top <= y_max
                                && let Some(cell) = canvas.get(x, top)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, x, top, conns_all, charset);
                            }
                            if y_min <= bottom
                                && bottom < y_max
                                && let Some(cell) = canvas.get(x, bottom)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, x, bottom, conns_all, charset);
                            }
                        }
                    }
                    Segment::Horizontal { y, x_start, x_end } => {
                        let (x_min, x_max) = if x_start <= x_end {
                            (x_start, x_end)
                        } else {
                            (x_end, x_start)
                        };
                        if y > top && y < bottom {
                            if x_min < left
                                && left <= x_max
                                && let Some(cell) = canvas.get(left, y)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, left, y, conns_all, charset);
                            }
                            if x_min <= right
                                && right < x_max
                                && let Some(cell) = canvas.get(right, y)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, right, y, conns_all, charset);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn set_junction_cell(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    conns: Connections,
    charset: &CharSet,
) {
    if let Some(cell) = canvas.get_mut(x, y) {
        cell.ch = charset.junction(conns);
        cell.connections = conns;
        cell.is_edge = true;
    }
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
    use crate::testing::{RenderOptions, render};

    #[test]
    fn test_render_with_subgraph_produces_borders() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        let output = render(&diagram, &RenderOptions::default());

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
        let output = render(&diagram, &RenderOptions::default());

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
