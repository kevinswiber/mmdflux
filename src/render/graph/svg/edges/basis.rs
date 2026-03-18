use super::super::{Point, Rect};
use super::endpoints::{clip_points_to_rect_end, clip_points_to_rect_start, intersect_svg_node};
use super::{
    SegmentAxis, dedup_consecutive_svg_points, point_inside_rect, points_approx_equal,
    points_are_axis_aligned, segment_axis, vectors_share_ray,
};
use crate::graph::geometry::GraphGeometry;
use crate::graph::{Direction, Edge, Graph, Shape, Stroke};

pub(super) fn enforce_basis_visible_terminal_stems(
    points: &[Point],
    min_stem: f64,
    compact_caps: bool,
) -> Vec<Point> {
    let mut adjusted = dedup_consecutive_svg_points(points);
    if adjusted.len() < 2 || min_stem <= 0.0 {
        return adjusted;
    }

    if compact_caps {
        if let Some((seg_idx, cap)) = start_cap_point_on_existing_run(&adjusted, min_stem) {
            adjusted = rebuild_with_start_cap(&adjusted, seg_idx, cap);
        }
        if let Some((seg_idx, cap)) = end_cap_point_on_existing_run(&adjusted, min_stem) {
            adjusted = rebuild_with_end_cap(&adjusted, seg_idx, cap);
        }
    } else {
        insert_basis_start_cap_if_needed(&mut adjusted, min_stem);
        insert_basis_end_cap_if_needed(&mut adjusted, min_stem);
    }
    dedup_consecutive_svg_points(&adjusted)
}

pub(super) fn clamp_basis_edge_endpoints_to_boundaries(
    diagram: &Graph,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let mut clamped = points.to_vec();
    if let Some(sg_id) = edge.from_subgraph.as_ref()
        && let Some(sg_geom) = geom.subgraphs.get(sg_id)
    {
        clamped = clip_points_to_rect_start(&clamped, &sg_geom.rect);
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.from), geom.nodes.get(&edge.from))
    {
        let from_rect: Rect = node_geom.rect;
        if !matches!(node.shape, Shape::Diamond | Shape::Hexagon)
            && point_inside_rect(&from_rect, clamped[0])
        {
            clamped[0] = intersect_svg_node(&from_rect, clamped[1], node.shape);
        }
    }

    if clamped.len() < 2 {
        return clamped;
    }

    if let Some(sg_id) = edge.to_subgraph.as_ref()
        && let Some(sg_geom) = geom.subgraphs.get(sg_id)
    {
        clamped = clip_points_to_rect_end(&clamped, &sg_geom.rect);
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.to), geom.nodes.get(&edge.to))
    {
        let to_rect: Rect = node_geom.rect;
        let last = clamped.len() - 1;
        if !matches!(node.shape, Shape::Diamond | Shape::Hexagon)
            && point_inside_rect(&to_rect, clamped[last])
        {
            clamped[last] = intersect_svg_node(&to_rect, clamped[last - 1], node.shape);
        }
    }

    dedup_consecutive_svg_points(&clamped)
}

pub(super) fn edge_rank_span_for_svg(geom: &GraphGeometry, edge: &Edge) -> Option<usize> {
    let crate::graph::geometry::EngineHints::Layered(hints) = geom.engine_hints.as_ref()?;
    let src_rank = *hints.node_ranks.get(&edge.from)?;
    let dst_rank = *hints.node_ranks.get(&edge.to)?;
    Some(src_rank.abs_diff(dst_rank) as usize)
}

pub(super) fn adapt_basis_anchor_points(
    points: &[Point],
    edge: &Edge,
    geom: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    if !points_are_axis_aligned(points) {
        return dedup_consecutive_svg_points(points);
    }

    let rank_span =
        edge_rank_span_for_svg(geom, edge).unwrap_or_else(|| edge.minlen.max(1) as usize);
    let starts_on_secondary_axis = first_segment_is_secondary_axis(points, direction);
    let compact_backward_return = is_backward
        && rank_span <= 1
        && edge.minlen <= 1
        && is_primary_secondary_primary_return(points, direction);
    if compact_backward_return {
        return dedup_consecutive_svg_points(&[points[0], points[2], points[3]]);
    }
    let preserve_extended_route = is_backward || rank_span >= 2 || edge.minlen > 1;

    let adapted = if preserve_extended_route {
        if points.len() <= 4 {
            points.to_vec()
        } else {
            vec![
                points[0],
                points[1],
                points[points.len() - 2],
                points[points.len() - 1],
            ]
        }
    } else if starts_on_secondary_axis && ends_on_secondary_axis(points, direction) {
        vec![
            points[0],
            points[points.len() - 2],
            points[points.len() - 1],
        ]
    } else if starts_on_secondary_axis {
        vec![points[0], points[1], points[points.len() - 1]]
    } else {
        vec![points[0], points[points.len() - 1]]
    };

    dedup_consecutive_svg_points(&adapted)
}

pub(super) fn apply_reciprocal_lane_offsets(
    start: Point,
    end: Point,
    direction: Direction,
    curve_sign: f64,
    source_rect: Rect,
    target_rect: Rect,
) -> (Point, Point) {
    let mut adjusted_start = start;
    let mut adjusted_end = end;
    let source_upper = (source_rect.height * 0.18).clamp(8.0, 14.0);
    let target_upper = (target_rect.height * 0.18).clamp(8.0, 14.0);
    let source_lower = (source_rect.height * 0.26).clamp(10.0, 18.0);
    let target_lower = (target_rect.height * 0.26).clamp(10.0, 18.0);
    let source_lane = if curve_sign < 0.0 {
        source_upper
    } else {
        source_lower
    };
    let target_lane = if curve_sign < 0.0 {
        target_upper
    } else {
        target_lower
    };

    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            let source_center = source_rect.y + source_rect.height / 2.0;
            let target_center = target_rect.y + target_rect.height / 2.0;
            adjusted_start.y = if curve_sign < 0.0 {
                source_center - source_lane
            } else {
                source_center + source_lane
            }
            .clamp(
                source_rect.y + 1.0,
                source_rect.y + source_rect.height - 1.0,
            );
            adjusted_end.y = if curve_sign < 0.0 {
                target_center - target_lane
            } else {
                target_center + target_lane
            }
            .clamp(
                target_rect.y + 1.0,
                target_rect.y + target_rect.height - 1.0,
            );
        }
        Direction::TopDown | Direction::BottomTop => {
            let source_upper = (source_rect.width * 0.18).clamp(8.0, 14.0);
            let target_upper = (target_rect.width * 0.18).clamp(8.0, 14.0);
            let source_lower = (source_rect.width * 0.26).clamp(10.0, 18.0);
            let target_lower = (target_rect.width * 0.26).clamp(10.0, 18.0);
            let source_lane = if curve_sign < 0.0 {
                source_upper
            } else {
                source_lower
            };
            let target_lane = if curve_sign < 0.0 {
                target_upper
            } else {
                target_lower
            };
            let source_center = source_rect.x + source_rect.width / 2.0;
            let target_center = target_rect.x + target_rect.width / 2.0;
            adjusted_start.x = if curve_sign < 0.0 {
                source_center - source_lane
            } else {
                source_center + source_lane
            }
            .clamp(source_rect.x + 1.0, source_rect.x + source_rect.width - 1.0);
            adjusted_end.x = if curve_sign < 0.0 {
                target_center - target_lane
            } else {
                target_center + target_lane
            }
            .clamp(target_rect.x + 1.0, target_rect.x + target_rect.width - 1.0);
        }
    }

    (adjusted_start, adjusted_end)
}

pub(super) fn collapse_primary_face_fan_channel_for_curved(
    geom: &GraphGeometry,
    edge: &Edge,
    direction: Direction,
    points: &[Point],
    center_eps: f64,
) -> Vec<Point> {
    const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;
    const MIN_STEM_FOR_COLLAPSE: f64 = 8.0;
    const MAX_STEM_FOR_COLLAPSE: f64 = 18.0;
    const MIN_TERMINAL_STEM_FOR_COLLAPSE: f64 = 10.0;
    const MAX_TERMINAL_STEM_FOR_COLLAPSE: f64 = 22.0;
    const TERMINAL_STEM_BIAS: f64 = 3.0;
    const MIN_CHANNEL_SPAN: f64 = 4.0;

    if points.len() != 4 {
        return points.to_vec();
    }

    let Some(target_geom) = geom.nodes.get(&edge.to) else {
        return points.to_vec();
    };
    let target_rect: Rect = target_geom.rect;
    let mut out = points.to_vec();
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let first_vertical = (out[0].x - out[1].x).abs() <= center_eps
                && (out[0].y - out[1].y).abs() > center_eps;
            let middle_horizontal = (out[1].y - out[2].y).abs() <= center_eps
                && (out[1].x - out[2].x).abs() > center_eps;
            let terminal_vertical = (out[2].x - out[3].x).abs() <= center_eps
                && (out[2].y - out[3].y).abs() > center_eps;
            if !(first_vertical && middle_horizontal && terminal_vertical) {
                return points.to_vec();
            }

            let has_lateral_offset = (out[0].x - out[3].x).abs() > center_eps;
            let target_is_primary_face = match direction {
                Direction::TopDown => out[3].y <= target_rect.y + MARKER_PULLBACK_TOLERANCE,
                Direction::BottomTop => {
                    out[3].y >= target_rect.y + target_rect.height - MARKER_PULLBACK_TOLERANCE
                }
                _ => false,
            };
            if has_lateral_offset && target_is_primary_face {
                let delta = out[3].y - out[0].y;
                if delta.abs()
                    > MIN_STEM_FOR_COLLAPSE + MIN_TERMINAL_STEM_FOR_COLLAPSE + MIN_CHANNEL_SPAN
                {
                    let source_stem =
                        (delta.abs() * 0.28).clamp(MIN_STEM_FOR_COLLAPSE, MAX_STEM_FOR_COLLAPSE);
                    let max_terminal_stem = delta.abs() - source_stem - MIN_CHANNEL_SPAN;
                    if max_terminal_stem < MIN_TERMINAL_STEM_FOR_COLLAPSE {
                        return points.to_vec();
                    }
                    let terminal_stem = (delta.abs() * 0.28 + TERMINAL_STEM_BIAS)
                        .clamp(
                            MIN_TERMINAL_STEM_FOR_COLLAPSE,
                            MAX_TERMINAL_STEM_FOR_COLLAPSE,
                        )
                        .min(max_terminal_stem);
                    let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
                    out[1].y = out[0].y + (dir * source_stem);
                    out[2].y = out[3].y - (dir * terminal_stem);
                }
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            let first_horizontal = (out[0].y - out[1].y).abs() <= center_eps
                && (out[0].x - out[1].x).abs() > center_eps;
            let middle_vertical = (out[1].x - out[2].x).abs() <= center_eps
                && (out[1].y - out[2].y).abs() > center_eps;
            let terminal_horizontal = (out[2].y - out[3].y).abs() <= center_eps
                && (out[2].x - out[3].x).abs() > center_eps;
            if !(first_horizontal && middle_vertical && terminal_horizontal) {
                return points.to_vec();
            }

            let has_lateral_offset = (out[0].y - out[3].y).abs() > center_eps;
            let target_is_primary_face = match direction {
                Direction::LeftRight => out[3].x <= target_rect.x + MARKER_PULLBACK_TOLERANCE,
                Direction::RightLeft => {
                    out[3].x >= target_rect.x + target_rect.width - MARKER_PULLBACK_TOLERANCE
                }
                _ => false,
            };
            if has_lateral_offset && target_is_primary_face {
                let delta = out[3].x - out[0].x;
                if delta.abs()
                    > MIN_STEM_FOR_COLLAPSE + MIN_TERMINAL_STEM_FOR_COLLAPSE + MIN_CHANNEL_SPAN
                {
                    let source_stem =
                        (delta.abs() * 0.28).clamp(MIN_STEM_FOR_COLLAPSE, MAX_STEM_FOR_COLLAPSE);
                    let max_terminal_stem = delta.abs() - source_stem - MIN_CHANNEL_SPAN;
                    if max_terminal_stem < MIN_TERMINAL_STEM_FOR_COLLAPSE {
                        return points.to_vec();
                    }
                    let terminal_stem = (delta.abs() * 0.28 + TERMINAL_STEM_BIAS)
                        .clamp(
                            MIN_TERMINAL_STEM_FOR_COLLAPSE,
                            MAX_TERMINAL_STEM_FOR_COLLAPSE,
                        )
                        .min(max_terminal_stem);
                    let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
                    out[1].x = out[0].x + (dir * source_stem);
                    out[2].x = out[3].x - (dir * terminal_stem);
                }
            }
        }
    }

    out
}

pub(super) fn synthesize_bezier_control_points(
    start: Point,
    end: Point,
    direction: Direction,
) -> (Point, Point) {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let dy = end.y - start.y;
            (
                Point {
                    x: start.x,
                    y: start.y + dy / 3.0,
                },
                Point {
                    x: end.x,
                    y: start.y + 2.0 * dy / 3.0,
                },
            )
        }
        Direction::LeftRight | Direction::RightLeft => {
            let dx = end.x - start.x;
            (
                Point {
                    x: start.x + dx / 3.0,
                    y: start.y,
                },
                Point {
                    x: start.x + 2.0 * dx / 3.0,
                    y: end.y,
                },
            )
        }
    }
}

pub(super) fn synthesize_reciprocal_bezier_control_points(
    start: Point,
    end: Point,
    direction: Direction,
    curve_sign: f64,
) -> (Point, Point) {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let dy = end.y - start.y;
            let bow = (dy.abs() * 0.25).clamp(12.0, 28.0) * curve_sign;
            (
                Point {
                    x: start.x + bow,
                    y: start.y + dy / 3.0,
                },
                Point {
                    x: end.x + bow,
                    y: start.y + 2.0 * dy / 3.0,
                },
            )
        }
        Direction::LeftRight | Direction::RightLeft => {
            let dx = end.x - start.x;
            let bow = (dx.abs() * 0.25).clamp(12.0, 28.0) * curve_sign;
            (
                Point {
                    x: start.x + dx / 3.0,
                    y: start.y + bow,
                },
                Point {
                    x: start.x + 2.0 * dx / 3.0,
                    y: end.y + bow,
                },
            )
        }
    }
}

fn insert_basis_start_cap_if_needed(points: &mut Vec<Point>, min_stem: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return;
    }

    let base_vec = Point {
        x: points[1].x - points[0].x,
        y: points[1].y - points[0].y,
    };
    let first_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if first_segment_len <= EPS || first_segment_len + EPS >= min_stem {
        return;
    }

    let mut traversed = 0.0;
    for seg_idx in 0..(points.len() - 1) {
        let seg_vec = Point {
            x: points[seg_idx + 1].x - points[seg_idx].x,
            y: points[seg_idx + 1].y - points[seg_idx].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            if t >= 1.0 - EPS {
                return;
            }
            let cap = Point {
                x: points[seg_idx].x + seg_vec.x * t,
                y: points[seg_idx].y + seg_vec.y * t,
            };
            if !points_approx_equal(cap, points[seg_idx])
                && !points_approx_equal(cap, points[seg_idx + 1])
            {
                points.insert(seg_idx + 1, cap);
            }
            return;
        }
        traversed += seg_len;
    }
}

fn insert_basis_end_cap_if_needed(points: &mut Vec<Point>, min_stem: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return;
    }

    let n = points.len();
    let base_vec = Point {
        x: points[n - 2].x - points[n - 1].x,
        y: points[n - 2].y - points[n - 1].y,
    };
    let last_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if last_segment_len <= EPS || last_segment_len + EPS >= min_stem {
        return;
    }

    let mut traversed = 0.0;
    for seg_idx in (0..(points.len() - 1)).rev() {
        let seg_vec = Point {
            x: points[seg_idx].x - points[seg_idx + 1].x,
            y: points[seg_idx].y - points[seg_idx + 1].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            if t >= 1.0 - EPS {
                return;
            }
            let cap = Point {
                x: points[seg_idx + 1].x + seg_vec.x * t,
                y: points[seg_idx + 1].y + seg_vec.y * t,
            };
            if !points_approx_equal(cap, points[seg_idx + 1])
                && !points_approx_equal(cap, points[seg_idx])
            {
                points.insert(seg_idx + 1, cap);
            }
            return;
        }
        traversed += seg_len;
    }
}

fn start_cap_point_on_existing_run(points: &[Point], min_stem: f64) -> Option<(usize, Point)> {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return None;
    }

    let base_vec = Point {
        x: points[1].x - points[0].x,
        y: points[1].y - points[0].y,
    };
    let first_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if first_segment_len <= EPS {
        return None;
    }

    let mut traversed = 0.0;
    let mut last_seg_idx_on_ray = 0usize;
    for seg_idx in 0..(points.len() - 1) {
        let seg_vec = Point {
            x: points[seg_idx + 1].x - points[seg_idx].x,
            y: points[seg_idx + 1].y - points[seg_idx].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        last_seg_idx_on_ray = seg_idx;
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            let cap = Point {
                x: points[seg_idx].x + seg_vec.x * t,
                y: points[seg_idx].y + seg_vec.y * t,
            };
            return Some((seg_idx, cap));
        }
        traversed += seg_len;
    }

    Some((last_seg_idx_on_ray, points[last_seg_idx_on_ray + 1]))
}

fn rebuild_with_start_cap(points: &[Point], seg_idx: usize, cap: Point) -> Vec<Point> {
    if points.len() < 2 || seg_idx >= points.len() - 1 {
        return points.to_vec();
    }

    let mut rebuilt = Vec::with_capacity(points.len() + 1);
    rebuilt.push(points[0]);
    rebuilt.push(cap);
    rebuilt.extend_from_slice(&points[(seg_idx + 1)..]);
    dedup_consecutive_svg_points(&rebuilt)
}

fn end_cap_point_on_existing_run(points: &[Point], min_stem: f64) -> Option<(usize, Point)> {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return None;
    }

    let n = points.len();
    let base_vec = Point {
        x: points[n - 2].x - points[n - 1].x,
        y: points[n - 2].y - points[n - 1].y,
    };
    let last_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if last_segment_len <= EPS {
        return None;
    }

    let mut traversed = 0.0;
    let mut first_seg_idx_on_ray = n - 2;
    for seg_idx in (0..(points.len() - 1)).rev() {
        let seg_vec = Point {
            x: points[seg_idx].x - points[seg_idx + 1].x,
            y: points[seg_idx].y - points[seg_idx + 1].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        first_seg_idx_on_ray = seg_idx;
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            let cap = Point {
                x: points[seg_idx + 1].x + seg_vec.x * t,
                y: points[seg_idx + 1].y + seg_vec.y * t,
            };
            return Some((seg_idx, cap));
        }
        traversed += seg_len;
    }

    Some((first_seg_idx_on_ray, points[first_seg_idx_on_ray]))
}

fn rebuild_with_end_cap(points: &[Point], seg_idx: usize, cap: Point) -> Vec<Point> {
    if points.len() < 2 || seg_idx >= points.len() - 1 {
        return points.to_vec();
    }

    let mut rebuilt = Vec::with_capacity(points.len() + 1);
    rebuilt.extend_from_slice(&points[..=seg_idx]);
    rebuilt.push(cap);
    rebuilt.push(points[points.len() - 1]);
    dedup_consecutive_svg_points(&rebuilt)
}

fn primary_axis_for_direction(direction: Direction) -> SegmentAxis {
    match direction {
        Direction::TopDown | Direction::BottomTop => SegmentAxis::Vertical,
        Direction::LeftRight | Direction::RightLeft => SegmentAxis::Horizontal,
    }
}

fn is_primary_secondary_primary_return(points: &[Point], direction: Direction) -> bool {
    if points.len() != 4 {
        return false;
    }
    let primary = primary_axis_for_direction(direction);
    let secondary = match primary {
        SegmentAxis::Horizontal => SegmentAxis::Vertical,
        SegmentAxis::Vertical => SegmentAxis::Horizontal,
    };
    matches!(
        (
            segment_axis(points[0], points[1]),
            segment_axis(points[1], points[2]),
            segment_axis(points[2], points[3]),
        ),
        (Some(a), Some(b), Some(c)) if a == primary && b == secondary && c == primary
    )
}

fn first_segment_is_secondary_axis(points: &[Point], direction: Direction) -> bool {
    if points.len() < 2 {
        return false;
    }
    match segment_axis(points[0], points[1]) {
        Some(SegmentAxis::Horizontal) => {
            matches!(direction, Direction::TopDown | Direction::BottomTop)
        }
        Some(SegmentAxis::Vertical) => {
            matches!(direction, Direction::LeftRight | Direction::RightLeft)
        }
        None => false,
    }
}

fn ends_on_secondary_axis(points: &[Point], direction: Direction) -> bool {
    if points.len() < 2 {
        return false;
    }
    let last = points.len() - 1;
    match segment_axis(points[last - 1], points[last]) {
        Some(SegmentAxis::Horizontal) => {
            matches!(direction, Direction::TopDown | Direction::BottomTop)
        }
        Some(SegmentAxis::Vertical) => {
            matches!(direction, Direction::LeftRight | Direction::RightLeft)
        }
        None => false,
    }
}

pub(super) fn is_simple_two_node_reciprocal_pair(diagram: &Graph, edge: &Edge) -> bool {
    if edge.from == edge.to || edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        return false;
    }

    let mut pair_edges = 0usize;
    let mut has_forward = false;
    let mut has_backward = false;

    for other in &diagram.edges {
        if other.stroke == Stroke::Invisible {
            continue;
        }

        let touches_endpoints = other.from == edge.from
            || other.to == edge.from
            || other.from == edge.to
            || other.to == edge.to;
        if !touches_endpoints {
            continue;
        }

        if other.from_subgraph.is_some() || other.to_subgraph.is_some() {
            return false;
        }

        let is_forward = other.from == edge.from && other.to == edge.to;
        let is_backward = other.from == edge.to && other.to == edge.from;
        if !is_forward && !is_backward {
            return false;
        }

        pair_edges += 1;
        has_forward |= is_forward;
        has_backward |= is_backward;
    }

    has_forward && has_backward && pair_edges == 2
}
