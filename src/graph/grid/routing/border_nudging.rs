use super::super::bounds::{
    NodeContainingSubgraphMap, containing_subgraph_id, node_inside_subgraph,
};
use super::super::layout::{GridLayout, SubgraphBounds};
use super::orthogonal::{
    normalize_polyline_points, point_segment_is_axis_aligned, polyline_points_from_segments,
    polyline_points_to_segments,
};
use super::types::{Point, RoutedEdge};
use crate::graph::Edge;

// Border-nudging policy:
// - skip routes fully contained in the same subgraph
// - skip routes crossing between two different subgraphs; those use the shared outer lane
// - nudge one-in/one-out routes only near the contained endpoint
// - nudge fully external routes away from unrelated borders along the middle spans
pub(super) fn nudge_routed_edge_clear_of_unrelated_subgraph_borders(
    mut routed: RoutedEdge,
    layout: &GridLayout,
    edge: &Edge,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'_>>,
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
    let from_container = containing_subgraph_id(layout, &edge.from, node_containing_subgraph);
    let to_container = containing_subgraph_id(layout, &edge.to, node_containing_subgraph);
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

    for (_, sg) in ordered_subgraph_bounds {
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

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let (a_min, a_max) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
    let (b_min, b_max) = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
    a_min <= b_max && b_min <= a_max
}
