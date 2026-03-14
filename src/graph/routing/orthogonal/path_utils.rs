use super::super::float_core::{build_orthogonal_path_float, normalize_orthogonal_route_contracts};
use super::constants::{LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE, POINT_EPS};
use crate::graph::Direction;
use crate::graph::space::FPoint;

pub(crate) fn segment_is_axis_aligned(a: FPoint, b: FPoint) -> bool {
    (a.x - b.x).abs() <= POINT_EPS || (a.y - b.y).abs() <= POINT_EPS
}

/// Lightweight normalization: dedup + remove collinear, without
/// `compact_terminal_staircase` which can collapse gathering columns.
pub(crate) fn light_normalize(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 1 {
        return points.to_vec();
    }
    let mut result: Vec<FPoint> = Vec::with_capacity(points.len());
    for &p in points {
        let dominated = result.last().is_some_and(|prev: &FPoint| {
            (prev.x - p.x).abs() <= POINT_EPS && (prev.y - p.y).abs() <= POINT_EPS
        });
        if !dominated {
            result.push(p);
        }
    }
    if result.len() <= 2 {
        return result;
    }
    let mut compacted = Vec::with_capacity(result.len());
    compacted.push(result[0]);
    for idx in 1..result.len() - 1 {
        let prev = *compacted.last().unwrap();
        let curr = result[idx];
        let next = result[idx + 1];
        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = (dx1 * dy2 - dy1 * dx2).abs();
        if cross > POINT_EPS {
            compacted.push(curr);
        }
    }
    compacted.push(*result.last().unwrap());
    compacted
}

pub(crate) fn revalidate_label_anchor(
    label_position: Option<FPoint>,
    path: &[FPoint],
) -> Option<FPoint> {
    let Some(anchor) = label_position else {
        return route_derived_label_anchor(path);
    };
    let drift = distance_point_to_path(anchor, path);
    if drift <= LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE {
        return Some(anchor);
    }
    route_derived_label_anchor(path).or(Some(anchor))
}

fn route_derived_label_anchor(path: &[FPoint]) -> Option<FPoint> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return path.first().copied();
    }

    let total_len: f64 = path
        .windows(2)
        .map(|segment| point_distance(segment[0], segment[1]))
        .sum();
    if total_len <= POINT_EPS {
        return path.get(path.len() / 2).copied();
    }

    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let seg_len = point_distance(a, b);
        if seg_len <= POINT_EPS {
            continue;
        }
        if traversed + seg_len >= target {
            let t = (target - traversed) / seg_len;
            return Some(FPoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
        traversed += seg_len;
    }
    path.last().copied()
}

pub(crate) fn point_distance(a: FPoint, b: FPoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

pub(crate) fn distance_point_to_segment(point: FPoint, a: FPoint, b: FPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= POINT_EPS {
        return point_distance(point, a);
    }
    let projection = ((point.x - a.x) * dx + (point.y - a.y) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = FPoint::new(a.x + t * dx, a.y + t * dy);
    point_distance(point, closest)
}

pub(crate) fn distance_point_to_path(point: FPoint, path: &[FPoint]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return point_distance(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

pub(crate) fn ranges_overlap(a_min: f64, a_max: f64, b_min: f64, b_max: f64) -> bool {
    let low = a_min.max(b_min);
    let high = a_max.min(b_max);
    high > low + POINT_EPS
}

/// Deterministically snap float path points onto a fixed grid.
pub(crate) fn snap_path_to_grid(path: &[FPoint], scale_x: f64, scale_y: f64) -> Vec<FPoint> {
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

pub(crate) fn build_contracted_path(
    control_points: &[FPoint],
    direction: Direction,
) -> Vec<FPoint> {
    if control_points.len() < 2 {
        return control_points.to_vec();
    }

    let start = control_points[0];
    let end = control_points[control_points.len() - 1];
    let waypoints = &control_points[1..(control_points.len() - 1)];
    let orthogonal = build_orthogonal_path_float(start, end, direction, waypoints);
    normalize_orthogonal_route_contracts(&orthogonal, direction)
}

pub(crate) fn points_match(a: FPoint, b: FPoint) -> bool {
    (a.x - b.x).abs() <= POINT_EPS && (a.y - b.y).abs() <= POINT_EPS
}

pub(crate) fn collapse_collinear_interior_points(path: &mut Vec<FPoint>) {
    if path.len() <= 2 {
        return;
    }

    let mut collapsed = Vec::with_capacity(path.len());
    collapsed.push(path[0]);
    for idx in 1..(path.len() - 1) {
        let prev = *collapsed.last().expect("collapsed is non-empty");
        let curr = path[idx];
        let next = path[idx + 1];

        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= POINT_EPS && dot >= -POINT_EPS;
        if !collinear_same_direction {
            collapsed.push(curr);
        }
    }
    collapsed.push(*path.last().expect("path has at least two points"));
    *path = collapsed;
}
