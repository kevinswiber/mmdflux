#![allow(dead_code, unused_imports)]

use std::collections::HashMap;

use mmdflux::engines::graph::algorithms::layered::{
    Direction as LayeredDirection, LayoutConfig as LayeredConfig, Ranker,
};
use mmdflux::engines::graph::contracts::{EngineConfig, GraphEngine, GraphSolveRequest};
use mmdflux::engines::graph::flux::FluxLayeredEngine;
use mmdflux::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use mmdflux::graph::grid_projection::GridRanker;
pub use mmdflux::render::graph::{GridLayoutConfig, Layout, NodeBounds, RoutedEdge, Segment};
use mmdflux::render::{Canvas, CharSet};
use mmdflux::{Diagram, Direction, Edge, OutputFormat, RenderConfig};

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

pub fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> Layout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::from_config(&RenderConfig::default(), OutputFormat::Text);
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("graph-family text-grid test solve failed");

    mmdflux::render::graph::geometry_to_text_layout_with_routed(
        diagram,
        &result.geometry,
        result.routed.as_ref(),
        config,
    )
}

pub fn geometry_to_text_layout_with_routed(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    config: &GridLayoutConfig,
) -> Layout {
    mmdflux::render::graph::geometry_to_text_layout_with_routed(diagram, geometry, routed, config)
}

pub fn render_all_edges_with_labels(
    canvas: &mut Canvas,
    routed_edges: &[RoutedEdge],
    charset: &CharSet,
    diagram_direction: mmdflux::Direction,
    label_positions: &HashMap<usize, (usize, usize)>,
) {
    mmdflux::render::graph::render_all_edges_with_labels(
        canvas,
        routed_edges,
        charset,
        diagram_direction,
        label_positions,
    );
}

pub fn route_all_edges(
    edges: &[Edge],
    layout: &Layout,
    diagram_direction: mmdflux::Direction,
) -> Vec<RoutedEdge> {
    mmdflux::render::graph::route_all_edges(edges, layout, diagram_direction)
}

pub fn render_node(
    canvas: &mut Canvas,
    node: &mmdflux::Node,
    x: usize,
    y: usize,
    charset: &CharSet,
    direction: mmdflux::Direction,
) -> NodeBounds {
    mmdflux::render::graph::render_node(canvas, node, x, y, charset, direction)
}
