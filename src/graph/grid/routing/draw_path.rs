use super::super::backward::is_backward_edge;
use super::super::bounds::{NodeContainingSubgraphMap, containing_subgraph_id};
use super::super::intersect::NodeFace;
use super::super::layout::{GridLayout, NodeBounds};
use super::attachment_resolution::{clamp_to_face, infer_face_from_attachment};
use super::orthogonal::ensure_source_face_launch_support;
use super::probe::TextPathRejection;
use super::route_variants::route_edge_with_waypoints;
use super::types::{EdgeEndpoints, RoutedEdge, RoutingOverrides, Segment};
use crate::graph::{Arrow, Direction, Edge};

pub(super) fn route_inter_subgraph_edge_via_outer_lane(
    edge: &Edge,
    layout: &GridLayout,
    ep: &EdgeEndpoints,
    draw_path: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'_>>,
) -> Option<RoutedEdge> {
    let from_subgraph = containing_subgraph_id(layout, &edge.from, node_containing_subgraph)?;
    let to_subgraph = containing_subgraph_id(layout, &edge.to, node_containing_subgraph)?;
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

pub(super) fn route_edge_from_draw_path(
    edge: &Edge,
    layout: &GridLayout,
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

pub(super) fn waypoints_from_draw_path(points: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut waypoints = Vec::new();
    for &(x, y) in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        if waypoints.last().copied() != Some((x, y)) {
            waypoints.push((x, y));
        }
    }
    waypoints
}

pub(super) fn normalize_draw_path_points(
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
    match face {
        NodeFace::Top => waypoint.1 <= top,
        NodeFace::Bottom => waypoint.1 >= bottom,
        NodeFace::Left => waypoint.0 <= left,
        NodeFace::Right => waypoint.0 >= right,
    }
}

fn segments_collide_with_other_nodes(
    segments: &[Segment],
    layout: &GridLayout,
    edge: &Edge,
) -> bool {
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

pub(super) fn repair_draw_path_segment_collisions(
    draw_path: &[(usize, usize)],
    layout: &GridLayout,
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
