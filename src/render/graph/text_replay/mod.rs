//! Explicit low-level text-grid replay APIs for graph-family rendering.
//!
//! This namespace groups the advanced text replay toolkit used by tests,
//! MMDS replay, and other callers that need to rebuild or inspect grid-space
//! render state directly.

pub(crate) mod intersect;

pub use self::intersect::{
    FloatPoint, NodeFace, calculate_attachment_points, classify_face, intersect_diamond,
    intersect_node, intersect_rect, spread_points_on_face,
};
pub use super::grid_routing::router::{RoutedEdge, Segment, route_all_edges};
pub use super::text_edge::render_all_edges_with_labels;
pub use super::text_shape::render_node;
pub use crate::graph::grid::{
    GridLayout, GridLayoutConfig, NodeBounds, geometry_to_grid_layout_with_routed,
};
