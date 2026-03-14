use super::constants::MIN_PORT_CORNER_INSET_FORWARD;
use super::endpoints::{
    RectFace, boundary_face_excluding_corners, boundary_face_including_corners,
    clamp_face_coordinate_with_corner_inset, clip_point_to_rect_face_with_inset,
    endpoint_rect_and_shape, project_endpoint_to_shape,
};
use super::path_utils::{collapse_collinear_interior_points, points_match};
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::FPoint;
use crate::graph::{Direction, Shape};

pub(super) fn avoid_forward_td_bt_primary_lane_node_intrusion(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    target_primary_channel_depth: Option<f64>,
) {
    const EPS: f64 = 0.000_001;
    const INTRUSION_MARGIN: f64 = -0.5;
    const NODE_CLEARANCE: f64 = 8.0;
    const MIN_SOURCE_STEM: f64 = 8.0;
    const MIN_TARGET_STEM: f64 = 16.0;

    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || path.len() != 4 {
        return;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];

    let first_vertical = (p0.x - p1.x).abs() <= EPS && (p0.y - p1.y).abs() > EPS;
    let middle_horizontal = (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS;
    let terminal_vertical = (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS;
    if !(first_vertical && middle_horizontal && terminal_vertical) {
        return;
    }

    let flow_sign = if p3.y >= p0.y { 1.0 } else { -1.0 };
    let mut candidate_lane_y = p1.y;
    let mut saw_primary_intrusion = false;

    for (node_id, node) in &geometry.nodes {
        if node_id == &edge.from || node_id == &edge.to {
            continue;
        }
        let rect = node.rect;
        let first_crosses = super::collision::axis_aligned_segment_crosses_rect_interior(
            p0,
            p1,
            rect,
            INTRUSION_MARGIN,
        );
        let middle_crosses = super::collision::axis_aligned_segment_crosses_rect_interior(
            p1,
            p2,
            rect,
            INTRUSION_MARGIN,
        );
        if !first_crosses && !middle_crosses {
            continue;
        }

        saw_primary_intrusion = true;
        if flow_sign > 0.0 {
            candidate_lane_y = candidate_lane_y.min(rect.y - NODE_CLEARANCE);
        } else {
            candidate_lane_y = candidate_lane_y.max(rect.y + rect.height + NODE_CLEARANCE);
        }
    }

    if saw_primary_intrusion {
        let min_lane = p0.y + flow_sign * MIN_SOURCE_STEM;
        let max_lane = p3.y - flow_sign * MIN_TARGET_STEM;
        let lane_y = if flow_sign > 0.0 {
            if max_lane < min_lane {
                return;
            }
            candidate_lane_y.clamp(min_lane, max_lane)
        } else {
            if min_lane < max_lane {
                return;
            }
            candidate_lane_y.clamp(max_lane, min_lane)
        };

        if (lane_y - p1.y).abs() > EPS {
            let new_p1 = FPoint::new(p1.x, lane_y);
            let new_p2 = FPoint::new(p2.x, lane_y);
            if (new_p1.y - p0.y).abs() > EPS
                && (new_p2.x - new_p1.x).abs() > EPS
                && (p3.y - new_p2.y).abs() > EPS
            {
                path[1] = new_p1;
                path[2] = new_p2;
            }
        }
    }

    if let Some(detoured) = reroute_forward_td_bt_terminal_intrusion_with_safe_vertical_corridor(
        path, edge, geometry, direction,
    ) {
        *path = detoured;
    }

    stagger_forward_td_bt_terminal_horizontal_support(path, target_primary_channel_depth);
    collapse_tiny_forward_td_bt_lateral_jog(path, edge, geometry, direction);
}

fn reroute_forward_td_bt_terminal_intrusion_with_safe_vertical_corridor(
    path: &[FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<Vec<FPoint>> {
    const MIN_TARGET_STEM: f64 = 16.0;
    const NODE_CLEARANCE: f64 = 8.0;
    const INTRUSION_MARGIN: f64 = -0.5;
    const EPS: f64 = 0.000_001;

    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || path.len() != 4 {
        return None;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];

    let first_vertical = (p0.x - p1.x).abs() <= EPS && (p0.y - p1.y).abs() > EPS;
    let middle_horizontal = (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS;
    let terminal_vertical = (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS;
    if !(first_vertical && middle_horizontal && terminal_vertical) {
        return None;
    }

    if !super::collision::segment_crosses_any_other_node_interior(
        edge,
        geometry,
        p2,
        p3,
        INTRUSION_MARGIN,
    ) {
        return None;
    }

    let flow_sign = if p3.y >= p0.y { 1.0 } else { -1.0 };
    let terminal_support_y = p3.y - flow_sign * MIN_TARGET_STEM;
    if (terminal_support_y - p0.y).abs() <= EPS {
        return None;
    }
    if flow_sign > 0.0 && terminal_support_y <= p0.y + EPS {
        return None;
    }
    if flow_sign < 0.0 && terminal_support_y >= p0.y - EPS {
        return None;
    }

    let y_min = p0.y.min(terminal_support_y);
    let y_max = p0.y.max(terminal_support_y);
    let mut candidates = vec![p0.x];
    for (node_id, node) in &geometry.nodes {
        if node_id == &edge.from || node_id == &edge.to {
            continue;
        }
        let rect = node.rect;
        if !super::path_utils::ranges_overlap(y_min, y_max, rect.y, rect.y + rect.height) {
            continue;
        }
        candidates.push(rect.x - NODE_CLEARANCE);
        candidates.push(rect.x + rect.width + NODE_CLEARANCE);
    }

    candidates.sort_by(|a, b| a.total_cmp(b));
    candidates.dedup_by(|a, b| (*a - *b).abs() <= 0.5);

    let mut best: Option<(f64, Vec<FPoint>)> = None;
    for corridor_x in candidates {
        if (corridor_x - p3.x).abs() < MIN_TARGET_STEM {
            continue;
        }

        let mut route: Vec<FPoint> = Vec::with_capacity(5);
        route.push(p0);
        if (corridor_x - p0.x).abs() > EPS {
            route.push(FPoint::new(corridor_x, p0.y));
        }
        route.push(FPoint::new(corridor_x, terminal_support_y));
        route.push(FPoint::new(p3.x, terminal_support_y));
        route.push(p3);

        let mut deduped: Vec<FPoint> = Vec::with_capacity(route.len());
        for point in route {
            if deduped
                .last()
                .is_none_or(|prev| !points_match(*prev, point))
            {
                deduped.push(point);
            }
        }

        if deduped.len() < 4 {
            continue;
        }

        let segments_clear = deduped.windows(2).all(|segment| {
            !super::collision::segment_crosses_any_other_node_interior(
                edge,
                geometry,
                segment[0],
                segment[1],
                INTRUSION_MARGIN,
            )
        });
        if !segments_clear {
            continue;
        }

        let score = (corridor_x - p0.x).abs();
        match &best {
            Some((best_score, _)) if score >= *best_score => {}
            _ => best = Some((score, deduped)),
        }
    }

    best.map(|(_, route)| route)
}

fn stagger_forward_td_bt_terminal_horizontal_support(
    path: &mut [FPoint],
    target_primary_channel_depth: Option<f64>,
) {
    const EPS: f64 = 0.000_001;
    const MIN_TARGET_STEM: f64 = 8.0;
    const MIN_SOURCE_STEM: f64 = 8.0;
    const MAX_TERMINAL_STAGGER: f64 = 24.0;
    const MIN_TOTAL_SPAN_FOR_STAGGER: f64 = 200.0;
    let Some(depth) = target_primary_channel_depth else {
        return;
    };
    if path.len() < 4 {
        return;
    }

    let n = path.len();
    let p0 = path[0];
    let p_prev = path[n - 4];
    let p_mid = path[n - 3];
    let p_support = path[n - 2];
    let p_end = path[n - 1];

    let pre_segment_vertical =
        (p_prev.x - p_mid.x).abs() <= EPS && (p_prev.y - p_mid.y).abs() > EPS;
    let support_segment_horizontal =
        (p_mid.y - p_support.y).abs() <= EPS && (p_mid.x - p_support.x).abs() > EPS;
    let tail_segment_vertical =
        (p_support.x - p_end.x).abs() <= EPS && (p_support.y - p_end.y).abs() > EPS;
    if !(pre_segment_vertical && support_segment_horizontal && tail_segment_vertical) {
        return;
    }

    let flow_sign = if p_end.y >= p0.y { 1.0 } else { -1.0 };
    if (p_end.y - p0.y).abs() < MIN_TOTAL_SPAN_FOR_STAGGER {
        return;
    }
    let source_anchor = p0.y + flow_sign * MIN_SOURCE_STEM;
    let target_anchor = p_end.y - flow_sign * MIN_TARGET_STEM;
    if (target_anchor - source_anchor).abs() <= EPS {
        return;
    }

    let desired = target_anchor - flow_sign * MAX_TERMINAL_STAGGER * (1.0 - depth.clamp(0.0, 1.0));
    let clamped = if flow_sign > 0.0 {
        desired.clamp(
            source_anchor.min(target_anchor),
            source_anchor.max(target_anchor),
        )
    } else {
        desired.clamp(
            target_anchor.min(source_anchor),
            target_anchor.max(source_anchor),
        )
    };

    if (clamped - p_support.y).abs() <= 1.0 {
        return;
    }

    path[n - 3].y = clamped;
    path[n - 2].y = clamped;
}

fn collapse_tiny_forward_td_bt_lateral_jog(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const EPS: f64 = 0.000_001;
    const MAX_TINY_JOG: f64 = 3.0;
    const INTRUSION_MARGIN: f64 = 1.0;

    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || path.len() != 4 {
        return;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];
    let first_vertical = (p0.x - p1.x).abs() <= EPS && (p0.y - p1.y).abs() > EPS;
    let middle_horizontal = (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS;
    let terminal_vertical = (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS;
    if !(first_vertical && middle_horizontal && terminal_vertical) {
        return;
    }

    let jog = (p2.x - p1.x).abs();
    if jog <= EPS || jog > MAX_TINY_JOG {
        return;
    }

    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return;
    };
    let Some(target_face) = boundary_face_excluding_corners(p3, target_rect, 0.5)
        .or_else(|| boundary_face_including_corners(p3, target_rect, 0.5))
    else {
        return;
    };
    if !matches!(target_face, RectFace::Top | RectFace::Bottom) {
        return;
    }

    let aligned_x = clamp_face_coordinate_with_corner_inset(
        p0.x,
        target_rect.x,
        target_rect.x + target_rect.width,
        MIN_PORT_CORNER_INSET_FORWARD,
    );
    let aligned_terminal = FPoint::new(aligned_x, p3.y);
    if super::collision::segment_crosses_any_other_node_interior(
        edge,
        geometry,
        p1,
        aligned_terminal,
        INTRUSION_MARGIN,
    ) {
        return;
    }

    path[2].x = aligned_x;
    path[3].x = aligned_x;
    collapse_collinear_interior_points(path);
}

pub(super) fn prefer_secondary_axis_departure_for_angular_sources(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const EPS: f64 = 0.000_001;
    const OFF_CENTER_MIN: f64 = 2.0;
    const MIN_SECONDARY_DEPARTURE: f64 = 2.0;
    const MIN_SECONDARY_DEPARTURE_DIAMOND: f64 = 0.1;
    const INTRUSION_MARGIN: f64 = 1.0;

    if path.len() != 4 {
        return;
    }

    let Some((source_rect, source_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return;
    };
    if !matches!(source_shape, Shape::Diamond | Shape::Hexagon) {
        return;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let (first_primary, middle_secondary, terminal_primary) = if primary_vertical {
        (
            (p0.x - p1.x).abs() <= EPS && (p0.y - p1.y).abs() > EPS,
            (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS,
            (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS,
        )
    } else {
        (
            (p0.y - p1.y).abs() <= EPS && (p0.x - p1.x).abs() > EPS,
            (p1.x - p2.x).abs() <= EPS && (p1.y - p2.y).abs() > EPS,
            (p2.y - p3.y).abs() <= EPS && (p2.x - p3.x).abs() > EPS,
        )
    };
    if !(first_primary && middle_secondary && terminal_primary) {
        return;
    }

    let source_cross_center = if primary_vertical {
        source_rect.x + source_rect.width / 2.0
    } else {
        source_rect.y + source_rect.height / 2.0
    };
    let start_offset = if primary_vertical {
        p0.x - source_cross_center
    } else {
        p0.y - source_cross_center
    };
    let target_offset = if primary_vertical {
        p3.x - source_cross_center
    } else {
        p3.y - source_cross_center
    };
    let allow_centered_diamond_departure = matches!(source_shape, Shape::Diamond);
    if (!allow_centered_diamond_departure && start_offset.abs() < OFF_CENTER_MIN)
        || target_offset.abs() < OFF_CENTER_MIN
    {
        return;
    }
    let secondary_delta = if primary_vertical {
        p3.x - p0.x
    } else {
        p3.y - p0.y
    };
    if secondary_delta.abs() < MIN_SECONDARY_DEPARTURE {
        return;
    }

    let flow_sign = match direction {
        Direction::TopDown | Direction::LeftRight => 1.0,
        Direction::BottomTop | Direction::RightLeft => -1.0,
    };
    let primary_delta = if primary_vertical {
        p3.y - p0.y
    } else {
        p3.x - p0.x
    };
    if primary_delta * flow_sign <= EPS {
        return;
    }

    let departure_face = if primary_vertical {
        if target_offset < 0.0 {
            RectFace::Left
        } else {
            RectFace::Right
        }
    } else if target_offset < 0.0 {
        RectFace::Top
    } else {
        RectFace::Bottom
    };
    let preferred_primary_lane = if primary_vertical { p1.y } else { p1.x };
    let rect_face_anchor = if primary_vertical {
        clip_point_to_rect_face_with_inset(
            FPoint::new(p0.x, preferred_primary_lane),
            source_rect,
            departure_face,
            MIN_PORT_CORNER_INSET_FORWARD,
        )
    } else {
        clip_point_to_rect_face_with_inset(
            FPoint::new(preferred_primary_lane, p0.y),
            source_rect,
            departure_face,
            MIN_PORT_CORNER_INSET_FORWARD,
        )
    };
    let provisional_elbow = if primary_vertical {
        FPoint::new(p3.x, preferred_primary_lane)
    } else {
        FPoint::new(preferred_primary_lane, p3.y)
    };
    let start = project_endpoint_to_shape(
        rect_face_anchor,
        provisional_elbow,
        source_rect,
        source_shape,
    );
    let elbow = if primary_vertical {
        FPoint::new(p3.x, start.y)
    } else {
        FPoint::new(start.x, p3.y)
    };
    if points_match(elbow, start) || points_match(elbow, p3) {
        return;
    }

    let secondary_departure = if primary_vertical {
        elbow.x - start.x
    } else {
        elbow.y - start.y
    };
    let min_secondary_departure = if matches!(source_shape, Shape::Diamond) {
        MIN_SECONDARY_DEPARTURE_DIAMOND
    } else {
        MIN_SECONDARY_DEPARTURE
    };
    if secondary_departure.abs() < min_secondary_departure {
        return;
    }
    let remaining_primary = if primary_vertical {
        p3.y - elbow.y
    } else {
        p3.x - elbow.x
    };
    if remaining_primary * flow_sign <= EPS {
        return;
    }

    let segments_clear = !super::collision::segment_crosses_any_other_node_interior(
        edge,
        geometry,
        start,
        elbow,
        INTRUSION_MARGIN,
    ) && !super::collision::segment_crosses_any_other_node_interior(
        edge,
        geometry,
        elbow,
        p3,
        INTRUSION_MARGIN,
    );
    if !segments_clear {
        return;
    }

    path.clear();
    path.push(start);
    path.push(elbow);
    path.push(p3);
    collapse_collinear_interior_points(path);
}

pub(super) fn ensure_primary_stem_for_flat_off_center_fanout_sources(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
) {
    const MIN_OFF_CENTER_ABS: f64 = 1.0;
    const MIN_PRIMARY_STEM: f64 = 8.0;
    const FANOUT_LANE_EPS: f64 = 1.0;
    const SEG_EPS: f64 = 0.000_001;

    if is_backward || path.len() < 3 {
        return;
    }

    let fanout_outbound: Vec<&LayoutEdge> = geometry
        .edges
        .iter()
        .filter(|candidate| candidate.from == edge.from)
        .collect();
    if fanout_outbound.len() < 2 {
        return;
    }
    if fanout_outbound
        .iter()
        .any(|candidate| geometry.reversed_edges.contains(&candidate.index))
    {
        return;
    }

    let Some((source_rect, source_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return;
    };
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let source_cross_center = if primary_vertical {
        source_rect.x + source_rect.width / 2.0
    } else {
        source_rect.y + source_rect.height / 2.0
    };
    let start = path[0];
    let first = path[1];
    let second = path[2];
    let source_offset = if primary_vertical {
        start.x - source_cross_center
    } else {
        start.y - source_cross_center
    };
    let angular_source = matches!(source_shape, Shape::Diamond | Shape::Hexagon);
    if source_offset.abs() < MIN_OFF_CENTER_ABS && !angular_source {
        return;
    }

    let (first_is_lateral, second_is_primary) = if primary_vertical {
        (
            (start.y - first.y).abs() <= SEG_EPS && (start.x - first.x).abs() > SEG_EPS,
            (first.x - second.x).abs() <= SEG_EPS && (first.y - second.y).abs() > SEG_EPS,
        )
    } else {
        (
            (start.x - first.x).abs() <= SEG_EPS && (start.y - first.y).abs() > SEG_EPS,
            (first.y - second.y).abs() <= SEG_EPS && (first.x - second.x).abs() > SEG_EPS,
        )
    };
    if !first_is_lateral || !second_is_primary {
        return;
    }

    let progresses_along_primary = match direction {
        Direction::TopDown => second.y > start.y + SEG_EPS,
        Direction::BottomTop => second.y < start.y - SEG_EPS,
        Direction::LeftRight => second.x > start.x + SEG_EPS,
        Direction::RightLeft => second.x < start.x - SEG_EPS,
    };
    if !progresses_along_primary {
        return;
    }

    let lateral_delta = if primary_vertical {
        first.x - start.x
    } else {
        first.y - start.y
    };
    if lateral_delta.abs() <= SEG_EPS {
        return;
    }
    if source_offset.abs() >= MIN_OFF_CENTER_ABS && lateral_delta.signum() != source_offset.signum()
    {
        return;
    }

    let mut outbound_target_primary_axis: Vec<f64> = Vec::with_capacity(fanout_outbound.len());
    for candidate in fanout_outbound {
        let Some((target_rect, _)) =
            endpoint_rect_and_shape(geometry, &candidate.to, candidate.to_subgraph.as_deref())
        else {
            return;
        };
        outbound_target_primary_axis.push(if primary_vertical {
            target_rect.y
        } else {
            target_rect.x
        });
    }
    let baseline_primary = outbound_target_primary_axis[0];
    if outbound_target_primary_axis
        .iter()
        .any(|primary| (primary - baseline_primary).abs() > FANOUT_LANE_EPS)
    {
        return;
    }

    let (stem, sweep) = match direction {
        Direction::TopDown => {
            let stem_y = start.y + MIN_PRIMARY_STEM;
            (FPoint::new(start.x, stem_y), FPoint::new(first.x, stem_y))
        }
        Direction::BottomTop => {
            let stem_y = start.y - MIN_PRIMARY_STEM;
            (FPoint::new(start.x, stem_y), FPoint::new(first.x, stem_y))
        }
        Direction::LeftRight => {
            let stem_x = start.x + MIN_PRIMARY_STEM;
            (FPoint::new(stem_x, start.y), FPoint::new(stem_x, first.y))
        }
        Direction::RightLeft => {
            let stem_x = start.x - MIN_PRIMARY_STEM;
            (FPoint::new(stem_x, start.y), FPoint::new(stem_x, first.y))
        }
    };
    if primary_vertical {
        if (stem.y - start.y).abs() <= SEG_EPS
            || (sweep.x - stem.x).abs() <= SEG_EPS
            || (second.y - sweep.y).abs() <= SEG_EPS
        {
            return;
        }
    } else if (stem.x - start.x).abs() <= SEG_EPS
        || (sweep.y - stem.y).abs() <= SEG_EPS
        || (second.x - sweep.x).abs() <= SEG_EPS
    {
        return;
    }

    let stem_stays_before_terminal_drop = match direction {
        Direction::TopDown => stem.y < second.y - SEG_EPS,
        Direction::BottomTop => stem.y > second.y + SEG_EPS,
        Direction::LeftRight => stem.x < second.x - SEG_EPS,
        Direction::RightLeft => stem.x > second.x + SEG_EPS,
    };
    if !stem_stays_before_terminal_drop {
        return;
    }

    path[1] = stem;
    path.insert(2, sweep);
}

pub(super) fn ensure_primary_stem_for_td_bt_angular_fanout_source(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
) {
    const SEG_EPS: f64 = 0.000_001;
    const MIN_PRIMARY_STEM: f64 = 8.0;
    const TERMINAL_CLEARANCE: f64 = 1.0;

    if is_backward
        || path.len() < 3
        || !matches!(direction, Direction::TopDown | Direction::BottomTop)
    {
        return;
    }

    let Some((_, source_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    else {
        return;
    };
    if !matches!(source_shape, Shape::Diamond | Shape::Hexagon) {
        return;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let first_is_horizontal = (p0.y - p1.y).abs() <= SEG_EPS && (p0.x - p1.x).abs() > SEG_EPS;
    let second_is_vertical = (p1.x - p2.x).abs() <= SEG_EPS && (p1.y - p2.y).abs() > SEG_EPS;
    if !first_is_horizontal || !second_is_vertical {
        return;
    }

    let flow_sign = match direction {
        Direction::TopDown => 1.0,
        Direction::BottomTop => -1.0,
        _ => 0.0,
    };
    if (p2.y - p0.y) * flow_sign <= SEG_EPS {
        return;
    }

    let desired_stem_y = p0.y + flow_sign * MIN_PRIMARY_STEM;
    let max_stem_y = p2.y - flow_sign * TERMINAL_CLEARANCE;
    let stem_y = if flow_sign > 0.0 {
        desired_stem_y.min(max_stem_y)
    } else {
        desired_stem_y.max(max_stem_y)
    };

    if (stem_y - p0.y).abs() <= SEG_EPS || (p2.y - stem_y) * flow_sign <= SEG_EPS {
        return;
    }

    path[1].y = stem_y;
    path.insert(1, FPoint::new(p0.x, stem_y));
}

pub(super) fn collapse_source_turnback_spikes(path: &mut Vec<FPoint>) {
    const SEG_EPS: f64 = 0.000_001;
    if path.len() < 4 {
        return;
    }

    let start = path[0];
    let step = path[1];
    let back = path[2];

    let out_is_axis = (start.x - step.x).abs() <= SEG_EPS || (start.y - step.y).abs() <= SEG_EPS;
    let back_is_axis = (step.x - back.x).abs() <= SEG_EPS || (step.y - back.y).abs() <= SEG_EPS;
    if !out_is_axis || !back_is_axis {
        return;
    }
    if points_match(start, back) {
        let resume = path[3];
        let collapsed_is_axis =
            (start.x - resume.x).abs() <= SEG_EPS || (start.y - resume.y).abs() <= SEG_EPS;
        if collapsed_is_axis && !points_match(start, resume) {
            path.drain(1..3);
        }
    }
}

pub(super) fn has_immediate_axial_turnback(path: &[FPoint]) -> bool {
    const EPS: f64 = 0.000_001;
    path.windows(3).any(|triple| {
        let a = triple[0];
        let b = triple[1];
        let c = triple[2];

        let first_vertical = (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() > EPS;
        let second_vertical = (b.x - c.x).abs() <= EPS && (b.y - c.y).abs() > EPS;
        if first_vertical && second_vertical {
            let dy1 = b.y - a.y;
            let dy2 = c.y - b.y;
            return dy1.abs() > EPS && dy2.abs() > EPS && dy1.signum() != dy2.signum();
        }

        let first_horizontal = (a.y - b.y).abs() <= EPS && (a.x - b.x).abs() > EPS;
        let second_horizontal = (b.y - c.y).abs() <= EPS && (b.x - c.x).abs() > EPS;
        if first_horizontal && second_horizontal {
            let dx1 = b.x - a.x;
            let dx2 = c.x - b.x;
            return dx1.abs() > EPS && dx2.abs() > EPS && dx1.signum() != dx2.signum();
        }

        false
    })
}

pub(super) fn has_forward_primary_axis_reversal(path: &[FPoint], direction: Direction) -> bool {
    const EPS: f64 = 0.000_001;
    path.windows(2).any(|segment| {
        let a = segment[0];
        let b = segment[1];
        match direction {
            Direction::TopDown => {
                (a.x - b.x).abs() <= EPS && (b.y - a.y) < -EPS && (a.y - b.y).abs() > EPS
            }
            Direction::BottomTop => {
                (a.x - b.x).abs() <= EPS && (b.y - a.y) > EPS && (a.y - b.y).abs() > EPS
            }
            Direction::LeftRight => {
                (a.y - b.y).abs() <= EPS && (b.x - a.x) < -EPS && (a.x - b.x).abs() > EPS
            }
            Direction::RightLeft => {
                (a.y - b.y).abs() <= EPS && (b.x - a.x) > EPS && (a.x - b.x).abs() > EPS
            }
        }
    })
}

pub(crate) fn collapse_forward_source_primary_turnback_hooks(
    path: &mut [FPoint],
    direction: Direction,
) -> bool {
    const EPS: f64 = 0.000_001;
    if path.len() < 5 {
        return false;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];
    let p3 = path[3];
    let p4 = path[4];

    let mut changed = false;
    match direction {
        Direction::LeftRight => {
            let first_primary = (p0.y - p1.y).abs() <= EPS && (p1.x - p0.x) > EPS;
            let first_secondary = (p1.x - p2.x).abs() <= EPS && (p1.y - p2.y).abs() > EPS;
            let hook_primary = (p2.y - p3.y).abs() <= EPS && (p2.x - p3.x).abs() > EPS;
            let second_secondary = (p3.x - p4.x).abs() <= EPS && (p3.y - p4.y).abs() > EPS;
            let has_hook = first_primary
                && first_secondary
                && hook_primary
                && second_secondary
                && (p3.x - p2.x) < -EPS;
            if has_hook {
                if path.len() > 5 {
                    path[3].x = p2.x;
                    path[4].x = p2.x;
                } else {
                    path[1].x = p3.x;
                    path[2].x = p3.x;
                }
                changed = true;
            }
        }
        Direction::RightLeft => {
            let first_primary = (p0.y - p1.y).abs() <= EPS && (p1.x - p0.x) < -EPS;
            let first_secondary = (p1.x - p2.x).abs() <= EPS && (p1.y - p2.y).abs() > EPS;
            let hook_primary = (p2.y - p3.y).abs() <= EPS && (p2.x - p3.x).abs() > EPS;
            let second_secondary = (p3.x - p4.x).abs() <= EPS && (p3.y - p4.y).abs() > EPS;
            let has_hook = first_primary
                && first_secondary
                && hook_primary
                && second_secondary
                && (p3.x - p2.x) > EPS;
            if has_hook {
                if path.len() > 5 {
                    path[3].x = p2.x;
                    path[4].x = p2.x;
                } else {
                    path[1].x = p3.x;
                    path[2].x = p3.x;
                }
                changed = true;
            }
        }
        Direction::TopDown => {
            let first_primary = (p0.x - p1.x).abs() <= EPS && (p1.y - p0.y) > EPS;
            let first_secondary = (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS;
            let hook_primary = (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS;
            let second_secondary = (p3.y - p4.y).abs() <= EPS && (p3.x - p4.x).abs() > EPS;
            let has_hook = first_primary
                && first_secondary
                && hook_primary
                && second_secondary
                && (p3.y - p2.y) < -EPS;
            if has_hook {
                if path.len() > 5 {
                    path[3].y = p2.y;
                    path[4].y = p2.y;
                } else {
                    path[1].y = p3.y;
                    path[2].y = p3.y;
                }
                changed = true;
            }
        }
        Direction::BottomTop => {
            let first_primary = (p0.x - p1.x).abs() <= EPS && (p1.y - p0.y) < -EPS;
            let first_secondary = (p1.y - p2.y).abs() <= EPS && (p1.x - p2.x).abs() > EPS;
            let hook_primary = (p2.x - p3.x).abs() <= EPS && (p2.y - p3.y).abs() > EPS;
            let second_secondary = (p3.y - p4.y).abs() <= EPS && (p3.x - p4.x).abs() > EPS;
            let has_hook = first_primary
                && first_secondary
                && hook_primary
                && second_secondary
                && (p3.y - p2.y) > EPS;
            if has_hook {
                if path.len() > 5 {
                    path[3].y = p2.y;
                    path[4].y = p2.y;
                } else {
                    path[1].y = p3.y;
                    path[2].y = p3.y;
                }
                changed = true;
            }
        }
    }

    changed
}
