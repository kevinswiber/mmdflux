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
use super::intersect::NodeFace;
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
        None,
        src_first_vertical,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn route_edge_with_probe_cached<'a>(
    edge: &Edge,
    layout: &Layout,
    diagram_direction: Direction,
    src_attach_override: Option<(usize, usize)>,
    tgt_attach_override: Option<(usize, usize)>,
    tgt_face_override: Option<NodeFace>,
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

    // For overflow edges with an explicit target face override (e.g., Bottom
    // instead of Left for a saturated LR face), route with waypoints directly.
    // This produces a short "turn the corner" path instead of the long corridor
    // backward detour that routes below all intermediate nodes.
    if tgt_face_override.is_some() {
        let wps = layout
            .edge_waypoints
            .get(&edge.index)
            .map(|w| w.to_vec())
            .unwrap_or_default();
        return route_edge_with_waypoints(
            edge,
            &endpoints,
            &wps,
            diagram_direction,
            RoutingOverrides {
                src_attach: src_attach_override,
                tgt_attach: tgt_attach_override,
                src_face: None,
                tgt_face: tgt_face_override,
                src_first_vertical,
            },
        )
        .map(|routed| {
            route_result(
                routed,
                TextPathFamily::Direct,
                None,
                layout,
                edge,
                node_containing_subgraph,
            )
        });
    }

    let draw_path_attempt = try_shared_draw_path(
        edge,
        layout,
        &endpoints,
        diagram_direction,
        RoutingOverrides {
            src_attach: src_attach_override,
            tgt_attach: tgt_attach_override,
            src_face: None,
            tgt_face: tgt_face_override,
            src_first_vertical,
        },
        node_containing_subgraph,
    );
    if let Some(result) = draw_path_attempt.routed {
        return Some(result);
    }
    let draw_path_rejection = draw_path_attempt.rejection;

    // For subgraph-as-node edges, synthesize a straight vertical route.
    // The probe router only produces local face-attachment segments and can't
    // fill the gap between a subgraph border and a distant external node.
    // Construct the segments directly instead of using the waypoint router,
    // which struggles with large subgraph source bounds.
    if (edge.from_subgraph.is_some() || edge.to_subgraph.is_some())
        && !is_backward_edge(&from_bounds, &to_bounds, diagram_direction)
    {
        let is_vertical = matches!(diagram_direction, Direction::TopDown | Direction::BottomTop);

        // Pick the cross-axis column: use the non-subgraph node's center.
        let cross = if is_vertical {
            if edge.from_subgraph.is_some() {
                to_bounds.x + to_bounds.width / 2
            } else {
                from_bounds.x + from_bounds.width / 2
            }
        } else if edge.from_subgraph.is_some() {
            to_bounds.y + to_bounds.height / 2
        } else {
            from_bounds.y + from_bounds.height / 2
        };

        let (primary_start, primary_end) = if is_vertical {
            (from_bounds.y + from_bounds.height, to_bounds.y)
        } else {
            (from_bounds.x + from_bounds.width, to_bounds.x)
        };

        // The arrowhead is drawn at the `end` point.  Place it one row/col
        // before the target face so it doesn't land on a node or subgraph
        // border cell (which would be skipped or nudged by draw_arrow).
        let arrow_end = if primary_end > primary_start + 1 {
            primary_end - 1
        } else {
            primary_end
        };
        if primary_end > primary_start {
            let segment = if is_vertical {
                Segment::Vertical {
                    x: cross,
                    y_start: primary_start + 1,
                    y_end: arrow_end,
                }
            } else {
                Segment::Horizontal {
                    y: cross,
                    x_start: primary_start + 1,
                    x_end: arrow_end,
                }
            };
            let entry_dir = if is_vertical {
                AttachDirection::Top
            } else {
                AttachDirection::Left
            };
            let (start_pt, end_pt) = if is_vertical {
                (
                    Point {
                        x: cross,
                        y: primary_start,
                    },
                    Point {
                        x: cross,
                        y: arrow_end,
                    },
                )
            } else {
                (
                    Point {
                        x: primary_start,
                        y: cross,
                    },
                    Point {
                        x: arrow_end,
                        y: cross,
                    },
                )
            };
            let routed = RoutedEdge {
                edge: edge.clone(),
                start: start_pt,
                end: end_pt,
                segments: vec![segment],
                source_connection: Some(if is_vertical {
                    AttachDirection::Bottom
                } else {
                    AttachDirection::Right
                }),
                entry_direction: entry_dir,
                is_backward: false,
                is_self_edge: false,
            };
            return Some(route_result(
                routed,
                TextPathFamily::WaypointFallback,
                draw_path_rejection,
                layout,
                edge,
                node_containing_subgraph,
            ));
        }
    }

    // Check for waypoints from normalization — works for both forward and backward long edges
    if let Some(wps) = layout.edge_waypoints.get(&edge.index)
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
            let (src_override, tgt_override, tgt_face_override, src_first_vertical) = plan
                .get(&edge.index)
                .map(|ov| {
                    (
                        ov.source,
                        ov.target,
                        ov.target_face,
                        ov.source_first_vertical,
                    )
                })
                .unwrap_or((None, None, None, false));
            let edge_dir = layout.effective_edge_direction(&edge.from, &edge.to, diagram_direction);
            let result = route_edge_with_probe_cached(
                edge,
                layout,
                edge_dir,
                src_override,
                tgt_override,
                tgt_face_override,
                src_first_vertical,
                node_containing_subgraph.as_ref(),
            );
            result.map(|result| result.routed)
        })
        .collect();

    // Post-routing: simplify terminal dip patterns.  When a path
    // overshoots the endpoint on one axis then corrects with a short
    // segment back, the intermediate row/column is shared with other
    // edges.  Flattening the dip avoids this shared-segment overlap.
    for edge_route in &mut routed {
        simplify_terminal_dip(edge_route, layout);
    }

    // Post-routing: separate adjacent parallel vertical segments that belong
    // to different edges.  In LR criss-cross patterns, crossing edges get
    // placed in adjacent columns (gap=1), making them look like a single
    // thick line.  Push them apart so each edge has a visually distinct lane.
    separate_adjacent_parallel_segments(&mut routed);

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

/// Simplify terminal dip patterns in a routed edge.
///
/// Detects when the last 3+ segments form a "dip and return": the path
/// overshoots the endpoint on one axis, runs a horizontal/vertical, then
/// comes back.  For example in LR:
///
/// ```text
///   Before: ..., V(28, 9→12), H(12, 28→41), V(41, 12→11), H(11, 41→49)
///   After:  ..., V(28, 9→11), H(11, 28→49)
/// ```
///
/// The dip to y=12 is eliminated, preventing shared segments with other
/// edges that also route through y=12.
fn simplify_terminal_dip(routed: &mut RoutedEdge, layout: &Layout) {
    if routed.segments.len() < 4 || routed.is_self_edge || routed.is_backward {
        return;
    }

    let len = routed.segments.len();
    // Pattern: ..., V(x1, a→b), H(b, x1→x2), V(x2, b→c), H(c, x2→end_x)
    //   where |b - c| == 1 (the dip correction is 1 cell)
    //   and the V segments go in opposite directions on y
    // Or the LR-rotated equivalent for TD graphs.
    let (dip_seg, horiz_seg, return_seg, final_seg) = (
        &routed.segments[len - 4],
        &routed.segments[len - 3],
        &routed.segments[len - 2],
        &routed.segments[len - 1],
    );

    // Match the vertical-dip pattern (common in LR graphs):
    // V down, H across, V up 1, H to end
    if let (
        Segment::Vertical {
            x: x1,
            y_start: a,
            y_end: b,
        },
        Segment::Horizontal {
            y: hy,
            x_start: hx1,
            x_end: hx2,
        },
        Segment::Vertical {
            x: x2,
            y_start: rb,
            y_end: c,
        },
        Segment::Horizontal {
            y: fy,
            x_start: fx1,
            x_end: fx2,
        },
    ) = (dip_seg, horiz_seg, return_seg, final_seg)
        && *hy == *b
        && *hx1 == *x1
        && *hx2 == *x2
        && *rb == *b
        && *fy == *c
        && *fx1 == *x2
        && b.abs_diff(*c) == 1
    {
        let simplified_v = Segment::Vertical {
            x: *x1,
            y_start: *a,
            y_end: *c,
        };
        let simplified_h = Segment::Horizontal {
            y: *c,
            x_start: *x1,
            x_end: *fx2,
        };

        if !segment_collides_with_nodes(&simplified_v, layout, &routed.edge)
            && !segment_collides_with_nodes(&simplified_h, layout, &routed.edge)
        {
            routed.segments.truncate(len - 4);
            routed.segments.push(simplified_v);
            routed.segments.push(simplified_h);
        }
        return;
    }

    // Match the horizontal-dip pattern (common in TD graphs):
    // H across, V down, H back 1, V to end
    if let (
        Segment::Horizontal {
            y: y1,
            x_start: a,
            x_end: b,
        },
        Segment::Vertical {
            x: vx,
            y_start: vy1,
            y_end: vy2,
        },
        Segment::Horizontal {
            y: y2,
            x_start: rb,
            x_end: c,
        },
        Segment::Vertical {
            x: fx,
            y_start: fy1,
            y_end: fy2,
        },
    ) = (dip_seg, horiz_seg, return_seg, final_seg)
        && *vx == *b
        && *vy1 == *y1
        && *vy2 == *y2
        && *rb == *b
        && *fx == *c
        && *fy1 == *y2
        && b.abs_diff(*c) == 1
    {
        let simplified_h = Segment::Horizontal {
            y: *y1,
            x_start: *a,
            x_end: *c,
        };
        let simplified_v = Segment::Vertical {
            x: *c,
            y_start: *y1,
            y_end: *fy2,
        };

        if !segment_collides_with_nodes(&simplified_h, layout, &routed.edge)
            && !segment_collides_with_nodes(&simplified_v, layout, &routed.edge)
        {
            routed.segments.truncate(len - 4);
            routed.segments.push(simplified_h);
            routed.segments.push(simplified_v);
        }
    }
}

fn segment_collides_with_nodes(segment: &Segment, layout: &Layout, edge: &Edge) -> bool {
    layout.node_bounds.iter().any(|(node_id, bounds)| {
        if node_id == &edge.from || node_id == &edge.to {
            return false;
        }
        let left = bounds.x;
        let right = bounds.x + bounds.width.saturating_sub(1);
        let top = bounds.y;
        let bottom = bounds.y + bounds.height.saturating_sub(1);
        match segment {
            Segment::Vertical { x, y_start, y_end } => {
                if *x < left || *x > right {
                    return false;
                }
                let (min_y, max_y) = if y_start <= y_end {
                    (*y_start, *y_end)
                } else {
                    (*y_end, *y_start)
                };
                min_y <= bottom && max_y >= top
            }
            Segment::Horizontal { y, x_start, x_end } => {
                if *y < top || *y > bottom {
                    return false;
                }
                let (min_x, max_x) = if x_start <= x_end {
                    (*x_start, *x_end)
                } else {
                    (*x_end, *x_start)
                };
                min_x <= right && max_x >= left
            }
        }
    })
}

/// Separate adjacent parallel vertical segments that belong to different edges.
///
/// In LR criss-cross patterns, two crossing edges can be placed in adjacent
/// columns (gap=1), making them look like a single thick line (`││`).  This
/// pass detects such pairs and pushes the right-side segment 1 cell further
/// right, creating a visible gap.
fn separate_adjacent_parallel_segments(routed: &mut [RoutedEdge]) {
    if routed.len() < 2 {
        return;
    }

    // Collect (edge_index, segment_index, x, y_min, y_max) for all vertical segments
    let mut verticals: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
    for (ei, edge_route) in routed.iter().enumerate() {
        for (si, seg) in edge_route.segments.iter().enumerate() {
            if let Segment::Vertical {
                x, y_start, y_end, ..
            } = seg
            {
                let (y_min, y_max) = if y_start <= y_end {
                    (*y_start, *y_end)
                } else {
                    (*y_end, *y_start)
                };
                // Only consider interior segments (not the first or last)
                if si > 0 && si < edge_route.segments.len() - 1 && y_max > y_min + 1 {
                    verticals.push((ei, si, *x, y_min, y_max));
                }
            }
        }
    }

    // Find pairs at the same or adjacent x-coordinates with overlapping y-ranges.
    // For each pair, try nudging one edge; if that would break its terminal,
    // try the other.  Collect at most one nudge per pair.
    let mut nudges: Vec<(usize, usize, isize)> = Vec::new();
    let mut already_nudged: Vec<usize> = Vec::new();
    for i in 0..verticals.len() {
        for j in (i + 1)..verticals.len() {
            let (ei_a, si_a, x_a, y_min_a, y_max_a) = verticals[i];
            let (ei_b, si_b, x_b, y_min_b, y_max_b) = verticals[j];
            if ei_a == ei_b || already_nudged.contains(&ei_a) || already_nudged.contains(&ei_b) {
                continue;
            }
            let overlaps_y = y_min_a < y_max_b && y_min_b < y_max_a;
            if !overlaps_y {
                continue;
            }
            let gap = x_a.abs_diff(x_b);
            if gap <= 1 {
                let dx = (2 - gap) as isize;
                // Prefer nudging the rightmost; fall back to the other.
                let (primary, fallback) = if x_b >= x_a {
                    ((ei_b, si_b), (ei_a, si_a))
                } else {
                    ((ei_a, si_a), (ei_b, si_b))
                };
                if can_nudge_vertical(&routed[primary.0], primary.1, dx) {
                    nudges.push((primary.0, primary.1, dx));
                    already_nudged.push(primary.0);
                } else if can_nudge_vertical(&routed[fallback.0], fallback.1, dx) {
                    nudges.push((fallback.0, fallback.1, dx));
                    already_nudged.push(fallback.0);
                }
            }
        }
    }

    // Apply nudges: shift the vertical and adjust its neighboring horizontals.
    for (ei, si, dx) in nudges {
        let edge_route = &mut routed[ei];
        if si >= edge_route.segments.len() {
            continue;
        }
        if let Segment::Vertical { x, .. } = &mut edge_route.segments[si] {
            *x = (*x as isize + dx) as usize;
        }
        if si > 0
            && let Segment::Horizontal { x_end, .. } = &mut edge_route.segments[si - 1]
        {
            *x_end = (*x_end as isize + dx) as usize;
        }
        if si + 1 < edge_route.segments.len()
            && let Segment::Horizontal { x_start, .. } = &mut edge_route.segments[si + 1]
        {
            *x_start = (*x_start as isize + dx) as usize;
        }
    }
}

/// Check whether nudging a vertical segment by `dx` would leave its
/// neighboring horizontal segments valid (retaining direction and at least
/// 2 cells of length for visible terminal connections).
fn can_nudge_vertical(edge_route: &RoutedEdge, si: usize, dx: isize) -> bool {
    let seg_count = edge_route.segments.len();
    if si >= seg_count {
        return false;
    }

    // Check preceding horizontal
    if si > 0
        && let Segment::Horizontal { x_start, x_end, .. } = &edge_route.segments[si - 1]
    {
        let new_end = (*x_end as isize + dx) as usize;
        let retains_dir = (*x_start <= *x_end && *x_start <= new_end)
            || (*x_start >= *x_end && *x_start >= new_end);
        if !retains_dir || x_start.abs_diff(new_end) < 2 {
            return false;
        }
    }

    // Check following horizontal
    if si + 1 < seg_count
        && let Segment::Horizontal { x_start, x_end, .. } = &edge_route.segments[si + 1]
    {
        let new_start = (*x_start as isize + dx) as usize;
        let retains_dir = (*x_start <= *x_end && new_start <= *x_end)
            || (*x_start >= *x_end && new_start >= *x_end);
        if !retains_dir || new_start.abs_diff(*x_end) < 2 {
            return false;
        }
    }

    true
}

#[cfg(test)]
#[path = "../routing_tests.rs"]
mod routing_tests;
