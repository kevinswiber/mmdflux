//! Grid-space types for the graph-family derived geometry pipeline.
//!
//! These types describe the integer-coordinate geometry derived from
//! float-space graph geometry. Downstream renderers consume this grid-space
//! layout to produce text or other discrete outputs.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use super::GridLayoutConfig;
use crate::graph::space::FRect;
use crate::graph::{Direction, Shape};

/// Bounding box for a node in grid coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeBounds {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    /// Layout-derived center x, avoids integer division rounding.
    pub layout_center_x: Option<usize>,
    /// Layout-derived center y, avoids integer division rounding.
    pub layout_center_y: Option<usize>,
}

impl NodeBounds {
    /// Get the center x coordinate.
    /// Uses the stored layout center if available, otherwise falls back to integer division.
    pub fn center_x(&self) -> usize {
        self.layout_center_x.unwrap_or(self.x + self.width / 2)
    }

    /// Get the center y coordinate.
    /// Uses the stored layout center if available, otherwise falls back to integer division.
    pub fn center_y(&self) -> usize {
        self.layout_center_y.unwrap_or(self.y + self.height / 2)
    }

    /// Check if a point (x, y) falls inside this bounding box.
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Get the top attachment point (center of top edge).
    pub fn top(&self) -> (usize, usize) {
        (self.center_x(), self.y)
    }

    /// Get the bottom attachment point (center of bottom edge).
    pub fn bottom(&self) -> (usize, usize) {
        (self.center_x(), self.y + self.height - 1)
    }

    /// Get the left attachment point (center of left edge).
    pub fn left(&self) -> (usize, usize) {
        (self.x, self.center_y())
    }

    /// Get the right attachment point (center of right edge).
    pub fn right(&self) -> (usize, usize) {
        (self.x + self.width - 1, self.center_y())
    }
}

/// Bounding box for a subgraph border in draw coordinates.
#[derive(Debug, Clone)]
pub struct SubgraphBounds {
    /// Left edge x coordinate.
    pub x: usize,
    /// Top edge y coordinate.
    pub y: usize,
    /// Total width including border.
    pub width: usize,
    /// Total height including border.
    pub height: usize,
    /// Display title for the subgraph.
    pub title: String,
    /// Nesting depth (0 = top-level, 1 = nested once, etc.)
    pub depth: usize,
}

/// Draw-coordinate data for a self-edge loop.
#[derive(Debug, Clone)]
pub struct SelfEdgeDrawData {
    /// Node ID the self-edge loops on.
    pub node_id: String,
    /// Original edge index.
    pub edge_index: usize,
    /// Draw-coordinate points for the orthogonal loop.
    pub points: Vec<(usize, usize)>,
}

/// Grid position of a node (layer/column in abstract grid coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPos {
    /// Layer (row for TD/BT, column for LR/RL).
    pub layer: usize,
    /// Position within layer.
    pub pos: usize,
}

/// Coordinate transformation context from layout float coordinates to draw coordinates.
///
/// Encapsulates the scaling, offset, and padding parameters needed to convert
/// the layout engine's floating-point coordinates to integer character-grid positions.
pub(crate) struct CoordTransform<'a> {
    pub(crate) scale_x: f64,
    pub(crate) scale_y: f64,
    pub(crate) layout_min_x: f64,
    pub(crate) layout_min_y: f64,
    pub(crate) max_overhang_x: usize,
    pub(crate) max_overhang_y: usize,
    pub(crate) config: &'a GridLayoutConfig,
}

impl CoordTransform<'_> {
    /// Convert layout coordinates to draw coordinates.
    pub(crate) fn to_draw(&self, x: f64, y: f64) -> (usize, usize) {
        let dx = ((x - self.layout_min_x) * self.scale_x).round() as isize;
        let dy = ((y - self.layout_min_y) * self.scale_y).round() as isize;
        let draw_x = dx.max(0) as usize
            + self.max_overhang_x
            + self.config.padding
            + self.config.left_label_margin;
        let draw_y = dy.max(0) as usize + self.max_overhang_y + self.config.padding;
        (draw_x, draw_y)
    }
}

/// Grid-space layout result containing node positions and canvas dimensions.
#[derive(Debug, Default)]
pub struct GridLayout {
    /// Node positions in grid coordinates.
    pub grid_positions: HashMap<String, GridPos>,
    /// Node positions in draw coordinates (x, y pixels/chars).
    pub draw_positions: HashMap<String, (usize, usize)>,
    /// Node bounding boxes in draw coordinates.
    pub node_bounds: HashMap<String, NodeBounds>,
    /// Total canvas width needed.
    pub width: usize,
    /// Total canvas height needed.
    pub height: usize,
    /// Spacing between nodes horizontally.
    pub h_spacing: usize,
    /// Spacing between nodes vertically.
    pub v_spacing: usize,

    // --- Edge routing data from normalization ---
    /// Waypoints for each edge, derived from dummy node positions.
    /// Key: edge index in `Diagram::edges`, Value: list of waypoint coordinates.
    /// Empty for short edges (span 1 rank), populated for long edges.
    pub edge_waypoints: HashMap<usize, Vec<(usize, usize)>>,

    /// Fully-routed edge paths transformed to draw coordinates.
    /// When present for an edge, text routing can consume these points directly
    /// and only perform grid/character conversion.
    pub routed_edge_paths: HashMap<usize, Vec<(usize, usize)>>,

    /// Routed edges whose deliberate orthogonal corridor should be preserved.
    pub preserve_routed_path_topology: HashSet<usize>,

    /// Pre-computed label positions for edges with labels.
    /// Key: edge index in `Diagram::edges`, Value: (x, y) position for the label center.
    /// Only populated for edges that have labels.
    pub edge_label_positions: HashMap<usize, (usize, usize)>,

    /// Node shapes for intersection calculation.
    /// Maps node ID to its shape for computing dynamic attachment points.
    pub node_shapes: HashMap<String, Shape>,

    /// Subgraph bounding boxes in draw coordinates.
    /// Key: subgraph ID, Value: bounds with title.
    /// Empty for diagrams without subgraphs.
    pub subgraph_bounds: HashMap<String, SubgraphBounds>,

    /// Self-edge loop data in draw coordinates.
    pub self_edges: Vec<SelfEdgeDrawData>,

    /// Effective layout direction per node.
    /// Nodes inside a direction-override subgraph use the subgraph's direction;
    /// other nodes use the diagram's root direction.
    pub node_directions: HashMap<String, Direction>,
}

impl GridLayout {
    /// Get the bounding box for a node.
    pub fn get_bounds(&self, node_id: &str) -> Option<&NodeBounds> {
        self.node_bounds.get(node_id)
    }

    /// Get the effective layout direction for an edge.
    ///
    /// If both endpoints share the same direction override (e.g. both are in an LR
    /// subgraph), returns that override direction.  Otherwise returns the fallback
    /// (typically the diagram's root direction).
    pub fn effective_edge_direction(&self, from: &str, to: &str, fallback: Direction) -> Direction {
        let src_dir = self.node_directions.get(from).copied().unwrap_or(fallback);
        let tgt_dir = self.node_directions.get(to).copied().unwrap_or(fallback);
        if src_dir == tgt_dir {
            return src_dir;
        }
        // If either endpoint uses the root direction, the edge crosses from an
        // override subgraph to the root part of the diagram — use the root direction.
        if src_dir == fallback || tgt_dir == fallback {
            return fallback;
        }
        // Both endpoints have non-root direction overrides (e.g., LR and BT in
        // nested subgraphs).  Infer direction from geometry.
        match (self.node_bounds.get(from), self.node_bounds.get(to)) {
            (Some(fb), Some(tb)) => {
                let dx = (fb.center_x() as isize - tb.center_x() as isize).unsigned_abs();
                let dy = (fb.center_y() as isize - tb.center_y() as isize).unsigned_abs();
                if dx > dy {
                    if fb.center_x() <= tb.center_x() {
                        Direction::LeftRight
                    } else {
                        Direction::RightLeft
                    }
                } else if dy > 0 {
                    if fb.center_y() <= tb.center_y() {
                        Direction::TopDown
                    } else {
                        Direction::BottomTop
                    }
                } else {
                    fallback
                }
            }
            _ => fallback,
        }
    }
}

/// Intermediate result for a node's scaled center and dimensions, used between
/// the overhang-detection pass and the draw-position pass.
pub(crate) struct RawCenter {
    pub(crate) id: String,
    pub(crate) cx: usize,
    pub(crate) cy: usize,
    pub(crate) w: usize,
    pub(crate) h: usize,
}

/// Shared parameters for transforming layout coordinates to ASCII draw coordinates.
pub(crate) struct TransformContext {
    pub(crate) layout_min_x: f64,
    pub(crate) layout_min_y: f64,
    pub(crate) scale_x: f64,
    pub(crate) scale_y: f64,
    pub(crate) padding: usize,
    pub(crate) left_label_margin: usize,
    pub(crate) overhang_x: usize,
    pub(crate) overhang_y: usize,
}

impl TransformContext {
    /// Transform a layout top-left-based rect to grid coordinates (x, y, width, height).
    #[allow(dead_code)]
    ///
    /// Transforms the top-left and bottom-right corners independently using
    /// `to_grid()`, then computes the grid rect between them. This ensures
    /// the transformed rect faithfully represents the layout bounding box in
    /// grid space.
    pub(crate) fn to_grid_rect(&self, rect: &FRect) -> (usize, usize, usize, usize) {
        let (x1, y1) = self.to_grid(rect.x, rect.y);
        let (x2, y2) = self.to_grid(rect.x + rect.width, rect.y + rect.height);
        let draw_x = x1.min(x2);
        let draw_y = y1.min(y2);
        let draw_w = x1.max(x2) - draw_x;
        let draw_h = y1.max(y2) - draw_y;
        (draw_x, draw_y, draw_w.max(1), draw_h.max(1))
    }

    /// Transform a layout (x, y) coordinate to grid coordinates.
    pub(crate) fn to_grid(&self, layout_x: f64, layout_y: f64) -> (usize, usize) {
        let x = ((layout_x - self.layout_min_x) * self.scale_x).round() as usize
            + self.overhang_x
            + self.padding
            + self.left_label_margin;
        let y = ((layout_y - self.layout_min_y) * self.scale_y).round() as usize
            + self.overhang_y
            + self.padding;
        (x, y)
    }
}
