use mmdflux::graph::geometry::FPoint;
#[allow(unused_imports)]
pub use mmdflux::graph::routing::{EdgeRouting, route_graph_geometry};

#[allow(dead_code)]
pub fn snap_path_to_grid_preview(path: &[FPoint], scale_x: f64, scale_y: f64) -> Vec<FPoint> {
    let sx = if scale_x.abs() < f64::EPSILON {
        1.0
    } else {
        scale_x.abs()
    };
    let sy = if scale_y.abs() < f64::EPSILON {
        1.0
    } else {
        scale_y.abs()
    };

    path.iter()
        .map(|p| FPoint::new((p.x / sx).round() * sx, (p.y / sy).round() * sy))
        .collect()
}
