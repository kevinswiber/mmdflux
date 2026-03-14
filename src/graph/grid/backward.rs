use super::bounds::node_inside_subgraph;
use super::{GridLayout, NodeBounds};
use crate::graph::{Direction, Edge};

/// Gap between node boundary and synthetic backward-edge waypoint path (in cells).
pub const BACKWARD_ROUTE_GAP: usize = 2;

/// Check if an edge is a backward edge (goes against the layout direction).
pub fn is_backward_edge(
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    match direction {
        Direction::TopDown => to_bounds.y < from_bounds.y,
        Direction::BottomTop => to_bounds.y > from_bounds.y,
        Direction::LeftRight => to_bounds.x < from_bounds.x,
        Direction::RightLeft => to_bounds.x > from_bounds.x,
    }
}

/// Generate synthetic waypoints for a single-rank-span backward edge.
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
            let right_edge = (src_bounds.x + src_bounds.width).max(tgt_bounds.x + tgt_bounds.width);
            let route_x = right_edge + BACKWARD_ROUTE_GAP;
            vec![
                (route_x, src_bounds.center_y()),
                (route_x, tgt_bounds.center_y()),
            ]
        }
        Direction::LeftRight => {
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

pub(crate) fn compact_lr_backward_attachments(
    edge: &Edge,
    layout: &GridLayout,
    src_bounds: &NodeBounds,
    tgt_bounds: &NodeBounds,
    direction: Direction,
) -> Option<((usize, usize), (usize, usize))> {
    if !matches!(direction, Direction::LeftRight | Direction::RightLeft)
        || !is_backward_edge(src_bounds, tgt_bounds, direction)
        || edge.from_subgraph.is_some()
        || edge.to_subgraph.is_some()
    {
        return None;
    }

    let overlap_top = src_bounds.y.max(tgt_bounds.y);
    let overlap_bottom = (src_bounds.y + src_bounds.height.saturating_sub(1))
        .min(tgt_bounds.y + tgt_bounds.height.saturating_sub(1));
    if overlap_top > overlap_bottom {
        return None;
    }

    let lane_y = overlap_bottom;
    let (src_attach, tgt_attach) = match direction {
        Direction::LeftRight => (
            (src_bounds.x, lane_y),
            (tgt_bounds.x + tgt_bounds.width.saturating_sub(1), lane_y),
        ),
        Direction::RightLeft => (
            (src_bounds.x + src_bounds.width.saturating_sub(1), lane_y),
            (tgt_bounds.x, lane_y),
        ),
        _ => unreachable!(),
    };

    let corridor_x_min = src_attach.0.min(tgt_attach.0);
    let corridor_x_max = src_attach.0.max(tgt_attach.0);
    for (node_id, bounds) in &layout.node_bounds {
        if node_id == &edge.from || node_id == &edge.to {
            continue;
        }

        let node_bottom = bounds.y + bounds.height.saturating_sub(1);
        let node_right = bounds.x + bounds.width.saturating_sub(1);
        let overlaps_lane = bounds.y <= lane_y && lane_y <= node_bottom;
        let overlaps_corridor = bounds.x <= corridor_x_max && corridor_x_min <= node_right;
        if overlaps_lane && overlaps_corridor {
            return None;
        }
    }

    for sg in layout.subgraph_bounds.values() {
        let source_inside = node_inside_subgraph(src_bounds, sg);
        let target_inside = node_inside_subgraph(tgt_bounds, sg);
        if source_inside && target_inside {
            continue;
        }

        let left = sg.x;
        let right = sg.x + sg.width.saturating_sub(1);
        let top = sg.y;
        let bottom = sg.y + sg.height.saturating_sub(1);

        let overlaps_horizontal_border = (lane_y == top || lane_y == bottom)
            && ranges_overlap(corridor_x_min, corridor_x_max, left, right);
        let overlaps_vertical_border = lane_y >= top
            && lane_y <= bottom
            && (corridor_x_min <= left && left <= corridor_x_max
                || corridor_x_min <= right && right <= corridor_x_max);

        if overlaps_horizontal_border || overlaps_vertical_border {
            return None;
        }
    }

    Some((src_attach, tgt_attach))
}

/// Generate backward channel waypoints that clear all intermediate nodes.
pub(crate) fn generate_corridor_backward_waypoints(
    edge: &Edge,
    layout: &GridLayout,
    src_bounds: &NodeBounds,
    tgt_bounds: &NodeBounds,
    direction: Direction,
) -> Vec<(usize, usize)> {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let y_min = src_bounds.y.min(tgt_bounds.y);
            let y_max = (src_bounds.y + src_bounds.height).max(tgt_bounds.y + tgt_bounds.height);

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

pub(crate) fn should_prefer_shared_backward_route_for_text(
    draw_path: &[(usize, usize)],
    direction: Direction,
) -> bool {
    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            draw_path.len() >= 6 && draw_waypoint_count(draw_path) >= 4
        }
        Direction::TopDown | Direction::BottomTop => true,
    }
}

fn draw_waypoint_count(points: &[(usize, usize)]) -> usize {
    let mut count = 0usize;
    let mut last = None;
    for &point in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        if last != Some(point) {
            count += 1;
            last = Some(point);
        }
    }
    count
}

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let a_min = a1.min(a2);
    let a_max = a1.max(a2);
    let b_min = b1.min(b2);
    let b_max = b1.max(b2);
    a_max >= b_min && b_max >= a_min
}
