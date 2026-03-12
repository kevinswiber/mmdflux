use mmdflux::engines::graph::algorithms::layered::{
    Direction as LayeredDirection, LayoutConfig as LayeredConfig, Ranker,
};
use mmdflux::engines::graph::contracts::{EngineConfig, GraphEngine};
use mmdflux::engines::graph::flux::FluxLayeredEngine;
use mmdflux::graph::grid_projection::GridRanker;
#[allow(unused_imports)]
pub use mmdflux::render::graph::text_replay::{
    GridLayoutConfig, Layout, NodeBounds, RoutedEdge, Segment, geometry_to_text_layout_with_routed,
    render_all_edges_with_labels, render_node, route_all_edges,
};
use mmdflux::{Diagram, Direction, GeometryLevel};

use super::graph_family::default_grid_request;

#[allow(dead_code)]
fn layered_config_for_layout(diagram: &Diagram, config: &GridLayoutConfig) -> LayeredConfig {
    let mut rank_sep = config.rank_sep;
    if !diagram.subgraphs.is_empty() && config.cluster_rank_sep > 0.0 {
        rank_sep += config.cluster_rank_sep;
    }

    LayeredConfig {
        direction: match diagram.direction {
            Direction::TopDown => LayeredDirection::TopBottom,
            Direction::BottomTop => LayeredDirection::BottomTop,
            Direction::LeftRight => LayeredDirection::LeftRight,
            Direction::RightLeft => LayeredDirection::RightLeft,
        },
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: match config.ranker.unwrap_or_default() {
            GridRanker::NetworkSimplex => Ranker::NetworkSimplex,
            GridRanker::LongestPath => Ranker::LongestPath,
        },
        ..Default::default()
    }
}

#[allow(dead_code)]
pub fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> Layout {
    let engine = FluxLayeredEngine::text();
    let request = default_grid_request(GeometryLevel::Layout, None);
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("graph-family text-grid test solve failed");

    geometry_to_text_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}
