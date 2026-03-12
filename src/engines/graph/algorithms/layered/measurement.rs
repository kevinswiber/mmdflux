use super::GridLayoutConfig;
use super::layout_building::{
    build_layered_layout_with_config, compute_sublayouts, layered_config_for_layout,
};
use super::layout_subgraph_ops::{center_override_subgraphs, expand_parent_bounds};
use crate::config::RenderConfig;
use crate::engines::graph::EngineConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::geometry::GraphGeometry;
use crate::graph::grid_projection::{GridProjection, GridRanker, OverrideSubgraphProjection};
use crate::graph::measure::{
    DEFAULT_FONT_SIZE, DEFAULT_SVG_NODE_PADDING_X, DEFAULT_SVG_NODE_PADDING_Y, SvgTextMetrics,
    svg_node_dimensions, text_edge_label_dimensions, text_node_dimensions,
};
use crate::graph::{Diagram, Direction, Edge, Node};

/// Measurement mode controls whether layout uses text-grid character
/// dimensions or SVG pixel dimensions for node sizing.
#[derive(Debug, Clone)]
pub enum MeasurementMode {
    /// Text-grid character dimensions (for text/ascii rendering).
    Text,
    /// SVG pixel dimensions (for MMDS and SVG output).
    Svg(SvgTextMetrics),
}

impl MeasurementMode {
    /// Determine the measurement mode from the output format.
    pub fn for_format(format: OutputFormat, config: &RenderConfig) -> Self {
        match format {
            OutputFormat::Mmds | OutputFormat::Svg => {
                let font_size = DEFAULT_FONT_SIZE;
                let node_padding_x = config
                    .svg_node_padding_x
                    .unwrap_or(DEFAULT_SVG_NODE_PADDING_X);
                let node_padding_y = config
                    .svg_node_padding_y
                    .unwrap_or(DEFAULT_SVG_NODE_PADDING_Y);
                let metrics = SvgTextMetrics::new(font_size, node_padding_x, node_padding_y);
                MeasurementMode::Svg(metrics)
            }
            _ => MeasurementMode::Text,
        }
    }
}

/// Build a flowchart `GridLayoutConfig` from layered-engine settings.
///
/// This bridges the engine-facing layered config back to the render-facing
/// config used by shared graph-family layout construction.
pub(crate) fn layout_config_from_layered(
    layered_cfg: &super::LayoutConfig,
    diagram: &Diagram,
) -> GridLayoutConfig {
    let defaults = GridLayoutConfig::default();
    let extra_padding = if diagram.has_subgraphs() {
        diagram
            .subgraphs
            .keys()
            .map(|id| diagram.subgraph_depth(id))
            .max()
            .unwrap_or(0)
            * 2
    } else {
        0
    };

    GridLayoutConfig {
        node_sep: layered_cfg.node_sep,
        edge_sep: layered_cfg.edge_sep,
        rank_sep: layered_cfg.rank_sep,
        margin: layered_cfg.margin,
        ranker: Some(match layered_cfg.ranker {
            super::Ranker::NetworkSimplex => GridRanker::NetworkSimplex,
            super::Ranker::LongestPath => GridRanker::LongestPath,
        }),
        padding: defaults.padding + extra_padding,
        ..defaults
    }
}

fn text_node_layout_dimensions(node: &Node, direction: Direction) -> (f64, f64) {
    let (width, height) = text_node_dimensions(node, direction);
    (width as f64, height as f64)
}

fn text_edge_label_layout_dimensions(edge: &Edge) -> Option<(f64, f64)> {
    edge.label
        .as_ref()
        .map(|label| text_edge_label_dimensions(label))
}

fn override_subgraph_projections(
    diagram: &Diagram,
    layered_cfg: &super::LayoutConfig,
) -> std::collections::HashMap<String, OverrideSubgraphProjection> {
    let text_config = layout_config_from_layered(layered_cfg, diagram);
    let layered_config = layered_config_for_layout(diagram, &text_config);
    let direction = diagram.direction;

    compute_sublayouts(
        diagram,
        &layered_config,
        |node| text_node_layout_dimensions(node, direction),
        text_edge_label_layout_dimensions,
        false,
    )
    .into_iter()
    .map(|(subgraph_id, sublayout)| {
        (
            subgraph_id,
            OverrideSubgraphProjection {
                nodes: sublayout
                    .result
                    .nodes
                    .into_iter()
                    .map(|(node_id, rect)| (node_id.0, rect.into()))
                    .collect(),
            },
        )
    })
    .collect()
}

/// Run layered layout with a given measurement mode.
///
/// Shared by the Flux and Mermaid-compatible engines. Both use the same
/// layered kernel; they diverge in adapter policy and routing behavior.
pub fn run_layered_layout(
    mode: &MeasurementMode,
    diagram: &Diagram,
    config: &EngineConfig,
) -> Result<GraphGeometry, RenderError> {
    use crate::engines::graph::algorithms::layered::from_layered_layout;

    let EngineConfig::Layered(layered_cfg) = config;
    let override_subgraphs = override_subgraph_projections(diagram, layered_cfg);
    let text_config = layout_config_from_layered(layered_cfg, diagram);
    let mut lc = layered_config_for_layout(diagram, &text_config);
    lc.greedy_switch = layered_cfg.greedy_switch;
    lc.model_order_tiebreak = layered_cfg.model_order_tiebreak;
    lc.variable_rank_spacing = layered_cfg.variable_rank_spacing;
    lc.always_compound_ordering = layered_cfg.always_compound_ordering;
    lc.track_reversed_chains = layered_cfg.track_reversed_chains;
    lc.per_edge_label_spacing = layered_cfg.per_edge_label_spacing;
    lc.label_side_selection = layered_cfg.label_side_selection;
    lc.label_dummy_strategy = layered_cfg.label_dummy_strategy;

    let direction = diagram.direction;
    let mut result = match mode {
        MeasurementMode::Text => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| text_node_layout_dimensions(node, direction),
            text_edge_label_layout_dimensions,
        ),
        MeasurementMode::Svg(metrics) => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| svg_node_dimensions(metrics, node, direction),
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| metrics.edge_label_dimensions(label))
            },
        ),
    };

    center_override_subgraphs(diagram, &mut result);
    expand_parent_bounds(diagram, &mut result, 0.0, 0.0);

    let mut geom = from_layered_layout(&result, diagram);
    if !override_subgraphs.is_empty() {
        let projection = geom
            .grid_projection
            .get_or_insert_with(GridProjection::default);
        projection.override_subgraphs = override_subgraphs;
    }
    let has_enhancements = layered_cfg.greedy_switch
        || layered_cfg.model_order_tiebreak
        || layered_cfg.variable_rank_spacing;
    geom.enhanced_backward_routing = has_enhancements;
    Ok(geom)
}
