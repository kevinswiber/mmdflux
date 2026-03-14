use super::super::intersect::NodeFace;
use super::super::layout::NodeBounds;
use crate::graph::{Edge, Shape};

/// Grouped endpoint parameters for edge routing functions.
pub(super) struct EdgeEndpoints {
    pub(super) from_bounds: NodeBounds,
    pub(super) from_shape: Shape,
    pub(super) to_bounds: NodeBounds,
    pub(super) to_shape: Shape,
}

#[derive(Clone, Copy)]
pub(super) struct RoutingOverrides {
    pub(super) src_attach: Option<(usize, usize)>,
    pub(super) tgt_attach: Option<(usize, usize)>,
    pub(super) src_face: Option<NodeFace>,
    pub(super) tgt_face: Option<NodeFace>,
    pub(super) src_first_vertical: bool,
}

/// A point on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

impl Point {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

/// A segment of an edge path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// Vertical line from start to end (same x, different y).
    Vertical {
        x: usize,
        y_start: usize,
        y_end: usize,
    },
    /// Horizontal line from start to end (same y, different x).
    Horizontal {
        y: usize,
        x_start: usize,
        x_end: usize,
    },
}

impl Segment {
    /// Manhattan length of this segment.
    pub fn length(&self) -> usize {
        match self {
            Segment::Vertical { y_start, y_end, .. } => y_start.abs_diff(*y_end),
            Segment::Horizontal { x_start, x_end, .. } => x_start.abs_diff(*x_end),
        }
    }

    /// Start point of this segment.
    pub fn start_point(&self) -> Point {
        match self {
            Segment::Vertical { x, y_start, .. } => Point { x: *x, y: *y_start },
            Segment::Horizontal { y, x_start, .. } => Point { x: *x_start, y: *y },
        }
    }

    /// End point of this segment.
    pub fn end_point(&self) -> Point {
        match self {
            Segment::Vertical { x, y_end, .. } => Point { x: *x, y: *y_end },
            Segment::Horizontal { y, x_end, .. } => Point { x: *x_end, y: *y },
        }
    }

    /// Point at a given offset from start along the segment direction.
    /// Clamps to segment bounds if offset exceeds length.
    pub fn point_at_offset(&self, offset: usize) -> Point {
        match self {
            Segment::Vertical { x, y_start, y_end } => {
                let clamped = offset.min(y_start.abs_diff(*y_end));
                let y = if *y_end >= *y_start {
                    y_start + clamped
                } else {
                    y_start - clamped
                };
                Point { x: *x, y }
            }
            Segment::Horizontal { y, x_start, x_end } => {
                let clamped = offset.min(x_start.abs_diff(*x_end));
                let x = if *x_end >= *x_start {
                    x_start + clamped
                } else {
                    x_start - clamped
                };
                Point { x, y: *y }
            }
        }
    }
}

/// A complete routed path for an edge.
#[derive(Debug, Clone)]
pub struct RoutedEdge {
    /// The edge this path represents.
    pub edge: Edge,
    /// Start point (attachment point on source node).
    pub start: Point,
    /// End point (attachment point on target node).
    pub end: Point,
    /// Path segments from start to end.
    pub segments: Vec<Segment>,
    /// Direction from the launch cell back toward the source node.
    pub source_connection: Option<AttachDirection>,
    /// Direction from which the edge enters the target node (for arrow drawing).
    pub entry_direction: AttachDirection,
    /// Whether this edge goes backward in the layout direction.
    pub is_backward: bool,
    /// Whether this is a self-edge (source == target).
    pub is_self_edge: bool,
}

/// Direction for attachment points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachDirection {
    Top,
    Bottom,
    Left,
    Right,
}

fn source_connection_direction(face: NodeFace) -> AttachDirection {
    match face {
        NodeFace::Top => AttachDirection::Bottom,
        NodeFace::Bottom => AttachDirection::Top,
        NodeFace::Left => AttachDirection::Right,
        NodeFace::Right => AttachDirection::Left,
    }
}

pub(super) fn build_routed_edge(
    edge: &Edge,
    start: Point,
    end: Point,
    segments: Vec<Segment>,
    source_face: NodeFace,
    entry_direction: AttachDirection,
    is_backward: bool,
) -> RoutedEdge {
    RoutedEdge {
        edge: edge.clone(),
        start,
        end,
        segments,
        source_connection: Some(source_connection_direction(source_face)),
        entry_direction,
        is_backward,
        is_self_edge: false,
    }
}

/// Pre-computed attachment override for one edge.
#[derive(Debug, Clone)]
pub struct AttachmentOverride {
    pub source: Option<(usize, usize)>,
    pub target: Option<(usize, usize)>,
    pub source_first_vertical: bool,
}
