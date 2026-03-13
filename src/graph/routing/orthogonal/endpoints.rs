use super::super::float_core::intersect_shape_boundary_float;
use super::constants::{MIN_PORT_CORNER_INSET_BACKWARD, MIN_PORT_CORNER_INSET_FORWARD, POINT_EPS};
use super::path_utils::points_match;
use crate::graph::attachment::{
    Face, canonical_backward_channel_face, resolve_overflow_backward_channel_conflict,
};
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Shape};

pub(crate) fn edge_endpoint_rects(
    geometry: &GraphGeometry,
    edge: &LayoutEdge,
) -> Option<(FRect, FRect)> {
    let source_rect = *endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref())?;
    let target_rect = *endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref())?;
    Some((source_rect, target_rect))
}

pub(crate) fn point_on_or_inside_rect(point: FPoint, rect: &FRect, eps: f64) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    point.x >= left - eps
        && point.x <= right + eps
        && point.y >= top - eps
        && point.y <= bottom + eps
}

pub(crate) fn endpoint_rect<'a>(
    geometry: &'a GraphGeometry,
    node_id: &str,
    subgraph_id: Option<&str>,
) -> Option<&'a FRect> {
    if let Some(sg_id) = subgraph_id {
        geometry.subgraphs.get(sg_id).map(|sg| &sg.rect)
    } else {
        geometry.nodes.get(node_id).map(|node| &node.rect)
    }
}

pub(crate) fn endpoint_rect_and_shape(
    geometry: &GraphGeometry,
    node_id: &str,
    subgraph_id: Option<&str>,
) -> Option<(FRect, Shape)> {
    if let Some(sg_id) = subgraph_id {
        return geometry
            .subgraphs
            .get(sg_id)
            .map(|sg| (sg.rect, Shape::Rectangle));
    }
    geometry
        .nodes
        .get(node_id)
        .map(|node| (node.rect, node.shape))
}

pub(crate) fn clamp_face_coordinate_with_corner_inset(
    value: f64,
    min: f64,
    max: f64,
    max_inset: f64,
) -> f64 {
    let lo = min.min(max);
    let hi = min.max(max);
    let span = hi - lo;
    if span <= POINT_EPS {
        (lo + hi) / 2.0
    } else {
        let inset = (span * 0.2).clamp(1.0, max_inset);
        if span <= inset * 2.0 {
            (lo + hi) / 2.0
        } else {
            value.clamp(lo + inset, hi - inset)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn anchor_path_endpoints_to_endpoint_faces(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
    overflow_policy_target_face: Option<Face>,
    overflow_policy_target_fraction: Option<f64>,
    source_primary_face_fraction: Option<f64>,
    target_overflowed: bool,
    target_has_backward_conflict: bool,
) {
    const EPS: f64 = 0.5;
    if path.len() < 2 {
        return;
    }

    if let Some((from_rect, from_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
    {
        let start = path[0];
        let next = path[1];
        if point_on_or_inside_rect(start, &from_rect, EPS) {
            let source_segment_on_flow_axis = match direction {
                Direction::TopDown | Direction::BottomTop => {
                    (start.x - next.x).abs() <= POINT_EPS && (start.y - next.y).abs() > POINT_EPS
                }
                Direction::LeftRight | Direction::RightLeft => {
                    (start.y - next.y).abs() <= POINT_EPS && (start.x - next.x).abs() > POINT_EPS
                }
            };
            let source_fraction_override_active = !is_backward
                && source_segment_on_flow_axis
                && source_primary_face_fraction.is_some();
            let clipped = if let Some(fraction) = source_primary_face_fraction
                && source_fraction_override_active
            {
                clip_point_to_rect_face_fraction_with_inset(
                    from_rect,
                    map_face_to_rect_face(flow_source_face_for_direction(direction)),
                    fraction,
                    MIN_PORT_CORNER_INSET_FORWARD,
                )
            } else {
                clip_point_to_axis_face(
                    start,
                    next,
                    from_rect,
                    direction,
                    is_backward,
                    false,
                    None,
                    None,
                    false,
                    false,
                )
            };
            let source_forward_outbound_count = geometry
                .edges
                .iter()
                .filter(|candidate| {
                    candidate.from == edge.from
                        && !geometry.reversed_edges.contains(&candidate.index)
                })
                .count();
            let source_slot_projection_preserves_ports = source_fraction_override_active
                && matches!(from_shape, Shape::Diamond | Shape::Hexagon)
                && (matches!(direction, Direction::TopDown | Direction::BottomTop)
                    || source_forward_outbound_count >= 3);
            let projection_approach = if source_slot_projection_preserves_ports {
                clipped
            } else {
                next
            };
            path[0] =
                project_endpoint_to_shape(clipped, projection_approach, from_rect, from_shape);
        }
    }

    if let Some((to_rect, to_shape)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    {
        let last = path.len() - 1;
        let end = path[last];
        let prev = path[last - 1];
        if point_on_or_inside_rect(end, &to_rect, EPS) {
            let clipped = clip_point_to_axis_face(
                end,
                prev,
                to_rect,
                direction,
                is_backward,
                true,
                overflow_policy_target_face,
                overflow_policy_target_fraction,
                target_overflowed,
                target_has_backward_conflict,
            );
            let target_fraction_override_active =
                !is_backward && overflow_policy_target_fraction.is_some();
            let projection_approach = if target_fraction_override_active
                && matches!(to_shape, Shape::Diamond | Shape::Hexagon)
            {
                clipped
            } else {
                prev
            };
            path[last] = project_endpoint_to_shape(clipped, projection_approach, to_rect, to_shape);
        }
    }
}

pub(crate) fn offset_backward_source_from_primary_face(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) {
    const FACE_EPS: f64 = 2.0;

    if path.len() < 2 {
        return;
    }

    let sr = if let Some(sg_id) = edge.from_subgraph.as_deref() {
        if let Some(sg) = geometry.subgraphs.get(sg_id) {
            sg.rect
        } else {
            return;
        }
    } else if let Some(node) = geometry.nodes.get(&edge.from) {
        node.rect
    } else {
        return;
    };
    let start = path[0];

    match direction {
        Direction::TopDown => {
            if (start.y - sr.y).abs() <= FACE_EPS {
                let offset = (sr.width / 4.0).clamp(8.0, 20.0);
                let new_x = (sr.center_x() + offset).min(sr.x + sr.width - FACE_EPS);
                path[0].x = new_x;
                if path.len() >= 2 && (path[1].x - start.x).abs() <= FACE_EPS {
                    path[1].x = new_x;
                }
            }
        }
        Direction::BottomTop => {
            if (start.y - (sr.y + sr.height)).abs() <= FACE_EPS {
                let offset = (sr.width / 4.0).clamp(8.0, 20.0);
                let new_x = (sr.center_x() + offset).min(sr.x + sr.width - FACE_EPS);
                path[0].x = new_x;
                if path.len() >= 2 && (path[1].x - start.x).abs() <= FACE_EPS {
                    path[1].x = new_x;
                }
            }
        }
        Direction::LeftRight => {
            if (start.x - sr.x).abs() <= FACE_EPS {
                let offset = (sr.height / 4.0).clamp(8.0, 20.0);
                let new_y = (sr.center_y() + offset).min(sr.y + sr.height - FACE_EPS);
                path[0].y = new_y;
                if path.len() >= 2 && (path[1].y - start.y).abs() <= FACE_EPS {
                    path[1].y = new_y;
                }
            }
        }
        Direction::RightLeft => {
            if (start.x - (sr.x + sr.width)).abs() <= FACE_EPS {
                let offset = (sr.height / 4.0).clamp(8.0, 20.0);
                let new_y = (sr.center_y() + offset).min(sr.y + sr.height - FACE_EPS);
                path[0].y = new_y;
                if path.len() >= 2 && (path[1].y - start.y).abs() <= FACE_EPS {
                    path[1].y = new_y;
                }
            }
        }
    }
}

pub(crate) fn snap_backward_endpoints_to_shape(
    path: &mut [FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) {
    if path.len() < 2 {
        return;
    }

    if let Some((from_rect, from_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
        && matches!(from_shape, Shape::Diamond | Shape::Hexagon)
    {
        let approach = path[1];
        path[0] = intersect_shape_boundary_float(from_rect, from_shape, approach);
    }

    let last = path.len() - 1;
    if let Some((to_rect, to_shape)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
        && matches!(to_shape, Shape::Diamond | Shape::Hexagon)
    {
        let approach = path[last - 1];
        let boundary = intersect_shape_boundary_float(to_rect, to_shape, approach);
        let dx = approach.x - boundary.x;
        let dy = approach.y - boundary.y;
        let dist = (dx * dx + dy * dy).sqrt();
        path[last] = if dist > f64::EPSILON {
            let margin = 4.0;
            let scale = margin / dist;
            FPoint::new(boundary.x + dx * scale, boundary.y + dy * scale)
        } else {
            boundary
        };
    }
}

pub(crate) fn project_endpoint_to_shape(
    rect_clipped: FPoint,
    approach: FPoint,
    rect: FRect,
    shape: Shape,
) -> FPoint {
    match shape {
        Shape::Diamond | Shape::Hexagon => intersect_shape_boundary_float(rect, shape, approach),
        _ => rect_clipped,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RectFace {
    Top,
    Bottom,
    Left,
    Right,
}

pub(crate) fn boundary_face_excluding_corners(
    point: FPoint,
    rect: FRect,
    eps: f64,
) -> Option<RectFace> {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    let on_left = (point.x - left).abs() <= eps;
    let on_right = (point.x - right).abs() <= eps;
    let on_top = (point.y - top).abs() <= eps;
    let on_bottom = (point.y - bottom).abs() <= eps;

    let within_x = point.x > left + eps && point.x < right - eps;
    let within_y = point.y > top + eps && point.y < bottom - eps;

    if on_left && within_y {
        Some(RectFace::Left)
    } else if on_right && within_y {
        Some(RectFace::Right)
    } else if on_top && within_x {
        Some(RectFace::Top)
    } else if on_bottom && within_x {
        Some(RectFace::Bottom)
    } else {
        None
    }
}

pub(crate) fn hint_face_for_td_bt_parity(point: FPoint, rect: FRect) -> Option<Face> {
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

    let horizontal_dist = dist_left.min(dist_right);
    let vertical_dist = dist_top.min(dist_bottom);

    if vertical_dist <= FACE_EPS && vertical_dist + CORNER_BIAS < horizontal_dist {
        return if dist_top <= dist_bottom {
            Some(Face::Top)
        } else {
            Some(Face::Bottom)
        };
    }
    if horizontal_dist <= FACE_EPS && horizontal_dist + CORNER_BIAS < vertical_dist {
        return if dist_left <= dist_right {
            Some(Face::Left)
        } else {
            Some(Face::Right)
        };
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn clip_point_to_axis_face(
    endpoint: FPoint,
    adjacent: FPoint,
    rect: FRect,
    direction: Direction,
    preserve_existing_face: bool,
    is_target_endpoint: bool,
    overflow_policy_face: Option<Face>,
    overflow_policy_fraction: Option<f64>,
    target_overflowed: bool,
    target_has_backward_conflict: bool,
) -> FPoint {
    const EPS: f64 = 0.000_001;
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    let x_min = left.min(right);
    let x_max = left.max(right);
    let y_min = top.min(bottom);
    let y_max = top.max(bottom);

    let dx = endpoint.x - adjacent.x;
    let dy = endpoint.y - adjacent.y;
    let max_corner_inset = if preserve_existing_face {
        MIN_PORT_CORNER_INSET_BACKWARD
    } else {
        MIN_PORT_CORNER_INSET_FORWARD
    };

    if preserve_existing_face && is_target_endpoint {
        let canonical_face = canonical_backward_channel_face(direction);
        let resolved_face = resolve_overflow_backward_channel_conflict(
            direction,
            true,
            target_has_backward_conflict,
            overflow_policy_face,
            canonical_face,
        );
        let mut clipped = clip_point_to_rect_face_with_inset(
            endpoint,
            rect,
            map_face_to_rect_face(resolved_face),
            max_corner_inset,
        );
        if matches!(resolved_face, Face::Bottom) {
            match direction {
                Direction::LeftRight => {
                    clipped.x = clamp_face_coordinate_with_corner_inset(
                        adjacent.x - 1.0,
                        rect.x,
                        rect.x + rect.width,
                        max_corner_inset,
                    );
                }
                Direction::RightLeft => {
                    clipped.x = clamp_face_coordinate_with_corner_inset(
                        adjacent.x + 1.0,
                        rect.x,
                        rect.x + rect.width,
                        max_corner_inset,
                    );
                }
                _ => {}
            }
        }
        return clipped;
    }

    if let Some(policy_face) = overflow_policy_face {
        let terminal_is_horizontal = dy.abs() <= EPS && dx.abs() > EPS;
        let terminal_is_vertical = dx.abs() <= EPS && dy.abs() > EPS;
        let policy_face_is_compatible = match policy_face {
            Face::Left | Face::Right => terminal_is_horizontal,
            Face::Top | Face::Bottom => terminal_is_vertical,
        };
        let resolved_face = resolve_overflow_backward_channel_conflict(
            direction,
            false,
            target_has_backward_conflict,
            Some(policy_face),
            policy_face,
        );

        if policy_face_is_compatible || !preserve_existing_face {
            let resolved_rect_face = map_face_to_rect_face(resolved_face);
            return clip_point_to_rect_face_fraction_with_inset(
                rect,
                resolved_rect_face,
                overflow_policy_fraction.unwrap_or(0.5),
                max_corner_inset,
            );
        }

        let fallback_face = if terminal_is_horizontal || dx.abs() >= dy.abs() {
            if adjacent.x < endpoint.x {
                Face::Left
            } else {
                Face::Right
            }
        } else if adjacent.y < endpoint.y {
            Face::Top
        } else {
            Face::Bottom
        };
        let resolved_fallback = resolve_overflow_backward_channel_conflict(
            direction,
            false,
            target_has_backward_conflict,
            Some(policy_face),
            fallback_face,
        );
        return clip_point_to_rect_face_with_inset(
            endpoint,
            rect,
            map_face_to_rect_face(resolved_fallback),
            max_corner_inset,
        );
    }

    if preserve_existing_face && is_target_endpoint && target_overflowed {
        let canonical = map_face_to_rect_face(canonical_backward_channel_face(direction));
        if let Some(actual_face) = boundary_face_excluding_corners(endpoint, rect, 0.5)
            && actual_face == canonical
        {
            return clip_point_to_rect_face_with_inset(
                endpoint,
                rect,
                opposite_rect_face(canonical),
                max_corner_inset,
            );
        }
    }

    if preserve_existing_face && matches!(direction, Direction::TopDown | Direction::BottomTop) {
        let canonical_side = map_face_to_rect_face(canonical_backward_channel_face(direction));
        if let Some(face) = boundary_face_excluding_corners(endpoint, rect, 0.5)
            && face == canonical_side
        {
            return clip_point_to_rect_face_with_inset(
                endpoint,
                rect,
                canonical_side,
                max_corner_inset,
            );
        }

        let dist_to_canonical = match canonical_side {
            RectFace::Right => (endpoint.x - right).abs(),
            RectFace::Left => (endpoint.x - left).abs(),
            _ => f64::INFINITY,
        };
        let side_bias_threshold = (rect.width * 0.2).clamp(1.0, 6.0);
        if dist_to_canonical <= side_bias_threshold {
            let mut y = endpoint.y.clamp(y_min, y_max);
            if (y - top).abs() <= EPS || (y - bottom).abs() <= EPS {
                y = (top + bottom) / 2.0;
            }
            let x = match canonical_side {
                RectFace::Right => right,
                RectFace::Left => left,
                _ => endpoint.x,
            };
            return clip_point_to_rect_face_with_inset(
                FPoint::new(x, y),
                rect,
                canonical_side,
                max_corner_inset,
            );
        }
    }

    if dy.abs() <= EPS && dx.abs() > EPS {
        let face = if adjacent.x < endpoint.x {
            RectFace::Left
        } else {
            RectFace::Right
        };
        return clip_point_to_rect_face_with_inset(endpoint, rect, face, max_corner_inset);
    }

    if dx.abs() <= EPS && dy.abs() > EPS {
        let face = if adjacent.y < endpoint.y {
            RectFace::Top
        } else {
            RectFace::Bottom
        };
        return clip_point_to_rect_face_with_inset(endpoint, rect, face, max_corner_inset);
    }

    FPoint::new(
        endpoint.x.clamp(x_min, x_max),
        endpoint.y.clamp(y_min, y_max),
    )
}

pub(crate) fn enforce_backward_terminal_corner_inset(
    path: &mut Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) {
    const EPS: f64 = 0.000_001;
    if path.len() < 2 {
        return;
    }
    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return;
    };

    let last = path.len() - 1;
    let end = path[last];
    let prev = path[last - 1];
    let dx = (end.x - prev.x).abs();
    let dy = (end.y - prev.y).abs();
    let face = if dx <= EPS && dy > EPS {
        let dist_top = (end.y - target_rect.y).abs();
        let dist_bottom = (end.y - (target_rect.y + target_rect.height)).abs();
        if dist_top <= dist_bottom {
            RectFace::Top
        } else {
            RectFace::Bottom
        }
    } else if dy <= EPS && dx > EPS {
        let dist_left = (end.x - target_rect.x).abs();
        let dist_right = (end.x - (target_rect.x + target_rect.width)).abs();
        if dist_left <= dist_right {
            RectFace::Left
        } else {
            RectFace::Right
        }
    } else if let Some(boundary_face) = boundary_face_excluding_corners(end, target_rect, 0.5)
        .or_else(|| boundary_face_including_corners(end, target_rect, 0.5))
    {
        boundary_face
    } else {
        return;
    };

    let clipped =
        clip_point_to_rect_face_with_inset(end, target_rect, face, MIN_PORT_CORNER_INSET_BACKWARD);
    if !points_match(clipped, end) {
        path[last] = clipped;
        ensure_endpoint_axis_aligned(path, false);
    }
}

pub(crate) fn map_face_to_rect_face(face: Face) -> RectFace {
    match face {
        Face::Top => RectFace::Top,
        Face::Bottom => RectFace::Bottom,
        Face::Left => RectFace::Left,
        Face::Right => RectFace::Right,
    }
}

fn opposite_rect_face(face: RectFace) -> RectFace {
    match face {
        RectFace::Top => RectFace::Bottom,
        RectFace::Bottom => RectFace::Top,
        RectFace::Left => RectFace::Right,
        RectFace::Right => RectFace::Left,
    }
}

pub(crate) fn clip_point_to_rect_face_with_inset(
    endpoint: FPoint,
    rect: FRect,
    face: RectFace,
    max_corner_inset: f64,
) -> FPoint {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    match face {
        RectFace::Top => FPoint::new(
            clamp_face_coordinate_with_corner_inset(endpoint.x, left, right, max_corner_inset),
            top,
        ),
        RectFace::Bottom => FPoint::new(
            clamp_face_coordinate_with_corner_inset(endpoint.x, left, right, max_corner_inset),
            bottom,
        ),
        RectFace::Left => FPoint::new(
            left,
            clamp_face_coordinate_with_corner_inset(endpoint.y, top, bottom, max_corner_inset),
        ),
        RectFace::Right => FPoint::new(
            right,
            clamp_face_coordinate_with_corner_inset(endpoint.y, top, bottom, max_corner_inset),
        ),
    }
}

fn clip_point_to_rect_face_fraction_with_inset(
    rect: FRect,
    face: RectFace,
    fraction: f64,
    max_corner_inset: f64,
) -> FPoint {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    let fraction = fraction.clamp(0.0, 1.0);

    match face {
        RectFace::Top => FPoint::new(
            clamp_face_coordinate_with_corner_inset(
                left + rect.width * fraction,
                left,
                right,
                max_corner_inset,
            ),
            top,
        ),
        RectFace::Bottom => FPoint::new(
            clamp_face_coordinate_with_corner_inset(
                left + rect.width * fraction,
                left,
                right,
                max_corner_inset,
            ),
            bottom,
        ),
        RectFace::Left => FPoint::new(
            left,
            clamp_face_coordinate_with_corner_inset(
                top + rect.height * fraction,
                top,
                bottom,
                max_corner_inset,
            ),
        ),
        RectFace::Right => FPoint::new(
            right,
            clamp_face_coordinate_with_corner_inset(
                top + rect.height * fraction,
                top,
                bottom,
                max_corner_inset,
            ),
        ),
    }
}

pub(crate) fn boundary_face_including_corners(
    point: FPoint,
    rect: FRect,
    eps: f64,
) -> Option<RectFace> {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    let dist_left = (point.x - left).abs();
    let dist_right = (point.x - right).abs();
    let dist_top = (point.y - top).abs();
    let dist_bottom = (point.y - bottom).abs();
    let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);
    if min_dist > eps {
        return None;
    }
    if (min_dist - dist_left).abs() <= eps {
        return Some(RectFace::Left);
    }
    if (min_dist - dist_right).abs() <= eps {
        return Some(RectFace::Right);
    }
    if (min_dist - dist_top).abs() <= eps {
        return Some(RectFace::Top);
    }
    Some(RectFace::Bottom)
}

pub(crate) fn bias_face_coordinate_toward_center(
    point: FPoint,
    rect: FRect,
    preserve_factor: f64,
    max_corner_inset: f64,
) -> FPoint {
    let factor = preserve_factor.clamp(0.0, 1.0);
    let center_x = rect.x + rect.width / 2.0;
    let center_y = rect.y + rect.height / 2.0;
    let face = boundary_face_excluding_corners(point, rect, 0.5)
        .or_else(|| boundary_face_including_corners(point, rect, 0.5));

    let biased = match face {
        Some(RectFace::Top) | Some(RectFace::Bottom) => {
            FPoint::new(center_x + (point.x - center_x) * factor, point.y)
        }
        Some(RectFace::Left) | Some(RectFace::Right) => {
            FPoint::new(point.x, center_y + (point.y - center_y) * factor)
        }
        None => point,
    };

    match face {
        Some(face) => clip_point_to_rect_face_with_inset(biased, rect, face, max_corner_inset),
        None => biased,
    }
}

pub(crate) fn ensure_endpoint_segments_axis_aligned(path: &mut Vec<FPoint>) {
    if path.len() < 2 {
        return;
    }

    ensure_endpoint_axis_aligned(path, true);
    ensure_endpoint_axis_aligned(path, false);
}

fn ensure_endpoint_axis_aligned(path: &mut Vec<FPoint>, at_start: bool) {
    const EPS: f64 = 0.000_001;
    if path.len() < 2 {
        return;
    }

    let (anchor_idx, adjacent_idx) = if at_start {
        (0usize, 1usize)
    } else {
        let n = path.len();
        (n - 1, n - 2)
    };

    let anchor = path[anchor_idx];
    let adjacent = path[adjacent_idx];
    if (anchor.x - adjacent.x).abs() <= EPS || (anchor.y - adjacent.y).abs() <= EPS {
        return;
    }

    let mut elbow = FPoint::new(anchor.x, adjacent.y);
    let mut use_fallback = points_match(elbow, anchor) || points_match(elbow, adjacent);
    if use_fallback {
        elbow = FPoint::new(adjacent.x, anchor.y);
        use_fallback = points_match(elbow, anchor) || points_match(elbow, adjacent);
    }
    if use_fallback {
        return;
    }

    if at_start {
        path.insert(1, elbow);
    } else {
        let insert_at = path.len() - 1;
        path.insert(insert_at, elbow);
    }
}

pub(crate) fn flow_target_face_for_direction(direction: Direction) -> Face {
    match direction {
        Direction::TopDown => Face::Top,
        Direction::BottomTop => Face::Bottom,
        Direction::LeftRight => Face::Left,
        Direction::RightLeft => Face::Right,
    }
}

fn flow_source_face_for_direction(direction: Direction) -> Face {
    match direction {
        Direction::TopDown => Face::Bottom,
        Direction::BottomTop => Face::Top,
        Direction::LeftRight => Face::Right,
        Direction::RightLeft => Face::Left,
    }
}

pub(crate) fn endpoint_is_on_policy_face(
    path: &[FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    face: RectFace,
) -> bool {
    let Some(end) = path.last().copied() else {
        return false;
    };
    let Some((target_rect, _)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
    else {
        return false;
    };

    boundary_face_excluding_corners(end, target_rect, 0.5)
        .or_else(|| boundary_face_including_corners(end, target_rect, 0.5))
        == Some(face)
}

pub(crate) fn enforce_terminal_support_normal_to_face(
    path: &mut Vec<FPoint>,
    face: Face,
    min_support: f64,
) {
    if path.len() < 2 || min_support <= 0.0 {
        return;
    }

    let last = path.len() - 1;
    let end = path[last];
    let support = match face {
        Face::Top => FPoint::new(end.x, end.y - min_support),
        Face::Bottom => FPoint::new(end.x, end.y + min_support),
        Face::Left => FPoint::new(end.x - min_support, end.y),
        Face::Right => FPoint::new(end.x + min_support, end.y),
    };

    if path.len() == 2 {
        if !points_match(path[0], support) && !points_match(support, end) {
            path.insert(1, support);
        }
        return;
    }

    let penult_idx = last - 1;
    path[penult_idx] = support;

    if penult_idx == 0 {
        return;
    }

    let pre_idx = penult_idx - 1;
    let pre = path[pre_idx];
    let penult = path[penult_idx];
    let pre_to_penult_axis =
        (pre.x - penult.x).abs() <= POINT_EPS || (pre.y - penult.y).abs() <= POINT_EPS;
    if pre_to_penult_axis {
        return;
    }

    let elbow_primary = FPoint::new(pre.x, penult.y);
    if !points_match(elbow_primary, pre) && !points_match(elbow_primary, penult) {
        path.insert(penult_idx, elbow_primary);
        return;
    }
    let elbow_fallback = FPoint::new(penult.x, pre.y);
    if !points_match(elbow_fallback, pre) && !points_match(elbow_fallback, penult) {
        path.insert(penult_idx, elbow_fallback);
    }
}
