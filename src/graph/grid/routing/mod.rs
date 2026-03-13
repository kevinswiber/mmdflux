//! Grid-space edge routing between derived node bounds.
//!
//! This module owns orthogonal edge routing over integer-coordinate grid
//! geometry. It consumes `GridLayout` and related grid-space helpers without
//! depending on render-owned text drawing modules.

mod attachment_resolution;
mod border_nudging;
mod draw_path;
mod orthogonal;
mod path_selection;
mod probe;
mod route_variants;
mod self_edges;
mod types;

pub use self::attachment_resolution::compute_attachment_plan;
#[cfg(test)]
use self::attachment_resolution::compute_attachment_plan_from_shared_planner;
#[cfg(test)]
pub use self::orthogonal::{build_orthogonal_path, orthogonalize};
#[cfg(test)]
use self::orthogonal::{compute_vertical_first_path, orthogonalize_segment};
use self::path_selection::{route_result, try_shared_draw_path};
#[cfg(test)]
pub(crate) use self::probe::TextPathRejection;
pub(crate) use self::probe::{RouteEdgeResult, TextPathFamily};
use self::route_variants::{
    route_backward_with_synthetic_waypoints, route_edge_direct, route_edge_with_waypoints,
};
use self::self_edges::route_self_edge;
pub use self::types::{AttachDirection, Point, RoutedEdge, Segment};
use self::types::{EdgeEndpoints, RoutingOverrides};
use super::GridLayout;
use super::backward::{
    compact_lr_backward_attachments, generate_backward_waypoints, is_backward_edge,
};
use super::bounds::{
    NodeContainingSubgraphMap, build_node_containing_subgraph_map, resolve_edge_bounds,
};
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
