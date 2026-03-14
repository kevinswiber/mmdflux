//! Measurement mode selection and layout construction for the layered algorithm.

use super::kernel::{LayoutConfig, Ranker};
use super::layout_building::{
    build_layered_layout_with_config, compute_sublayouts, layered_config_for_layout,
};
use super::layout_subgraph_ops::{center_override_subgraphs, expand_parent_bounds};
use crate::engines::graph::EngineConfig;
use crate::errors::RenderError;
use crate::graph::geometry::GraphGeometry;
use crate::graph::grid::{GridLayoutConfig, GridRanker};
use crate::graph::measure::{
    ProportionalTextMetrics, grid_edge_label_dimensions, grid_node_dimensions,
    proportional_node_dimensions,
};
use crate::graph::projection::{GridProjection, OverrideSubgraphProjection};
use crate::graph::{Direction, Edge, Graph, Node};

/// Measurement mode controls whether layout uses grid-cell dimensions or
/// proportional float-space dimensions for node sizing.
#[derive(Debug, Clone)]
pub enum MeasurementMode {
    /// Grid-cell dimensions for discrete grid replay.
    Grid,
    /// Proportional dimensions for unitless float-space geometry.
    Proportional(ProportionalTextMetrics),
}

/// Build a flowchart `GridLayoutConfig` from layered-engine settings.
///
/// This bridges the engine-facing layered config back to the render-facing
/// config used by shared graph-family layout construction.
pub(crate) fn layout_config_from_layered(
    layered_cfg: &LayoutConfig,
    diagram: &Graph,
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
            Ranker::NetworkSimplex => GridRanker::NetworkSimplex,
            Ranker::LongestPath => GridRanker::LongestPath,
        }),
        padding: defaults.padding + extra_padding,
        ..defaults
    }
}

fn grid_node_layout_dimensions(node: &Node, direction: Direction) -> (f64, f64) {
    let (width, height) = grid_node_dimensions(node, direction);
    (width as f64, height as f64)
}

fn grid_edge_label_layout_dimensions(edge: &Edge) -> Option<(f64, f64)> {
    edge.label
        .as_ref()
        .map(|label| grid_edge_label_dimensions(label))
}

fn override_subgraph_projections(
    diagram: &Graph,
    layered_cfg: &super::LayoutConfig,
) -> std::collections::HashMap<String, OverrideSubgraphProjection> {
    let grid_config = layout_config_from_layered(layered_cfg, diagram);
    let layered_config = layered_config_for_layout(diagram, &grid_config);
    let direction = diagram.direction;

    compute_sublayouts(
        diagram,
        &layered_config,
        |node| grid_node_layout_dimensions(node, direction),
        grid_edge_label_layout_dimensions,
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
    diagram: &Graph,
    config: &EngineConfig,
) -> Result<GraphGeometry, RenderError> {
    use super::from_layered_layout;

    let EngineConfig::Layered(layered_cfg) = config;
    let override_subgraphs = override_subgraph_projections(diagram, layered_cfg);
    let grid_config = layout_config_from_layered(layered_cfg, diagram);
    let mut lc = layered_config_for_layout(diagram, &grid_config);
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
        MeasurementMode::Grid => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| grid_node_layout_dimensions(node, direction),
            grid_edge_label_layout_dimensions,
        ),
        MeasurementMode::Proportional(metrics) => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| proportional_node_dimensions(metrics, node, direction),
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
