#![allow(dead_code)]

use mmdflux::Diagram;
pub use mmdflux::graph::EdgeRouting;
use mmdflux::graph::geometry::{FPoint, GraphGeometry, RoutedGraphGeometry};

pub fn route_graph_geometry(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> RoutedGraphGeometry {
    mmdflux::graph::route_graph_geometry(diagram, geometry, edge_routing)
}

pub fn snap_path_to_grid_preview(path: &[FPoint], scale_x: f64, scale_y: f64) -> Vec<FPoint> {
    mmdflux::graph::snap_path_to_grid_preview(path, scale_x, scale_y)
}
