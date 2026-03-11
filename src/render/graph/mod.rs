//! Graph-family rendering and routing.
//!
//! `render::graph` owns the advanced direct-render APIs and all shared
//! graph-family output helpers. Adapters should normally use the high-level
//! runtime facade, but lower-level callers can parse with `frontends`,
//! compile with `diagrams`, and render through this namespace.

pub mod backends;
pub(crate) mod backward_policy;
pub(crate) mod layout_building;
pub(crate) mod layout_subgraph_ops;
pub(crate) mod orthogonal_router;
pub mod route_policy;
pub mod routing;
pub mod svg;
pub(crate) mod svg_metrics;
pub(crate) mod svg_router;
pub mod text_adapter;
pub mod text_edge;
pub mod text_layout;
pub mod text_router;
pub(crate) mod text_routing_core;
pub mod text_shape;
pub mod text_subgraph;
pub mod text_types;

pub use self::svg::{render_svg, render_svg_from_geometry};
use self::svg_metrics::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE};
use self::text_edge::render_all_edges_with_labels;
use self::text_router::{RoutedEdge, Segment, route_all_edges};
use self::text_shape::render_node;
use self::text_types::{Layout, SubgraphBounds, TextLayoutConfig};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::engines::graph::{
    AlgorithmId, Curve, EdgePreset, EdgeRouting, EngineAlgorithmId, EngineId, GraphEngine,
    OutputFormat, PathSimplification, RenderConfig, RoutingStyle, TextColorMode,
};
use crate::graph::{Diagram, Direction};
use crate::render::primitives::canvas::{Cell, Connections};
use crate::render::{Canvas, CharSet};

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

impl From<&RenderConfig> for RenderOptions {
    fn from(config: &RenderConfig) -> Self {
        let mut svg = SvgOptions::default();
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
        let resolved_curve = config.curve.unwrap_or(preset_curve);
        svg.routing_style = config.routing_style.unwrap_or(preset_routing);
        svg.curve = resolved_curve;

        let resolved_routing = svg.routing_style;
        let default_engine = EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered);
        let engine_id = config.layout_engine.unwrap_or(default_engine);
        let edge_routing = engine_id.edge_routing_for_style(Some(resolved_routing));

        RenderOptions {
            output_format: OutputFormat::Text,
            text_color_mode: config.text_color_mode,
            svg,
            ranker: Some(config.layout.ranker.into()),
            node_spacing: Some(config.layout.node_sep),
            rank_spacing: Some(config.layout.rank_sep),
            edge_spacing: Some(config.layout.edge_sep),
            margin: Some(config.layout.margin),
            cluster_ranksep: config.cluster_ranksep,
            padding: config.padding,
            path_simplification: config.path_simplification,
            edge_routing: Some(edge_routing),
        }
    }
}

/// SVG render options.
#[derive(Debug, Clone)]
pub struct SvgOptions {
    pub scale: f64,
    pub font_family: String,
    pub font_size: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub routing_style: RoutingStyle,
    pub curve: Curve,
    pub edge_radius: f64,
    pub diagram_padding: f64,
}

impl Default for SvgOptions {
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
        }
    }
}

/// Render options for graph-family direct rendering.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub output_format: OutputFormat,
    pub text_color_mode: TextColorMode,
    pub svg: SvgOptions,
    pub ranker: Option<crate::engines::graph::algorithms::layered::Ranker>,
    pub node_spacing: Option<f64>,
    pub rank_spacing: Option<f64>,
    pub edge_spacing: Option<f64>,
    pub margin: Option<f64>,
    pub cluster_ranksep: Option<f64>,
    pub padding: Option<usize>,
    pub path_simplification: PathSimplification,
    pub edge_routing: Option<EdgeRouting>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Text,
            text_color_mode: TextColorMode::Plain,
            svg: SvgOptions::default(),
            ranker: None,
            node_spacing: None,
            rank_spacing: None,
            edge_spacing: None,
            margin: None,
            cluster_ranksep: None,
            padding: None,
            path_simplification: PathSimplification::default(),
            edge_routing: None,
        }
    }
}

impl RenderOptions {
    pub fn default_svg() -> Self {
        Self {
            output_format: OutputFormat::Svg,
            ..Self::default()
        }
    }
}

/// Render a diagram to the configured output format.
///
/// # Example
///
/// ```
/// use mmdflux::diagrams::flowchart::compile_to_graph;
/// use mmdflux::frontends::mermaid::parse_flowchart;
/// use mmdflux::render::graph::{render, RenderOptions};
///
/// let input = "graph TD\nA[Start] --> B[End]\n";
/// let flowchart = parse_flowchart(input).unwrap();
/// let diagram = compile_to_graph(&flowchart);
/// let ascii = render(&diagram, &RenderOptions::default());
/// ```
pub fn render(diagram: &Diagram, options: &RenderOptions) -> String {
    let engine = FluxLayeredEngine::text();
    let engine_config = layered_engine_config_for_render(diagram, options);
    let request_config = RenderConfig {
        routing_style: routing_style_from_edge_routing(options.edge_routing),
        path_simplification: options.path_simplification,
        ..RenderConfig::default()
    };
    let request = crate::engines::graph::GraphSolveRequest::from_config(
        &request_config,
        options.output_format,
    );
    let result = engine
        .solve(diagram, &engine_config, &request)
        .expect("engine solve failed in render()");

    match options.output_format {
        OutputFormat::Svg => backends::svg::render_svg_with_options(diagram, &result, options),
        OutputFormat::Text | OutputFormat::Ascii => {
            backends::text::render_text_with_options(diagram, &result, options)
        }
        OutputFormat::Mmds => {
            panic!("use render::graph::backends::mmds::render_mmds_full for MMDS output")
        }
        other => panic!("graph-family direct render does not support {other} output"),
    }
}

fn layered_engine_config_for_render(
    diagram: &Diagram,
    options: &RenderOptions,
) -> crate::engines::graph::EngineConfig {
    let mut config = crate::engines::graph::algorithms::layered::LayoutConfig {
        direction: match diagram.direction {
            Direction::TopDown => crate::engines::graph::algorithms::layered::Direction::TopBottom,
            Direction::BottomTop => {
                crate::engines::graph::algorithms::layered::Direction::BottomTop
            }
            Direction::LeftRight => {
                crate::engines::graph::algorithms::layered::Direction::LeftRight
            }
            Direction::RightLeft => {
                crate::engines::graph::algorithms::layered::Direction::RightLeft
            }
        },
        ..Default::default()
    };

    if let Some(node_spacing) = options.node_spacing {
        config.node_sep = node_spacing;
    }
    if let Some(rank_spacing) = options.rank_spacing {
        config.rank_sep = rank_spacing;
    }
    if let Some(edge_spacing) = options.edge_spacing {
        config.edge_sep = edge_spacing;
    }
    if let Some(margin) = options.margin {
        config.margin = margin;
    }
    if let Some(ranker) = options.ranker {
        config.ranker = ranker;
    }

    crate::engines::graph::EngineConfig::Layered(config)
}

fn routing_style_from_edge_routing(edge_routing: Option<EdgeRouting>) -> Option<RoutingStyle> {
    match edge_routing {
        Some(EdgeRouting::DirectRoute) => Some(RoutingStyle::Direct),
        Some(EdgeRouting::PolylineRoute) => Some(RoutingStyle::Polyline),
        Some(EdgeRouting::OrthogonalRoute) => Some(RoutingStyle::Orthogonal),
        Some(EdgeRouting::EngineProvided) | None => None,
    }
}

pub(crate) fn render_text_from_layout(
    diagram: &Diagram,
    layout: &Layout,
    options: &RenderOptions,
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
    options: &RenderOptions,
) -> TextLayoutConfig {
    let mut config = TextLayoutConfig::default();

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

    if let Some(node_spacing) = options.node_spacing {
        config.node_sep = node_spacing;
    }
    if let Some(rank_spacing) = options.rank_spacing {
        config.rank_sep = rank_spacing;
    }
    if let Some(edge_spacing) = options.edge_spacing {
        config.edge_sep = edge_spacing;
    }
    if let Some(margin) = options.margin {
        config.margin = margin;
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
    use super::*;
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::frontends::mermaid::parse_flowchart;

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
