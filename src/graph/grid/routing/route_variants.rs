use super::super::backward::is_backward_edge;
use super::super::bounds::subgraph_edge_face;
use super::super::intersect::{NodeFace, calculate_attachment_points, classify_face};
use super::attachment_resolution::{
    clamp_to_boundary, clamp_to_face, edge_faces, infer_face_from_attachment, offset_for_face,
    resolve_attachment_points,
};
use super::orthogonal::{
    add_connector_segment, build_orthogonal_path_for_direction,
    build_orthogonal_path_with_waypoints, ensure_terminal_face_support,
    entry_direction_from_segments,
};
use super::types::{
    AttachDirection, EdgeEndpoints, Point, RoutedEdge, RoutingOverrides, Segment, build_routed_edge,
};
use crate::graph::{Arrow, Direction, Edge};

/// Route an edge using waypoints from normalization.
pub(super) fn route_edge_with_waypoints(
    edge: &Edge,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Option<RoutedEdge> {
    let (src_attach_raw, tgt_attach_raw) = resolve_attachment_points(
        overrides.src_attach,
        overrides.tgt_attach,
        ep,
        waypoints,
        direction,
    );

    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|v| v == "1") {
        eprintln!(
            "[route] {} -> {}: waypoints={:?}",
            edge.from, edge.to, waypoints
        );
    }

    let is_backward = is_backward_edge(&ep.from_bounds, &ep.to_bounds, direction);
    let (src_face, tgt_face) = if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        (
            classify_face(&ep.from_bounds, src_attach, ep.from_shape),
            classify_face(&ep.to_bounds, tgt_attach, ep.to_shape),
        )
    } else {
        let (default_src_face, default_tgt_face) = edge_faces(direction, is_backward);
        (
            overrides.src_face.unwrap_or_else(|| {
                infer_face_from_attachment(&ep.from_bounds, src_attach, default_src_face)
            }),
            overrides.tgt_face.unwrap_or_else(|| {
                infer_face_from_attachment(&ep.to_bounds, tgt_attach, default_tgt_face)
            }),
        )
    };
    let mut start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);

    if let Some(&(wp_x, wp_y)) = waypoints.first() {
        let should_skip_offset = match src_face {
            NodeFace::Top => wp_y >= src_attach.1,
            NodeFace::Bottom => wp_y <= src_attach.1,
            NodeFace::Left => wp_x >= src_attach.0,
            NodeFace::Right => wp_x <= src_attach.0,
        };
        if should_skip_offset {
            start = Point::new(src_attach.0, src_attach.1);
        }
    }

    let mut segments = Vec::new();
    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    let vertical_first = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let trivial_waypoints = !is_backward
        && ((vertical_first
            && start.x == end.x
            && waypoints.iter().all(|(x, _)| x.abs_diff(start.x) <= 1))
            || (!vertical_first
                && start.y == end.y
                && waypoints.iter().all(|(_, y)| y.abs_diff(start.y) <= 1)));

    if trivial_waypoints {
        let mut current = start;
        if vertical_first {
            for &(_, wp_y) in waypoints {
                if wp_y != current.y {
                    segments.push(Segment::Vertical {
                        x: start.x,
                        y_start: current.y,
                        y_end: wp_y,
                    });
                    current = Point::new(start.x, wp_y);
                }
            }
            if current.y != end.y {
                segments.push(Segment::Vertical {
                    x: start.x,
                    y_start: current.y,
                    y_end: end.y,
                });
            }
        } else {
            for &(wp_x, _) in waypoints {
                if wp_x != current.x {
                    segments.push(Segment::Horizontal {
                        y: start.y,
                        x_start: current.x,
                        x_end: wp_x,
                    });
                    current = Point::new(wp_x, start.y);
                }
            }
            if current.x != end.x {
                segments.push(Segment::Horizontal {
                    y: start.y,
                    x_start: current.x,
                    x_end: end.x,
                });
            }
        }
    } else {
        segments.extend(build_orthogonal_path_with_waypoints(
            start,
            waypoints,
            end,
            direction,
            overrides.src_first_vertical,
            edge.arrow_start != Arrow::None,
        ));
    }

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);
    let entry_direction = entry_direction_from_segments(&segments);

    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|v| v == "1") {
        eprintln!(
            "[route] {} -> {}: start={:?} end={:?} segments={:?}",
            edge.from, edge.to, start, end, segments
        );
    }

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        is_backward,
    ))
}

pub(super) fn route_backward_with_synthetic_waypoints(
    edge: &Edge,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
    overrides: RoutingOverrides,
) -> Option<RoutedEdge> {
    let (src_attach_raw, tgt_attach_raw) = calculate_attachment_points(
        &ep.from_bounds,
        ep.from_shape,
        &ep.to_bounds,
        ep.to_shape,
        waypoints,
    );

    let src_attach_raw = overrides.src_attach.unwrap_or(src_attach_raw);
    let tgt_attach_raw = overrides.tgt_attach.unwrap_or(tgt_attach_raw);

    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    let src_face = classify_face(&ep.from_bounds, waypoints[0], ep.from_shape);
    let tgt_face = classify_face(&ep.to_bounds, *waypoints.last().unwrap(), ep.to_shape);

    let start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);

    let mut segments = Vec::new();
    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    segments.extend(build_orthogonal_path_with_waypoints(
        start,
        waypoints,
        end,
        direction,
        overrides.src_first_vertical,
        edge.arrow_start != Arrow::None,
    ));

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);
    let entry_direction = entry_direction_from_segments(&segments);

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        true,
    ))
}

pub(super) fn route_edge_direct(
    edge: &Edge,
    ep: &EdgeEndpoints,
    direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RoutedEdge> {
    let empty_waypoints: &[(usize, usize)] = &[];
    let (mut src_attach_raw, mut tgt_attach_raw) = resolve_attachment_points(
        src_attach_override,
        tgt_attach_override,
        ep,
        empty_waypoints,
        direction,
    );
    let mut src_face_override = None;
    let mut tgt_face_override = None;
    if edge.from_subgraph.is_some() && src_attach_override.is_none() {
        let face = subgraph_edge_face(&ep.from_bounds, &ep.to_bounds, direction);
        src_face_override = Some(face);
        src_attach_raw = clamp_to_face(
            &ep.from_bounds,
            face,
            (ep.to_bounds.center_x(), ep.to_bounds.center_y()),
        );
    }
    if edge.to_subgraph.is_some() && tgt_attach_override.is_none() {
        let face = subgraph_edge_face(&ep.to_bounds, &ep.from_bounds, direction);
        tgt_face_override = Some(face);
        tgt_attach_raw = clamp_to_face(
            &ep.to_bounds,
            face,
            (ep.from_bounds.center_x(), ep.from_bounds.center_y()),
        );
    }

    let src_attach_point = clamp_to_boundary(src_attach_raw, &ep.from_bounds);
    let tgt_attach_point = clamp_to_boundary(tgt_attach_raw, &ep.to_bounds);
    let src_attach = (src_attach_point.x, src_attach_point.y);
    let tgt_attach = (tgt_attach_point.x, tgt_attach_point.y);

    let is_backward = is_backward_edge(&ep.from_bounds, &ep.to_bounds, direction);
    let (src_face, tgt_face) = if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        (
            src_face_override
                .unwrap_or_else(|| classify_face(&ep.from_bounds, src_attach, ep.from_shape)),
            tgt_face_override
                .unwrap_or_else(|| classify_face(&ep.to_bounds, tgt_attach, ep.to_shape)),
        )
    } else {
        edge_faces(direction, is_backward)
    };
    let start = offset_for_face(src_attach, src_face);
    let end = offset_for_face(tgt_attach, tgt_face);
    let mut segments = Vec::new();

    if src_attach != (start.x, start.y) {
        add_connector_segment(&mut segments, src_attach, start);
    }

    let mut path_direction = direction;
    if (edge.from_subgraph.is_some() || edge.to_subgraph.is_some())
        && (matches!(src_face, NodeFace::Left | NodeFace::Right)
            || matches!(tgt_face, NodeFace::Left | NodeFace::Right))
    {
        path_direction = if start.x <= end.x {
            Direction::LeftRight
        } else {
            Direction::RightLeft
        };
    }
    let stagger_mid_y = if !src_first_vertical
        && matches!(path_direction, Direction::TopDown | Direction::BottomTop)
        && start.x != end.x
        && start.y != end.y
    {
        let mid = if matches!(path_direction, Direction::TopDown) {
            start.y + 1
        } else {
            start.y.saturating_sub(1)
        };
        if mid != end.y { Some(mid) } else { None }
    } else {
        None
    };
    if let Some(mid_y) = stagger_mid_y {
        segments.push(Segment::Vertical {
            x: start.x,
            y_start: start.y,
            y_end: mid_y,
        });
        segments.push(Segment::Horizontal {
            y: mid_y,
            x_start: start.x,
            x_end: end.x,
        });
        segments.push(Segment::Vertical {
            x: end.x,
            y_start: mid_y,
            y_end: end.y,
        });
    } else {
        segments.extend(build_orthogonal_path_for_direction(
            start,
            end,
            path_direction,
        ));
    }

    ensure_terminal_face_support(&mut segments, start, end, tgt_face);

    let entry_direction = if start == end {
        match direction {
            Direction::TopDown => AttachDirection::Top,
            Direction::BottomTop => AttachDirection::Bottom,
            Direction::LeftRight => AttachDirection::Left,
            Direction::RightLeft => AttachDirection::Right,
        }
    } else {
        entry_direction_from_segments(&segments)
    };

    Some(build_routed_edge(
        edge,
        start,
        end,
        segments,
        src_face,
        entry_direction,
        is_backward,
    ))
}
