//! Waypoint and label projection helpers for the grid derivation pipeline.
//!
//! These functions snap routed float-space hints onto the derived grid, repair
//! waypoint collisions, and clip cross-subgraph segments to grid-space bounds.

use std::collections::HashMap;

use super::super::layout::{NodeBounds, SubgraphBounds, TransformContext};
use crate::graph::Edge;
use crate::graph::space::FPoint;

pub(super) fn nudge_colliding_waypoints(
    edge_waypoints: &mut HashMap<usize, Vec<(usize, usize)>>,
    node_bounds: &HashMap<String, NodeBounds>,
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) {
    let mut sorted_bounds: Vec<NodeBounds> = node_bounds.values().copied().collect();
    sorted_bounds.sort_by_key(|bounds| (bounds.y, bounds.x, bounds.width, bounds.height));

    for waypoints in edge_waypoints.values_mut() {
        nudge_waypoint_points(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
        *waypoints = repair_quantized_waypoint_segments(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
        nudge_waypoint_points(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
    }
}

fn nudge_waypoint_points(
    waypoints: &mut [(usize, usize)],
    node_bounds: &[NodeBounds],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) {
    for wp in waypoints.iter_mut() {
        for bounds in node_bounds {
            if bounds.contains(wp.0, wp.1) {
                if is_vertical {
                    wp.0 = bounds.x + bounds.width + 1;
                } else {
                    wp.1 = bounds.y + bounds.height + 1;
                }
                break;
            }
        }
        clamp_waypoint(wp, canvas_width, canvas_height);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaypointSegment {
    start: (usize, usize),
    end: (usize, usize),
    axis: SegmentAxis,
}

fn repair_quantized_waypoint_segments(
    waypoints: &[(usize, usize)],
    node_bounds: &[NodeBounds],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> Vec<(usize, usize)> {
    if waypoints.len() < 2 || node_bounds.is_empty() {
        return waypoints.to_vec();
    }

    let mut repaired = waypoints.to_vec();
    let max_repairs = node_bounds.len().saturating_mul(waypoints.len().max(1)) * 2;
    let mut repairs = 0usize;

    loop {
        let mut changed = false;

        for idx in 0..repaired.len().saturating_sub(1) {
            let from = repaired[idx];
            let to = repaired[idx + 1];
            let Some((blocker, axis)) = first_blocking_segment(from, to, node_bounds, is_vertical)
            else {
                continue;
            };

            let detour = detour_waypoints_around_blocker(
                from,
                to,
                blocker,
                axis,
                canvas_width,
                canvas_height,
            );
            if detour.is_empty() {
                continue;
            }

            repaired.splice(idx + 1..idx + 1, detour);
            changed = true;
            repairs += 1;
            break;
        }

        if !changed || repairs >= max_repairs {
            break;
        }
    }

    repaired.dedup();
    repaired
}

fn first_blocking_segment(
    from: (usize, usize),
    to: (usize, usize),
    node_bounds: &[NodeBounds],
    is_vertical: bool,
) -> Option<(NodeBounds, SegmentAxis)> {
    let segments = orthogonal_segments_between_waypoints(from, to, is_vertical);

    for segment in segments {
        for bounds in node_bounds {
            if orthogonal_segment_intersects_bounds(segment.start, segment.end, bounds) {
                return Some((*bounds, segment.axis));
            }
        }
    }

    None
}

fn orthogonal_segments_between_waypoints(
    from: (usize, usize),
    to: (usize, usize),
    is_vertical: bool,
) -> Vec<WaypointSegment> {
    if from == to {
        return Vec::new();
    }

    if from.0 == to.0 {
        return vec![WaypointSegment {
            start: from,
            end: to,
            axis: SegmentAxis::Vertical,
        }];
    }

    if from.1 == to.1 {
        return vec![WaypointSegment {
            start: from,
            end: to,
            axis: SegmentAxis::Horizontal,
        }];
    }

    if is_vertical {
        let elbow = (to.0, from.1);
        vec![
            WaypointSegment {
                start: from,
                end: elbow,
                axis: SegmentAxis::Horizontal,
            },
            WaypointSegment {
                start: elbow,
                end: to,
                axis: SegmentAxis::Vertical,
            },
        ]
    } else {
        let elbow = (from.0, to.1);
        vec![
            WaypointSegment {
                start: from,
                end: elbow,
                axis: SegmentAxis::Vertical,
            },
            WaypointSegment {
                start: elbow,
                end: to,
                axis: SegmentAxis::Horizontal,
            },
        ]
    }
}

fn orthogonal_segment_intersects_bounds(
    start: (usize, usize),
    end: (usize, usize),
    bounds: &NodeBounds,
) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    if start.0 == end.0 {
        let x = start.0;
        let (y_min, y_max) = if start.1 <= end.1 {
            (start.1, end.1)
        } else {
            (end.1, start.1)
        };
        return x >= left && x <= right && y_min <= bottom && top <= y_max;
    }

    if start.1 == end.1 {
        let y = start.1;
        let (x_min, x_max) = if start.0 <= end.0 {
            (start.0, end.0)
        } else {
            (end.0, start.0)
        };
        return y >= top && y <= bottom && x_min <= right && left <= x_max;
    }

    false
}

fn detour_waypoints_around_blocker(
    from: (usize, usize),
    to: (usize, usize),
    blocker: NodeBounds,
    axis: SegmentAxis,
    canvas_width: usize,
    canvas_height: usize,
) -> Vec<(usize, usize)> {
    let mut detour = Vec::with_capacity(2);

    match axis {
        SegmentAxis::Horizontal => {
            let detour_y =
                choose_detour_coordinate(from.1, to.1, blocker.y, blocker.height, canvas_height);
            if detour_y != from.1 {
                detour.push((from.0, detour_y));
            }
            if detour.last().copied() != Some((to.0, detour_y)) {
                detour.push((to.0, detour_y));
            }
        }
        SegmentAxis::Vertical => {
            let detour_x =
                choose_detour_coordinate(from.0, to.0, blocker.x, blocker.width, canvas_width);
            if detour_x != from.0 {
                detour.push((detour_x, from.1));
            }
            if detour.last().copied() != Some((detour_x, to.1)) {
                detour.push((detour_x, to.1));
            }
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

fn clamp_waypoint(waypoint: &mut (usize, usize), canvas_width: usize, canvas_height: usize) {
    waypoint.0 = waypoint.0.min(canvas_width.saturating_sub(1));
    waypoint.1 = waypoint.1.min(canvas_height.saturating_sub(1));
}

/// Transform layout waypoints to ASCII draw coordinates using uniform scale factors.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses `layer_starts` to snap to
/// the correct rank position. The cross axis uses uniform scaling from layout
/// coordinates, ensuring consistency with node positions.
pub(super) fn transform_waypoints_direct(
    edge_waypoints: &HashMap<usize, Vec<(FPoint, i32)>>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, Vec<(usize, usize)>> {
    let mut converted = HashMap::new();

    for (edge_idx, waypoints) in edge_waypoints {
        if edges.get(*edge_idx).is_some() {
            let wps: Vec<(usize, usize)> = waypoints
                .iter()
                .map(|(fp, rank)| {
                    let rank_idx = *rank as usize;
                    let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
                    let (scaled_x, scaled_y) = ctx.to_grid(fp.x, fp.y);

                    if is_vertical {
                        (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
                    } else {
                        (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
                    }
                })
                .collect();

            converted.insert(*edge_idx, wps);
        }
    }

    converted
}

/// Transform layout label positions to ASCII draw coordinates.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses rank-based snapping via
/// `layer_starts[rank]`, matching how `transform_waypoints_direct()` works.
/// The cross axis uses uniform scaling from layout coordinates.
pub(super) fn transform_label_positions_direct(
    label_positions: &HashMap<usize, (FPoint, i32)>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, (usize, usize)> {
    let mut converted = HashMap::new();

    for (edge_idx, (fp, rank)) in label_positions {
        if edges.get(*edge_idx).is_some() {
            let rank_idx = *rank as usize;
            let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
            let (scaled_x, scaled_y) = ctx.to_grid(fp.x, fp.y);

            let pos = if is_vertical {
                (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
            } else {
                (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
            };
            converted.insert(*edge_idx, pos);
        }
    }

    converted
}

fn waypoint_inside_bounds(bounds: &SubgraphBounds, point: (usize, usize)) -> bool {
    let (x, y) = point;
    let max_x = bounds.x + bounds.width.saturating_sub(1);
    let max_y = bounds.y + bounds.height.saturating_sub(1);
    x > bounds.x && x < max_x && y > bounds.y && y < max_y
}

fn segment_bounds_intersection(
    start: (usize, usize),
    end: (usize, usize),
    bounds: &SubgraphBounds,
) -> Option<(usize, usize)> {
    let (x0, y0) = (start.0 as f64, start.1 as f64);
    let (x1, y1) = (end.0 as f64, end.1 as f64);
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return None;
    }

    let x_min = bounds.x as f64;
    let x_max = (bounds.x + bounds.width) as f64;
    let y_min = bounds.y as f64;
    let y_max = (bounds.y + bounds.height) as f64;

    let mut candidates: Vec<(f64, (usize, usize))> = Vec::new();

    if dx.abs() > f64::EPSILON {
        let t_left = (x_min - x0) / dx;
        if (0.0..=1.0).contains(&t_left) {
            let y = y0 + t_left * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_left, (x_min.round() as usize, y.round() as usize)));
            }
        }
        let t_right = (x_max - x0) / dx;
        if (0.0..=1.0).contains(&t_right) {
            let y = y0 + t_right * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_right, (x_max.round() as usize, y.round() as usize)));
            }
        }
    }

    if dy.abs() > f64::EPSILON {
        let t_top = (y_min - y0) / dy;
        if (0.0..=1.0).contains(&t_top) {
            let x = x0 + t_top * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_top, (x.round() as usize, y_min.round() as usize)));
            }
        }
        let t_bottom = (y_max - y0) / dy;
        if (0.0..=1.0).contains(&t_bottom) {
            let x = x0 + t_bottom * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_bottom, (x.round() as usize, y_max.round() as usize)));
            }
        }
    }

    candidates
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

pub(super) fn clip_waypoints_to_subgraph(
    waypoints: &[(usize, usize)],
    bounds: &SubgraphBounds,
    clip_start: bool,
    clip_end: bool,
) -> Vec<(usize, usize)> {
    if waypoints.len() < 2 {
        return waypoints.to_vec();
    }
    let mut out = waypoints.to_vec();

    if clip_start && waypoint_inside_bounds(bounds, out[0]) {
        let mut idx = 0usize;
        while idx + 1 < out.len() && waypoint_inside_bounds(bounds, out[idx]) {
            idx += 1;
        }
        if idx < out.len() {
            let inside = out[idx.saturating_sub(1)];
            let outside = out[idx];
            let intersection =
                segment_bounds_intersection(inside, outside, bounds).unwrap_or(inside);
            let mut new_points = Vec::new();
            new_points.push(intersection);
            new_points.extend_from_slice(&out[idx..]);
            out = new_points;
        }
    }

    if clip_end && out.len() >= 2 {
        let last_idx = out.len() - 1;
        if waypoint_inside_bounds(bounds, out[last_idx]) {
            let mut idx = last_idx;
            while idx > 0 && waypoint_inside_bounds(bounds, out[idx]) {
                idx -= 1;
            }
            if idx < last_idx {
                let outside = out[idx];
                let inside = out[idx + 1];
                let intersection =
                    segment_bounds_intersection(outside, inside, bounds).unwrap_or(inside);
                let mut new_points = out[..=idx].to_vec();
                new_points.push(intersection);
                out = new_points;
            }
        }
    }

    out
}
