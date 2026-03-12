#![allow(dead_code, unused_imports)]

use std::collections::HashMap;

use mmdflux::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
pub use mmdflux::render::graph::{GridLayoutConfig, Layout, NodeBounds, RoutedEdge, Segment};
use mmdflux::render::{Canvas, CharSet};
use mmdflux::{Diagram, Edge};

pub fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> Layout {
    mmdflux::render::graph::compute_text_layout(diagram, config)
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
