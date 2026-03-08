//! Edge routing between nodes.
//!
//! Computes paths for edges, avoiding node boundaries.

use std::collections::HashMap;

use super::text_layout::{Layout, SelfEdgeDrawData, SubgraphBounds};
use super::text_routing_core::{
    Face as SharedFace,
    LARGE_HORIZONTAL_OFFSET_THRESHOLD as SHARED_LARGE_HORIZONTAL_OFFSET_THRESHOLD,
    edge_faces as shared_edge_faces, plan_attachments as shared_plan_attachments,
};
use super::text_shape::NodeBounds;
use crate::graph::{Arrow, Direction, Edge, Shape, Stroke};
use crate::render::intersect::{
    NodeFace, calculate_attachment_points, classify_face, spread_points_on_face,
};

/// Grouped endpoint parameters for edge routing functions.
struct EdgeEndpoints {
    from_bounds: NodeBounds,
    from_shape: Shape,
    to_bounds: NodeBounds,
    to_shape: Shape,
}

struct RoutingOverrides {
    src_attach: Option<(usize, usize)>,
    tgt_attach: Option<(usize, usize)>,
    src_face: Option<NodeFace>,
    tgt_face: Option<NodeFace>,
    src_first_vertical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPathFamily {
    SharedRoutedDrawPath,
    WaypointFallback,
    SyntheticBackward,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPathRejection {
    TooShort,
    NoWaypoints,
    FaceInference,
    WaypointInsideFace,
    SegmentCollision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextRouteProbe {
    pub(crate) path_family: TextPathFamily,
    pub(crate) rejection_reason: Option<TextPathRejection>,
}

#[derive(Debug, Clone)]
pub(crate) struct RouteEdgeResult {
    pub(crate) routed: RoutedEdge,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) probe: TextRouteProbe,
}

fn subgraph_edge_face(bounds: &NodeBounds, other: &NodeBounds, direction: Direction) -> NodeFace {
    let bounds_right = bounds.x + bounds.width.saturating_sub(1);
    let bounds_bottom = bounds.y + bounds.height.saturating_sub(1);
    let other_right = other.x + other.width.saturating_sub(1);
    let other_bottom = other.y + other.height.saturating_sub(1);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            if other_bottom < bounds.y {
                return NodeFace::Top;
            }
            if other.y > bounds_bottom {
                return NodeFace::Bottom;
            }
            if other_right < bounds.x {
                return NodeFace::Left;
            }
            if other.x > bounds_right {
                return NodeFace::Right;
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if other_right < bounds.x {
                return NodeFace::Left;
            }
            if other.x > bounds_right {
                return NodeFace::Right;
            }
            if other_bottom < bounds.y {
                return NodeFace::Top;
            }
            if other.y > bounds_bottom {
                return NodeFace::Bottom;
            }
        }
    }

    classify_face(
        bounds,
        (other.center_x(), other.center_y()),
        Shape::Rectangle,
    )
}

fn subgraph_bounds_as_node(bounds: &SubgraphBounds) -> NodeBounds {
    NodeBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

fn resolve_edge_bounds(layout: &Layout, edge: &Edge) -> Option<(NodeBounds, NodeBounds)> {
    let from_bounds = if let Some(sg_id) = edge.from_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.from)?
    };
    let to_bounds = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.to)?
    };
    Some((from_bounds, to_bounds))
}

fn bounds_for_node_id(layout: &Layout, node_id: &str) -> Option<NodeBounds> {
    if let Some(bounds) = layout.get_bounds(node_id) {
        return Some(*bounds);
    }
    layout
        .subgraph_bounds
        .get(node_id)
        .map(subgraph_bounds_as_node)
}

fn node_inside_subgraph(bounds: &NodeBounds, sg: &SubgraphBounds) -> bool {
    let node_right = bounds.x + bounds.width;
    let node_bottom = bounds.y + bounds.height;
    let sg_right = sg.x + sg.width;
    let sg_bottom = sg.y + sg.height;
    bounds.x >= sg.x && bounds.y >= sg.y && node_right <= sg_right && node_bottom <= sg_bottom
}

fn containing_subgraph_id<'a>(layout: &'a Layout, node_id: &str) -> Option<&'a str> {
    let bounds = layout.node_bounds.get(node_id)?;
    layout
        .subgraph_bounds
        .iter()
        .filter(|(_, sg)| node_inside_subgraph(bounds, sg))
        .max_by_key(|(_, sg)| (sg.depth, usize::MAX - (sg.width * sg.height)))
        .map(|(id, _)| id.as_str())
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

fn build_routed_edge(
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

/// Get the outgoing and incoming attachment directions based on diagram direction.
#[cfg(test)]
fn attachment_directions(diagram_direction: Direction) -> (AttachDirection, AttachDirection) {
    match diagram_direction {
        Direction::TopDown => (AttachDirection::Bottom, AttachDirection::Top),
        Direction::BottomTop => (AttachDirection::Top, AttachDirection::Bottom),
        Direction::LeftRight => (AttachDirection::Right, AttachDirection::Left),
        Direction::RightLeft => (AttachDirection::Left, AttachDirection::Right),
    }
}

/// Check if an edge is a backward edge (goes against the layout direction).
///
/// In a normal layout, edges flow in the diagram direction (e.g., top to bottom for TD).
/// A backward edge goes against this flow, typically creating a cycle.
pub fn is_backward_edge(
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    // Backward means target is "before" source in the flow direction
    match direction {
        Direction::TopDown => to_bounds.y < from_bounds.y,
        Direction::BottomTop => to_bounds.y > from_bounds.y,
        Direction::LeftRight => to_bounds.x < from_bounds.x,
        Direction::RightLeft => to_bounds.x > from_bounds.x,
    }
}

/// Gap between node boundary and synthetic backward-edge waypoint path (in cells).
pub const BACKWARD_ROUTE_GAP: usize = 2;

/// Generate synthetic waypoints for a single-rank-span backward edge.
///
/// Routes around the right side (TD/BT) or bottom side (LR/RL) of the nodes
/// when no layout-assigned waypoints exist.
///
/// Returns empty vec for forward edges or same-position nodes.
pub fn generate_backward_waypoints(
    src_bounds: &NodeBounds,
    tgt_bounds: &NodeBounds,
    direction: Direction,
) -> Vec<(usize, usize)> {
    if !is_backward_edge(src_bounds, tgt_bounds, direction) {
        return vec![];
    }

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            // Route to the right of both nodes
            let right_edge = (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);
            let route_x = right_edge + BACKWARD_ROUTE_GAP;
            vec![
                (route_x, src_bounds.center_y()),
                (route_x, tgt_bounds.center_y()),
            ]
        }
        Direction::LeftRight => {
            // Route below both nodes; backward edge flows right-to-left
            let bottom_edge =
                (src_bounds.y + src_bounds.height).max(tgt_bounds.y + tgt_bounds.height);
            let route_y = bottom_edge + BACKWARD_ROUTE_GAP;
            let right_edge = (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);
            vec![
                (src_bounds.x.saturating_sub(1), route_y),
                (right_edge + BACKWARD_ROUTE_GAP, route_y),
            ]
        }
        Direction::RightLeft => {
            // Route below both nodes; backward edge flows left-to-right
            let bottom_edge =
                (src_bounds.y + src_bounds.height).max(tgt_bounds.y + tgt_bounds.height);
            let route_y = bottom_edge + BACKWARD_ROUTE_GAP;
            let left_edge = src_bounds.x.min(tgt_bounds.x);
            vec![
                (src_bounds.x + src_bounds.width, route_y),
                (left_edge.saturating_sub(BACKWARD_ROUTE_GAP), route_y),
            ]
        }
    }
}

/// Generate backward channel waypoints that clear all intermediate nodes.
///
/// Unlike `generate_backward_waypoints` which only considers source and target,
/// this checks ALL nodes in the corridor between source and target to ensure
/// the channel lane clears them all. Used when the orthogonal router identified
/// a corridor-obstructed backward edge (signalled by a routed draw path).
fn generate_corridor_backward_waypoints(
    edge: &Edge,
    layout: &Layout,
    src_bounds: &NodeBounds,
    tgt_bounds: &NodeBounds,
    direction: Direction,
) -> Vec<(usize, usize)> {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let y_min = src_bounds.y.min(tgt_bounds.y);
            let y_max = (src_bounds.y + src_bounds.height).max(tgt_bounds.y + tgt_bounds.height);

            // Find the rightmost edge of any node in the vertical corridor
            let mut right_edge =
                (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);
            for (node_id, bounds) in &layout.node_bounds {
                if node_id == &edge.from || node_id == &edge.to {
                    continue;
                }
                let node_bottom = bounds.y + bounds.height;
                if bounds.y < y_max && node_bottom > y_min {
                    right_edge = right_edge.max(bounds.x + bounds.width);
                }
            }

            let route_x = right_edge + BACKWARD_ROUTE_GAP;
            vec![
                (route_x, src_bounds.center_y()),
                (route_x, tgt_bounds.center_y()),
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            let x_min = src_bounds.x.min(tgt_bounds.x);
            let x_max = (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);

            // Find the bottommost edge of any node in the horizontal corridor
            let mut bottom_edge =
                (src_bounds.y + src_bounds.height).max(tgt_bounds.y + tgt_bounds.height);
            for (node_id, bounds) in &layout.node_bounds {
                if node_id == &edge.from || node_id == &edge.to {
                    continue;
                }
                let node_right = bounds.x + bounds.width;
                if bounds.x < x_max && node_right > x_min {
                    bottom_edge = bottom_edge.max(bounds.y + bounds.height);
                }
            }

            let route_y = bottom_edge + BACKWARD_ROUTE_GAP;
            match direction {
                Direction::LeftRight => {
                    let right_edge =
                        (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);
                    vec![
                        (src_bounds.x.saturating_sub(1), route_y),
                        (right_edge + BACKWARD_ROUTE_GAP, route_y),
                    ]
                }
                Direction::RightLeft => {
                    let left_edge = src_bounds.x.min(tgt_bounds.x);
                    vec![
                        (src_bounds.x + src_bounds.width, route_y),
                        (left_edge.saturating_sub(BACKWARD_ROUTE_GAP), route_y),
                    ]
                }
                _ => unreachable!(),
            }
        }
    }
}

fn should_use_routed_draw_path(
    edge: &Edge,
    layout: &Layout,
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        return false;
    }

    let Some(draw_path) = layout.routed_edge_paths.get(&edge.index) else {
        return false;
    };

    let from_subgraph = containing_subgraph_id(layout, &edge.from);
    let to_subgraph = containing_subgraph_id(layout, &edge.to);
    if from_subgraph.is_some() && from_subgraph == to_subgraph {
        return false;
    }
    if from_subgraph.is_some() ^ to_subgraph.is_some() {
        return false;
    }

    if is_backward_edge(from_bounds, to_bounds, direction) {
        return should_prefer_shared_backward_route_for_text(draw_path, direction);
    }

    should_prefer_shared_forward_route_for_text(
        edge,
        layout,
        draw_path,
        from_bounds,
        to_bounds,
        direction,
    )
}

fn should_prefer_shared_backward_route_for_text(
    draw_path: &[(usize, usize)],
    direction: Direction,
) -> bool {
    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            draw_path.len() >= 6 && waypoints_from_draw_path(draw_path).len() >= 4
        }
        Direction::TopDown | Direction::BottomTop => true,
    }
}

fn should_prefer_shared_forward_route_for_text(
    edge: &Edge,
    layout: &Layout,
    draw_path: &[(usize, usize)],
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    if edge.from == edge.to || is_backward_edge(from_bounds, to_bounds, direction) {
        return false;
    }

    let draw_waypoint_count = waypoints_from_draw_path(draw_path).len();
    let has_normalized_waypoints = layout
        .edge_waypoints
        .get(&edge.index)
        .is_some_and(|waypoints| waypoints.len() >= 2);
    let structured_short_forward =
        layout.preserve_routed_path_topology.contains(&edge.index) && draw_waypoint_count >= 2;

    let structured_draw_path = draw_path.len() >= 4 && draw_waypoint_count >= 2;

    (has_normalized_waypoints && structured_draw_path)
        || structured_short_forward
        || (!layout.subgraph_bounds.is_empty() && structured_draw_path)
}

fn route_inter_subgraph_edge_via_outer_lane(
    edge: &Edge,
    layout: &Layout,
    ep: &EdgeEndpoints,
    draw_path: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Option<RoutedEdge> {
    let from_subgraph = containing_subgraph_id(layout, &edge.from)?;
    let to_subgraph = containing_subgraph_id(layout, &edge.to)?;
    if from_subgraph == to_subgraph || is_backward_edge(&ep.from_bounds, &ep.to_bounds, direction) {
        return None;
    }

    let points = normalize_draw_path_points(draw_path, direction);
    if points.len() < 3 {
        return None;
    }

    let (lane_waypoint, src_attach, tgt_attach, src_face, tgt_face) = match direction {
        Direction::TopDown | Direction::BottomTop => {
            let lane_x = points[1].0;
            let src_face = if lane_x >= ep.from_bounds.center_x() {
                NodeFace::Right
            } else {
                NodeFace::Left
            };
            let tgt_face = if lane_x >= ep.to_bounds.center_x() {
                NodeFace::Right
            } else {
                NodeFace::Left
            };
            (
                (lane_x, ep.to_bounds.center_y()),
                Some(clamp_to_face(
                    &ep.from_bounds,
                    src_face,
                    (lane_x, ep.from_bounds.center_y()),
                )),
                Some(clamp_to_face(
                    &ep.to_bounds,
                    tgt_face,
                    (lane_x, ep.to_bounds.center_y()),
                )),
                Some(src_face),
                Some(tgt_face),
            )
        }
        Direction::LeftRight | Direction::RightLeft => {
            let lane_y = points[1].1;
            let src_face = if lane_y >= ep.from_bounds.center_y() {
                NodeFace::Bottom
            } else {
                NodeFace::Top
            };
            let tgt_face = if lane_y >= ep.to_bounds.center_y() {
                NodeFace::Bottom
            } else {
                NodeFace::Top
            };
            (
                (ep.to_bounds.center_x(), lane_y),
                Some(clamp_to_face(
                    &ep.from_bounds,
                    src_face,
                    (ep.from_bounds.center_x(), lane_y),
                )),
                Some(clamp_to_face(
                    &ep.to_bounds,
                    tgt_face,
                    (ep.to_bounds.center_x(), lane_y),
                )),
                Some(src_face),
                Some(tgt_face),
            )
        }
    };

    route_edge_with_waypoints(
        edge,
        ep,
        &[lane_waypoint],
        direction,
        RoutingOverrides {
            src_attach: src_attach.or(overrides.src_attach),
            tgt_attach: tgt_attach.or(overrides.tgt_attach),
            src_face: src_face.or(overrides.src_face),
            tgt_face: tgt_face.or(overrides.tgt_face),
            src_first_vertical: overrides.src_first_vertical,
        },
    )
}

/// Route an edge between two nodes.
pub fn route_edge(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RoutedEdge> {
    route_edge_with_probe(
        edge,
        layout,
        diagram_direction,
        src_attach_override,
        tgt_attach_override,
        src_first_vertical,
    )
    .map(|result| result.routed)
}

pub(crate) fn route_edge_with_probe(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RouteEdgeResult> {
    let (from_bounds, to_bounds) = resolve_edge_bounds(layout, edge)?;

    // Get node shapes for intersection calculation
    let from_shape = if edge.from_subgraph.is_some() {
        Shape::Rectangle
    } else {
        layout
            .node_shapes
            .get(&edge.from)
            .copied()
            .unwrap_or(Shape::Rectangle)
    };
    let to_shape = if edge.to_subgraph.is_some() {
        Shape::Rectangle
    } else {
        layout
            .node_shapes
            .get(&edge.to)
            .copied()
            .unwrap_or(Shape::Rectangle)
    };

    let endpoints = EdgeEndpoints {
        from_bounds,
        from_shape,
        to_bounds,
        to_shape,
    };
    let mut draw_path_rejection = None;

    let use_routed_draw_path =
        should_use_routed_draw_path(edge, layout, &from_bounds, &to_bounds, diagram_direction);

    if use_routed_draw_path {
        if let Some(draw_path) = layout.routed_edge_paths.get(&edge.index) {
            match route_edge_from_draw_path(
                edge,
                layout,
                &endpoints,
                draw_path,
                diagram_direction,
                RoutingOverrides {
                    src_attach: src_attach_override,
                    tgt_attach: tgt_attach_override,
                    src_face: None,
                    tgt_face: None,
                    src_first_vertical,
                },
            ) {
                Ok(routed) => {
                    return Some(route_result(
                        post_process_routed_edge(routed, layout, edge),
                        TextPathFamily::SharedRoutedDrawPath,
                        None,
                    ));
                }
                Err(TextPathRejection::SegmentCollision) => {
                    if let Some(routed) = route_inter_subgraph_edge_via_outer_lane(
                        edge,
                        layout,
                        &endpoints,
                        draw_path,
                        diagram_direction,
                        RoutingOverrides {
                            src_attach: src_attach_override,
                            tgt_attach: tgt_attach_override,
                            src_face: None,
                            tgt_face: None,
                            src_first_vertical,
                        },
                    ) {
                        return Some(route_result(
                            post_process_routed_edge(routed, layout, edge),
                            TextPathFamily::SharedRoutedDrawPath,
                            None,
                        ));
                    }
                    let repaired_draw_path = if layout.subgraph_bounds.is_empty() {
                        draw_path.to_vec()
                    } else {
                        repair_draw_path_segment_collisions(draw_path, layout, edge)
                    };
                    if repaired_draw_path.as_slice() != draw_path.as_slice()
                        && let Ok(routed) = route_edge_from_draw_path(
                            edge,
                            layout,
                            &endpoints,
                            &repaired_draw_path,
                            diagram_direction,
                            RoutingOverrides {
                                src_attach: src_attach_override,
                                tgt_attach: tgt_attach_override,
                                src_face: None,
                                tgt_face: None,
                                src_first_vertical,
                            },
                        )
                    {
                        return Some(route_result(
                            post_process_routed_edge(routed, layout, edge),
                            TextPathFamily::SharedRoutedDrawPath,
                            None,
                        ));
                    }
                    draw_path_rejection = Some(TextPathRejection::SegmentCollision);
                    debug_draw_path_rejection(edge, TextPathRejection::SegmentCollision, draw_path);
                }
                Err(rejection) => {
                    draw_path_rejection = Some(rejection);
                    debug_draw_path_rejection(edge, rejection, draw_path);
                }
            }
        }
        // Float→integer draw path collided with intermediate nodes.
        // Build a clean channel path directly in integer coordinates.
        let channel_wps = generate_corridor_backward_waypoints(
            edge,
            layout,
            &from_bounds,
            &to_bounds,
            diagram_direction,
        );
        if !channel_wps.is_empty() {
            // Don't pass plan overrides — they target the old face assignment.
            // The waypoint approach angles determine the correct attachment.
            return route_backward_with_synthetic_waypoints(
                edge,
                &endpoints,
                &channel_wps,
                diagram_direction,
                RoutingOverrides {
                    src_attach: None,
                    tgt_attach: None,
                    src_face: None,
                    tgt_face: None,
                    src_first_vertical,
                },
            )
            .map(|routed| {
                route_result(
                    post_process_routed_edge(routed, layout, edge),
                    TextPathFamily::SyntheticBackward,
                    draw_path_rejection,
                )
            });
        }
    }

    // Check for waypoints from normalization — works for both forward and backward long edges
    let allow_waypoints = edge.from_subgraph.is_none() && edge.to_subgraph.is_none();
    if allow_waypoints
        && let Some(wps) = layout.edge_waypoints.get(&edge.index)
        && !wps.is_empty()
    {
        let is_backward = is_backward_edge(&from_bounds, &to_bounds, diagram_direction);

        // For backward edges, reverse waypoints so they go from source to target.
        // The layout stores them in effective/forward order (low rank → high rank),
        // but the backward edge goes from high rank → low rank.
        let waypoints: Vec<(usize, usize)> = if is_backward {
            wps.iter().rev().copied().collect()
        } else {
            wps.to_vec()
        };

        return route_edge_with_waypoints(
            edge,
            &endpoints,
            &waypoints,
            diagram_direction,
            RoutingOverrides {
                src_attach: src_attach_override,
                tgt_attach: tgt_attach_override,
                src_face: None,
                tgt_face: None,
                src_first_vertical,
            },
        )
        .map(|routed| {
            route_result(
                post_process_routed_edge(routed, layout, edge),
                TextPathFamily::WaypointFallback,
                draw_path_rejection,
            )
        });
    }

    // For backward edges with no layout waypoints, generate synthetic ones
    if is_backward_edge(&from_bounds, &to_bounds, diagram_direction) {
        let synthetic_wps =
            generate_backward_waypoints(&from_bounds, &to_bounds, diagram_direction);
        if !synthetic_wps.is_empty() {
            if matches!(
                diagram_direction,
                Direction::LeftRight | Direction::RightLeft
            ) {
                return route_edge_with_waypoints(
                    edge,
                    &endpoints,
                    &synthetic_wps,
                    diagram_direction,
                    RoutingOverrides {
                        src_attach: src_attach_override,
                        tgt_attach: tgt_attach_override,
                        src_face: None,
                        tgt_face: None,
                        src_first_vertical,
                    },
                )
                .map(|routed| {
                    route_result(
                        post_process_routed_edge(routed, layout, edge),
                        TextPathFamily::SyntheticBackward,
                        draw_path_rejection,
                    )
                });
            }
            return route_backward_with_synthetic_waypoints(
                edge,
                &endpoints,
                &synthetic_wps,
                diagram_direction,
                RoutingOverrides {
                    src_attach: src_attach_override,
                    tgt_attach: tgt_attach_override,
                    src_face: None,
                    tgt_face: None,
                    src_first_vertical,
                },
            )
            .map(|routed| {
                route_result(
                    post_process_routed_edge(routed, layout, edge),
                    TextPathFamily::SyntheticBackward,
                    draw_path_rejection,
                )
            });
        }
    }

    // No waypoints: direct routing for forward edges
    route_edge_direct(
        edge,
        &endpoints,
        diagram_direction,
        src_attach_override,
        tgt_attach_override,
        src_first_vertical,
    )
    .map(|routed| {
        route_result(
            post_process_routed_edge(routed, layout, edge),
            TextPathFamily::Direct,
            draw_path_rejection,
        )
    })
}

fn route_edge_from_draw_path(
    edge: &Edge,
    layout: &Layout,
    ep: &EdgeEndpoints,
    draw_path: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Result<RoutedEdge, TextPathRejection> {
    if draw_path.len() < 3 {
        return Err(TextPathRejection::TooShort);
    }

    let points = if layout.preserve_routed_path_topology.contains(&edge.index) {
        normalize_draw_path_points(draw_path, direction)
    } else {
        let mut points = draw_path.to_vec();
        points.dedup();
        points
    };
    if points.len() < 3 {
        return Err(TextPathRejection::TooShort);
    }

    let waypoints = waypoints_from_draw_path(&points);
    if waypoints.is_empty() {
        return Err(TextPathRejection::NoWaypoints);
    }

    let inferred_src_face = source_face_from_step(&points);
    let inferred_tgt_face = target_face_from_step(&points);

    let first_anchor = waypoints.first().copied().unwrap_or(points[1]);
    let last_anchor = waypoints
        .last()
        .copied()
        .unwrap_or(points[points.len().saturating_sub(2)]);
    let inferred_src_override =
        inferred_src_face.map(|face| clamp_to_face(&ep.from_bounds, face, first_anchor));
    let inferred_tgt_override =
        inferred_tgt_face.map(|face| clamp_to_face(&ep.to_bounds, face, last_anchor));
    if inferred_src_face
        .is_some_and(|face| !waypoint_is_outside_face(first_anchor, &ep.from_bounds, face))
        || inferred_tgt_face
            .is_some_and(|face| !waypoint_is_outside_face(last_anchor, &ep.to_bounds, face))
    {
        return Err(TextPathRejection::WaypointInsideFace);
    }

    let prefer_planned_face_spread = layout.preserve_routed_path_topology.contains(&edge.index);
    let src_override = inferred_src_override.or(overrides.src_attach);
    let tgt_override = select_draw_path_attachment_override(
        &ep.to_bounds,
        inferred_tgt_face,
        inferred_tgt_override,
        overrides.tgt_attach,
        prefer_planned_face_spread,
    );
    let routing = RoutingOverrides {
        src_attach: src_override,
        tgt_attach: tgt_override,
        src_face: inferred_src_face.or(overrides.src_face),
        tgt_face: inferred_tgt_face.or(overrides.tgt_face),
        src_first_vertical: overrides.src_first_vertical,
    };
    let mut routed = route_edge_with_waypoints(edge, ep, &waypoints, direction, routing)
        .ok_or(TextPathRejection::FaceInference)?;
    if prefer_planned_face_spread
        && edge.arrow_start == Arrow::None
        && points.len() <= 4
        && let Some(src_face) = inferred_src_face
    {
        ensure_source_face_launch_support(&mut routed.segments, routed.start, src_face);
    }
    if segments_collide_with_other_nodes(routed.segments.as_slice(), layout, edge) {
        return Err(TextPathRejection::SegmentCollision);
    }

    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|value| value == "1") {
        eprintln!(
            "[route-routed] {} -> {}: points={points:?} waypoints={waypoints:?} start={:?} end={:?} segments={:?}",
            edge.from, edge.to, routed.start, routed.end, routed.segments
        );
    }

    Ok(routed)
}

fn select_draw_path_attachment_override(
    bounds: &NodeBounds,
    inferred_face: Option<NodeFace>,
    inferred_override: Option<(usize, usize)>,
    planned_override: Option<(usize, usize)>,
    prefer_planned_face_spread: bool,
) -> Option<(usize, usize)> {
    if prefer_planned_face_spread
        && let (Some(face), Some(planned)) = (inferred_face, planned_override)
        && infer_face_from_attachment(bounds, planned, face) == face
    {
        return Some(planned);
    }

    inferred_override.or(planned_override)
}

fn route_result(
    routed: RoutedEdge,
    path_family: TextPathFamily,
    rejection_reason: Option<TextPathRejection>,
) -> RouteEdgeResult {
    RouteEdgeResult {
        routed,
        probe: TextRouteProbe {
            path_family,
            rejection_reason,
        },
    }
}

fn post_process_routed_edge(routed: RoutedEdge, layout: &Layout, edge: &Edge) -> RoutedEdge {
    nudge_routed_edge_clear_of_unrelated_subgraph_borders(routed, layout, edge)
}

fn debug_draw_path_rejection(
    edge: &Edge,
    rejection: TextPathRejection,
    draw_path: &[(usize, usize)],
) {
    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|value| value == "1") {
        eprintln!(
            "[route-routed-reject] {} -> {}: reason={rejection:?} points={draw_path:?}",
            edge.from, edge.to
        );
    }
}

fn source_face_from_step(points: &[(usize, usize)]) -> Option<NodeFace> {
    let first = points.first().copied()?;
    let second = points.iter().copied().find(|point| *point != first)?;
    let dx = second.0 as isize - first.0 as isize;
    let dy = second.1 as isize - first.1 as isize;
    if dx.abs() >= dy.abs() && dx != 0 {
        if dx > 0 {
            Some(NodeFace::Right)
        } else {
            Some(NodeFace::Left)
        }
    } else if dy != 0 {
        if dy > 0 {
            Some(NodeFace::Bottom)
        } else {
            Some(NodeFace::Top)
        }
    } else {
        None
    }
}

fn target_face_from_step(points: &[(usize, usize)]) -> Option<NodeFace> {
    let end = points.last().copied()?;
    let prev = points.iter().rev().copied().find(|point| *point != end)?;
    let dx = end.0 as isize - prev.0 as isize;
    let dy = end.1 as isize - prev.1 as isize;
    if dx.abs() >= dy.abs() && dx != 0 {
        if dx > 0 {
            Some(NodeFace::Left)
        } else {
            Some(NodeFace::Right)
        }
    } else if dy != 0 {
        if dy > 0 {
            Some(NodeFace::Top)
        } else {
            Some(NodeFace::Bottom)
        }
    } else {
        None
    }
}

fn waypoints_from_draw_path(points: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut waypoints = Vec::new();
    for &(x, y) in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        if waypoints.last().copied() != Some((x, y)) {
            waypoints.push((x, y));
        }
    }
    waypoints
}

fn normalize_draw_path_points(
    points: &[(usize, usize)],
    direction: Direction,
) -> Vec<(usize, usize)> {
    let mut deduped: Vec<(usize, usize)> = Vec::with_capacity(points.len());
    for &point in points {
        if deduped.last().copied() != Some(point) {
            deduped.push(point);
        }
    }
    if deduped.len() <= 2 {
        return deduped;
    }

    let repaired = repair_terminal_staircase_draw_path(&deduped, direction);
    if repaired
        .windows(2)
        .all(|segment| draw_segment_is_axis_aligned(segment[0], segment[1]))
    {
        repaired
    } else {
        deduped
    }
}

fn repair_terminal_staircase_draw_path(
    points: &[(usize, usize)],
    direction: Direction,
) -> Vec<(usize, usize)> {
    if points.len() <= 4 {
        return points.to_vec();
    }

    let len = points.len();
    let a = points[len - 4];
    let b = points[len - 3];
    let c = points[len - 2];
    let d = points[len - 1];

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            if draw_segment_is_vertical(a, b)
                && draw_segment_is_horizontal(b, c)
                && draw_segment_is_vertical(c, d)
                && draw_segment_sign(b.1 as isize - a.1 as isize)
                    == draw_segment_sign(d.1 as isize - c.1 as isize)
                && draw_segment_sign(b.1 as isize - a.1 as isize) != 0
            {
                let pullback_y = if d.1 > c.1 {
                    c.1.saturating_sub(1)
                } else {
                    c.1.saturating_add(1)
                };
                let adjusted_b = (b.0, pullback_y);
                let adjusted_c = (c.0, pullback_y);
                if adjusted_b != a
                    && adjusted_b != d
                    && adjusted_c != a
                    && adjusted_c != d
                    && pullback_y != b.1
                    && !would_introduce_axial_turnback_draw_path(points, len - 4, a, adjusted_b)
                {
                    let mut compacted = points[..(len - 3)].to_vec();
                    compacted.push(adjusted_b);
                    compacted.push(adjusted_c);
                    compacted.push(d);
                    return compacted;
                }
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if draw_segment_is_horizontal(a, b)
                && draw_segment_is_vertical(b, c)
                && draw_segment_is_horizontal(c, d)
                && draw_segment_sign(b.0 as isize - a.0 as isize)
                    == draw_segment_sign(d.0 as isize - c.0 as isize)
                && draw_segment_sign(b.0 as isize - a.0 as isize) != 0
            {
                let pullback_x = if d.0 > c.0 {
                    c.0.saturating_sub(1)
                } else {
                    c.0.saturating_add(1)
                };
                let adjusted_b = (pullback_x, b.1);
                let adjusted_c = (pullback_x, c.1);
                if adjusted_b != a
                    && adjusted_b != d
                    && adjusted_c != a
                    && adjusted_c != d
                    && pullback_x != b.0
                    && !would_introduce_axial_turnback_draw_path(points, len - 4, a, adjusted_b)
                {
                    let mut compacted = points[..(len - 3)].to_vec();
                    compacted.push(adjusted_b);
                    compacted.push(adjusted_c);
                    compacted.push(d);
                    return compacted;
                }
            }
        }
    }

    points.to_vec()
}

fn would_introduce_axial_turnback_draw_path(
    points: &[(usize, usize)],
    anchor_idx: usize,
    anchor: (usize, usize),
    elbow: (usize, usize),
) -> bool {
    if anchor_idx == 0 || anchor_idx >= points.len() {
        return false;
    }

    let prefix = points[anchor_idx - 1];
    let dx1 = anchor.0 as isize - prefix.0 as isize;
    let dy1 = anchor.1 as isize - prefix.1 as isize;
    let dx2 = elbow.0 as isize - anchor.0 as isize;
    let dy2 = elbow.1 as isize - anchor.1 as isize;
    let cross = dx1 * dy2 - dy1 * dx2;
    let dot = dx1 * dx2 + dy1 * dy2;
    cross == 0 && dot < 0
}

fn draw_segment_is_vertical(start: (usize, usize), end: (usize, usize)) -> bool {
    start.0 == end.0 && start.1 != end.1
}

fn draw_segment_is_horizontal(start: (usize, usize), end: (usize, usize)) -> bool {
    start.1 == end.1 && start.0 != end.0
}

fn draw_segment_is_axis_aligned(start: (usize, usize), end: (usize, usize)) -> bool {
    draw_segment_is_vertical(start, end) || draw_segment_is_horizontal(start, end)
}

fn draw_segment_sign(delta: isize) -> i8 {
    match delta.cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn waypoint_is_outside_face(waypoint: (usize, usize), bounds: &NodeBounds, face: NodeFace) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);
    // Allow waypoints on the face boundary (>=, <=) — float-to-integer
    // rounding can place channel-lane waypoints exactly on the face edge.
    match face {
        NodeFace::Top => waypoint.1 <= top,
        NodeFace::Bottom => waypoint.1 >= bottom,
        NodeFace::Left => waypoint.0 <= left,
        NodeFace::Right => waypoint.0 >= right,
    }
}

fn segments_collide_with_other_nodes(segments: &[Segment], layout: &Layout, edge: &Edge) -> bool {
    layout.node_bounds.iter().any(|(node_id, bounds)| {
        if node_id == &edge.from || node_id == &edge.to {
            return false;
        }
        segments
            .iter()
            .any(|segment| segment_intersects_bounds(*segment, bounds))
    })
}

fn segment_intersects_bounds(segment: Segment, bounds: &NodeBounds) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    match segment {
        Segment::Vertical { x, y_start, y_end } => {
            if x < left || x > right {
                return false;
            }
            ranges_overlap(y_start, y_end, top, bottom)
        }
        Segment::Horizontal { y, x_start, x_end } => {
            if y < top || y > bottom {
                return false;
            }
            ranges_overlap(x_start, x_end, left, right)
        }
    }
}

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let (a_min, a_max) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
    let (b_min, b_max) = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
    a_min <= b_max && b_min <= a_max
}

fn repair_draw_path_segment_collisions(
    draw_path: &[(usize, usize)],
    layout: &Layout,
    edge: &Edge,
) -> Vec<(usize, usize)> {
    let mut repaired = draw_path.to_vec();
    repaired.dedup();
    if repaired.len() < 2 {
        return repaired;
    }

    let blockers: Vec<NodeBounds> = layout
        .node_bounds
        .iter()
        .filter(|(node_id, _)| *node_id != &edge.from && *node_id != &edge.to)
        .map(|(_, bounds)| *bounds)
        .collect();
    if blockers.is_empty() {
        return repaired;
    }

    let max_repairs = blockers.len().saturating_mul(repaired.len().max(1)) * 2;
    let mut repairs = 0usize;

    loop {
        let mut changed = false;

        for idx in 0..repaired.len().saturating_sub(1) {
            let from = repaired[idx];
            let to = repaired[idx + 1];
            let Some((blocker, vertical)) = first_blocking_draw_path_segment(from, to, &blockers)
            else {
                continue;
            };

            let detour = detour_draw_path_around_blocker(
                from,
                to,
                blocker,
                vertical,
                layout.width,
                layout.height,
            );
            if detour.is_empty() {
                continue;
            }

            repaired.splice(idx + 1..idx + 1, detour);
            repaired.dedup();
            changed = true;
            repairs += 1;
            break;
        }

        if !changed || repairs >= max_repairs {
            break;
        }
    }

    repaired
}

fn first_blocking_draw_path_segment(
    from: (usize, usize),
    to: (usize, usize),
    blockers: &[NodeBounds],
) -> Option<(NodeBounds, bool)> {
    if from == to {
        return None;
    }

    if from.0 == to.0 {
        let segment = Segment::Vertical {
            x: from.0,
            y_start: from.1,
            y_end: to.1,
        };
        return blockers
            .iter()
            .copied()
            .find(|bounds| segment_intersects_bounds(segment, bounds))
            .map(|bounds| (bounds, true));
    }

    if from.1 == to.1 {
        let segment = Segment::Horizontal {
            y: from.1,
            x_start: from.0,
            x_end: to.0,
        };
        return blockers
            .iter()
            .copied()
            .find(|bounds| segment_intersects_bounds(segment, bounds))
            .map(|bounds| (bounds, false));
    }

    None
}

fn detour_draw_path_around_blocker(
    from: (usize, usize),
    to: (usize, usize),
    blocker: NodeBounds,
    vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> Vec<(usize, usize)> {
    let mut detour = Vec::with_capacity(2);

    if vertical {
        let detour_x =
            choose_detour_coordinate(from.0, to.0, blocker.x, blocker.width, canvas_width);
        if detour_x != from.0 {
            detour.push((detour_x, from.1));
        }
        if detour.last().copied() != Some((detour_x, to.1)) {
            detour.push((detour_x, to.1));
        }
    } else {
        let detour_y =
            choose_detour_coordinate(from.1, to.1, blocker.y, blocker.height, canvas_height);
        if detour_y != from.1 {
            detour.push((from.0, detour_y));
        }
        if detour.last().copied() != Some((to.0, detour_y)) {
            detour.push((to.0, detour_y));
        }
    }

    detour
}

fn choose_detour_coordinate(
    start_coord: usize,
    end_coord: usize,
    blocker_origin: usize,
    blocker_span: usize,
    canvas_limit: usize,
) -> usize {
    let max_coord = canvas_limit.saturating_sub(1);
    let before = blocker_origin.saturating_sub(1);
    let after = blocker_origin
        .saturating_add(blocker_span)
        .saturating_add(1)
        .min(max_coord);

    let mut candidates = [before, after];
    candidates.sort_by_key(|candidate| {
        (
            start_coord.abs_diff(*candidate) + end_coord.abs_diff(*candidate),
            usize::MAX - *candidate,
        )
    });
    candidates[0]
}

fn nudge_routed_edge_clear_of_unrelated_subgraph_borders(
    mut routed: RoutedEdge,
    layout: &Layout,
    edge: &Edge,
) -> RoutedEdge {
    if layout.subgraph_bounds.is_empty() {
        return routed;
    }

    let mut points = polyline_points_from_segments(routed.start, &routed.segments);
    if points.last().copied() != Some(routed.end) {
        points.push(routed.end);
    }
    if points.len() < 4 {
        return routed;
    }

    let from_bounds = layout.node_bounds.get(&edge.from);
    let to_bounds = layout.node_bounds.get(&edge.to);
    let from_container = containing_subgraph_id(layout, &edge.from);
    let to_container = containing_subgraph_id(layout, &edge.to);
    let from_inside_any = from_container.is_some();
    let to_inside_any = to_container.is_some();

    let mut ordered_subgraph_bounds: Vec<(&String, &SubgraphBounds)> =
        layout.subgraph_bounds.iter().collect();
    ordered_subgraph_bounds.sort_by(|(left_id, left_bounds), (right_id, right_bounds)| {
        left_bounds
            .depth
            .cmp(&right_bounds.depth)
            .then_with(|| left_bounds.y.cmp(&right_bounds.y))
            .then_with(|| left_bounds.x.cmp(&right_bounds.x))
            .then_with(|| left_id.cmp(right_id))
    });

    for (_sg_id, sg) in ordered_subgraph_bounds {
        let from_inside = from_bounds.is_some_and(|bounds| node_inside_subgraph(bounds, sg));
        let to_inside = to_bounds.is_some_and(|bounds| node_inside_subgraph(bounds, sg));
        let is_inter_subgraph_crossing =
            from_inside != to_inside && from_container.is_some() && to_container.is_some();
        let allow_cross_boundary_nudge = (from_inside && !to_inside && !to_inside_any)
            || (to_inside && !from_inside && !from_inside_any);
        if (from_inside && to_inside) || is_inter_subgraph_crossing {
            continue;
        }

        let left = sg.x;
        let right = sg.x + sg.width.saturating_sub(1);
        let top = sg.y;
        let bottom = sg.y + sg.height.saturating_sub(1);

        if allow_cross_boundary_nudge {
            if from_inside {
                nudge_endpoint_segment_clear_of_subgraph_border(
                    &mut points,
                    left,
                    right,
                    top,
                    bottom,
                    true,
                );
            }
            if to_inside {
                nudge_endpoint_segment_clear_of_subgraph_border(
                    &mut points,
                    left,
                    right,
                    top,
                    bottom,
                    false,
                );
            }
        } else if from_inside || to_inside {
            continue;
        }

        for idx in 1..points.len().saturating_sub(2) {
            let current = points[idx];
            let next = points[idx + 1];

            if current.y == next.y && ranges_overlap(current.x, next.x, left, right) {
                if current.y == bottom || current.y == bottom.saturating_add(1) {
                    let target_y = bottom.saturating_add(2);
                    points[idx].y = target_y;
                    points[idx + 1].y = target_y;
                } else if current.y == top || current.y == top.saturating_sub(1) {
                    let target_y = top.saturating_sub(2);
                    points[idx].y = target_y;
                    points[idx + 1].y = target_y;
                }
            } else if current.x == next.x && ranges_overlap(current.y, next.y, top, bottom) {
                if current.x == right || current.x == right.saturating_add(1) {
                    let target_x = right.saturating_add(2);
                    points[idx].x = target_x;
                    points[idx + 1].x = target_x;
                } else if current.x == left || current.x == left.saturating_sub(1) {
                    let target_x = left.saturating_sub(2);
                    points[idx].x = target_x;
                    points[idx + 1].x = target_x;
                }
            }
        }
    }

    normalize_polyline_points(&mut points);
    if points
        .windows(2)
        .all(|segment| point_segment_is_axis_aligned(segment[0], segment[1]))
    {
        routed.start = points[0];
        routed.end = *points.last().unwrap_or(&routed.end);
        routed.segments = polyline_points_to_segments(&points);
    }

    routed
}

fn nudge_endpoint_segment_clear_of_subgraph_border(
    points: &mut Vec<Point>,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    source_segment: bool,
) {
    if points.len() < 2 {
        return;
    }

    let (current, next) = if source_segment {
        (points[0], points[1])
    } else {
        let len = points.len();
        (points[len - 2], points[len - 1])
    };

    if current.y == next.y && ranges_overlap(current.x, next.x, left, right) {
        let target_y = if current.y == bottom || current.y == bottom.saturating_add(1) {
            Some(bottom.saturating_add(2))
        } else if current.y == top || current.y == top.saturating_sub(1) {
            Some(top.saturating_sub(2))
        } else {
            None
        };
        if let Some(target_y) = target_y {
            if source_segment {
                let detour = Point::new(current.x, target_y);
                points[1].y = target_y;
                if detour != current && detour != points[1] {
                    points.insert(1, detour);
                }
            } else {
                let len = points.len();
                let detour = Point::new(next.x, target_y);
                points[len - 2].y = target_y;
                if detour != points[len - 2] && detour != next {
                    points.insert(len - 1, detour);
                }
            }
        }
    } else if current.x == next.x && ranges_overlap(current.y, next.y, top, bottom) {
        let target_x = if current.x == right || current.x == right.saturating_add(1) {
            Some(right.saturating_add(2))
        } else if current.x == left || current.x == left.saturating_sub(1) {
            Some(left.saturating_sub(2))
        } else {
            None
        };
        if let Some(target_x) = target_x {
            if source_segment {
                let detour = Point::new(target_x, current.y);
                points[1].x = target_x;
                if detour != current && detour != points[1] {
                    points.insert(1, detour);
                }
            } else {
                let len = points.len();
                let detour = Point::new(target_x, next.y);
                points[len - 2].x = target_x;
                if detour != points[len - 2] && detour != next {
                    points.insert(len - 1, detour);
                }
            }
        }
    }
}

/// Route an edge using waypoints from normalization.
///
/// Uses dynamic intersection calculation to determine attachment points
/// based on the approach angle from the first/last waypoint.
fn route_edge_with_waypoints(
    edge: &Edge,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Option<RoutedEdge> {
    // Calculate attachment points, using overrides where provided
    let (src_attach_raw, tgt_attach_raw) = resolve_attachment_points(
        overrides.src_attach,
        overrides.tgt_attach,
        ep,
        waypoints,
        direction,
    );

    // Clamp attachment points to actual node boundaries
    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|v| v == "1") {
        eprintln!(
            "[route] {} -> {}: waypoints={:?}",
            edge.from, edge.to, waypoints
        );
    }

    // Use face-aware offset to avoid corner ambiguity (e.g., a point at
    // the top-right corner should offset RIGHT for LR layouts, not UP)
    let is_backward = is_backward_edge(&ep.from_bounds, &ep.to_bounds, direction);
    let (src_face, tgt_face) = if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        (
            classify_face(&ep.from_bounds, src_attach, ep.from_shape),
            classify_face(&ep.to_bounds, tgt_attach, ep.to_shape),
        )
    } else {
        let (default_src_face, default_tgt_face) = edge_faces(direction, is_backward);
        (
            overrides.src_face.unwrap_or_else(|| {
                infer_face_from_attachment(&ep.from_bounds, src_attach, default_src_face)
            }),
            overrides.tgt_face.unwrap_or_else(|| {
                infer_face_from_attachment(&ep.to_bounds, tgt_attach, default_tgt_face)
            }),
        )
    };
    let mut start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);

    if let Some(&(wp_x, wp_y)) = waypoints.first() {
        let should_skip_offset = match src_face {
            NodeFace::Top => wp_y >= src_attach.1,
            NodeFace::Bottom => wp_y <= src_attach.1,
            NodeFace::Left => wp_x >= src_attach.0,
            NodeFace::Right => wp_x <= src_attach.0,
        };
        if should_skip_offset {
            start = Point::new(src_attach.0, src_attach.1);
        }
    }

    let mut segments = Vec::new();

    // Add connector segment from source node boundary to offset start point
    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    // When the layout's float-space waypoints collapse to ≤1 cell offset from the
    // straight path on the discrete text grid, they produce stub artifacts
    // (e.g. `├─`). Project them onto the straight path to eliminate the jog
    // while preserving segment splits (important for label placement).
    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let trivial_waypoints = !is_backward
        && ((vertical_first
            && start.x == end.x
            && waypoints.iter().all(|(x, _)| x.abs_diff(start.x) <= 1))
            || (!vertical_first
                && start.y == end.y
                && waypoints.iter().all(|(_, y)| y.abs_diff(start.y) <= 1)));

    if trivial_waypoints {
        // Project waypoints onto the straight axis, preserving splits
        let mut current = start;
        if vertical_first {
            for &(_, wp_y) in waypoints {
                if wp_y != current.y {
                    segments.push(Segment::Vertical {
                        x: start.x,
                        y_start: current.y,
                        y_end: wp_y,
                    });
                    current = Point::new(start.x, wp_y);
                }
            }
            if current.y != end.y {
                segments.push(Segment::Vertical {
                    x: start.x,
                    y_start: current.y,
                    y_end: end.y,
                });
            }
        } else {
            for &(wp_x, _) in waypoints {
                if wp_x != current.x {
                    segments.push(Segment::Horizontal {
                        y: start.y,
                        x_start: current.x,
                        x_end: wp_x,
                    });
                    current = Point::new(wp_x, start.y);
                }
            }
            if current.x != end.x {
                segments.push(Segment::Horizontal {
                    y: start.y,
                    x_start: current.x,
                    x_end: end.x,
                });
            }
        }
    } else {
        // Build orthogonal path through waypoints
        segments.extend(build_orthogonal_path_with_waypoints(
            start,
            waypoints,
            end,
            direction,
            overrides.src_first_vertical,
            edge.arrow_start != Arrow::None,
        ));
    }

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);

    // Determine entry direction based on final segment orientation
    let entry_direction = entry_direction_from_segments(&segments);

    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|v| v == "1") {
        eprintln!(
            "[route] {} -> {}: start={:?} end={:?} segments={:?}",
            edge.from, edge.to, start, end, segments
        );
    }

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        is_backward,
    ))
}

/// Route a backward edge using synthetic waypoints (generated by `generate_backward_waypoints`).
///
/// Unlike `route_edge_with_waypoints`, this determines faces from the waypoint
/// approach angle rather than the layout direction, since synthetic waypoints
/// route around the side of nodes.
fn route_backward_with_synthetic_waypoints(
    edge: &Edge,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Option<RoutedEdge> {
    // Use intersection calculation from waypoint approach angles
    let (src_attach_raw, tgt_attach_raw) = calculate_attachment_points(
        &ep.from_bounds,
        ep.from_shape,
        &ep.to_bounds,
        ep.to_shape,
        waypoints,
    );

    let src_attach_raw = overrides.src_attach.unwrap_or(src_attach_raw);
    let tgt_attach_raw = overrides.tgt_attach.unwrap_or(tgt_attach_raw);

    // Clamp to boundaries
    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    // Determine faces from waypoint approach angle
    let src_face = classify_face(&ep.from_bounds, waypoints[0], ep.from_shape);
    let tgt_face = classify_face(&ep.to_bounds, *waypoints.last().unwrap(), ep.to_shape);

    let start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);

    let mut segments = Vec::new();

    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    segments.extend(build_orthogonal_path_with_waypoints(
        start,
        waypoints,
        end,
        direction,
        overrides.src_first_vertical,
        edge.arrow_start != Arrow::None,
    ));

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);

    let entry_direction = entry_direction_from_segments(&segments);

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        true,
    ))
}

/// Route an edge directly between two nodes (no intermediate waypoints).
///
/// Uses intersection calculation to determine attachment points based on
/// the relative positions of the nodes.
fn route_edge_direct(
    edge: &Edge,
    ep: &EdgeEndpoints,
    direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RoutedEdge> {
    // For direct routing, use the other node's center as the "approach point"
    let empty_waypoints: &[(usize, usize)] = &[];
    let (mut src_attach_raw, mut tgt_attach_raw) = resolve_attachment_points(
        src_attach_override,
        tgt_attach_override,
        ep,
        empty_waypoints,
        direction,
    );
    let mut src_face_override = None;
    let mut tgt_face_override = None;
    if edge.from_subgraph.is_some() && src_attach_override.is_none() {
        let face = subgraph_edge_face(&ep.from_bounds, &ep.to_bounds, direction);
        src_face_override = Some(face);
        src_attach_raw = clamp_to_face(
            &ep.from_bounds,
            face,
            (ep.to_bounds.center_x(), ep.to_bounds.center_y()),
        );
    }
    if edge.to_subgraph.is_some() && tgt_attach_override.is_none() {
        let face = subgraph_edge_face(&ep.to_bounds, &ep.from_bounds, direction);
        tgt_face_override = Some(face);
        tgt_attach_raw = clamp_to_face(
            &ep.to_bounds,
            face,
            (ep.from_bounds.center_x(), ep.from_bounds.center_y()),
        );
    }

    // Clamp attachment points to actual node boundaries
    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    // Use face-aware offset to avoid corner ambiguity
    let is_backward = is_backward_edge(&ep.from_bounds, &ep.to_bounds, direction);
    let (src_face, tgt_face) = if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        (
            src_face_override
                .unwrap_or_else(|| classify_face(&ep.from_bounds, src_attach, ep.from_shape)),
            tgt_face_override
                .unwrap_or_else(|| classify_face(&ep.to_bounds, tgt_attach, ep.to_shape)),
        )
    } else {
        edge_faces(direction, is_backward)
    };
    let start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);
    let mut segments = Vec::new();

    // Add connector segment from source node boundary to offset start point
    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    // Build orthogonal path with direction-appropriate segment ordering.
    // For subgraph edges that attach on left/right faces, route horizontally
    // to avoid running straight through vertical stacks.
    let mut path_direction = direction;
    if (edge.from_subgraph.is_some() || edge.to_subgraph.is_some())
        && (matches!(src_face, NodeFace::Left | NodeFace::Right)
            || matches!(tgt_face, NodeFace::Left | NodeFace::Right))
    {
        path_direction = if start.x <= end.x {
            Direction::LeftRight
        } else {
            Direction::RightLeft
        };
    }
    // When src_first_vertical is false (horizontal-first stagger), turn
    // horizontal one row after the source so sibling edges separate early.
    // Fall back to standard routing when the stagger would land on the
    // target row (creating a turn+arrowhead on the same row, e.g. ┌▲).
    let stagger_mid_y = if !src_first_vertical
        && matches!(path_direction, Direction::TopDown | Direction::BottomTop)
        && start.x != end.x
        && start.y != end.y
    {
        let mid = if matches!(path_direction, Direction::TopDown) {
            start.y + 1
        } else {
            start.y.saturating_sub(1)
        };
        if mid != end.y { Some(mid) } else { None }
    } else {
        None
    };
    if let Some(mid_y) = stagger_mid_y {
        segments.push(Segment::Vertical {
            x: start.x,
            y_start: start.y,
            y_end: mid_y,
        });
        segments.push(Segment::Horizontal {
            y: mid_y,
            x_start: start.x,
            x_end: end.x,
        });
        segments.push(Segment::Vertical {
            x: end.x,
            y_start: mid_y,
            y_end: end.y,
        });
    } else {
        segments.extend(build_orthogonal_path_for_direction(
            start,
            end,
            path_direction,
        ));
    }

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);

    // Determine entry direction: use canonical direction when start == end
    // (zero-length path produces a degenerate segment that can't indicate direction)
    let entry_direction = if start == end {
        match direction {
            Direction::TopDown => AttachDirection::Top,
            Direction::BottomTop => AttachDirection::Bottom,
            Direction::LeftRight => AttachDirection::Left,
            Direction::RightLeft => AttachDirection::Right,
        }
    } else {
        entry_direction_from_segments(&segments)
    };

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        is_backward,
    ))
}

/// Resolve attachment points, using overrides when provided, falling back to
/// `calculate_attachment_points()` for non-overridden sides.
///
/// For LR/RL layouts, non-overridden attachment points are forced to the
/// appropriate side face (right/left for source, left/right for target)
/// instead of using geometric center-to-center intersection, which can
/// hit top/bottom faces when nodes aren't horizontally aligned.
fn resolve_attachment_points(
    src_override: Option<(usize, usize)>,
    tgt_override: Option<(usize, usize)>,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
) -> ((usize, usize), (usize, usize)) {
    let from_bounds = ep.from_bounds;
    let to_bounds = ep.to_bounds;

    let is_backward = is_backward_edge(&from_bounds, &to_bounds, direction);

    // LR/RL layouts: all edges attach to side faces.
    // Forward edges use consensus-y for straight horizontal segments.
    // Backward edges use center-y on the opposite side face.
    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            let flows_right = matches!(direction, Direction::LeftRight) != is_backward;
            let y = if is_backward {
                // Backward: each node uses its own center_y
                // (consensus doesn't apply since the edge wraps around)
                from_bounds.center_y()
            } else {
                consensus_y(&from_bounds, &to_bounds)
            };
            let tgt_y = if is_backward { to_bounds.center_y() } else { y };
            let (src, tgt) = if flows_right {
                // Source exits right face, target enters left face
                (
                    src_override.unwrap_or((from_bounds.x + from_bounds.width - 1, y)),
                    tgt_override.unwrap_or((to_bounds.x, tgt_y)),
                )
            } else {
                // Source exits left face, target enters right face
                (
                    src_override.unwrap_or((from_bounds.x, y)),
                    tgt_override.unwrap_or((to_bounds.x + to_bounds.width - 1, tgt_y)),
                )
            };
            return (src, tgt);
        }
        _ => {}
    }

    if matches!(direction, Direction::TopDown | Direction::BottomTop)
        && let (Some(&first_wp), Some(&last_wp)) = (waypoints.first(), waypoints.last())
    {
        let (src_face, tgt_face) = if is_backward && waypoints.len() <= 1 {
            let (default_src_face, default_tgt_face) = edge_faces(direction, is_backward);
            let inferred_src_face = classify_face(&from_bounds, first_wp, ep.from_shape);
            let inferred_tgt_face = classify_face(&to_bounds, last_wp, ep.to_shape);
            (
                if matches!(inferred_src_face, NodeFace::Left | NodeFace::Right) {
                    inferred_src_face
                } else {
                    default_src_face
                },
                if matches!(inferred_tgt_face, NodeFace::Left | NodeFace::Right) {
                    inferred_tgt_face
                } else {
                    default_tgt_face
                },
            )
        } else {
            edge_faces(direction, is_backward)
        };
        let src = src_override.unwrap_or_else(|| clamp_to_face(&from_bounds, src_face, first_wp));
        let tgt = tgt_override.unwrap_or_else(|| clamp_to_face(&to_bounds, tgt_face, last_wp));
        return (src, tgt);
    }

    // TD/BT layouts: use geometric intersection to find attachment points.
    let fallback = || {
        calculate_attachment_points(
            &from_bounds,
            ep.from_shape,
            &to_bounds,
            ep.to_shape,
            waypoints,
        )
    };
    let src = src_override.unwrap_or_else(|| fallback().0);
    let tgt = tgt_override.unwrap_or_else(|| fallback().1);
    (src, tgt)
}

/// Clamp a waypoint to a node face, returning a point on that face.
fn clamp_to_face(bounds: &NodeBounds, face: NodeFace, waypoint: (usize, usize)) -> (usize, usize) {
    let (min, max) = bounds.face_extent(&face);
    let fixed = bounds.face_fixed_coord(&face);
    match face {
        NodeFace::Top | NodeFace::Bottom => (waypoint.0.clamp(min, max), fixed),
        NodeFace::Left | NodeFace::Right => (fixed, waypoint.1.clamp(min, max)),
    }
}

fn infer_face_from_attachment(
    bounds: &NodeBounds,
    attach: (usize, usize),
    fallback: NodeFace,
) -> NodeFace {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    if attach.0 == left {
        NodeFace::Left
    } else if attach.0 == right {
        NodeFace::Right
    } else if attach.1 == top {
        NodeFace::Top
    } else if attach.1 == bottom {
        NodeFace::Bottom
    } else {
        fallback
    }
}

fn ensure_terminal_face_support(
    segments: &mut Vec<Segment>,
    start: Point,
    end: Point,
    target_face: NodeFace,
) {
    let mut points = polyline_points_from_segments(start, segments);
    if points.last().copied() != Some(end) {
        points.push(end);
    }
    normalize_polyline_points(&mut points);
    let original_points = points.clone();
    if points.len() < 2 || terminal_support_matches_face(&points, target_face) {
        *segments = polyline_points_to_segments(&points);
        return;
    }

    let support = terminal_support_point(end, target_face);
    if support == end {
        *segments = polyline_points_to_segments(&points);
        return;
    }

    let pre_end_idx = points.len() - 2;
    match target_face {
        NodeFace::Top | NodeFace::Bottom => {
            let anchor = points[pre_end_idx];
            let adjusted_anchor = Point::new(anchor.x, support.y);
            points[pre_end_idx] = adjusted_anchor;
            if adjusted_anchor.x != end.x {
                points.insert(points.len() - 1, Point::new(end.x, support.y));
            }
        }
        NodeFace::Left | NodeFace::Right => {
            let anchor = points[pre_end_idx];
            let adjusted_anchor = Point::new(support.x, anchor.y);
            points[pre_end_idx] = adjusted_anchor;
            if adjusted_anchor.y != end.y {
                points.insert(points.len() - 1, Point::new(support.x, end.y));
            }
        }
    }

    normalize_polyline_points(&mut points);
    if points
        .windows(2)
        .all(|segment| point_segment_is_axis_aligned(segment[0], segment[1]))
    {
        *segments = polyline_points_to_segments(&points);
    } else {
        *segments = polyline_points_to_segments(&original_points);
    }
}

fn ensure_source_face_launch_support(
    segments: &mut Vec<Segment>,
    start: Point,
    source_face: NodeFace,
) {
    let mut points = polyline_points_from_segments(start, segments);
    if points.len() < 2 {
        return;
    }

    let next = points[1];
    let (support, corner) = match source_face {
        NodeFace::Top if next.y == start.y => {
            let support = Point::new(start.x, start.y.saturating_sub(1));
            (support, Point::new(next.x, support.y))
        }
        NodeFace::Bottom if next.y == start.y => {
            let support = Point::new(start.x, start.y + 1);
            (support, Point::new(next.x, support.y))
        }
        NodeFace::Left if next.x == start.x => {
            let support = Point::new(start.x.saturating_sub(1), start.y);
            (support, Point::new(support.x, next.y))
        }
        NodeFace::Right if next.x == start.x => {
            let support = Point::new(start.x + 1, start.y);
            (support, Point::new(support.x, next.y))
        }
        _ => return,
    };

    if support == start || support == next {
        return;
    }

    points.insert(1, support);
    if corner != start && corner != support && corner != next {
        points.insert(2, corner);
    }
    normalize_polyline_points(&mut points);
    *segments = polyline_points_to_segments(&points);
}

fn polyline_points_from_segments(start: Point, segments: &[Segment]) -> Vec<Point> {
    let mut points = vec![start];
    for segment in segments {
        let end = segment.end_point();
        if points.last().copied() != Some(end) {
            points.push(end);
        }
    }
    points
}

fn normalize_polyline_points(points: &mut Vec<Point>) {
    points.dedup();
    let mut idx = 1;
    while idx + 1 < points.len() {
        let prev = points[idx - 1];
        let curr = points[idx];
        let next = points[idx + 1];
        let collinear_vertical = prev.x == curr.x && curr.x == next.x;
        let collinear_horizontal = prev.y == curr.y && curr.y == next.y;
        if collinear_vertical || collinear_horizontal {
            points.remove(idx);
        } else {
            idx += 1;
        }
    }
}

fn polyline_points_to_segments(points: &[Point]) -> Vec<Segment> {
    let mut segments = Vec::new();
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start == end {
            continue;
        }
        if start.x == end.x {
            segments.push(Segment::Vertical {
                x: start.x,
                y_start: start.y,
                y_end: end.y,
            });
        } else if start.y == end.y {
            segments.push(Segment::Horizontal {
                y: start.y,
                x_start: start.x,
                x_end: end.x,
            });
        } else {
            debug_assert!(
                false,
                "polyline_points_to_segments requires axis-aligned points: {start:?} -> {end:?}"
            );
        }
    }
    segments
}

fn point_segment_is_axis_aligned(start: Point, end: Point) -> bool {
    start.x == end.x || start.y == end.y
}

fn terminal_support_matches_face(points: &[Point], target_face: NodeFace) -> bool {
    if points.len() < 2 {
        return false;
    }

    let prev = points[points.len() - 2];
    let end = points[points.len() - 1];
    match target_face {
        NodeFace::Top => prev.x == end.x && prev.y < end.y,
        NodeFace::Bottom => prev.x == end.x && prev.y > end.y,
        NodeFace::Left => prev.y == end.y && prev.x < end.x,
        NodeFace::Right => prev.y == end.y && prev.x > end.x,
    }
}

fn terminal_support_point(end: Point, target_face: NodeFace) -> Point {
    match target_face {
        NodeFace::Top => Point::new(end.x, end.y.saturating_sub(1)),
        NodeFace::Bottom => Point::new(end.x, end.y + 1),
        NodeFace::Left => Point::new(end.x.saturating_sub(1), end.y),
        NodeFace::Right => Point::new(end.x + 1, end.y),
    }
}

/// Compute a shared y-coordinate for LR/RL attachment, clamped to both nodes' y-ranges.
fn consensus_y(a: &NodeBounds, b: &NodeBounds) -> usize {
    let avg = (a.center_y() + b.center_y()) / 2;
    avg.max(a.y)
        .min(a.y + a.height - 1)
        .max(b.y)
        .min(b.y + b.height - 1)
}

/// Clamp an attachment point to the actual node boundary.
///
/// The intersection calculation may return points slightly outside the
/// actual boundary cells due to rounding. This function ensures the
/// point is on a valid boundary cell.
fn clamp_to_boundary(point: (usize, usize), bounds: &NodeBounds) -> Point {
    let (x, y) = point;

    let left = bounds.x;
    let right = bounds.x + bounds.width - 1;
    let top = bounds.y;
    let bottom = bounds.y + bounds.height - 1;

    let clamped_x = x.clamp(left, right);
    let clamped_y = y.clamp(top, bottom);

    Point::new(clamped_x, clamped_y)
}

/// Determine the source and target faces for an edge based on layout direction
/// and whether the edge is backward.
///
/// Forward edges exit the "downstream" face and enter the "upstream" face.
/// Backward edges reverse these faces.
fn edge_faces(direction: Direction, is_backward: bool) -> (NodeFace, NodeFace) {
    let (src, tgt) = shared_edge_faces(direction, is_backward);
    (src.to_node_face(), tgt.to_node_face())
}

/// Offset an attachment point by 1 cell in the direction of the given face.
///
/// Unlike `offset_from_boundary`, this doesn't infer the face from position,
/// avoiding ambiguity when the point is at a corner of the node.
fn offset_for_face(point: (usize, usize), face: NodeFace) -> Point {
    let (x, y) = point;
    match face {
        NodeFace::Top => Point::new(x, y.saturating_sub(1)),
        NodeFace::Bottom => Point::new(x, y + 1),
        NodeFace::Left => Point::new(x.saturating_sub(1), y),
        NodeFace::Right => Point::new(x + 1, y),
    }
}

/// Add a connector segment from a node boundary point to an offset point.
///
/// This creates the short segment that visually connects the edge to the node.
fn add_connector_segment(segments: &mut Vec<Segment>, boundary: (usize, usize), offset: Point) {
    let (bx, by) = boundary;
    if bx == offset.x {
        // Vertical connector
        segments.push(Segment::Vertical {
            x: bx,
            y_start: by,
            y_end: offset.y,
        });
    } else if by == offset.y {
        // Horizontal connector
        segments.push(Segment::Horizontal {
            y: by,
            x_start: bx,
            x_end: offset.x,
        });
    }
    // If neither aligned, skip (shouldn't happen with proper offset)
}

/// Determine the entry direction based on the final segment's orientation.
///
/// With L-shaped paths, the final segment directly represents the approach direction:
/// - Vertical final segment going down → entry from Top (arrow ▼)
/// - Vertical final segment going up → entry from Bottom (arrow ▲)
/// - Horizontal final segment going right → entry from Left (arrow ►)
/// - Horizontal final segment going left → entry from Right (arrow ◄)
fn entry_direction_from_segments(segments: &[Segment]) -> AttachDirection {
    match segments.iter().rev().find(|segment| segment.length() > 0) {
        Some(Segment::Vertical { y_start, y_end, .. }) if *y_end > *y_start => AttachDirection::Top,
        Some(Segment::Vertical { .. }) => AttachDirection::Bottom,
        Some(Segment::Horizontal { x_start, x_end, .. }) if *x_end > *x_start => {
            AttachDirection::Left
        }
        Some(Segment::Horizontal { .. }) => AttachDirection::Right,
        None => AttachDirection::Top,
    }
}

/// Build an orthogonal path that ends with a segment matching the approach direction.
///
/// The final segment orientation determines the arrow direction:
/// - Vertical final segment → ▼ or ▲ arrow
/// - Horizontal final segment → ► or ◄ arrow
///
/// Creates an L-shaped path (2 segments) when start and end are not aligned:
/// - First segment moves in the primary layout direction
/// - Second segment moves in the cross direction to reach the target
///
/// This ensures the arrow glyph visually matches the line direction entering the target.
///
/// For edges with large horizontal offset (source far from target horizontally),
/// the routing is adjusted to place the horizontal segment closer to the target,
/// avoiding the congested middle region of the diagram.
fn build_orthogonal_path_for_direction(
    start: Point,
    end: Point,
    direction: Direction,
) -> Vec<Segment> {
    // If start and end are already aligned, just create a single segment
    if start.x == end.x {
        return vec![Segment::Vertical {
            x: start.x,
            y_start: start.y,
            y_end: end.y,
        }];
    }
    if start.y == end.y {
        return vec![Segment::Horizontal {
            y: start.y,
            x_start: start.x,
            x_end: end.x,
        }];
    }

    // For non-aligned paths, the final segment should match the layout's canonical
    // entry direction so arrows visually connect to the expected side of the target:
    // - TD/BT: final segment is vertical (arrows ▼/▲ enter from top/bottom)
    // - LR/RL: final segment is horizontal (arrows ►/◄ enter from left/right)
    //
    // Both use Z-shaped paths to ensure the correct entry direction:
    // - TD/BT: V-H-V (vertical entry)
    // - LR/RL: H-V-H (horizontal entry)
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            // Vertical layouts: V-H-V (Z-shape) to enter target from top/bottom
            // Final segment is vertical, so arrow will be ▼ or ▲
            let mid_y = compute_mid_y_for_vertical_layout(start, end, direction);
            vec![
                Segment::Vertical {
                    x: start.x,
                    y_start: start.y,
                    y_end: mid_y,
                },
                Segment::Horizontal {
                    y: mid_y,
                    x_start: start.x,
                    x_end: end.x,
                },
                Segment::Vertical {
                    x: end.x,
                    y_start: mid_y,
                    y_end: end.y,
                },
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            // Horizontal layouts: H-V-H (Z-shape) to enter target from left/right
            // Final segment is horizontal, so arrow will be ► or ◄
            let mid_x = (start.x + end.x) / 2;
            vec![
                Segment::Horizontal {
                    y: start.y,
                    x_start: start.x,
                    x_end: mid_x,
                },
                Segment::Vertical {
                    x: mid_x,
                    y_start: start.y,
                    y_end: end.y,
                },
                Segment::Horizontal {
                    y: end.y,
                    x_start: mid_x,
                    x_end: end.x,
                },
            ]
        }
    }
}

/// Compute the Y coordinate for the horizontal segment in a Z-shaped path.
///
/// For edges with large horizontal offset (source far right, target more centered),
/// we place the horizontal segment closer to the target to avoid routing through
/// the congested middle region of the diagram. This creates an asymmetric Z-path
/// that "hugs" the bottom (for TD) or top (for BT).
///
/// For normal edges, uses the standard midpoint calculation.
fn compute_mid_y_for_vertical_layout(start: Point, end: Point, direction: Direction) -> usize {
    let horizontal_offset = start.x.abs_diff(end.x);

    // Check if this edge has a large horizontal offset
    let mut mid_y = if horizontal_offset > SHARED_LARGE_HORIZONTAL_OFFSET_THRESHOLD {
        // Determine if source is to the right of target (right-to-left routing)
        let is_right_to_left = start.x > end.x;

        if is_right_to_left {
            // For right-to-left edges with large offset:
            // Place the horizontal segment closer to the target to avoid
            // crossing through the congested middle of the diagram.
            //
            // For TD: place horizontal near the bottom (closer to end.y)
            // For BT: place horizontal near the top (closer to end.y, which is lower)
            match direction {
                Direction::TopDown => {
                    // TD: end.y > start.y, so we want mid_y close to end.y
                    // Leave room for the final vertical segment (at least 2 rows)
                    let target_mid = end.y.saturating_sub(2);
                    // But don't go above the standard midpoint (avoid going too high)
                    let standard_mid = (start.y + end.y) / 2;
                    target_mid.max(standard_mid)
                }
                Direction::BottomTop => {
                    // BT: end.y < start.y, so we want mid_y close to end.y
                    // Leave room for the final vertical segment (at least 2 rows)
                    let target_mid = end.y + 2;
                    // But don't go below the standard midpoint
                    let standard_mid = (start.y + end.y) / 2;
                    target_mid.min(standard_mid)
                }
                _ => (start.y + end.y) / 2,
            }
        } else {
            // Left-to-right edges: use standard midpoint
            // (these typically don't have the same congestion issue)
            (start.y + end.y) / 2
        }
    } else {
        // Normal edges: use standard midpoint
        (start.y + end.y) / 2
    };

    // Avoid placing the horizontal segment on the target row, which creates a
    // zero-length final vertical segment and makes arrowheads look like they
    // attach to horizontal lines.
    if mid_y == end.y {
        if start.y > end.y {
            mid_y = end.y + 1;
        } else {
            mid_y = end.y.saturating_sub(1);
        }
    }

    mid_y
}

/// Build an orthogonal path through waypoints, ending with appropriate segment for layout.
///
/// Similar to build_orthogonal_path but ensures the final segment type matches the
/// layout direction for proper arrow positioning.
fn build_orthogonal_path_with_waypoints(
    start: Point,
    waypoints: &[(usize, usize)],
    end: Point,
    direction: Direction,
    start_vertical: bool,
    has_arrow_start: bool,
) -> Vec<Segment> {
    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);

    if waypoints.is_empty() {
        // No waypoints: use direction-appropriate path
        return build_orthogonal_path_for_direction(start, end, direction);
    }

    let mut start_vertical_override = start_vertical;
    let mut waypoint_slice = waypoints;
    if vertical_first
        && let Some(&(wp_x, wp_y)) = waypoint_slice.first()
        && wp_y == start.y
    {
        waypoint_slice = &waypoint_slice[1..];
        if wp_x != start.x {
            start_vertical_override = false;
        }
    }
    if waypoint_slice.is_empty() {
        return build_orthogonal_path_for_direction(start, end, direction);
    }
    if vertical_first && start.x != end.x && waypoint_slice.iter().all(|(x, _)| *x == end.x) {
        let mut mid_y = start.y;
        if mid_y == end.y {
            mid_y = match direction {
                Direction::TopDown => end.y.saturating_sub(1),
                Direction::BottomTop => end.y.saturating_add(1),
                _ => mid_y,
            };
        }
        // When the edge has a visible arrow_start marker, avoid placing
        // the horizontal jog at the start row. Nudge mid_y one cell in
        // the layout direction so there is always a vertical segment
        // approaching the source-end arrow.
        if has_arrow_start && mid_y == start.y && start.y != end.y {
            mid_y = match direction {
                Direction::TopDown => start.y + 1,
                Direction::BottomTop => start.y.saturating_sub(1),
                _ => mid_y,
            };
        }

        let mut segments = Vec::new();
        if start.y != mid_y {
            segments.push(Segment::Vertical {
                x: start.x,
                y_start: start.y,
                y_end: mid_y,
            });
        }
        segments.push(Segment::Horizontal {
            y: mid_y,
            x_start: start.x,
            x_end: end.x,
        });
        if mid_y != end.y {
            segments.push(Segment::Vertical {
                x: end.x,
                y_start: mid_y,
                y_end: end.y,
            });
        }
        return segments;
    }

    let mut segments = Vec::new();

    // Start → first waypoint
    let first_wp = Point::new(waypoint_slice[0].0, waypoint_slice[0].1);
    let first_vertical = start_vertical_override || !vertical_first;

    segments.extend(orthogonalize_segment(start, first_wp, first_vertical));

    // Through all intermediate waypoints
    for window in waypoint_slice.windows(2) {
        let from = Point::new(window[0].0, window[0].1);
        let to = Point::new(window[1].0, window[1].1);
        segments.extend(orthogonalize_segment(from, to, !vertical_first));
    }

    // Last waypoint → end: use direction-appropriate final segment
    let &(last_x, last_y) = waypoint_slice.last().unwrap();
    let last_wp = Point::new(last_x, last_y);
    segments.extend(build_orthogonal_path_for_direction(last_wp, end, direction));

    segments
}

/// Compute path preferring vertical movement first (used in tests).
///
/// Delegates to `build_orthogonal_path_for_direction` with TD direction,
/// which produces the same V-H-V Z-shaped path.
#[cfg(test)]
fn compute_vertical_first_path(start: Point, end: Point) -> Vec<Segment> {
    build_orthogonal_path_for_direction(start, end, Direction::TopDown)
}

/// Convert a single diagonal segment into orthogonal (axis-aligned) segments.
///
/// For diagonal paths (where neither x nor y coordinates match), this creates
/// a Z-shaped path with a horizontal and vertical component. The direction
/// preference determines whether we go vertical-first or horizontal-first.
///
/// # Arguments
/// * `from` - Starting point
/// * `to` - Ending point
/// * `vertical_first` - If true, prefer vertical-then-horizontal routing (for TD/BT).
///   If false, prefer horizontal-then-vertical routing (for LR/RL).
fn orthogonalize_segment(from: Point, to: Point, vertical_first: bool) -> Vec<Segment> {
    if from == to {
        vec![]
    } else if from.x == to.x {
        // Already vertical
        vec![Segment::Vertical {
            x: from.x,
            y_start: from.y,
            y_end: to.y,
        }]
    } else if from.y == to.y {
        // Already horizontal
        vec![Segment::Horizontal {
            y: from.y,
            x_start: from.x,
            x_end: to.x,
        }]
    } else if vertical_first {
        // Z-path: vertical → horizontal
        // Go vertically from `from` to `to.y`, then horizontally to `to.x`
        vec![
            Segment::Vertical {
                x: from.x,
                y_start: from.y,
                y_end: to.y,
            },
            Segment::Horizontal {
                y: to.y,
                x_start: from.x,
                x_end: to.x,
            },
        ]
    } else {
        // Z-path: horizontal → vertical
        // Go horizontally from `from` to `to.x`, then vertically to `to.y`
        vec![
            Segment::Horizontal {
                y: from.y,
                x_start: from.x,
                x_end: to.x,
            },
            Segment::Vertical {
                x: to.x,
                y_start: from.y,
                y_end: to.y,
            },
        ]
    }
}

/// Convert a series of waypoints into orthogonal path segments.
///
/// Waypoints from the layout's normalization may be at arbitrary positions. This
/// function converts each consecutive pair of points into axis-aligned segments,
/// creating Z-paths for any diagonal sections.
///
/// # Arguments
/// * `waypoints` - List of (x, y) coordinates representing the path
/// * `direction` - Layout direction (determines vertical-first vs horizontal-first preference)
///
/// # Returns
/// A list of orthogonal segments connecting all waypoints.
#[cfg(test)]
pub fn orthogonalize(waypoints: &[(usize, usize)], direction: Direction) -> Vec<Segment> {
    if waypoints.len() < 2 {
        return Vec::new();
    }

    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let mut segments = Vec::new();

    for window in waypoints.windows(2) {
        let from = Point::new(window[0].0, window[0].1);
        let to = Point::new(window[1].0, window[1].1);
        segments.extend(orthogonalize_segment(from, to, vertical_first));
    }

    segments
}

/// Build a complete orthogonal path from start attachment through waypoints to end attachment.
///
/// This is the main entry point for creating routed edge paths that use waypoint
/// information from normalization and dynamic attachment points from intersection
/// calculation.
///
/// # Arguments
/// * `start` - Attachment point on the source node boundary
/// * `waypoints` - Intermediate waypoints from dummy node positions (may be empty)
/// * `end` - Attachment point on the target node boundary
/// * `direction` - Layout direction
///
/// # Returns
/// A list of orthogonal segments forming the complete path from start to end.
#[cfg(test)]
pub fn build_orthogonal_path(
    start: Point,
    waypoints: &[(usize, usize)],
    end: Point,
    direction: Direction,
) -> Vec<Segment> {
    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);

    if waypoints.is_empty() {
        // No intermediate waypoints: direct path from start to end
        return orthogonalize_segment(start, end, vertical_first);
    }

    let mut segments = Vec::new();

    // Start → first waypoint
    let first_wp = Point::new(waypoints[0].0, waypoints[0].1);
    segments.extend(orthogonalize_segment(start, first_wp, vertical_first));

    // Through all waypoints
    for window in waypoints.windows(2) {
        let from = Point::new(window[0].0, window[0].1);
        let to = Point::new(window[1].0, window[1].1);
        segments.extend(orthogonalize_segment(from, to, vertical_first));
    }

    // Last waypoint → end
    let &(last_x, last_y) = waypoints.last().unwrap();
    let last_wp = Point::new(last_x, last_y);
    segments.extend(orthogonalize_segment(last_wp, end, vertical_first));

    segments
}

/// Pre-computed attachment override for one edge.
#[derive(Debug, Clone)]
pub struct AttachmentOverride {
    pub source: Option<(usize, usize)>,
    pub target: Option<(usize, usize)>,
    pub source_first_vertical: bool,
}

/// Compute pre-assigned attachment points for edges that share a node face.
///
/// Only produces overrides for faces with >1 edge. Single-edge faces
/// use the default intersect_rect() calculation (no override).
pub fn compute_attachment_plan(
    edges: &[Edge],
    layout: &Layout,
    direction: Direction,
) -> HashMap<usize, AttachmentOverride> {
    compute_attachment_plan_from_shared_planner(edges, layout, direction)
}

fn compute_attachment_plan_from_shared_planner(
    edges: &[Edge],
    layout: &Layout,
    direction: Direction,
) -> HashMap<usize, AttachmentOverride> {
    let shared = shared_plan_attachments(edges, layout, direction);
    let mut overrides: HashMap<usize, AttachmentOverride> = HashMap::new();

    for edge in edges {
        if edge.from == edge.to || edge.stroke == Stroke::Invisible {
            continue;
        }

        let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
        let tgt_id = edge.to_subgraph.as_deref().unwrap_or(edge.to.as_str());

        let Some(attachments) = shared.edge(edge.index) else {
            continue;
        };

        let entry = overrides.entry(edge.index).or_insert(AttachmentOverride {
            source: None,
            target: None,
            source_first_vertical: false,
        });

        if let Some(source_attachment) = attachments.source
            && shared.group_size(src_id, source_attachment.face) > 1
            && let Some(src_bounds) = bounds_for_node_id(layout, src_id)
        {
            let group_size = shared.group_size(src_id, source_attachment.face);
            entry.source = Some(point_on_face_grid(
                &src_bounds,
                source_attachment.face.to_node_face(),
                source_attachment.fraction,
                group_size,
            ));
        }

        if let Some(target_attachment) = attachments.target
            && shared.group_size(tgt_id, target_attachment.face) > 1
            && let Some(tgt_bounds) = bounds_for_node_id(layout, tgt_id)
        {
            let group_size = shared.group_size(tgt_id, target_attachment.face);
            entry.target = Some(point_on_face_grid(
                &tgt_bounds,
                target_attachment.face.to_node_face(),
                target_attachment.fraction,
                group_size,
            ));
        }
    }

    // Preserve existing TD/BT source-lane staggering behavior for long edges.
    let flow_face = match direction {
        Direction::TopDown => Some(SharedFace::Bottom),
        Direction::BottomTop => Some(SharedFace::Top),
        _ => None,
    };
    if let Some(flow_face) = flow_face {
        let mut side_lanes: HashMap<(String, i8), Vec<(usize, f64)>> = HashMap::new();
        let mut override_side_lanes: HashMap<(String, i8), Vec<(usize, f64)>> = HashMap::new();
        for edge in edges {
            if edge.from == edge.to || edge.stroke == Stroke::Invisible {
                continue;
            }
            let has_waypoints = edge.from_subgraph.is_none()
                && edge.to_subgraph.is_none()
                && layout
                    .edge_waypoints
                    .get(&edge.index)
                    .is_some_and(|wps| !wps.is_empty());
            let Some(source_attachment) = shared.edge(edge.index).and_then(|a| a.source) else {
                continue;
            };
            if source_attachment.face != flow_face {
                continue;
            }
            let Some((src_bounds, tgt_bounds)) = resolve_edge_bounds(layout, edge) else {
                continue;
            };
            let cross = if has_waypoints {
                let Some(first_wp) = layout
                    .edge_waypoints
                    .get(&edge.index)
                    .and_then(|wps| wps.first())
                    .copied()
                else {
                    continue;
                };
                match source_attachment.face {
                    SharedFace::Top | SharedFace::Bottom => first_wp.0 as f64,
                    SharedFace::Left | SharedFace::Right => first_wp.1 as f64,
                }
            } else {
                match source_attachment.face {
                    SharedFace::Top | SharedFace::Bottom => tgt_bounds.center_x() as f64,
                    SharedFace::Left | SharedFace::Right => tgt_bounds.center_y() as f64,
                }
            };
            let center_cross = match source_attachment.face {
                SharedFace::Top | SharedFace::Bottom => src_bounds.center_x() as f64,
                SharedFace::Left | SharedFace::Right => src_bounds.center_y() as f64,
            };
            let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
            let side = if cross >= center_cross { 1 } else { -1 };
            if has_waypoints {
                side_lanes
                    .entry((src_id.to_string(), side))
                    .or_default()
                    .push((edge.index, cross));
            } else {
                let target_in_override = layout
                    .node_directions
                    .get(&edge.to)
                    .is_some_and(|d| *d != direction);
                if target_in_override {
                    override_side_lanes
                        .entry((src_id.to_string(), side))
                        .or_default()
                        .push((edge.index, cross));
                }
            }
        }

        for ((_node_id, _side), mut lanes) in side_lanes {
            if lanes.len() <= 1 {
                continue;
            }
            lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (idx, (edge_index, _)) in lanes.into_iter().enumerate() {
                if let Some(entry) = overrides.get_mut(&edge_index) {
                    entry.source_first_vertical = idx % 2 == 1;
                }
            }
        }

        for ((_node_id, _side), mut lanes) in override_side_lanes {
            if lanes.len() <= 1 {
                continue;
            }
            lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (idx, (edge_index, _)) in lanes.into_iter().enumerate() {
                if let Some(entry) = overrides.get_mut(&edge_index) {
                    entry.source_first_vertical = idx % 2 == 0;
                }
            }
        }
    }

    overrides.retain(|_, ov| ov.source.is_some() || ov.target.is_some());
    overrides
}

fn point_on_face_grid(
    bounds: &NodeBounds,
    face: NodeFace,
    fraction: f64,
    group_size: usize,
) -> (usize, usize) {
    if group_size == 0 {
        return (bounds.center_x(), bounds.center_y());
    }

    let points = spread_points_on_face(
        face,
        bounds.face_fixed_coord(&face),
        bounds.face_extent(&face),
        group_size,
    );
    if group_size == 1 {
        return points[0];
    }

    let fraction = fraction.clamp(0.0, 1.0);
    let rank = ((group_size - 1) as f64 * fraction).round() as usize;
    points[rank.min(group_size - 1)]
}

/// Route a self-edge as orthogonal segments from pre-computed draw-coordinate points.
fn route_self_edge(data: &SelfEdgeDrawData, edge: &Edge, direction: Direction) -> RoutedEdge {
    let segments: Vec<Segment> = data
        .points
        .windows(2)
        .flat_map(|window| {
            let (x1, y1) = window[0];
            let (x2, y2) = window[1];

            match (x1 == x2, y1 == y2) {
                (_, true) => vec![Segment::Horizontal {
                    y: y1,
                    x_start: x1.min(x2),
                    x_end: x1.max(x2),
                }],
                (true, _) => vec![Segment::Vertical {
                    x: x1,
                    y_start: y1.min(y2),
                    y_end: y1.max(y2),
                }],
                // Diagonal — split into L-shape (shouldn't happen with orthogonal points)
                _ => vec![
                    Segment::Vertical {
                        x: x1,
                        y_start: y1.min(y2),
                        y_end: y1.max(y2),
                    },
                    Segment::Horizontal {
                        y: y2,
                        x_start: x1.min(x2),
                        x_end: x1.max(x2),
                    },
                ],
            }
        })
        .collect();

    let to_point = |&(x, y)| Point::new(x, y);
    let start = data
        .points
        .first()
        .map(to_point)
        .unwrap_or(Point::new(0, 0));
    let end = data.points.last().map(to_point).unwrap_or(Point::new(0, 0));

    // Entry direction: the arrow enters from the side where the loop is.
    // TD/BT: loop is on the right face. LR/RL: loop is on the bottom face.
    let entry_direction = match direction {
        Direction::TopDown | Direction::BottomTop => AttachDirection::Right,
        Direction::LeftRight | Direction::RightLeft => AttachDirection::Bottom,
    };

    RoutedEdge {
        edge: edge.clone(),
        start,
        end,
        segments,
        source_connection: None,
        entry_direction,
        is_backward: false,
        is_self_edge: true,
    }
}

/// Route all edges in the layout.
pub fn route_all_edges(
    edges: &[Edge],
    layout: &Layout,
    diagram_direction: Direction,
) -> Vec<RoutedEdge> {
    // Pre-pass: compute attachment plan for edges sharing a face
    let plan = compute_attachment_plan(edges, layout, diagram_direction);

    let mut routed: Vec<RoutedEdge> = edges
        .iter()
        .filter_map(|edge| {
            // Skip self-edges in normal routing
            if edge.from == edge.to {
                return None;
            }
            // Skip invisible edges — they affect layout but are not rendered
            if edge.stroke == Stroke::Invisible {
                return None;
            }
            let (src_override, tgt_override, src_first_vertical) = plan
                .get(&edge.index)
                .map(|ov| (ov.source, ov.target, ov.source_first_vertical))
                .unwrap_or((None, None, false));
            let edge_dir = layout.effective_edge_direction(&edge.from, &edge.to, diagram_direction);
            route_edge(
                edge,
                layout,
                edge_dir,
                src_override,
                tgt_override,
                src_first_vertical,
            )
        })
        .collect();

    // Route self-edges separately using pre-computed loop points
    for se_data in &layout.self_edges {
        if let Some(edge) = edges
            .iter()
            .find(|e| e.from == e.to && e.from == se_data.node_id)
            && !se_data.points.is_empty()
        {
            routed.push(route_self_edge(se_data, edge, diagram_direction));
        }
    }

    routed
}

#[cfg(test)]
#[path = "text_router_tests.rs"]
mod tests;
