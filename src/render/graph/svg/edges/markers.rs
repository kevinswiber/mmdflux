use std::fmt::Write;

use super::super::writer::fmt_f64;
use super::super::{Point, STROKE_COLOR};
use super::{SegmentAxis, points_approx_equal, segment_axis, segment_manhattan_len};
use crate::format::{CornerStyle, Curve};
use crate::graph::{Arrow, Direction, Edge, Stroke};

pub(super) fn edge_style_attrs(edge: &Edge, scale: f64) -> String {
    let stroke_width = match edge.stroke {
        Stroke::Thick => 2.0 * scale,
        _ => 1.0 * scale,
    };
    let mut attrs = format!(
        " stroke=\"{stroke}\" stroke-width=\"{width}\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
        stroke = STROKE_COLOR,
        width = fmt_f64(stroke_width)
    );
    if edge.stroke == Stroke::Dotted {
        let dash = fmt_f64(2.0 * scale);
        let gap = fmt_f64(4.0 * scale);
        let _ = write!(attrs, " stroke-dasharray=\"{dash},{gap}\"");
    }
    attrs
}

pub(super) fn edge_marker_attrs(edge: &Edge) -> String {
    let mut attrs = String::new();
    if let Some(marker_id) = marker_id_for_arrow(edge.arrow_start) {
        let _ = write!(attrs, " marker-start=\"url(#{marker_id})\"");
    }
    if let Some(marker_id) = marker_id_for_arrow(edge.arrow_end) {
        let _ = write!(attrs, " marker-end=\"url(#{marker_id})\"");
    }
    attrs
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MarkerOffsetOptions {
    pub(super) is_backward: bool,
    pub(super) allow_interior_nudges: bool,
    pub(super) enforce_primary_axis_no_backtrack: bool,
    pub(super) preserve_orthogonal: bool,
    pub(super) collapse_terminal_elbows: bool,
    pub(super) is_curved_style: bool,
    pub(super) is_rounded_style: bool,
    pub(super) skip_end_pullback: bool,
    pub(super) preserve_terminal_axis: bool,
}

pub(super) fn apply_marker_offsets(
    points: &[Point],
    edge: &Edge,
    direction: Direction,
    options: MarkerOffsetOptions,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let MarkerOffsetOptions {
        is_backward,
        allow_interior_nudges,
        enforce_primary_axis_no_backtrack,
        preserve_orthogonal,
        collapse_terminal_elbows,
        is_curved_style,
        is_rounded_style,
        skip_end_pullback,
        preserve_terminal_axis,
    } = options;
    let expected_end_axis = if preserve_terminal_axis {
        segment_axis(points[points.len() - 2], points[points.len() - 1])
    } else {
        None
    };

    let mut start_offset = marker_offset_for_arrow(edge.arrow_start);
    let mut end_offset = marker_offset_for_arrow(edge.arrow_end);
    if skip_end_pullback {
        end_offset = 0.0;
    }

    let mut points = points.to_vec();
    if preserve_orthogonal {
        if segment_axis(points[0], points[1]).is_none() {
            start_offset = 0.0;
        }
        if segment_axis(points[points.len() - 2], points[points.len() - 1]).is_none() {
            end_offset = 0.0;
        }
    }
    if !preserve_orthogonal && collapse_terminal_elbows && !is_backward {
        points = collapse_narrow_terminal_elbows_for_non_orth(&points, 14.0, is_rounded_style);
    }
    if !preserve_orthogonal && is_backward {
        const MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT: f64 = 10.0;
        points = enforce_min_orthogonal_endpoint_support(
            &points,
            start_offset + MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT,
            end_offset + MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT,
        );
        let start_support = segment_manhattan_len(points[0], points[1]);
        let end_support = segment_manhattan_len(points[points.len() - 2], points[points.len() - 1]);
        start_offset =
            start_offset.min((start_support - MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT).max(0.0));
        end_offset =
            end_offset.min((end_support - MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT).max(0.0));
    }
    if preserve_orthogonal {
        const MIN_ENDPOINT_SUPPORT: f64 = 12.0;
        const MIN_BACKWARD_CURVED_ENDPOINT_SUPPORT: f64 = 20.0;
        let min_endpoint_support = if is_backward && is_curved_style {
            MIN_BACKWARD_CURVED_ENDPOINT_SUPPORT
        } else {
            MIN_ENDPOINT_SUPPORT
        };
        let start_min_endpoint_support = if edge.arrow_start == Arrow::None {
            0.0
        } else {
            min_endpoint_support
        };
        let end_min_endpoint_support = if edge.arrow_end == Arrow::None {
            0.0
        } else {
            min_endpoint_support
        };
        let original_start = points[0];
        let original_end = points[points.len() - 1];
        points = enforce_min_orthogonal_endpoint_support(
            &points,
            start_offset + start_min_endpoint_support,
            end_offset + end_min_endpoint_support,
        );

        if is_backward {
            const DRIFT_EPS: f64 = 0.5;
            let start_drifted = (points[0].x - original_start.x).abs() > DRIFT_EPS
                || (points[0].y - original_start.y).abs() > DRIFT_EPS;
            let end_drifted = {
                let last = points.len() - 1;
                (points[last].x - original_end.x).abs() > DRIFT_EPS
                    || (points[last].y - original_end.y).abs() > DRIFT_EPS
            };
            if start_drifted {
                points.insert(0, original_start);
            }
            if end_drifted {
                points.push(original_end);
            }
        }

        let start_support = segment_manhattan_len(points[0], points[1]);
        let end_support = segment_manhattan_len(points[points.len() - 2], points[points.len() - 1]);
        start_offset = start_offset.min((start_support - start_min_endpoint_support).max(0.0));
        end_offset = end_offset.min((end_support - end_min_endpoint_support).max(0.0));
    }

    let mut out = Vec::with_capacity(points.len());
    let start = points[0];
    let end = points[points.len() - 1];
    let direction_x = if start.x < end.x { "left" } else { "right" };
    let direction_y = if start.y < end.y { "down" } else { "up" };
    for (i, point) in points.iter().enumerate() {
        let mut offset_x = 0.0;
        if i == 0 && start_offset > 0.0 {
            offset_x = marker_offset_component(points[0], points[1], start_offset, true);
        } else if i == points.len() - 1 && end_offset > 0.0 {
            offset_x = marker_offset_component(
                points[points.len() - 1],
                points[points.len() - 2],
                end_offset,
                true,
            );
        }

        let diff_end = (point.x - end.x).abs();
        let diff_in_y_end = (point.y - end.y).abs();
        let diff_start = (point.x - start.x).abs();
        let diff_in_y_start = (point.y - start.y).abs();
        let extra_room = 1.0;

        if allow_interior_nudges && !preserve_orthogonal {
            if end_offset > 0.0
                && diff_end < end_offset
                && diff_end > 0.0
                && diff_in_y_end < end_offset
            {
                let mut adjustment = end_offset + extra_room - diff_end;
                if direction_x == "right" {
                    adjustment *= -1.0;
                }
                offset_x -= adjustment;
            }

            if start_offset > 0.0
                && diff_start < start_offset
                && diff_start > 0.0
                && diff_in_y_start < start_offset
            {
                let mut adjustment = start_offset + extra_room - diff_start;
                if direction_x == "right" {
                    adjustment *= -1.0;
                }
                offset_x += adjustment;
            }
        }

        let mut offset_y = 0.0;
        if i == 0 && start_offset > 0.0 {
            offset_y = marker_offset_component(points[0], points[1], start_offset, false);
        } else if i == points.len() - 1 && end_offset > 0.0 {
            offset_y = marker_offset_component(
                points[points.len() - 1],
                points[points.len() - 2],
                end_offset,
                false,
            );
        }

        let diff_end_y = (point.y - end.y).abs();
        let diff_in_x_end = (point.x - end.x).abs();
        let diff_start_y = (point.y - start.y).abs();
        let diff_in_x_start = (point.x - start.x).abs();

        if allow_interior_nudges && !preserve_orthogonal {
            if end_offset > 0.0
                && diff_end_y < end_offset
                && diff_end_y > 0.0
                && diff_in_x_end < end_offset
            {
                let mut adjustment = end_offset + extra_room - diff_end_y;
                if direction_y == "up" {
                    adjustment *= -1.0;
                }
                offset_y -= adjustment;
            }

            if start_offset > 0.0
                && diff_start_y < start_offset
                && diff_start_y > 0.0
                && diff_in_x_start < start_offset
            {
                let mut adjustment = start_offset + extra_room - diff_start_y;
                if direction_y == "up" {
                    adjustment *= -1.0;
                }
                offset_y += adjustment;
            }
        }

        out.push(Point {
            x: point.x + offset_x,
            y: point.y + offset_y,
        });
    }

    if enforce_primary_axis_no_backtrack && !preserve_orthogonal {
        enforce_primary_axis_tail_contracts(&mut out, direction, 8.0);
    }
    if let Some(axis) = expected_end_axis {
        preserve_path_terminal_axis(&mut out, axis);
    }

    out
}

pub(super) fn collapse_tiny_straight_smoothing_jogs(
    points: &[Point],
    short_tol: f64,
) -> Vec<Point> {
    if points.len() < 4 || short_tol <= 0.0 {
        return points.to_vec();
    }

    let mut collapsed = points.to_vec();
    let mut idx = 1usize;
    while idx + 1 < collapsed.len() {
        let prev = collapsed[idx - 1];
        let curr = collapsed[idx];
        let next = collapsed[idx + 1];

        let prev_axis = segment_axis(prev, curr);
        let next_axis = segment_axis(curr, next);
        let prev_len = ((curr.x - prev.x).powi(2) + (curr.y - prev.y).powi(2)).sqrt();
        let next_len = ((next.x - curr.x).powi(2) + (next.y - curr.y).powi(2)).sqrt();
        let both_diagonal = prev_axis.is_none() && next_axis.is_none();
        let axis_then_diagonal = prev_axis.is_some() && next_axis.is_none();
        let diagonal_then_axis = prev_axis.is_none() && next_axis.is_some();

        let v1x = curr.x - prev.x;
        let v1y = curr.y - prev.y;
        let v2x = next.x - curr.x;
        let v2y = next.y - curr.y;
        let dot = v1x * v2x + v1y * v2y;

        let should_collapse = (both_diagonal && (prev_len < short_tol || next_len < short_tol))
            || (axis_then_diagonal && prev_len < short_tol)
            || (diagonal_then_axis && next_len < short_tol);
        if should_collapse && dot > 0.0 {
            collapsed.remove(idx);
            idx = idx.saturating_sub(1).max(1);
            continue;
        }

        idx += 1;
    }

    collapsed
}

pub(super) fn curve_adaptive_orthogonal_terminal_support(
    curve: Curve,
    edge_radius: f64,
) -> Option<f64> {
    match curve {
        Curve::Linear(CornerStyle::Rounded) => Some((10.0 + edge_radius).max(12.0)),
        Curve::Basis => Some(16.0),
        Curve::Linear(CornerStyle::Sharp) => None,
    }
}

fn marker_id_for_arrow(arrow: Arrow) -> Option<&'static str> {
    match arrow {
        Arrow::Normal => Some("arrowhead"),
        Arrow::Cross => Some("crosshead"),
        Arrow::Circle => Some("circlehead"),
        Arrow::OpenTriangle => Some("open-arrowhead"),
        Arrow::Diamond => Some("diamondhead"),
        Arrow::OpenDiamond => Some("open-diamondhead"),
        Arrow::None => None,
    }
}

fn marker_offset_for_arrow(arrow: Arrow) -> f64 {
    match arrow {
        Arrow::Normal | Arrow::OpenTriangle => 4.0,
        Arrow::Diamond | Arrow::OpenDiamond => 5.0,
        Arrow::Cross | Arrow::Circle | Arrow::None => 0.0,
    }
}

pub(super) fn enforce_primary_axis_tail_contracts_if_primary_terminal(
    points: &mut [Point],
    direction: Direction,
    min_terminal_support: f64,
) {
    if points.len() < 3 || min_terminal_support <= 0.0 {
        return;
    }
    let n = points.len();
    let expected_axis = match direction {
        Direction::TopDown | Direction::BottomTop => SegmentAxis::Vertical,
        Direction::LeftRight | Direction::RightLeft => SegmentAxis::Horizontal,
    };
    if segment_axis(points[n - 2], points[n - 1]) != Some(expected_axis) {
        return;
    }
    enforce_primary_axis_tail_contracts(points, direction, min_terminal_support);
}

fn preserve_path_terminal_axis(points: &mut [Point], axis: SegmentAxis) {
    if points.len() < 3 {
        return;
    }
    let last = points.len() - 1;
    if segment_axis(points[last - 1], points[last]) == Some(axis) {
        return;
    }

    let prev = points[last - 2];
    let end = points[last];
    let candidate = match axis {
        SegmentAxis::Vertical => Point {
            x: end.x,
            y: prev.y,
        },
        SegmentAxis::Horizontal => Point {
            x: prev.x,
            y: end.y,
        },
    };

    if segment_axis(prev, candidate).is_some()
        && segment_axis(candidate, end) == Some(axis)
        && !points_approx_equal(candidate, end)
    {
        points[last - 1] = candidate;
    }
}

fn collapse_narrow_terminal_elbows_for_non_orth(
    points: &[Point],
    min_terminal_leg: f64,
    preserve_axis: bool,
) -> Vec<Point> {
    if points.len() < 4 || min_terminal_leg <= 0.0 {
        return points.to_vec();
    }

    let mut collapsed = points.to_vec();

    if collapsed.len() >= 4 {
        let n = collapsed.len();
        let before_pre = (n >= 5).then(|| collapsed[n - 4]);
        let pre = collapsed[n - 3];
        let elbow = collapsed[n - 2];
        let end = collapsed[n - 1];
        let pre_axis = segment_axis(pre, elbow);
        let end_axis = segment_axis(elbow, end);
        if let (Some(a), Some(b)) = (pre_axis, end_axis)
            && a != b
            && segment_manhattan_len(elbow, end) < min_terminal_leg
            && segment_manhattan_len(pre, end) > 0.001
        {
            let mut replacement_pre = pre;
            match b {
                SegmentAxis::Horizontal => replacement_pre.y = end.y,
                SegmentAxis::Vertical => replacement_pre.x = end.x,
            }
            if preserve_axis {
                if segment_axis(replacement_pre, end).is_some()
                    && before_pre.is_none_or(|pp| segment_axis(pp, replacement_pre).is_some())
                    && segment_manhattan_len(replacement_pre, end) > 0.001
                {
                    collapsed[n - 3] = replacement_pre;
                    collapsed.remove(n - 2);
                }
            } else {
                if segment_axis(replacement_pre, end).is_some()
                    && segment_manhattan_len(replacement_pre, end) > 0.001
                {
                    collapsed[n - 3] = replacement_pre;
                }
                collapsed.remove(n - 2);
            }
        }
    }

    if collapsed.len() >= 4 {
        let start = collapsed[0];
        let elbow = collapsed[1];
        let post = collapsed[2];
        let start_axis = segment_axis(start, elbow);
        let post_axis = segment_axis(elbow, post);
        if let (Some(a), Some(b)) = (start_axis, post_axis)
            && a != b
            && segment_manhattan_len(start, elbow) < min_terminal_leg
            && segment_manhattan_len(start, post) > 0.001
        {
            collapsed.remove(1);
        }
    }

    collapsed
}

fn enforce_primary_axis_tail_contracts(
    points: &mut [Point],
    direction: Direction,
    min_terminal_support: f64,
) {
    if points.len() < 2 || min_terminal_support <= 0.0 {
        return;
    }

    let n = points.len();
    let end_idx = n - 1;
    let penult_idx = n - 2;

    match direction {
        Direction::TopDown => {
            let target_penult_y = points[end_idx].y - min_terminal_support;
            if points[penult_idx].y > target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y > points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Direction::BottomTop => {
            let target_penult_y = points[end_idx].y + min_terminal_support;
            if points[penult_idx].y < target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y < points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Direction::LeftRight => {
            let target_penult_x = points[end_idx].x - min_terminal_support;
            if points[penult_idx].x > target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x > points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
        Direction::RightLeft => {
            let target_penult_x = points[end_idx].x + min_terminal_support;
            if points[penult_idx].x < target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x < points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
    }
}

fn enforce_min_orthogonal_endpoint_support(
    points: &[Point],
    min_start_support: f64,
    min_end_support: f64,
) -> Vec<Point> {
    let mut adjusted = points.to_vec();
    extend_endpoint_support(&mut adjusted, true, min_start_support);
    extend_endpoint_support(&mut adjusted, false, min_end_support);
    adjusted
}

fn extend_endpoint_support(points: &mut Vec<Point>, at_start: bool, min_support: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_support <= 0.0 {
        return;
    }

    let (anchor_idx, adjacent_idx, before_adjacent_idx, before_before_adjacent_idx) = if at_start {
        (
            0usize,
            1usize,
            (points.len() > 2).then_some(2usize),
            (points.len() > 3).then_some(3usize),
        )
    } else {
        let n = points.len();
        (n - 1, n - 2, n.checked_sub(3), n.checked_sub(4))
    };

    let anchor = points[anchor_idx];
    let adjacent = points[adjacent_idx];
    let Some(axis) = segment_axis(adjacent, anchor) else {
        return;
    };
    let current_support = segment_manhattan_len(adjacent, anchor);
    if current_support >= min_support {
        return;
    }

    let new_adjacent = match axis {
        SegmentAxis::Vertical => {
            let sign = if anchor.y >= adjacent.y { 1.0 } else { -1.0 };
            Point {
                x: anchor.x,
                y: anchor.y - sign * min_support,
            }
        }
        SegmentAxis::Horizontal => {
            let sign = if anchor.x >= adjacent.x { 1.0 } else { -1.0 };
            Point {
                x: anchor.x - sign * min_support,
                y: anchor.y,
            }
        }
    };

    points[adjacent_idx] = new_adjacent;
    let Some(before_adjacent_idx) = before_adjacent_idx else {
        return;
    };

    let before_adjacent = points[before_adjacent_idx];
    if segment_axis(before_adjacent, new_adjacent).is_some() {
        collapse_endpoint_axis_backtrack(points, at_start);
        return;
    }

    let mut shifted_before = before_adjacent;
    match axis {
        SegmentAxis::Vertical => shifted_before.y = new_adjacent.y,
        SegmentAxis::Horizontal => shifted_before.x = new_adjacent.x,
    }
    let keeps_adjacent_axis = segment_axis(shifted_before, new_adjacent).is_some();
    let keeps_prev_axis = before_before_adjacent_idx
        .and_then(|idx| points.get(idx).copied())
        .is_none_or(|prev| segment_axis(prev, shifted_before).is_some());
    if keeps_adjacent_axis {
        if !keeps_prev_axis {
            if let Some(prev_idx) = before_before_adjacent_idx
                && let Some(prev) = points.get(prev_idx).copied()
            {
                let shifted_prev = match axis {
                    SegmentAxis::Vertical => Point {
                        x: prev.x,
                        y: shifted_before.y,
                    },
                    SegmentAxis::Horizontal => Point {
                        x: shifted_before.x,
                        y: prev.y,
                    },
                };
                let keeps_next_axis = segment_axis(shifted_prev, shifted_before).is_some();
                let keeps_prev_axis = prev_idx
                    .checked_sub(1)
                    .and_then(|idx| points.get(idx).copied())
                    .is_none_or(|pprev| segment_axis(pprev, shifted_prev).is_some());
                if keeps_next_axis && keeps_prev_axis {
                    points[before_adjacent_idx] = shifted_before;
                    points[prev_idx] = shifted_prev;
                    collapse_endpoint_axis_backtrack(points, at_start);
                    return;
                }
            }
        } else {
            points[before_adjacent_idx] = shifted_before;
            let mut propagate_idx = before_before_adjacent_idx;
            while let Some(idx) = propagate_idx {
                let Some(current) = points.get(idx).copied() else {
                    break;
                };
                let should_shift = match axis {
                    SegmentAxis::Vertical => (current.y - before_adjacent.y).abs() <= EPS,
                    SegmentAxis::Horizontal => (current.x - before_adjacent.x).abs() <= EPS,
                };
                if !should_shift {
                    break;
                }

                let candidate = match axis {
                    SegmentAxis::Vertical => Point {
                        x: current.x,
                        y: shifted_before.y,
                    },
                    SegmentAxis::Horizontal => Point {
                        x: shifted_before.x,
                        y: current.y,
                    },
                };
                let keeps_next_axis = points
                    .get(idx + 1)
                    .copied()
                    .is_some_and(|next| segment_axis(candidate, next).is_some());
                let keeps_prev_axis = idx
                    .checked_sub(1)
                    .and_then(|prev_idx| points.get(prev_idx).copied())
                    .is_none_or(|prev| segment_axis(prev, candidate).is_some());
                if !keeps_next_axis || !keeps_prev_axis {
                    break;
                }

                points[idx] = candidate;
                propagate_idx = idx.checked_sub(1);
            }
            collapse_endpoint_axis_backtrack(points, at_start);
            return;
        }
    }

    let elbow = match axis {
        SegmentAxis::Vertical => Point {
            x: before_adjacent.x,
            y: new_adjacent.y,
        },
        SegmentAxis::Horizontal => Point {
            x: new_adjacent.x,
            y: before_adjacent.y,
        },
    };

    if at_start {
        points.insert(2, elbow);
    } else {
        let insert_at = points.len() - 2;
        points.insert(insert_at, elbow);
    }
    collapse_endpoint_axis_backtrack(points, at_start);
}

fn collapse_endpoint_axis_backtrack(points: &mut Vec<Point>, at_start: bool) {
    if points.len() < 4 {
        return;
    }

    let (outer_idx, middle_idx, inner_idx) = if at_start {
        (3usize, 2usize, 1usize)
    } else {
        let n = points.len();
        (n - 4, n - 3, n - 2)
    };

    let outer = points[outer_idx];
    let middle = points[middle_idx];
    let inner = points[inner_idx];
    let Some(first_axis) = segment_axis(outer, middle) else {
        return;
    };
    let Some(second_axis) = segment_axis(middle, inner) else {
        return;
    };
    if first_axis != second_axis {
        return;
    }

    let (delta1, delta2) = match first_axis {
        SegmentAxis::Vertical => (middle.y - outer.y, inner.y - middle.y),
        SegmentAxis::Horizontal => (middle.x - outer.x, inner.x - middle.x),
    };
    if delta1.abs() <= f64::EPSILON || delta2.abs() <= f64::EPSILON {
        return;
    }
    if delta1.signum() != delta2.signum() {
        points.remove(middle_idx);
    }
}

fn marker_offset_component(point_a: Point, point_b: Point, offset: f64, use_x: bool) -> f64 {
    let delta_x = point_b.x - point_a.x;
    let delta_y = point_b.y - point_a.y;
    let angle = if delta_x.abs() < f64::EPSILON {
        if delta_y >= 0.0 {
            std::f64::consts::FRAC_PI_2
        } else {
            -std::f64::consts::FRAC_PI_2
        }
    } else {
        (delta_y / delta_x).atan()
    };

    if use_x {
        offset * angle.cos() * if delta_x >= 0.0 { 1.0 } else { -1.0 }
    } else {
        offset * angle.sin().abs() * if delta_y >= 0.0 { 1.0 } else { -1.0 }
    }
}
