use super::super::float_core::normalize_orthogonal_route_contracts;
use super::collision::{
    axis_aligned_segment_crosses_rect_interior, segment_crosses_any_other_node_interior,
};
use super::constants::{MIN_PORT_CORNER_INSET_BACKWARD, POINT_EPS};
use super::endpoints::{
    bias_face_coordinate_toward_center, clamp_face_coordinate_with_corner_inset,
    endpoint_rect_and_shape, enforce_backward_terminal_corner_inset, hint_face_for_td_bt_parity,
};
use super::forward;
use super::path_utils::{collapse_collinear_interior_points, points_match, ranges_overlap};
use crate::graph::Direction;
use crate::graph::attachment::{
    Face, can_apply_td_bt_backward_hint_parity, canonical_backward_channel_face,
    prefer_backward_side_channel,
};
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::{FPoint, FRect};

pub(super) fn reroute_skip_backward_lane_for_node_clearance(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    node_clearance: f64,
    min_source_stem: f64,
    min_target_stem: f64,
) -> bool {
    if path.len() != 4 || node_clearance <= 0.0 {
        return false;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];
    let v_h_v = (p0.x - p1.x).abs() <= POINT_EPS
        && (p0.y - p1.y).abs() > POINT_EPS
        && (p1.y - p2.y).abs() <= POINT_EPS
        && (p1.x - p2.x).abs() > POINT_EPS
        && (p2.x - p3.x).abs() <= POINT_EPS
        && (p2.y - p3.y).abs() > POINT_EPS;
    let h_v_h = (p0.y - p1.y).abs() <= POINT_EPS
        && (p0.x - p1.x).abs() > POINT_EPS
        && (p1.x - p2.x).abs() <= POINT_EPS
        && (p1.y - p2.y).abs() > POINT_EPS
        && (p2.y - p3.y).abs() <= POINT_EPS
        && (p2.x - p3.x).abs() > POINT_EPS;
    if !v_h_v && !h_v_h {
        return false;
    }

    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    if primary_vertical != v_h_v {
        return false;
    }

    let flow_sign = if primary_vertical {
        (p3.y - p0.y).signum()
    } else {
        (p3.x - p0.x).signum()
    };
    if flow_sign.abs() <= POINT_EPS {
        return false;
    }

    let mut lane = if primary_vertical { p1.y } else { p1.x };
    let mut saw_intrusion = false;

    for (node_id, node) in &geometry.nodes {
        if node_id == &edge.from || node_id == &edge.to {
            continue;
        }

        let rect = node.rect;
        let center_segment_crosses = axis_aligned_segment_crosses_rect_interior(p1, p2, rect, -0.5);
        let side_segment_crosses = axis_aligned_segment_crosses_rect_interior(p0, p1, rect, -0.5)
            || axis_aligned_segment_crosses_rect_interior(p2, p3, rect, -0.5);
        let overlaps_lane_span = if primary_vertical {
            ranges_overlap(p1.x.min(p2.x), p1.x.max(p2.x), rect.x, rect.x + rect.width)
        } else {
            ranges_overlap(p1.y.min(p2.y), p1.y.max(p2.y), rect.y, rect.y + rect.height)
        };
        if !overlaps_lane_span {
            continue;
        }

        let (blocked_min, blocked_max) = if primary_vertical {
            (rect.y, rect.y + rect.height)
        } else {
            (rect.x, rect.x + rect.width)
        };
        let near_corridor =
            lane > blocked_min - node_clearance && lane < blocked_max + node_clearance;
        if !(center_segment_crosses || side_segment_crosses || near_corridor) {
            continue;
        }

        saw_intrusion = true;
        if flow_sign > 0.0 {
            lane = lane.min(blocked_min - node_clearance);
        } else {
            lane = lane.max(blocked_max + node_clearance);
        }
    }

    if !saw_intrusion {
        return false;
    }

    if primary_vertical {
        let min_lane = p0.y + flow_sign * min_source_stem;
        let max_lane = p3.y - flow_sign * min_target_stem;
        let clamped_lane = if flow_sign > 0.0 {
            if max_lane < min_lane {
                return false;
            }
            lane.clamp(min_lane, max_lane)
        } else {
            if min_lane < max_lane {
                return false;
            }
            lane.clamp(max_lane, min_lane)
        };
        if (clamped_lane - p1.y).abs() <= POINT_EPS {
            return false;
        }
        let new_p1 = FPoint::new(p1.x, clamped_lane);
        let new_p2 = FPoint::new(p2.x, clamped_lane);
        if (new_p1.y - p0.y).abs() <= POINT_EPS
            || (new_p2.x - new_p1.x).abs() <= POINT_EPS
            || (p3.y - new_p2.y).abs() <= POINT_EPS
        {
            return false;
        }
        path[1] = new_p1;
        path[2] = new_p2;
    } else {
        let min_lane = p0.x + flow_sign * min_source_stem;
        let max_lane = p3.x - flow_sign * min_target_stem;
        let clamped_lane = if flow_sign > 0.0 {
            if max_lane < min_lane {
                return false;
            }
            lane.clamp(min_lane, max_lane)
        } else {
            if min_lane < max_lane {
                return false;
            }
            lane.clamp(max_lane, min_lane)
        };
        if (clamped_lane - p1.x).abs() <= POINT_EPS {
            return false;
        }
        let new_p1 = FPoint::new(clamped_lane, p1.y);
        let new_p2 = FPoint::new(clamped_lane, p2.y);
        if (new_p1.x - p0.x).abs() <= POINT_EPS
            || (new_p2.y - new_p1.y).abs() <= POINT_EPS
            || (p3.x - new_p2.x).abs() <= POINT_EPS
        {
            return false;
        }
        path[1] = new_p1;
        path[2] = new_p2;
    }

    true
}

pub(super) fn avoid_backward_td_bt_vertical_lane_node_intrusion(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const INTRUSION_MARGIN: f64 = 1.0;
    const NODE_CLEARANCE: f64 = 8.0;
    const MIN_SOURCE_STEM: f64 = 8.0;
    const MIN_TARGET_STEM: f64 = 8.0;
    const EPS: f64 = 0.000_001;

    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || path.len() < 4 {
        return;
    }

    let n = path.len();
    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[n - 2];
    let p3 = path[n - 1];
    let first_horizontal = (p0.y - p1.y).abs() <= EPS && (p0.x - p1.x).abs() > EPS;
    let middle_vertical = (p1.x - p2.x).abs() <= EPS && (p1.y - p2.y).abs() > EPS;
    let terminal_horizontal = (p2.y - p3.y).abs() <= EPS && (p2.x - p3.x).abs() > EPS;
    if !(first_horizontal && middle_vertical && terminal_horizontal) {
        return;
    }
    let lane_x = p1.x;
    let interior_stays_on_lane = path[1..(n - 1)]
        .iter()
        .all(|point| (point.x - lane_x).abs() <= EPS);
    if !interior_stays_on_lane {
        return;
    }

    if !segment_crosses_any_other_node_interior(edge, geometry, p1, p2, INTRUSION_MARGIN) {
        return;
    }

    let y_min = p0.y.min(p3.y);
    let y_max = p0.y.max(p3.y);
    let mut candidates = vec![p1.x];
    for (node_id, node) in &geometry.nodes {
        if node_id == &edge.from || node_id == &edge.to {
            continue;
        }
        let rect = node.rect;
        if !ranges_overlap(y_min, y_max, rect.y, rect.y + rect.height) {
            continue;
        }
        candidates.push(rect.x - NODE_CLEARANCE);
        candidates.push(rect.x + rect.width + NODE_CLEARANCE);
    }
    candidates.sort_by(|a, b| a.total_cmp(b));
    candidates.dedup_by(|a, b| (*a - *b).abs() <= 0.5);

    let preferred_min_x = p0.x.max(p3.x);
    let mut best: Option<(f64, f64)> = None;
    for lane_x in candidates {
        if (lane_x - p0.x).abs() < MIN_SOURCE_STEM || (lane_x - p3.x).abs() < MIN_TARGET_STEM {
            continue;
        }
        let a = FPoint::new(lane_x, p0.y);
        let b = FPoint::new(lane_x, p3.y);
        let segments_clear =
            !segment_crosses_any_other_node_interior(edge, geometry, p0, a, INTRUSION_MARGIN)
                && !segment_crosses_any_other_node_interior(edge, geometry, a, b, INTRUSION_MARGIN)
                && !segment_crosses_any_other_node_interior(
                    edge,
                    geometry,
                    b,
                    p3,
                    INTRUSION_MARGIN,
                );
        if !segments_clear {
            continue;
        }

        let side_penalty = if lane_x <= preferred_min_x + EPS {
            10_000.0
        } else {
            0.0
        };
        let score = (lane_x - p1.x).abs() + side_penalty;
        match &best {
            Some((best_score, _)) if score >= *best_score => {}
            _ => best = Some((score, lane_x)),
        }
    }

    if let Some((_, lane_x)) = best {
        for point in path.iter_mut().take(n - 1).skip(1) {
            point.x = lane_x;
        }
    }
}

pub(super) fn has_backward_corridor_obstructions(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> bool {
    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return false;
    };

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let corridor_left = sr.x.min(tr.x);
            let corridor_right = (sr.x + sr.width).max(tr.x + tr.width);
            let min_y = sr.y.min(tr.y);
            let max_y = (sr.y + sr.height).max(tr.y + tr.height);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                cy > min_y
                    && cy < max_y
                    && node.rect.x < corridor_right
                    && node_right > corridor_left
            })
        }
        Direction::LeftRight | Direction::RightLeft => {
            let corridor_top = sr.y.min(tr.y);
            let corridor_bottom = (sr.y + sr.height).max(tr.y + tr.height);
            let min_x = sr.x.min(tr.x);
            let max_x = (sr.x + sr.width).max(tr.x + tr.width);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                let cx = node.rect.center_x();
                let node_bottom = node.rect.y + node.rect.height;
                cx > min_x
                    && cx < max_x
                    && node.rect.y < corridor_bottom
                    && node_bottom > corridor_top
            })
        }
    }
}

pub(crate) fn build_backward_orthogonal_channel_path(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<Vec<FPoint>> {
    const CHANNEL_CLEARANCE: f64 = 12.0;

    let sr = geometry.nodes.get(&edge.from)?.rect;
    let tr = geometry.nodes.get(&edge.to)?.rect;

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let source_right = sr.x + sr.width;
            let target_right = tr.x + tr.width;
            let source_cy = sr.center_y();
            let target_cy = tr.center_y();

            let face_envelope = source_right.max(target_right);
            let min_y = sr.y.min(tr.y);
            let max_y = (sr.y + sr.height).max(tr.y + tr.height);
            let mut lane_x = face_envelope + CHANNEL_CLEARANCE;
            for node in geometry.nodes.values() {
                if node.id == edge.from || node.id == edge.to {
                    continue;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                if cy >= min_y && cy <= max_y {
                    lane_x = lane_x.max(node_right + CHANNEL_CLEARANCE);
                }
            }

            Some(vec![
                FPoint::new(source_right, source_cy),
                FPoint::new(lane_x, source_cy),
                FPoint::new(lane_x, target_cy),
                FPoint::new(target_right, target_cy),
            ])
        }
        Direction::LeftRight | Direction::RightLeft => {
            let source_bottom = sr.y + sr.height;
            let target_bottom = tr.y + tr.height;
            let source_cx = sr.center_x();
            let target_cx = tr.center_x();

            let face_envelope = source_bottom.max(target_bottom);
            let min_x = sr.x.min(tr.x);
            let max_x = (sr.x + sr.width).max(tr.x + tr.width);
            let corridor_top = sr.y.min(tr.y);
            let mut lane_y = face_envelope + CHANNEL_CLEARANCE;
            for node in geometry.nodes.values() {
                if node.id == edge.from || node.id == edge.to {
                    continue;
                }
                let cx = node.rect.center_x();
                let node_bottom = node.rect.y + node.rect.height;
                if cx >= min_x && cx <= max_x && node.rect.y < lane_y && node_bottom > corridor_top
                {
                    lane_y = lane_y.max(node_bottom + CHANNEL_CLEARANCE);
                }
            }

            Some(vec![
                FPoint::new(source_cx, source_bottom),
                FPoint::new(source_cx, lane_y),
                FPoint::new(target_cx, lane_y),
                FPoint::new(target_cx, target_bottom),
            ])
        }
    }
}

pub(super) fn build_short_backward_side_lane_path(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<Vec<FPoint>> {
    const FACE_EPS: f64 = 1.0;

    let (source_rect, _) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())?;
    let (target_rect, _) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())?;

    let max_offset = (source_rect.height.min(target_rect.height) / 3.0).min(20.0);
    let offset = max_offset.max(8.0);

    let source_x = match direction {
        Direction::LeftRight => source_rect.x,
        Direction::RightLeft => source_rect.x + source_rect.width,
        _ => return None,
    };
    let target_x = match direction {
        Direction::LeftRight => target_rect.x + target_rect.width,
        Direction::RightLeft => target_rect.x,
        _ => return None,
    };

    let source_y = (source_rect.center_y() + offset).clamp(
        source_rect.y + FACE_EPS,
        source_rect.y + source_rect.height - FACE_EPS,
    );
    let target_y = (target_rect.center_y() + offset).clamp(
        target_rect.y + FACE_EPS,
        target_rect.y + target_rect.height - FACE_EPS,
    );

    if (source_y - target_y).abs() <= POINT_EPS {
        return Some(vec![
            FPoint::new(source_x, source_y),
            FPoint::new(target_x, target_y),
        ]);
    }

    let lane_y = source_y.max(target_y);
    Some(vec![
        FPoint::new(source_x, source_y),
        FPoint::new(source_x, lane_y),
        FPoint::new(target_x, lane_y),
        FPoint::new(target_x, target_y),
    ])
}

pub(super) fn backward_td_bt_face_overrides(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
    _target_overflowed: bool,
    rank_span: usize,
) -> (Option<Face>, Option<Face>) {
    if !is_backward || !matches!(direction, Direction::TopDown | Direction::BottomTop) {
        return (None, None);
    }
    let has_subgraph_endpoint = edge.from_subgraph.is_some() || edge.to_subgraph.is_some();
    if has_subgraph_endpoint {
        return (None, None);
    }
    if prefer_backward_side_channel(is_backward, true, Some(rank_span)) {
        return (None, None);
    }
    let hint = edge.layout_path_hint.as_ref();
    let Some(hint) = hint else {
        return (None, None);
    };
    if hint.len() < 2 {
        return (None, None);
    }

    let Some((source_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return (None, None);
    };
    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return (None, None);
    };

    let source_hint = hint[0];
    let target_hint = hint[hint.len() - 1];
    let source_override = hint_face_for_td_bt_parity(source_hint, source_rect)
        .filter(|face| matches!(face, Face::Top | Face::Bottom));
    let target_override = hint_face_for_td_bt_parity(target_hint, target_rect)
        .filter(|face| matches!(face, Face::Top | Face::Bottom));
    if target_override.is_none() {
        return (None, None);
    }

    let source_override = source_override.map(|face| {
        let forward_source_face = match direction {
            Direction::TopDown => Face::Bottom,
            Direction::BottomTop => Face::Top,
            _ => return face,
        };
        if face == forward_source_face {
            match face {
                Face::Top => Face::Bottom,
                Face::Bottom => Face::Top,
                other => other,
            }
        } else {
            face
        }
    });

    let source_center_x = source_rect.x + source_rect.width / 2.0;
    if !can_apply_td_bt_backward_hint_parity(
        direction,
        is_backward,
        has_subgraph_endpoint,
        rank_span,
        source_rect,
        target_rect,
        source_center_x,
    ) {
        return (None, None);
    }

    (source_override, target_override)
}

pub(super) fn ensure_backward_outer_lane_clearance(
    path: &mut [FPoint],
    direction: Direction,
    min_clearance: f64,
) {
    const EPS: f64 = 0.000_001;
    if path.len() < 3 || min_clearance <= 0.0 {
        return;
    }

    let last = path.len() - 1;
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let baseline = path[0].x.max(path[last].x);
            let route_max = path
                .iter()
                .map(|point| point.x)
                .fold(f64::NEG_INFINITY, f64::max);
            if route_max - baseline + EPS >= min_clearance {
                return;
            }
            let interior_at_max: Vec<usize> = path
                .iter()
                .enumerate()
                .filter(|(idx, point)| {
                    *idx > 0 && *idx < last && (point.x - route_max).abs() <= EPS
                })
                .map(|(idx, _)| idx)
                .collect();
            if interior_at_max.is_empty() {
                return;
            }
            let target_x = baseline + min_clearance;
            for idx in interior_at_max {
                path[idx].x = target_x;
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            let baseline = path[0].y.max(path[last].y);
            let route_max = path
                .iter()
                .map(|point| point.y)
                .fold(f64::NEG_INFINITY, f64::max);
            if route_max - baseline + EPS >= min_clearance {
                return;
            }
            let interior_at_max: Vec<usize> = path
                .iter()
                .enumerate()
                .filter(|(idx, point)| {
                    *idx > 0 && *idx < last && (point.y - route_max).abs() <= EPS
                })
                .map(|(idx, _)| idx)
                .collect();
            if interior_at_max.is_empty() {
                return;
            }
            let target_y = baseline + min_clearance;
            for idx in interior_at_max {
                path[idx].y = target_y;
            }
        }
    }
}

pub(super) fn enforce_backward_minimum_channel_floor(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    min_clearance: f64,
) {
    const EPS: f64 = 0.000_001;
    if path.len() < 3 || min_clearance <= 0.0 {
        return;
    }

    let last = path.len() - 1;
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let src_rect =
                endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref());
            let tgt_rect = endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref());
            let (Some((sr, _)), Some((tr, _))) = (src_rect, tgt_rect) else {
                return;
            };
            let node_envelope = (sr.x + sr.width).max(tr.x + tr.width);
            let any_beyond = path[1..last].iter().any(|p| p.x > node_envelope - EPS);
            if !any_beyond {
                return;
            }
            let min_channel = node_envelope + min_clearance;
            for point in path.iter_mut().take(last).skip(1) {
                if point.x > node_envelope + EPS && point.x < min_channel - EPS {
                    point.x = min_channel;
                }
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            let src_bottom =
                endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
                    .map(|(r, _)| r.y + r.height);
            let tgt_bottom =
                endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
                    .map(|(r, _)| r.y + r.height);
            let node_envelope = match (src_bottom, tgt_bottom) {
                (Some(s), Some(t)) => s.max(t),
                (Some(s), None) => s,
                (None, Some(t)) => t,
                (None, None) => return,
            };
            let min_channel = node_envelope + min_clearance;
            for point in path.iter_mut().take(last).skip(1) {
                if point.y > node_envelope + EPS && point.y < min_channel - EPS {
                    point.y = min_channel;
                }
            }
        }
    }
}

pub(super) fn fix_backward_diagonal_node_collision(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const EPS: f64 = 0.000_001;
    const MARGIN: f64 = 8.0;

    if path.len() < 3 {
        return;
    }

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            fix_backward_diagonal_source_td_bt(path, edge, geometry, MARGIN, EPS);
            fix_backward_diagonal_target_td_bt(path, edge, geometry, MARGIN, EPS);
        }
        Direction::LeftRight | Direction::RightLeft => {
            fix_backward_diagonal_source_lr_rl(path, edge, geometry, MARGIN, EPS);
            fix_backward_diagonal_target_lr_rl(path, edge, geometry, MARGIN, EPS);
        }
    }
}

fn fix_backward_diagonal_source_td_bt(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    margin: f64,
    eps: f64,
) {
    if path.len() < 3 {
        return;
    }
    let source = path[0];
    let next = path[1];

    let dx = (source.x - next.x).abs();
    let dy = (source.y - next.y).abs();
    if dx <= eps || dy <= eps {
        return;
    }

    let vert_x = source.x;
    let vert_y_min = source.y.min(next.y);
    let vert_y_max = source.y.max(next.y);

    let collides = geometry.nodes.values().any(|node| {
        if node.id == edge.from || node.id == edge.to {
            return false;
        }
        let n_left = node.rect.x;
        let n_right = node.rect.x + node.rect.width;
        let n_top = node.rect.y;
        let n_bottom = node.rect.y + node.rect.height;
        vert_x > n_left + eps
            && vert_x < n_right - eps
            && vert_y_max > n_top + eps
            && vert_y_min < n_bottom - eps
    });
    if !collides {
        return;
    }

    let last = path.len() - 1;
    let mut safe_x = path[1..last]
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);

    let mut changed = true;
    while changed {
        changed = false;
        for node in geometry.nodes.values() {
            if node.id == edge.from || node.id == edge.to {
                continue;
            }
            let n_left = node.rect.x;
            let n_right = node.rect.x + node.rect.width;
            let n_top = node.rect.y;
            let n_bottom = node.rect.y + node.rect.height;
            if n_bottom > vert_y_min + eps
                && n_top < vert_y_max - eps
                && safe_x > n_left + eps
                && safe_x < n_right - eps
            {
                safe_x = n_right + margin;
                changed = true;
            }
        }
    }

    path[1] = FPoint::new(safe_x, next.y);
    path.insert(1, FPoint::new(safe_x, source.y));
}

fn fix_backward_diagonal_target_td_bt(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    margin: f64,
    eps: f64,
) {
    if path.len() < 3 {
        return;
    }
    let last = path.len() - 1;
    let target = path[last];
    let prev = path[last - 1];

    let dx = (target.x - prev.x).abs();
    let dy = (target.y - prev.y).abs();
    if dx <= eps || dy <= eps {
        return;
    }

    let vert_x = target.x;
    let vert_y_min = target.y.min(prev.y);
    let vert_y_max = target.y.max(prev.y);

    let collides = geometry.nodes.values().any(|node| {
        if node.id == edge.from || node.id == edge.to {
            return false;
        }
        let n_left = node.rect.x;
        let n_right = node.rect.x + node.rect.width;
        let n_top = node.rect.y;
        let n_bottom = node.rect.y + node.rect.height;
        vert_x > n_left + eps
            && vert_x < n_right - eps
            && vert_y_max > n_top + eps
            && vert_y_min < n_bottom - eps
    });
    if !collides {
        return;
    }

    let mut safe_x = path[1..last]
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);

    let mut changed = true;
    while changed {
        changed = false;
        for node in geometry.nodes.values() {
            if node.id == edge.from || node.id == edge.to {
                continue;
            }
            let n_left = node.rect.x;
            let n_right = node.rect.x + node.rect.width;
            let n_top = node.rect.y;
            let n_bottom = node.rect.y + node.rect.height;
            if n_bottom > vert_y_min + eps
                && n_top < vert_y_max - eps
                && safe_x > n_left + eps
                && safe_x < n_right - eps
            {
                safe_x = n_right + margin;
                changed = true;
            }
        }
    }

    let last = path.len() - 1;
    path[last - 1] = FPoint::new(safe_x, prev.y);
    path.insert(last, FPoint::new(safe_x, target.y));
}

fn fix_backward_diagonal_source_lr_rl(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    margin: f64,
    eps: f64,
) {
    if path.len() < 3 {
        return;
    }
    let source = path[0];
    let next = path[1];

    let dx = (source.x - next.x).abs();
    let dy = (source.y - next.y).abs();
    if dx <= eps || dy <= eps {
        return;
    }

    let horiz_y = source.y;
    let horiz_x_min = source.x.min(next.x);
    let horiz_x_max = source.x.max(next.x);

    let collides = geometry.nodes.values().any(|node| {
        if node.id == edge.from || node.id == edge.to {
            return false;
        }
        let n_left = node.rect.x;
        let n_right = node.rect.x + node.rect.width;
        let n_top = node.rect.y;
        let n_bottom = node.rect.y + node.rect.height;
        horiz_y > n_top + eps
            && horiz_y < n_bottom - eps
            && horiz_x_max > n_left + eps
            && horiz_x_min < n_right - eps
    });
    if !collides {
        return;
    }

    let last = path.len() - 1;
    let mut safe_y = path[1..last]
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);

    let mut changed = true;
    while changed {
        changed = false;
        for node in geometry.nodes.values() {
            if node.id == edge.from || node.id == edge.to {
                continue;
            }
            let n_top = node.rect.y;
            let n_bottom = node.rect.y + node.rect.height;
            let n_left = node.rect.x;
            let n_right = node.rect.x + node.rect.width;
            if n_right > horiz_x_min + eps
                && n_left < horiz_x_max - eps
                && safe_y > n_top + eps
                && safe_y < n_bottom - eps
            {
                safe_y = n_bottom + margin;
                changed = true;
            }
        }
    }

    path[1] = FPoint::new(next.x, safe_y);
    path.insert(1, FPoint::new(source.x, safe_y));
}

fn fix_backward_diagonal_target_lr_rl(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    margin: f64,
    eps: f64,
) {
    if path.len() < 3 {
        return;
    }
    let last = path.len() - 1;
    let target = path[last];
    let prev = path[last - 1];

    let dx = (target.x - prev.x).abs();
    let dy = (target.y - prev.y).abs();
    if dx <= eps || dy <= eps {
        return;
    }

    let horiz_y = target.y;
    let horiz_x_min = target.x.min(prev.x);
    let horiz_x_max = target.x.max(prev.x);

    let collides = geometry.nodes.values().any(|node| {
        if node.id == edge.from || node.id == edge.to {
            return false;
        }
        let n_left = node.rect.x;
        let n_right = node.rect.x + node.rect.width;
        let n_top = node.rect.y;
        let n_bottom = node.rect.y + node.rect.height;
        horiz_y > n_top + eps
            && horiz_y < n_bottom - eps
            && horiz_x_max > n_left + eps
            && horiz_x_min < n_right - eps
    });
    if !collides {
        return;
    }

    let mut safe_y = path[1..last]
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);

    let mut changed = true;
    while changed {
        changed = false;
        for node in geometry.nodes.values() {
            if node.id == edge.from || node.id == edge.to {
                continue;
            }
            let n_top = node.rect.y;
            let n_bottom = node.rect.y + node.rect.height;
            let n_left = node.rect.x;
            let n_right = node.rect.x + node.rect.width;
            if n_right > horiz_x_min + eps
                && n_left < horiz_x_max - eps
                && safe_y > n_top + eps
                && safe_y < n_bottom - eps
            {
                safe_y = n_bottom + margin;
                changed = true;
            }
        }
    }

    let last = path.len() - 1;
    path[last - 1] = FPoint::new(prev.x, safe_y);
    path.insert(last, FPoint::new(target.x, safe_y));
}

pub(super) fn align_backward_source_stem_to_outer_lane(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const EPS: f64 = 0.000_001;
    const FACE_MARGIN: f64 = 1.0;
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || path.len() < 3 {
        return;
    }

    let Some((source_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return;
    };

    let top = source_rect.y;
    let bottom = source_rect.y + source_rect.height;
    let mut start = path[0];
    let support = path[1];
    let next = path[2];

    let start_on_top_or_bottom = (start.y - top).abs() <= EPS || (start.y - bottom).abs() <= EPS;
    if !start_on_top_or_bottom {
        return;
    }

    let stem_is_diagonal = (start.x - support.x).abs() > EPS && (start.y - support.y).abs() > EPS;
    if !stem_is_diagonal {
        return;
    }

    let support_to_next_is_horizontal =
        (support.y - next.y).abs() <= EPS && (support.x - next.x).abs() > EPS;
    if !support_to_next_is_horizontal {
        return;
    }

    let left = source_rect.x;
    let right = source_rect.x + source_rect.width;
    let min_x = left + FACE_MARGIN;
    let max_x = right - FACE_MARGIN;
    let lane_x = support.x;
    if lane_x < min_x - EPS || lane_x > max_x + EPS {
        return;
    }

    start.x = lane_x.clamp(min_x, max_x);
    path[0] = start;
}

pub(super) fn align_backward_outer_lane_to_hint(
    path: &mut [FPoint],
    hint: Option<&[FPoint]>,
    direction: Direction,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) {
    const EPS: f64 = 0.000_001;
    if path.len() < 3 {
        return;
    }
    let Some(hint) = hint else {
        return;
    };
    if hint.len() < 2 {
        return;
    }

    let last = path.len() - 1;
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let Some((target_rect, _)) =
                endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
            else {
                return;
            };
            let hint_target = hint[hint.len() - 1];
            if hint_side_face_for_td_alignment(hint_target, target_rect).is_none() {
                return;
            }

            let hint_outer = hint
                .iter()
                .map(|point| point.x)
                .fold(f64::NEG_INFINITY, f64::max);
            let mut min_outer = f64::NEG_INFINITY;
            if let Some((src_rect, _)) =
                endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
            {
                min_outer = min_outer.max(src_rect.x + src_rect.width);
            }
            min_outer = min_outer.max(target_rect.x + target_rect.width);
            if hint_outer < min_outer {
                return;
            }
            let route_outer = path
                .iter()
                .map(|point| point.x)
                .fold(f64::NEG_INFINITY, f64::max);
            if (hint_outer - route_outer).abs() <= EPS {
                return;
            }

            for (idx, point) in path.iter_mut().enumerate() {
                if idx == 0 || idx == last {
                    continue;
                }
                if (point.x - route_outer).abs() <= EPS {
                    point.x = hint_outer;
                }
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            let hint_outer = hint
                .iter()
                .map(|point| point.y)
                .fold(f64::NEG_INFINITY, f64::max);
            let mut min_outer = f64::NEG_INFINITY;
            if let Some((src_rect, _)) =
                endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
            {
                min_outer = min_outer.max(src_rect.y + src_rect.height);
            }
            if let Some((tgt_rect, _)) =
                endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
            {
                min_outer = min_outer.max(tgt_rect.y + tgt_rect.height);
            }
            if hint_outer < min_outer {
                return;
            }
            let route_outer = path
                .iter()
                .map(|point| point.y)
                .fold(f64::NEG_INFINITY, f64::max);
            if (hint_outer - route_outer).abs() <= EPS {
                return;
            }

            for (idx, point) in path.iter_mut().enumerate() {
                if idx == 0 || idx == last {
                    continue;
                }
                if (point.y - route_outer).abs() <= EPS {
                    point.y = hint_outer;
                }
            }

            let max_allowed = hint_outer + 3.0;
            for (idx, point) in path.iter_mut().enumerate() {
                if idx == 0 || idx == last {
                    continue;
                }
                if point.y > max_allowed {
                    point.y = max_allowed;
                }
            }
        }
    }
}

fn hint_side_face_for_td_alignment(point: FPoint, rect: FRect) -> Option<Face> {
    const FACE_EPS: f64 = 2.0;
    const CORNER_BIAS: f64 = 0.5;
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    let dist_left = (point.x - left).abs();
    let dist_right = (point.x - right).abs();
    let dist_top = (point.y - top).abs();
    let dist_bottom = (point.y - bottom).abs();

    let side_dist = dist_left.min(dist_right);
    let vertical_dist = dist_top.min(dist_bottom);
    if side_dist <= FACE_EPS && side_dist + CORNER_BIAS < vertical_dist {
        if dist_left <= dist_right {
            Some(Face::Left)
        } else {
            Some(Face::Right)
        }
    } else {
        None
    }
}

pub(super) fn enforce_backward_terminal_tangent_direction(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    preserve_terminal_lane_on_overflow_target: bool,
    preferred_target_face: Option<Face>,
) {
    const EPS: f64 = 0.000_001;
    const TANGENT_STEP: f64 = 12.0;
    if path.len() < 2 {
        return;
    }

    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return;
    };

    let last = path.len() - 1;
    let canonical_face =
        preferred_target_face.unwrap_or_else(|| canonical_backward_channel_face(direction));
    let left = target_rect.x;
    let right = target_rect.x + target_rect.width;
    let top = target_rect.y;
    let bottom = target_rect.y + target_rect.height;

    let existing_support = if path.len() > 2 {
        Some(path[last - 1])
    } else {
        None
    };

    let mut end = path[last];
    let mut support = match canonical_face {
        Face::Left => {
            end.x = left;
            end.y = clamp_face_coordinate_with_corner_inset(
                end.y,
                top,
                bottom,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            FPoint::new(end.x - TANGENT_STEP, end.y)
        }
        Face::Right => {
            end.x = right;
            end.y = clamp_face_coordinate_with_corner_inset(
                end.y,
                top,
                bottom,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            FPoint::new(end.x + TANGENT_STEP, end.y)
        }
        Face::Top => {
            end.x = clamp_face_coordinate_with_corner_inset(
                end.x,
                left,
                right,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            end.y = top;
            FPoint::new(end.x, end.y - TANGENT_STEP)
        }
        Face::Bottom => {
            end.x = clamp_face_coordinate_with_corner_inset(
                end.x,
                left,
                right,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            end.y = bottom;
            FPoint::new(end.x, end.y + TANGENT_STEP)
        }
    };

    if let Some(existing) = existing_support {
        match canonical_face {
            Face::Left => {
                if (existing.y - end.y).abs() <= EPS && existing.x < end.x - EPS {
                    support.x = support.x.min(existing.x);
                }
            }
            Face::Right => {
                if (existing.y - end.y).abs() <= EPS && existing.x > end.x + EPS {
                    support.x = support.x.max(existing.x);
                }
            }
            Face::Top => {
                if (existing.x - end.x).abs() <= EPS && existing.y < end.y - EPS {
                    support.y = support.y.min(existing.y);
                }
            }
            Face::Bottom => {
                if (existing.x - end.x).abs() <= EPS && existing.y > end.y + EPS {
                    support.y = support.y.max(existing.y);
                }
            }
        }
    }

    if path.len() >= 3 {
        let prev = path[last - 2];
        match canonical_face {
            Face::Left => {
                if prev.x < support.x - EPS {
                    support.x = prev.x;
                }
            }
            Face::Right => {
                if prev.x > support.x + EPS {
                    support.x = prev.x;
                }
            }
            Face::Top => {
                if prev.y < support.y - EPS {
                    support.y = prev.y;
                }
            }
            Face::Bottom => {
                if prev.y > support.y + EPS {
                    support.y = prev.y;
                }
            }
        }
    }

    if path.len() >= 4 {
        let pre_prev = path[last - 3];
        match canonical_face {
            Face::Left => {
                if (pre_prev.y - end.y).abs() <= EPS && pre_prev.x < support.x - EPS {
                    support.x = pre_prev.x;
                }
            }
            Face::Right => {
                if (pre_prev.y - end.y).abs() <= EPS && pre_prev.x > support.x + EPS {
                    support.x = pre_prev.x;
                }
            }
            Face::Top => {
                if (pre_prev.x - end.x).abs() <= EPS && pre_prev.y < support.y - EPS {
                    support.y = pre_prev.y;
                }
            }
            Face::Bottom => {
                if (pre_prev.x - end.x).abs() <= EPS && pre_prev.y > support.y + EPS {
                    support.y = pre_prev.y;
                }
            }
        }
    }

    path[last] = end;

    if path.len() == 2 {
        path.insert(last, support);
    } else {
        path[last - 1] = support;
    }

    if path.len() < 3 {
        return;
    }

    let support_idx = path.len() - 2;
    let prev_idx = support_idx - 1;
    let prev = path[prev_idx];
    let support = path[support_idx];
    let support_is_axis_aligned =
        (prev.x - support.x).abs() <= EPS || (prev.y - support.y).abs() <= EPS;
    if support_is_axis_aligned {
        if !preserve_terminal_lane_on_overflow_target {
            collapse_terminal_turnback_spikes(path, canonical_face);
        }
        return;
    }

    let primary_elbow = FPoint::new(prev.x, support.y);
    let fallback_elbow = FPoint::new(support.x, prev.y);

    let can_use_primary =
        !points_match(primary_elbow, prev) && !points_match(primary_elbow, support);
    let can_use_fallback =
        !points_match(fallback_elbow, prev) && !points_match(fallback_elbow, support);

    let prefer_outer_corner_first = matches!(canonical_face, Face::Left | Face::Right);
    if prefer_outer_corner_first {
        if can_use_fallback {
            path.insert(support_idx, fallback_elbow);
        } else if can_use_primary {
            path.insert(support_idx, primary_elbow);
        }
    } else if can_use_primary {
        path.insert(support_idx, primary_elbow);
    } else if can_use_fallback {
        path.insert(support_idx, fallback_elbow);
    }

    if !preserve_terminal_lane_on_overflow_target {
        collapse_terminal_turnback_spikes(path, canonical_face);
    }
}

fn collapse_terminal_turnback_spikes(path: &mut Vec<FPoint>, canonical_face: Face) {
    const EPS: f64 = 0.000_001;
    if path.len() < 4 {
        return;
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum Axis {
        Horizontal,
        Vertical,
    }

    let segment_axis = |a: FPoint, b: FPoint| -> Option<Axis> {
        if (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() > EPS {
            Some(Axis::Vertical)
        } else if (a.y - b.y).abs() <= EPS && (a.x - b.x).abs() > EPS {
            Some(Axis::Horizontal)
        } else {
            None
        }
    };
    let deltas_for_axis = |a: FPoint, b: FPoint, axis: Axis| -> f64 {
        match axis {
            Axis::Horizontal => b.x - a.x,
            Axis::Vertical => b.y - a.y,
        }
    };

    if path.len() >= 4 {
        let n = path.len();
        let pre = path[n - 4];
        let turn = path[n - 3];
        let mut support = path[n - 2];
        let mut end = path[n - 1];
        if let (Some(axis1), Some(axis2), Some(axis3)) = (
            segment_axis(pre, turn),
            segment_axis(turn, support),
            segment_axis(support, end),
        ) {
            let d1 = deltas_for_axis(pre, turn, axis1);
            let d2 = deltas_for_axis(turn, support, axis2);
            let has_reversal = axis1 == axis2
                && axis2 != axis3
                && d1.abs() > EPS
                && d2.abs() > EPS
                && d1.signum() != d2.signum();
            if has_reversal {
                match canonical_face {
                    Face::Left | Face::Right => {
                        support.y = turn.y;
                        end.y = turn.y;
                        path[n - 2] = support;
                        path[n - 1] = end;
                    }
                    Face::Top | Face::Bottom => {
                        support.x = turn.x;
                        end.x = turn.x;
                        path[n - 2] = support;
                        path[n - 1] = end;
                    }
                }
            }
        }
    }

    if path.len() >= 4 {
        let n = path.len();
        let pre = path[n - 4];
        let turn = path[n - 3];
        let support = path[n - 2];
        let end = path[n - 1];
        if let (Some(axis1), Some(axis2)) =
            (segment_axis(turn, support), segment_axis(support, end))
        {
            let d1 = deltas_for_axis(turn, support, axis1);
            let d2 = deltas_for_axis(support, end, axis2);
            let has_reversal =
                axis1 == axis2 && d1.abs() > EPS && d2.abs() > EPS && d1.signum() != d2.signum();
            if has_reversal {
                let candidate = match canonical_face {
                    Face::Left | Face::Right => FPoint::new(support.x, pre.y),
                    Face::Top | Face::Bottom => FPoint::new(pre.x, support.y),
                };
                let candidate_is_valid = !points_match(candidate, pre)
                    && !points_match(candidate, support)
                    && segment_axis(pre, candidate).is_some()
                    && segment_axis(candidate, support).is_some();
                if candidate_is_valid {
                    path[n - 3] = candidate;
                }
            }
        }
    }

    let mut idx = 1usize;
    while idx < path.len() {
        if points_match(path[idx - 1], path[idx]) {
            path.remove(idx);
        } else {
            idx += 1;
        }
    }
}

pub(super) fn collapse_tiny_backward_terminal_staircase(
    path: &mut Vec<FPoint>,
    direction: Direction,
    min_lateral_run: f64,
) {
    const EPS: f64 = 0.000_001;
    if !matches!(direction, Direction::TopDown | Direction::BottomTop)
        || path.len() < 3
        || min_lateral_run <= 0.0
    {
        return;
    }

    let n = path.len();
    let a = path[n - 3];
    let b = path[n - 2];
    let mut c = path[n - 1];

    let ab_is_horizontal = (a.y - b.y).abs() <= EPS && (a.x - b.x).abs() > EPS;
    let bc_is_vertical = (b.x - c.x).abs() <= EPS && (b.y - c.y).abs() > EPS;
    if !ab_is_horizontal || !bc_is_vertical {
        return;
    }

    let lateral_run = (b.x - a.x).abs();
    if lateral_run + EPS >= min_lateral_run {
        return;
    }

    c.x = a.x;
    path[n - 1] = c;
    path[n - 2] = FPoint::new(a.x, b.y);

    let mut idx = 1usize;
    while idx < path.len() {
        if points_match(path[idx - 1], path[idx]) {
            path.remove(idx);
        } else {
            idx += 1;
        }
    }
}

pub(super) fn collapse_backward_terminal_node_intrusion(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> bool {
    const EPS: f64 = 0.000_001;
    const INTRUSION_MARGIN: f64 = 1.0;
    if !matches!(direction, Direction::LeftRight | Direction::RightLeft) || path.len() < 4 {
        return false;
    }

    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return false;
    };

    let left = target_rect.x + INTRUSION_MARGIN;
    let right = target_rect.x + target_rect.width - INTRUSION_MARGIN;
    let top = target_rect.y + INTRUSION_MARGIN;
    let bottom = target_rect.y + target_rect.height - INTRUSION_MARGIN;
    if left >= right || top >= bottom {
        return false;
    }

    let canonical_face = canonical_backward_channel_face(direction);
    let point_is_intrusion =
        |point: FPoint| point.x > left && point.x < right && point.y > top && point.y < bottom;
    let point_is_clean_for_face = |point: FPoint| match canonical_face {
        Face::Top => point.y <= top,
        Face::Bottom => point.y >= bottom,
        Face::Left => point.x <= left,
        Face::Right => point.x >= right,
    };
    let last = path.len() - 1;
    let Some(first_intrusion_idx) = (1..last).find(|&idx| point_is_intrusion(path[idx])) else {
        return false;
    };
    let Some(clean_idx) = (0..first_intrusion_idx).rev().find(|&idx| {
        let point = path[idx];
        !point_is_intrusion(point) && point_is_clean_for_face(point)
    }) else {
        return false;
    };

    let clean = path[clean_idx];
    let endpoint = path[last];
    let elbow = match canonical_face {
        Face::Top | Face::Bottom => FPoint::new(endpoint.x, clean.y),
        Face::Left | Face::Right => FPoint::new(clean.x, endpoint.y),
    };

    path.truncate(clean_idx + 1);
    let tail = *path
        .last()
        .expect("truncated path should keep at least one clean point");
    if !points_match(tail, elbow) && !points_match(elbow, endpoint) {
        path.push(elbow);
    }
    let tail = *path
        .last()
        .expect("path should retain at least one point before terminal endpoint");
    if !points_match(tail, endpoint) {
        path.push(endpoint);
    }

    if path.len() > 5 && clean_idx > 2 {
        match canonical_face {
            Face::Top | Face::Bottom => {
                let lane_y = path[clean_idx].y;
                let stem_is_vertical =
                    (path[0].x - path[1].x).abs() <= EPS && (path[0].y - path[1].y).abs() > EPS;
                let run_is_horizontal = path[1..=clean_idx]
                    .iter()
                    .all(|point| (point.y - lane_y).abs() <= EPS);
                if stem_is_vertical && run_is_horizontal {
                    path.drain(2..clean_idx);
                }
            }
            Face::Left | Face::Right => {
                let lane_x = path[clean_idx].x;
                let stem_is_horizontal =
                    (path[0].y - path[1].y).abs() <= EPS && (path[0].x - path[1].x).abs() > EPS;
                let run_is_vertical = path[1..=clean_idx]
                    .iter()
                    .all(|point| (point.x - lane_x).abs() <= EPS);
                if stem_is_horizontal && run_is_vertical {
                    path.drain(2..clean_idx);
                }
            }
        }
    }

    let mut idx = 1usize;
    while idx < path.len() {
        if points_match(path[idx - 1], path[idx]) {
            path.remove(idx);
        } else {
            idx += 1;
        }
    }
    true
}

pub(super) fn enforce_backward_source_tangent_direction(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    preferred_source_face: Option<Face>,
) {
    const EPS: f64 = 0.000_001;
    const TANGENT_STEP: f64 = 8.0;
    if path.len() < 2 {
        return;
    }

    let Some((source_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return;
    };

    let canonical_face =
        preferred_source_face.unwrap_or_else(|| canonical_backward_channel_face(direction));
    let left = source_rect.x;
    let right = source_rect.x + source_rect.width;
    let top = source_rect.y;
    let bottom = source_rect.y + source_rect.height;

    let existing_support = if path.len() > 2 { Some(path[1]) } else { None };

    let mut start = path[0];
    match canonical_face {
        Face::Left => {
            start.x = left;
            start.y = clamp_face_coordinate_with_corner_inset(
                start.y,
                top,
                bottom,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
        }
        Face::Right => {
            start.x = right;
            start.y = clamp_face_coordinate_with_corner_inset(
                start.y,
                top,
                bottom,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
        }
        Face::Top => {
            start.x = clamp_face_coordinate_with_corner_inset(
                start.x,
                left,
                right,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            start.y = top;
        }
        Face::Bottom => {
            start.x = clamp_face_coordinate_with_corner_inset(
                start.x,
                left,
                right,
                MIN_PORT_CORNER_INSET_BACKWARD,
            );
            start.y = bottom;
        }
    }
    if matches!(canonical_face, Face::Left | Face::Right) {
        start = bias_face_coordinate_toward_center(
            start,
            source_rect,
            0.84,
            MIN_PORT_CORNER_INSET_BACKWARD,
        );
    }
    let mut support = match canonical_face {
        Face::Left => FPoint::new(start.x - TANGENT_STEP, start.y),
        Face::Right => FPoint::new(start.x + TANGENT_STEP, start.y),
        Face::Top => FPoint::new(start.x, start.y - TANGENT_STEP),
        Face::Bottom => FPoint::new(start.x, start.y + TANGENT_STEP),
    };

    if let Some(existing) = existing_support {
        match canonical_face {
            Face::Left => {
                if (existing.y - start.y).abs() <= EPS && existing.x < start.x - EPS {
                    support.x = support.x.min(existing.x);
                }
            }
            Face::Right => {
                if (existing.y - start.y).abs() <= EPS && existing.x > start.x + EPS {
                    support.x = support.x.max(existing.x);
                }
            }
            Face::Top => {
                if (existing.x - start.x).abs() <= EPS && existing.y < start.y - EPS {
                    support.y = support.y.min(existing.y);
                }
            }
            Face::Bottom => {
                if (existing.x - start.x).abs() <= EPS && existing.y > start.y + EPS {
                    support.y = support.y.max(existing.y);
                }
            }
        }
    }

    if path.len() >= 3 {
        let next = path[2];
        match canonical_face {
            Face::Left => {
                if next.x < support.x - EPS {
                    support.x = next.x;
                }
            }
            Face::Right => {
                if next.x > support.x + EPS {
                    support.x = next.x;
                }
            }
            Face::Top => {
                if next.y < support.y - EPS {
                    support.y = next.y;
                }
            }
            Face::Bottom => {
                if next.y > support.y + EPS {
                    support.y = next.y;
                }
            }
        }
    }

    if path.len() >= 4 {
        let next_next = path[3];
        match canonical_face {
            Face::Left => {
                if (next_next.y - start.y).abs() <= EPS && next_next.x < support.x - EPS {
                    support.x = next_next.x;
                }
            }
            Face::Right => {
                if (next_next.y - start.y).abs() <= EPS && next_next.x > support.x + EPS {
                    support.x = next_next.x;
                }
            }
            Face::Top => {
                if (next_next.x - start.x).abs() <= EPS && next_next.y < support.y - EPS {
                    support.y = next_next.y;
                }
            }
            Face::Bottom => {
                if (next_next.x - start.x).abs() <= EPS && next_next.y > support.y + EPS {
                    support.y = next_next.y;
                }
            }
        }
    }

    path[0] = start;
    if path.len() == 2 {
        path.insert(1, support);
    } else {
        path[1] = support;
    }

    if path.len() < 3 {
        return;
    }

    let support_idx = 1;
    let next_idx = 2;
    let support = path[support_idx];
    let next = path[next_idx];
    let support_is_axis_aligned =
        (support.x - next.x).abs() <= EPS || (support.y - next.y).abs() <= EPS;
    if support_is_axis_aligned {
        return;
    }

    let primary_elbow = FPoint::new(support.x, next.y);
    if !points_match(primary_elbow, support) && !points_match(primary_elbow, next) {
        path.insert(next_idx, primary_elbow);
        return;
    }

    let fallback_elbow = FPoint::new(next.x, support.y);
    if !points_match(fallback_elbow, support) && !points_match(fallback_elbow, next) {
        path.insert(next_idx, fallback_elbow);
    }
}

pub(super) struct BackwardFinalizeOptions<'a> {
    pub(super) target_overflowed: bool,
    pub(super) source_face_override: Option<Face>,
    pub(super) target_face_override: Option<Face>,
    pub(super) base_finalized: &'a [FPoint],
}

pub(super) fn finalize_backward_path(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    options: BackwardFinalizeOptions<'_>,
) {
    let BackwardFinalizeOptions {
        target_overflowed,
        source_face_override,
        target_face_override,
        base_finalized,
    } = options;
    let mut compact_short_backward = false;
    let use_channel_path = geometry.enhanced_backward_routing
        && has_backward_corridor_obstructions(edge, geometry, direction);

    if use_channel_path {
        if let Some(channel_path) =
            build_backward_orthogonal_channel_path(edge, geometry, direction)
        {
            *path = channel_path;
        }
    } else {
        let use_compact_side_lane =
            matches!(direction, Direction::LeftRight | Direction::RightLeft)
                && direction != geometry.direction;
        if use_compact_side_lane
            && let Some(compact_path) =
                build_short_backward_side_lane_path(edge, geometry, direction)
        {
            *path = compact_path;
            compact_short_backward = true;
        } else {
            enforce_backward_source_tangent_direction(
                path,
                edge,
                geometry,
                direction,
                source_face_override,
            );
            ensure_backward_outer_lane_clearance(path, direction, 12.0);
            align_backward_source_stem_to_outer_lane(path, edge, geometry, direction);
            enforce_backward_terminal_tangent_direction(
                path,
                edge,
                geometry,
                direction,
                target_overflowed,
                target_face_override,
            );
            let parity_override_active =
                source_face_override.is_some() || target_face_override.is_some();
            if parity_override_active {
                *path = normalize_orthogonal_route_contracts(path, direction);
            }
            if parity_override_active && forward::has_immediate_axial_turnback(path) {
                *path = base_finalized.to_vec();
                enforce_backward_source_tangent_direction(path, edge, geometry, direction, None);
                ensure_backward_outer_lane_clearance(path, direction, 12.0);
                align_backward_source_stem_to_outer_lane(path, edge, geometry, direction);
                enforce_backward_terminal_tangent_direction(
                    path,
                    edge,
                    geometry,
                    direction,
                    target_overflowed,
                    None,
                );
            }
            collapse_tiny_backward_terminal_staircase(path, direction, 8.0);
            align_backward_outer_lane_to_hint(
                path,
                edge.layout_path_hint.as_deref(),
                direction,
                edge,
                geometry,
            );
            collapse_tiny_backward_terminal_staircase(path, direction, 8.0);
            enforce_backward_minimum_channel_floor(path, edge, geometry, direction, 12.0);
            avoid_backward_td_bt_vertical_lane_node_intrusion(path, edge, geometry, direction);
            collapse_backward_terminal_node_intrusion(path, edge, geometry, direction);
        }
    }
    if !compact_short_backward {
        enforce_backward_terminal_corner_inset(path, edge, geometry);
    }
    collapse_collinear_interior_points(path);
    fix_backward_diagonal_node_collision(path, edge, geometry, direction);
}
