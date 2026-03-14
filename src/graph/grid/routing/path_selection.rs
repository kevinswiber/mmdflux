use super::super::backward::{
    generate_corridor_backward_waypoints, is_backward_edge,
    should_prefer_shared_backward_route_for_text,
};
use super::super::bounds::{NodeContainingSubgraphMap, containing_subgraph_id};
use super::super::layout::{GridLayout, NodeBounds};
use super::border_nudging::nudge_routed_edge_clear_of_unrelated_subgraph_borders;
use super::draw_path::{
    repair_draw_path_segment_collisions, route_edge_from_draw_path,
    route_inter_subgraph_edge_via_outer_lane, waypoints_from_draw_path,
};
use super::probe::{RouteEdgeResult, TextPathFamily, TextPathRejection, TextRouteProbe};
use super::route_variants::route_backward_with_synthetic_waypoints;
use super::types::{EdgeEndpoints, RoutedEdge, RoutingOverrides};
use crate::graph::{Direction, Edge};

pub(super) struct SharedDrawPathAttempt {
    pub(super) routed: Option<RouteEdgeResult>,
    pub(super) rejection: Option<TextPathRejection>,
}

pub(super) fn should_use_routed_draw_path(
    edge: &Edge,
    layout: &GridLayout,
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'_>>,
) -> bool {
    if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        return false;
    }

    let Some(draw_path) = layout.routed_edge_paths.get(&edge.index) else {
        return false;
    };

    let from_subgraph = containing_subgraph_id(layout, &edge.from, node_containing_subgraph);
    let to_subgraph = containing_subgraph_id(layout, &edge.to, node_containing_subgraph);
    if from_subgraph.is_some() && from_subgraph == to_subgraph {
        return false;
    }
    if from_subgraph.is_some() ^ to_subgraph.is_some() {
        return false;
    }

    if is_backward_edge(from_bounds, to_bounds, direction) {
        return should_prefer_shared_backward_route_for_text(draw_path, direction);
    }

    should_prefer_shared_forward_route_for_text(
        edge,
        layout,
        draw_path,
        from_bounds,
        to_bounds,
        direction,
    )
}

fn should_prefer_shared_forward_route_for_text(
    edge: &Edge,
    layout: &GridLayout,
    draw_path: &[(usize, usize)],
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    if edge.from == edge.to || is_backward_edge(from_bounds, to_bounds, direction) {
        return false;
    }

    let draw_waypoint_count = waypoints_from_draw_path(draw_path).len();
    let has_normalized_waypoints = layout
        .edge_waypoints
        .get(&edge.index)
        .is_some_and(|waypoints| waypoints.len() >= 2);
    let structured_short_forward =
        layout.preserve_routed_path_topology.contains(&edge.index) && draw_waypoint_count >= 2;

    let structured_draw_path = draw_path.len() >= 4 && draw_waypoint_count >= 2;

    (has_normalized_waypoints && structured_draw_path)
        || structured_short_forward
        || (!layout.subgraph_bounds.is_empty() && structured_draw_path)
}

pub(super) fn try_shared_draw_path<'a>(
    edge: &Edge,
    layout: &GridLayout,
    endpoints: &EdgeEndpoints,
    diagram_direction: Direction,
    overrides: RoutingOverrides,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'a>>,
) -> SharedDrawPathAttempt {
    let mut rejection = None;

    if !should_use_routed_draw_path(
        edge,
        layout,
        &endpoints.from_bounds,
        &endpoints.to_bounds,
        diagram_direction,
        node_containing_subgraph,
    ) {
        return SharedDrawPathAttempt {
            routed: None,
            rejection: None,
        };
    }

    if let Some(draw_path) = layout.routed_edge_paths.get(&edge.index) {
        match route_edge_from_draw_path(
            edge,
            layout,
            endpoints,
            draw_path,
            diagram_direction,
            overrides,
        ) {
            Ok(routed) => {
                return SharedDrawPathAttempt {
                    routed: Some(route_result(
                        routed,
                        TextPathFamily::SharedRoutedDrawPath,
                        None,
                        layout,
                        edge,
                        node_containing_subgraph,
                    )),
                    rejection: None,
                };
            }
            Err(TextPathRejection::SegmentCollision) => {
                if let Some(routed) = route_inter_subgraph_edge_via_outer_lane(
                    edge,
                    layout,
                    endpoints,
                    draw_path,
                    diagram_direction,
                    overrides,
                    node_containing_subgraph,
                ) {
                    return SharedDrawPathAttempt {
                        routed: Some(route_result(
                            routed,
                            TextPathFamily::SharedRoutedDrawPath,
                            None,
                            layout,
                            edge,
                            node_containing_subgraph,
                        )),
                        rejection: None,
                    };
                }
                let repaired_draw_path = if layout.subgraph_bounds.is_empty() {
                    draw_path.to_vec()
                } else {
                    repair_draw_path_segment_collisions(draw_path, layout, edge)
                };
                if repaired_draw_path.as_slice() != draw_path.as_slice()
                    && let Ok(routed) = route_edge_from_draw_path(
                        edge,
                        layout,
                        endpoints,
                        &repaired_draw_path,
                        diagram_direction,
                        overrides,
                    )
                {
                    return SharedDrawPathAttempt {
                        routed: Some(route_result(
                            routed,
                            TextPathFamily::SharedRoutedDrawPath,
                            None,
                            layout,
                            edge,
                            node_containing_subgraph,
                        )),
                        rejection: None,
                    };
                }
                rejection = Some(TextPathRejection::SegmentCollision);
                debug_draw_path_rejection(edge, TextPathRejection::SegmentCollision, draw_path);
            }
            Err(other) => {
                rejection = Some(other);
                debug_draw_path_rejection(edge, other, draw_path);
            }
        }
    }

    let channel_wps = generate_corridor_backward_waypoints(
        edge,
        layout,
        &endpoints.from_bounds,
        &endpoints.to_bounds,
        diagram_direction,
    );
    if !channel_wps.is_empty() {
        return SharedDrawPathAttempt {
            routed: route_backward_with_synthetic_waypoints(
                edge,
                endpoints,
                &channel_wps,
                diagram_direction,
                RoutingOverrides {
                    src_attach: None,
                    tgt_attach: None,
                    src_face: None,
                    tgt_face: None,
                    src_first_vertical: overrides.src_first_vertical,
                },
            )
            .map(|routed| {
                route_result(
                    routed,
                    TextPathFamily::SyntheticBackward,
                    rejection,
                    layout,
                    edge,
                    node_containing_subgraph,
                )
            }),
            rejection,
        };
    }

    SharedDrawPathAttempt {
        routed: None,
        rejection,
    }
}

pub(super) fn route_result(
    routed: RoutedEdge,
    path_family: TextPathFamily,
    rejection_reason: Option<TextPathRejection>,
    layout: &GridLayout,
    edge: &Edge,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'_>>,
) -> RouteEdgeResult {
    RouteEdgeResult {
        routed: nudge_routed_edge_clear_of_unrelated_subgraph_borders(
            routed,
            layout,
            edge,
            node_containing_subgraph,
        ),
        probe: TextRouteProbe {
            path_family,
            rejection_reason,
        },
    }
}

fn debug_draw_path_rejection(
    edge: &Edge,
    rejection: TextPathRejection,
    draw_path: &[(usize, usize)],
) {
    if std::env::var("MMDFLUX_DEBUG_ROUTE_SEGMENTS").is_ok_and(|value| value == "1") {
        eprintln!(
            "[route-routed-reject] {} -> {}: reason={rejection:?} points={draw_path:?}",
            edge.from, edge.to
        );
    }
}
