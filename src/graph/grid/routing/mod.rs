//! Grid-space edge routing between derived node bounds.
//!
//! This module owns orthogonal edge routing over integer-coordinate grid
//! geometry. It consumes `GridLayout` and related grid-space helpers without
//! depending on render-owned text drawing modules.

mod attachment_resolution;
mod draw_path;
mod orthogonal;
mod path_selection;
mod route_variants;
mod self_edges;
mod types;

pub use self::attachment_resolution::compute_attachment_plan;
#[cfg(test)]
use self::attachment_resolution::compute_attachment_plan_from_shared_planner;
use self::attachment_resolution::{
    clamp_to_boundary, clamp_to_face, edge_faces, infer_face_from_attachment, offset_for_face,
    resolve_attachment_points,
};
use self::orthogonal::{
    add_connector_segment, build_orthogonal_path_for_direction,
    build_orthogonal_path_with_waypoints, ensure_source_face_launch_support,
    ensure_terminal_face_support, entry_direction_from_segments, normalize_polyline_points,
    point_segment_is_axis_aligned, polyline_points_from_segments, polyline_points_to_segments,
};
#[cfg(test)]
pub use self::orthogonal::{build_orthogonal_path, orthogonalize};
#[cfg(test)]
use self::orthogonal::{compute_vertical_first_path, orthogonalize_segment};
use self::path_selection::{
    RouteEdgeResult, TextPathFamily, TextPathRejection, route_result, try_shared_draw_path,
};
use self::route_variants::{
    route_backward_with_synthetic_waypoints, route_edge_direct, route_edge_with_waypoints,
};
use self::self_edges::route_self_edge;
pub use self::types::{AttachDirection, AttachmentOverride, Point, RoutedEdge, Segment};
use self::types::{EdgeEndpoints, RoutingOverrides, build_routed_edge};
use super::backward::{
    compact_lr_backward_attachments, generate_backward_waypoints, is_backward_edge,
};
use super::bounds::{
    NodeContainingSubgraphMap, build_node_containing_subgraph_map, containing_subgraph_id,
    node_inside_subgraph, resolve_edge_bounds,
};
use super::intersect::NodeFace;
use super::{GridLayout, NodeBounds, SelfEdgeDrawData, SubgraphBounds};
use crate::graph::{Direction, Edge, Shape, Stroke};

type Layout = GridLayout;

/// Get the outgoing and incoming attachment directions based on diagram direction.
#[cfg(test)]
fn attachment_directions(diagram_direction: Direction) -> (AttachDirection, AttachDirection) {
    match diagram_direction {
        Direction::TopDown => (AttachDirection::Bottom, AttachDirection::Top),
        Direction::BottomTop => (AttachDirection::Top, AttachDirection::Bottom),
        Direction::LeftRight => (AttachDirection::Right, AttachDirection::Left),
        Direction::RightLeft => (AttachDirection::Left, AttachDirection::Right),
    }
}

/// Route an edge between two nodes.
#[cfg_attr(not(test), allow(dead_code))]
pub fn route_edge(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RoutedEdge> {
    route_edge_with_probe(
        edge,
        layout,
        diagram_direction,
        src_attach_override,
        tgt_attach_override,
        src_first_vertical,
    )
    .map(|result| result.routed)
}

pub(crate) fn route_edge_with_probe(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
) -> Option<RouteEdgeResult> {
    route_edge_with_probe_cached(
        edge,
        layout,
        diagram_direction,
        src_attach_override,
        tgt_attach_override,
        src_first_vertical,
        None,
    )
}

fn route_edge_with_probe_cached<'a>(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    src_first_vertical: bool,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'a>>,
) -> Option<RouteEdgeResult> {
    let (from_bounds, to_bounds) = resolve_edge_bounds(layout, edge)?;

    // Get node shapes for intersection calculation
    let from_shape = if edge.from_subgraph.is_some() {
        Shape::Rectangle
    } else {
        layout
            .node_shapes
            .get(&edge.from)
            .copied()
            .unwrap_or(Shape::Rectangle)
    };
    let to_shape = if edge.to_subgraph.is_some() {
        Shape::Rectangle
    } else {
        layout
            .node_shapes
            .get(&edge.to)
            .copied()
            .unwrap_or(Shape::Rectangle)
    };

    let endpoints = EdgeEndpoints {
        from_bounds,
        from_shape,
        to_bounds,
        to_shape,
    };
    let draw_path_attempt = try_shared_draw_path(
        edge,
        layout,
        &endpoints,
        diagram_direction,
        RoutingOverrides {
            src_attach: src_attach_override,
            tgt_attach: tgt_attach_override,
            src_face: None,
            tgt_face: None,
            src_first_vertical,
        },
        node_containing_subgraph,
    );
    if let Some(result) = draw_path_attempt.routed {
        return Some(result);
    }
    let draw_path_rejection = draw_path_attempt.rejection;

    // Check for waypoints from normalization — works for both forward and backward long edges
    let allow_waypoints = edge.from_subgraph.is_none() && edge.to_subgraph.is_none();
    if allow_waypoints
        && let Some(wps) = layout.edge_waypoints.get(&edge.index)
        && !wps.is_empty()
    {
        let is_backward = is_backward_edge(&from_bounds, &to_bounds, diagram_direction);

        // For backward edges, reverse waypoints so they go from source to target.
        // The layout stores them in effective/forward order (low rank → high rank),
        // but the backward edge goes from high rank → low rank.
        let waypoints: Vec<(usize, usize)> = if is_backward {
            wps.iter().rev().copied().collect()
        } else {
            wps.to_vec()
        };

        return route_edge_with_waypoints(
            edge,
            &endpoints,
            &waypoints,
            diagram_direction,
            RoutingOverrides {
                src_attach: src_attach_override,
                tgt_attach: tgt_attach_override,
                src_face: None,
                tgt_face: None,
                src_first_vertical,
            },
        )
        .map(|routed| {
            route_result(
                routed,
                TextPathFamily::WaypointFallback,
                draw_path_rejection,
                layout,
                edge,
                node_containing_subgraph,
            )
        });
    }

    // For backward edges with no layout waypoints, generate synthetic ones
    if is_backward_edge(&from_bounds, &to_bounds, diagram_direction) {
        if let Some((compact_src, compact_tgt)) = compact_lr_backward_attachments(
            edge,
            layout,
            &from_bounds,
            &to_bounds,
            diagram_direction,
        ) {
            return route_edge_direct(
                edge,
                &endpoints,
                diagram_direction,
                Some(compact_src),
                Some(compact_tgt),
                src_first_vertical,
            )
            .map(|routed| {
                route_result(
                    routed,
                    TextPathFamily::Direct,
                    draw_path_rejection,
                    layout,
                    edge,
                    node_containing_subgraph,
                )
            });
        }

        let synthetic_wps =
            generate_backward_waypoints(&from_bounds, &to_bounds, diagram_direction);
        if !synthetic_wps.is_empty() {
            if matches!(
                diagram_direction,
                Direction::LeftRight | Direction::RightLeft
            ) {
                return route_edge_with_waypoints(
                    edge,
                    &endpoints,
                    &synthetic_wps,
                    diagram_direction,
                    RoutingOverrides {
                        src_attach: src_attach_override,
                        tgt_attach: tgt_attach_override,
                        src_face: None,
                        tgt_face: None,
                        src_first_vertical,
                    },
                )
                .map(|routed| {
                    route_result(
                        routed,
                        TextPathFamily::SyntheticBackward,
                        draw_path_rejection,
                        layout,
                        edge,
                        node_containing_subgraph,
                    )
                });
            }
            return route_backward_with_synthetic_waypoints(
                edge,
                &endpoints,
                &synthetic_wps,
                diagram_direction,
                RoutingOverrides {
                    src_attach: src_attach_override,
                    tgt_attach: tgt_attach_override,
                    src_face: None,
                    tgt_face: None,
                    src_first_vertical,
                },
            )
            .map(|routed| {
                route_result(
                    routed,
                    TextPathFamily::SyntheticBackward,
                    draw_path_rejection,
                    layout,
                    edge,
                    node_containing_subgraph,
                )
            });
        }
    }

    // No waypoints: direct routing for forward edges
    route_edge_direct(
        edge,
        &endpoints,
        diagram_direction,
        src_attach_override,
        tgt_attach_override,
        src_first_vertical,
    )
    .map(|routed| {
        route_result(
            routed,
            TextPathFamily::Direct,
            draw_path_rejection,
            layout,
            edge,
            node_containing_subgraph,
        )
    })
}

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let (a_min, a_max) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
    let (b_min, b_max) = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
    a_min <= b_max && b_min <= a_max
}

// Border-nudging policy:
// - skip routes fully contained in the same subgraph
// - skip routes crossing between two different subgraphs; those use the shared outer lane
// - nudge one-in/one-out routes only near the contained endpoint
// - nudge fully external routes away from unrelated borders along the middle spans
fn nudge_routed_edge_clear_of_unrelated_subgraph_borders(
    mut routed: RoutedEdge,
    layout: &Layout,
    edge: &Edge,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'_>>,
) -> RoutedEdge {
    if layout.subgraph_bounds.is_empty() {
        return routed;
    }

    let mut points = polyline_points_from_segments(routed.start, &routed.segments);
    if points.last().copied() != Some(routed.end) {
        points.push(routed.end);
    }
    if points.len() < 4 {
        return routed;
    }

    let from_bounds = layout.node_bounds.get(&edge.from);
    let to_bounds = layout.node_bounds.get(&edge.to);
    let from_container = containing_subgraph_id(layout, &edge.from, node_containing_subgraph);
    let to_container = containing_subgraph_id(layout, &edge.to, node_containing_subgraph);
    let from_inside_any = from_container.is_some();
    let to_inside_any = to_container.is_some();

    let mut ordered_subgraph_bounds: Vec<(&String, &SubgraphBounds)> =
        layout.subgraph_bounds.iter().collect();
    ordered_subgraph_bounds.sort_by(|(left_id, left_bounds), (right_id, right_bounds)| {
        left_bounds
            .depth
            .cmp(&right_bounds.depth)
            .then_with(|| left_bounds.y.cmp(&right_bounds.y))
            .then_with(|| left_bounds.x.cmp(&right_bounds.x))
            .then_with(|| left_id.cmp(right_id))
    });

    for (_, sg) in ordered_subgraph_bounds {
        let from_inside = from_bounds.is_some_and(|bounds| node_inside_subgraph(bounds, sg));
        let to_inside = to_bounds.is_some_and(|bounds| node_inside_subgraph(bounds, sg));
        let is_inter_subgraph_crossing =
            from_inside != to_inside && from_container.is_some() && to_container.is_some();
        let allow_cross_boundary_nudge = (from_inside && !to_inside && !to_inside_any)
            || (to_inside && !from_inside && !from_inside_any);
        if (from_inside && to_inside) || is_inter_subgraph_crossing {
            continue;
        }

        let left = sg.x;
        let right = sg.x + sg.width.saturating_sub(1);
        let top = sg.y;
        let bottom = sg.y + sg.height.saturating_sub(1);

        if allow_cross_boundary_nudge {
            if from_inside {
                nudge_endpoint_segment_clear_of_subgraph_border(
                    &mut points,
                    left,
                    right,
                    top,
                    bottom,
                    true,
                );
            }
            if to_inside {
                nudge_endpoint_segment_clear_of_subgraph_border(
                    &mut points,
                    left,
                    right,
                    top,
                    bottom,
                    false,
                );
            }
        } else if from_inside || to_inside {
            continue;
        }

        for idx in 1..points.len().saturating_sub(2) {
            let current = points[idx];
            let next = points[idx + 1];

            if current.y == next.y && ranges_overlap(current.x, next.x, left, right) {
                if current.y == bottom || current.y == bottom.saturating_add(1) {
                    let target_y = bottom.saturating_add(2);
                    points[idx].y = target_y;
                    points[idx + 1].y = target_y;
                } else if current.y == top || current.y == top.saturating_sub(1) {
                    let target_y = top.saturating_sub(2);
                    points[idx].y = target_y;
                    points[idx + 1].y = target_y;
                }
            } else if current.x == next.x && ranges_overlap(current.y, next.y, top, bottom) {
                if current.x == right || current.x == right.saturating_add(1) {
                    let target_x = right.saturating_add(2);
                    points[idx].x = target_x;
                    points[idx + 1].x = target_x;
                } else if current.x == left || current.x == left.saturating_sub(1) {
                    let target_x = left.saturating_sub(2);
                    points[idx].x = target_x;
                    points[idx + 1].x = target_x;
                }
            }
        }
    }

    normalize_polyline_points(&mut points);
    if points
        .windows(2)
        .all(|segment| point_segment_is_axis_aligned(segment[0], segment[1]))
    {
        routed.start = points[0];
        routed.end = *points.last().unwrap_or(&routed.end);
        routed.segments = polyline_points_to_segments(&points);
    }

    routed
}

fn nudge_endpoint_segment_clear_of_subgraph_border(
    points: &mut Vec<Point>,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    source_segment: bool,
) {
    if points.len() < 2 {
        return;
    }

    let (current, next) = if source_segment {
        (points[0], points[1])
    } else {
        let len = points.len();
        (points[len - 2], points[len - 1])
    };

    if current.y == next.y && ranges_overlap(current.x, next.x, left, right) {
        let target_y = if current.y == bottom || current.y == bottom.saturating_add(1) {
            Some(bottom.saturating_add(2))
        } else if current.y == top || current.y == top.saturating_sub(1) {
            Some(top.saturating_sub(2))
        } else {
            None
        };
        if let Some(target_y) = target_y {
            if source_segment {
                let detour = Point::new(current.x, target_y);
                points[1].y = target_y;
                if detour != current && detour != points[1] {
                    points.insert(1, detour);
                }
            } else {
                let len = points.len();
                let detour = Point::new(next.x, target_y);
                points[len - 2].y = target_y;
                if detour != points[len - 2] && detour != next {
                    points.insert(len - 1, detour);
                }
            }
        }
    } else if current.x == next.x && ranges_overlap(current.y, next.y, top, bottom) {
        let target_x = if current.x == right || current.x == right.saturating_add(1) {
            Some(right.saturating_add(2))
        } else if current.x == left || current.x == left.saturating_sub(1) {
            Some(left.saturating_sub(2))
        } else {
            None
        };
        if let Some(target_x) = target_x {
            if source_segment {
                let detour = Point::new(target_x, current.y);
                points[1].x = target_x;
                if detour != current && detour != points[1] {
                    points.insert(1, detour);
                }
            } else {
                let len = points.len();
                let detour = Point::new(target_x, next.y);
                points[len - 2].x = target_x;
                if detour != points[len - 2] && detour != next {
                    points.insert(len - 1, detour);
                }
            }
        }
    }
}

/// Route an edge using waypoints from normalization.
///
/// Uses dynamic intersection calculation to determine attachment points
/// based on the approach angle from the first/last waypoint.
/// Route all edges in the layout.
pub fn route_all_edges(
    edges: &[Edge],
    layout: &Layout,
    diagram_direction: Direction,
) -> Vec<RoutedEdge> {
    // Pre-pass: compute attachment plan for edges sharing a face
    let plan = compute_attachment_plan(edges, layout, diagram_direction);
    let node_containing_subgraph = if layout.subgraph_bounds.is_empty() {
        None
    } else {
        Some(build_node_containing_subgraph_map(layout))
    };

    let mut routed: Vec<RoutedEdge> = edges
        .iter()
        .filter_map(|edge| {
            // Skip self-edges in normal routing
            if edge.from == edge.to {
                return None;
            }
            // Skip invisible edges — they affect layout but are not rendered
            if edge.stroke == Stroke::Invisible {
                return None;
            }
            let (src_override, tgt_override, src_first_vertical) = plan
                .get(&edge.index)
                .map(|ov| (ov.source, ov.target, ov.source_first_vertical))
                .unwrap_or((None, None, false));
            let edge_dir = layout.effective_edge_direction(&edge.from, &edge.to, diagram_direction);
            route_edge_with_probe_cached(
                edge,
                layout,
                edge_dir,
                src_override,
                tgt_override,
                src_first_vertical,
                node_containing_subgraph.as_ref(),
            )
            .map(|result| result.routed)
        })
        .collect();

    // Route self-edges separately using pre-computed loop points
    for se_data in &layout.self_edges {
        if let Some(edge) = edges
            .iter()
            .find(|e| e.from == e.to && e.from == se_data.node_id)
            && !se_data.points.is_empty()
        {
            routed.push(route_self_edge(se_data, edge, diagram_direction));
        }
    }

    routed
}

#[cfg(test)]
#[path = "../routing_tests.rs"]
mod routing_tests;
