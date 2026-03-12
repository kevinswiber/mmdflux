use super::orthogonal::snap_path_to_grid;
use crate::graph::geometry::FPoint;

/// Preview helper: snap a float path to a deterministic grid.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn snap_path_to_grid_preview(
    path: &[FPoint],
    scale_x: f64,
    scale_y: f64,
) -> Vec<FPoint> {
    snap_path_to_grid(path, scale_x, scale_y)
}
