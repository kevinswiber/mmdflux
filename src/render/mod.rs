//! Rendering for flowchart diagrams.
//!
//! Supports text (Unicode/ASCII) and SVG output formats.

// Shared rendering infrastructure
pub(crate) mod canvas;
pub(crate) mod chars;
pub mod intersect;

// Re-export flowchart render modules through canonical paths
pub use canvas::Canvas;
use canvas::{Cell, Connections};
pub use chars::CharSet;

use crate::diagram::{
    AlgorithmId, CornerStyle, EdgePreset, EdgeRouting, EngineAlgorithmId, EngineId, GraphEngine,
    InterpolationStyle, OutputFormat, PathSimplification, RenderConfig, RoutingStyle,
};
pub use crate::diagrams::flowchart::render::svg::{render_svg, render_svg_from_geometry};
use crate::diagrams::flowchart::render::svg_metrics::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE};
pub use crate::diagrams::flowchart::render::text_adapter::{
    compute_layout, geometry_to_text_layout,
};
pub use crate::diagrams::flowchart::render::text_edge::{
    render_all_edges, render_all_edges_with_labels, render_edge,
};
pub use crate::diagrams::flowchart::render::text_layout::{
    Layout, SubgraphBounds, TextLayoutConfig,
};
pub use crate::diagrams::flowchart::render::text_router::{
    Point, RoutedEdge, Segment, route_all_edges, route_edge,
};
pub use crate::diagrams::flowchart::render::text_shape::{
    NodeBounds, node_dimensions, render_node,
};
use crate::diagrams::flowchart::render::text_subgraph;
use crate::graph::{Diagram, Direction};

/// Engine defaults for SVG style (routing + interpolation + corner).
///
/// When no preset or explicit style is specified, these engine-specific defaults
/// preserve the pre-Phase-7 rendering behaviour.
fn engine_style_defaults(
    engine: Option<EngineId>,
) -> (RoutingStyle, InterpolationStyle, CornerStyle) {
    match engine {
        // mermaid-layered: polyline routing (PolylineRoute), bezier interpolation (Mermaid default)
        Some(EngineId::Mermaid) => (
            RoutingStyle::Polyline,
            InterpolationStyle::Bezier,
            CornerStyle::Sharp,
        ),
        // flux-layered (default) and ELK: orthogonal routing, bezier interpolation
        _ => (
            RoutingStyle::Orthogonal,
            InterpolationStyle::Bezier,
            CornerStyle::Sharp,
        ),
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

        // Resolve style model: explicit low-level > preset defaults > engine defaults.
        let engine_id = config.layout_engine.map(|id| id.engine());
        let (def_routing, def_interp, def_corner) = engine_style_defaults(engine_id);
        let (preset_routing, preset_interp, preset_corner) = config
            .edge_preset
            .map(EdgePreset::expand)
            .unwrap_or((def_routing, def_interp, def_corner));
        svg.routing_style = config.routing_style.unwrap_or(preset_routing);
        svg.interpolation_style = config.interpolation_style.unwrap_or(preset_interp);
        svg.corner_style = config.corner_style.unwrap_or(preset_corner);

        // Derive edge routing from engine capabilities + resolved routing style.
        // Uses EngineAlgorithmId::edge_routing_for_style() for consistent selection.
        // Default engine (None) behaves as flux-layered (Native + Orthogonal).
        let resolved_routing = svg.routing_style; // already resolved above
        let default_engine = EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered);
        let engine_id = config.layout_engine.unwrap_or(default_engine);
        let edge_routing = engine_id.edge_routing_for_style(Some(resolved_routing));

        RenderOptions {
            output_format: OutputFormat::Text,
            svg,
            ranker: Some(config.layout.ranker),
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
    /// Path routing topology for SVG edge rendering.
    /// Drives orthogonalization post-processing in the SVG path builder.
    pub routing_style: RoutingStyle,
    /// Path interpolation treatment for SVG edge rendering.
    pub interpolation_style: InterpolationStyle,
    /// Corner arc treatment for SVG edge rendering (only for `InterpolationStyle::Linear`).
    pub corner_style: CornerStyle,
    /// Corner arc radius in pixels (for `CornerStyle::Rounded`).
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
            // Default matches flux-layered engine: orthogonal routing + bezier interpolation
            // (equivalent to the former EdgeStyle::Smooth default).
            routing_style: RoutingStyle::Orthogonal,
            interpolation_style: InterpolationStyle::Bezier,
            corner_style: CornerStyle::Sharp,
            edge_radius: 5.0,
            diagram_padding: 8.0,
        }
    }
}

/// Render options for flowcharts.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Output format (text, ascii, or svg).
    pub output_format: OutputFormat,
    /// SVG-specific options.
    pub svg: SvgOptions,
    /// Ranking algorithm override. None uses the default (NetworkSimplex).
    pub ranker: Option<crate::layered::types::Ranker>,
    /// Node spacing override (nodesep).
    pub node_spacing: Option<f64>,
    /// Rank spacing override (ranksep).
    pub rank_spacing: Option<f64>,
    /// Edge segment spacing override (edgesep).
    pub edge_spacing: Option<f64>,
    /// Layout margin override (translateGraph margin).
    pub margin: Option<f64>,
    /// Extra ranksep applied when subgraphs are present (Mermaid clusters).
    pub cluster_ranksep: Option<f64>,
    /// ASCII padding around the diagram (diagramPadding analog).
    pub padding: Option<usize>,
    /// Path simplification level (MMDS and SVG only).
    pub path_simplification: PathSimplification,
    /// Optional edge routing override for graph-family renderers.
    pub edge_routing: Option<EdgeRouting>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Text,
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
/// use mmdflux::{parse_flowchart, build_diagram};
/// use mmdflux::render::{render, RenderOptions};
///
/// let input = "graph TD\nA[Start] --> B[End]\n";
/// let flowchart = parse_flowchart(input).unwrap();
/// let diagram = build_diagram(&flowchart);
/// let ascii = render(&diagram, &RenderOptions::default());
/// ```
pub fn render(diagram: &Diagram, options: &RenderOptions) -> String {
    if matches!(options.output_format, OutputFormat::Svg) {
        return render_svg(diagram, options);
    }

    // Engine → text adapter → text renderer.
    let mut config = layout_config_for_diagram(diagram, options);
    config.ranker = options.ranker;

    let engine = crate::diagrams::flowchart::engine::FluxLayeredEngine::text();
    // Construct LayeredConfig from raw LayoutConfig values. Do NOT call
    // layered_config_for_layout() here — the engine's internal round-trip
    // (layout_config_from_layered → build_layered_layout → layered_config_for_layout)
    // applies cluster_rank_sep once. Pre-applying it here would double it.
    let engine_config = crate::diagram::EngineConfig::Layered(crate::layered::LayoutConfig {
        direction: match diagram.direction {
            Direction::TopDown => crate::layered::Direction::TopBottom,
            Direction::BottomTop => crate::layered::Direction::BottomTop,
            Direction::LeftRight => crate::layered::Direction::LeftRight,
            Direction::RightLeft => crate::layered::Direction::RightLeft,
        },
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep: config.rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or_default(),
    });
    let request = crate::diagram::GraphSolveRequest::from_config(
        &RenderConfig::default(),
        options.output_format,
    );
    let result = engine
        .solve(diagram, &engine_config, &request)
        .expect("engine solve failed in render()");

    let layout = geometry_to_text_layout(diagram, &result.geometry, &config);
    render_text_from_layout(diagram, &layout, options)
}

/// Render a diagram to text from a pre-computed `Layout`.
///
/// This is the text rendering pipeline: Layout → Canvas → String.
/// Separated from `render()` so that callers who produce a Layout via a
/// different path (e.g. the text adapter) can share the same rendering logic.
pub fn render_text_from_layout(
    diagram: &Diagram,
    layout: &Layout,
    options: &RenderOptions,
) -> String {
    let charset = match options.output_format {
        OutputFormat::Ascii => CharSet::ascii(),
        _ => CharSet::unicode(),
    };

    // Step 2: Create canvas
    let mut canvas = Canvas::new(layout.width, layout.height);

    // Step 2.5: Render subgraph borders FIRST (z-order: background)
    if !layout.subgraph_bounds.is_empty() {
        text_subgraph::render_subgraph_borders(&mut canvas, &layout.subgraph_bounds, &charset);
    }

    // Step 3: Render nodes
    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for node_id in node_keys {
        let node = &diagram.nodes[node_id];
        if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
            render_node(&mut canvas, node, x, y, &charset, diagram.direction);
        }
    }

    // Step 4: Route and render edges
    let routed_edges = route_all_edges(&diagram.edges, layout, diagram.direction);
    render_all_edges_with_labels(
        &mut canvas,
        &routed_edges,
        &charset,
        diagram.direction,
        &layout.edge_label_positions,
    );

    // Step 4.5: Pierce subgraph borders where edges cross.
    apply_subgraph_border_junctions(
        &mut canvas,
        &layout.subgraph_bounds,
        &routed_edges,
        &charset,
    );

    // Step 5: Convert canvas to string
    canvas.to_string()
}

/// Compute layout configuration appropriate for the diagram.
///
/// For LR/RL layouts, we need more horizontal spacing to accommodate edge labels.
pub fn layout_config_for_diagram(diagram: &Diagram, options: &RenderOptions) -> TextLayoutConfig {
    let mut config = TextLayoutConfig::default();

    // Check if any edges have labels
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
            // For horizontal layouts, increase h_spacing to fit labels
            // The edge attachment points are 1 cell inside the gap on each side,
            // so available space for label = h_spacing - 2
            // We need: label_len + 2 (1 space before, 1 space after arrow)
            // Therefore: h_spacing = label_len + 4
            config.h_spacing = config.h_spacing.max(max_label_len + 4);
        }
        Direction::TopDown | Direction::BottomTop => {
            // For vertical layouts, increase v_spacing to fit labels
            if max_label_len > 0 {
                // Check if any source node has multiple labeled edges (branching)
                // These need extra space so labels don't overlap
                let (has_branching, left_len, right_len) = branching_label_info(diagram);
                if has_branching {
                    // Branching edges need more vertical space:
                    // - 1 row for edge chars leaving source
                    // - 1 row for horizontal spread
                    // - 1 row for labels
                    // - 1 row for arrows/entry
                    config.v_spacing = config.v_spacing.max(5);
                    // Also need more horizontal space for labels on each branch
                    let max_branching_len = left_len.max(right_len);
                    config.h_spacing = config.h_spacing.max(max_branching_len + 4);
                    // Asymmetric margins: only add margin where the label extends
                    config.left_label_margin = left_len;
                    config.right_label_margin = right_len;
                } else {
                    config.v_spacing = config.v_spacing.max(3);
                }
            }
        }
    }

    // Increase padding for nested subgraphs so outer borders have room
    if diagram.has_subgraphs() {
        let max_depth = diagram
            .subgraphs
            .keys()
            .map(|id| diagram.subgraph_depth(id))
            .max()
            .unwrap_or(0);
        if max_depth > 0 {
            // Each nesting level needs border_padding (2) extra chars
            config.padding += max_depth * 2;
        }
    }

    // Apply layout config overrides
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

/// Check if the diagram has branching edges with labels and return margin info.
///
/// Returns (has_branching, left_label_len, right_label_len) where:
/// - has_branching is true if any source node has multiple outgoing edges with labels
/// - left_label_len is the max label length for left branches (first target in declaration order)
/// - right_label_len is the max label length for right branches (subsequent targets)
fn branching_label_info(diagram: &Diagram) -> (bool, usize, usize) {
    // Group labeled edges by source node, preserving declaration order
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

    // Find sources with 2+ labeled edges
    // First label goes left, rest go right (based on typical layout ordering)
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
    use crate::graph::build_diagram;
    use crate::parser::parse_flowchart;

    #[test]
    fn test_render_with_subgraph_produces_borders() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let output = render(&diagram, &RenderOptions::default());

        // Output should contain border characters
        assert!(
            output.contains('┌') || output.contains('+'),
            "output should contain top-left corner: {output}"
        );
        assert!(
            output.contains('┘') || output.contains('+'),
            "output should contain bottom-right corner: {output}"
        );
        // Output should contain the title (embedded in border)
        assert!(
            output.contains("Group"),
            "output should contain title: {output}"
        );
    }

    #[test]
    fn test_render_simple_diagram_unchanged() {
        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let output = render(&diagram, &RenderOptions::default());

        // Should not contain subgraph border artifacts (no ┌ corners
        // that aren't part of node shapes)
        // Simple check: output should contain nodes and edges
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
