//! Explicit low-level text-grid replay APIs for graph-family rendering.
//!
//! This namespace groups the advanced text replay toolkit used by tests,
//! MMDS replay, and other callers that need to rebuild or inspect grid-space
//! render state directly.

pub use super::text_edge::render_all_edges_with_labels;
pub use super::text_shape::render_node;
pub use crate::graph::grid::{
    FloatPoint, GridLayout, GridLayoutConfig, NodeBounds, NodeFace, RoutedEdge, Segment,
    calculate_attachment_points, classify_face, geometry_to_grid_layout_with_routed,
    intersect_diamond, intersect_node, intersect_rect, route_all_edges, spread_points_on_face,
};
