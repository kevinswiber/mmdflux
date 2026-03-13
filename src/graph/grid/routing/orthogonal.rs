use super::super::attachments::LARGE_HORIZONTAL_OFFSET_THRESHOLD as SHARED_LARGE_HORIZONTAL_OFFSET_THRESHOLD;
use super::super::intersect::NodeFace;
use super::types::{AttachDirection, Point, Segment};
use crate::graph::Direction;

pub(super) fn ensure_terminal_face_support(
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

pub(super) fn ensure_source_face_launch_support(
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

pub(super) fn polyline_points_from_segments(start: Point, segments: &[Segment]) -> Vec<Point> {
    let mut points = vec![start];
    for segment in segments {
        let end = segment.end_point();
        if points.last().copied() != Some(end) {
            points.push(end);
        }
    }
    points
}

pub(super) fn normalize_polyline_points(points: &mut Vec<Point>) {
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

pub(super) fn polyline_points_to_segments(points: &[Point]) -> Vec<Segment> {
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

pub(super) fn point_segment_is_axis_aligned(start: Point, end: Point) -> bool {
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

pub(super) fn add_connector_segment(
    segments: &mut Vec<Segment>,
    boundary: (usize, usize),
    offset: Point,
) {
    let (bx, by) = boundary;
    if bx == offset.x {
        segments.push(Segment::Vertical {
            x: bx,
            y_start: by,
            y_end: offset.y,
        });
    } else if by == offset.y {
        segments.push(Segment::Horizontal {
            y: by,
            x_start: bx,
            x_end: offset.x,
        });
    }
}

pub(super) fn entry_direction_from_segments(segments: &[Segment]) -> AttachDirection {
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

pub(super) fn build_orthogonal_path_for_direction(
    start: Point,
    end: Point,
    direction: Direction,
) -> Vec<Segment> {
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

    match direction {
        Direction::TopDown | Direction::BottomTop => {
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

fn compute_mid_y_for_vertical_layout(start: Point, end: Point, direction: Direction) -> usize {
    let horizontal_offset = start.x.abs_diff(end.x);

    let mut mid_y = if horizontal_offset > SHARED_LARGE_HORIZONTAL_OFFSET_THRESHOLD {
        let is_right_to_left = start.x > end.x;

        if is_right_to_left {
            match direction {
                Direction::TopDown => {
                    let target_mid = end.y.saturating_sub(2);
                    let standard_mid = (start.y + end.y) / 2;
                    target_mid.max(standard_mid)
                }
                Direction::BottomTop => {
                    let target_mid = end.y + 2;
                    let standard_mid = (start.y + end.y) / 2;
                    target_mid.min(standard_mid)
                }
                _ => (start.y + end.y) / 2,
            }
        } else {
            (start.y + end.y) / 2
        }
    } else {
        (start.y + end.y) / 2
    };

    if mid_y == end.y {
        if start.y > end.y {
            mid_y = end.y + 1;
        } else {
            mid_y = end.y.saturating_sub(1);
        }
    }

    mid_y
}

pub(super) fn build_orthogonal_path_with_waypoints(
    start: Point,
    waypoints: &[(usize, usize)],
    end: Point,
    direction: Direction,
    start_vertical: bool,
    has_arrow_start: bool,
) -> Vec<Segment> {
    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);

    if waypoints.is_empty() {
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
    let first_wp = Point::new(waypoint_slice[0].0, waypoint_slice[0].1);
    let first_vertical = start_vertical_override || !vertical_first;

    segments.extend(orthogonalize_segment(start, first_wp, first_vertical));

    for window in waypoint_slice.windows(2) {
        let from = Point::new(window[0].0, window[0].1);
        let to = Point::new(window[1].0, window[1].1);
        segments.extend(orthogonalize_segment(from, to, !vertical_first));
    }

    let &(last_x, last_y) = waypoint_slice.last().unwrap();
    let last_wp = Point::new(last_x, last_y);
    segments.extend(build_orthogonal_path_for_direction(last_wp, end, direction));

    segments
}

#[cfg(test)]
pub(super) fn compute_vertical_first_path(start: Point, end: Point) -> Vec<Segment> {
    build_orthogonal_path_for_direction(start, end, Direction::TopDown)
}

pub(super) fn orthogonalize_segment(from: Point, to: Point, vertical_first: bool) -> Vec<Segment> {
    if from == to {
        vec![]
    } else if from.x == to.x {
        vec![Segment::Vertical {
            x: from.x,
            y_start: from.y,
            y_end: to.y,
        }]
    } else if from.y == to.y {
        vec![Segment::Horizontal {
            y: from.y,
            x_start: from.x,
            x_end: to.x,
        }]
    } else if vertical_first {
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

#[cfg(test)]
pub fn build_orthogonal_path(
    start: Point,
    waypoints: &[(usize, usize)],
    end: Point,
    direction: Direction,
) -> Vec<Segment> {
    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);

    if waypoints.is_empty() {
        return orthogonalize_segment(start, end, vertical_first);
    }

    let mut segments = Vec::new();
    let first_wp = Point::new(waypoints[0].0, waypoints[0].1);
    segments.extend(orthogonalize_segment(start, first_wp, vertical_first));

    for window in waypoints.windows(2) {
        let from = Point::new(window[0].0, window[0].1);
        let to = Point::new(window[1].0, window[1].1);
        segments.extend(orthogonalize_segment(from, to, vertical_first));
    }

    let &(last_x, last_y) = waypoints.last().unwrap();
    let last_wp = Point::new(last_x, last_y);
    segments.extend(orthogonalize_segment(last_wp, end, vertical_first));

    segments
}
