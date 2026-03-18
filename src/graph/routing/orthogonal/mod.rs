//! Float-first orthogonal routing preview helpers.
//!
//! This module routes edges in float space first, then optionally applies a
//! deterministic grid snap adapter for grid replay.

pub(crate) mod backward;
pub(crate) mod collision;
mod constants;
pub(crate) mod endpoints;
pub(crate) mod fan;
pub(crate) mod forward;
pub(crate) mod hints;
pub(crate) mod overlap;
pub(crate) mod path_utils;

use std::collections::HashMap;

use self::endpoints::{
    anchor_path_endpoints_to_endpoint_faces, endpoint_is_on_policy_face,
    enforce_terminal_support_normal_to_face, ensure_endpoint_segments_axis_aligned,
    flow_target_face_for_direction, map_face_to_rect_face,
    offset_backward_source_from_primary_face, snap_backward_endpoints_to_shape,
};
pub(crate) use self::hints::build_path_from_hints;
use self::path_utils::{
    build_contracted_path, collapse_collinear_interior_points, light_normalize,
    revalidate_label_anchor, snap_path_to_grid,
};
use super::super::direction_policy::{
    build_override_node_map, cross_boundary_edge_direction, effective_edge_direction,
};
use super::float_core::normalize_orthogonal_route_contracts;
use super::labels::compute_end_labels_for_edge;
use crate::graph::attachment::Face;
use crate::graph::geometry::{GraphGeometry, RoutedEdgeGeometry};
use crate::graph::space::FPoint;
use crate::graph::{Direction, Graph};

/// Preview options for orthogonal float-first routing.
#[derive(Debug, Clone, Copy)]
pub struct OrthogonalRoutingOptions {
    /// Keep existing behavior for backward edges while previewing forward routing.
    pub backward_fallback_to_hints: bool,
    /// Optional grid snap `(scale_x, scale_y)` applied after routing.
    pub grid_snap: Option<(f64, f64)>,
}

impl OrthogonalRoutingOptions {
    /// Conservative preview: orthogonal routing for forward edges only.
    pub fn preview() -> Self {
        Self {
            backward_fallback_to_hints: true,
            grid_snap: None,
        }
    }
}

/// Route all edges using float-first orthogonal routing.
pub fn route_edges_orthogonal(
    diagram: &Graph,
    geometry: &GraphGeometry,
    options: OrthogonalRoutingOptions,
) -> Vec<RoutedEdgeGeometry> {
    let fan_in_target_conflict =
        fan::fan_in_target_overflow_context(geometry, geometry.direction, diagram.edges.len());
    let fan_out_source_stagger =
        fan::fan_out_source_stagger_context(geometry, geometry.direction, diagram.edges.len());
    let override_nodes = build_override_node_map(diagram);
    let mut routed: Vec<RoutedEdgeGeometry> = geometry
        .edges
        .iter()
        .map(|edge| {
            let is_backward = geometry.reversed_edges.contains(&edge.index);
            let edge_direction = orthogonal_edge_direction(
                diagram,
                &geometry.node_directions,
                &override_nodes,
                &edge.from,
                &edge.to,
                geometry.direction,
            );
            let route_direction = if is_backward
                && options.backward_fallback_to_hints
                && edge_direction == geometry.direction
            {
                geometry.direction
            } else {
                edge_direction
            };
            let overflow_policy_target_face = fan_in_target_conflict
                .target_face_for_edge
                .get(&edge.index)
                .copied();
            let overflow_policy_target_fraction = fan_in_target_conflict
                .target_fraction_for_edge
                .get(&edge.index)
                .copied();
            let target_primary_channel_depth = fan_in_target_conflict
                .target_primary_channel_depth_for_edge
                .get(&edge.index)
                .copied();
            let source_primary_channel_depth = fan_out_source_stagger
                .source_primary_channel_depth_for_edge
                .get(&edge.index)
                .copied();
            let source_primary_face_fraction = fan_out_source_stagger
                .source_fraction_for_edge
                .get(&edge.index)
                .copied();
            let target_overflowed = fan_in_target_conflict.overflow_targeted.contains(&edge.to);
            let target_has_backward_conflict = fan_in_target_conflict
                .targets_with_backward_inbound
                .contains(&edge.to);
            let rank_span = fan::edge_rank_span(geometry, edge).unwrap_or(0);
            let mut path = build_orthogonal_path(
                edge,
                geometry,
                route_direction,
                is_backward,
                overflow_policy_target_face,
                overflow_policy_target_fraction,
                target_primary_channel_depth,
                source_primary_channel_depth,
                source_primary_face_fraction,
                target_overflowed,
                target_has_backward_conflict,
                rank_span,
            );

            // Offset backward edge source port from the forward arrival port
            // so they don't share the same position on the primary flow face.
            if is_backward && geometry.enhanced_backward_routing {
                offset_backward_source_from_primary_face(
                    &mut path,
                    edge,
                    geometry,
                    route_direction,
                );
            }
            // Re-project backward edge endpoints to diamond/hexagon boundaries.
            // Must run after offset_backward_source_from_primary_face because
            // the source offset can shift the target endpoint (when source and
            // target share the same x/y on a 2-point path), invalidating the
            // shape projection done inside build_orthogonal_path.
            if is_backward {
                snap_backward_endpoints_to_shape(&mut path, edge, geometry);
            }
            if let Some((sx, sy)) = options.grid_snap {
                path = snap_path_to_grid(&path, sx, sy);
            }
            // Skip revalidation for labels with intentional side offsets
            // (Above/Below have thickness-based offsets that would exceed the drift threshold)
            let label_position = if edge
                .label_side
                .is_some_and(|s| s != crate::graph::geometry::EdgeLabelSide::Center)
            {
                edge.label_position
            } else {
                revalidate_label_anchor(edge.label_position, &path)
            };

            let (head_label_position, tail_label_position) =
                compute_end_labels_for_edge(diagram, edge.index, &path);
            RoutedEdgeGeometry {
                index: edge.index,
                from: edge.from.clone(),
                to: edge.to.clone(),
                path,
                label_position,
                label_side: edge.label_side,
                head_label_position,
                tail_label_position,
                is_backward,
                from_subgraph: edge.from_subgraph.clone(),
                to_subgraph: edge.to_subgraph.clone(),
                source_port: None,
                target_port: None,
                preserve_orthogonal_topology: false,
            }
        })
        .collect();

    overlap::resolve_forward_td_bt_criss_cross_overlaps(diagram, geometry, &mut routed);
    routed
}

fn orthogonal_edge_direction(
    diagram: &Graph,
    node_directions: &HashMap<String, Direction>,
    override_nodes: &HashMap<String, String>,
    from: &str,
    to: &str,
    fallback: Direction,
) -> Direction {
    let from_sg = override_nodes.get(from);
    let to_sg = override_nodes.get(to);

    match (from_sg, to_sg) {
        (None, None) => effective_edge_direction(node_directions, from, to, fallback),
        (Some(sg_a), Some(sg_b)) if sg_a == sg_b => diagram
            .subgraphs
            .get(sg_a.as_str())
            .and_then(|sg| sg.dir)
            .unwrap_or_else(|| effective_edge_direction(node_directions, from, to, fallback)),
        _ => cross_boundary_edge_direction(
            diagram,
            node_directions,
            from_sg,
            to_sg,
            from,
            to,
            fallback,
        ),
    }
}

// Primary knob for TD/BT fan lane compaction near shared faces.
// Increase for longer endpoint stems and tighter shared lanes;
// decrease for wider lane spread.

#[allow(clippy::too_many_arguments)]
fn build_orthogonal_path(
    edge: &crate::graph::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
    overflow_policy_target_face: Option<Face>,
    overflow_policy_target_fraction: Option<f64>,
    target_primary_channel_depth: Option<f64>,
    source_primary_channel_depth: Option<f64>,
    source_primary_face_fraction: Option<f64>,
    target_overflowed: bool,
    target_has_backward_conflict: bool,
    rank_span: usize,
) -> Vec<FPoint> {
    let (backward_source_face_override, backward_target_face_override) =
        backward::backward_td_bt_face_overrides(
            edge,
            geometry,
            direction,
            is_backward,
            target_overflowed,
            rank_span,
        );
    let control_points = build_path_from_hints(edge, geometry);
    let mut path = build_contracted_path(&control_points, direction);
    anchor_path_endpoints_to_endpoint_faces(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
        overflow_policy_target_face,
        overflow_policy_target_fraction,
        source_primary_face_fraction,
        target_overflowed,
        target_has_backward_conflict,
    );
    forward::ensure_primary_stem_for_flat_off_center_fanout_sources(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    ensure_endpoint_segments_axis_aligned(&mut path);
    forward::ensure_primary_stem_for_flat_off_center_fanout_sources(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    forward::ensure_primary_stem_for_td_bt_angular_fanout_source(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    forward::collapse_source_turnback_spikes(&mut path);
    if !is_backward {
        enforce_primary_axis_terminal_direction(
            &mut path,
            direction,
            8.0,
            overflow_policy_target_face,
        );
    }
    let mut normalized = normalize_orthogonal_route_contracts(&path, direction);
    if is_backward {
        backward::ensure_backward_outer_lane_clearance(&mut normalized, direction, 12.0);
    }
    forward::collapse_source_turnback_spikes(&mut normalized);
    let base_finalized = normalize_orthogonal_route_contracts(&normalized, direction);
    let mut finalized = base_finalized.clone();
    if !is_backward {
        let stagger_depth = target_primary_channel_depth.or(source_primary_channel_depth);
        let pre_stagger = finalized.clone();
        fan::stagger_primary_face_shared_axis_segment(&mut finalized, direction, stagger_depth);
        // Use lighter normalization after stagger to avoid compact_terminal_staircase
        // collapsing the gathering column that stagger just created.
        if finalized != pre_stagger {
            finalized = light_normalize(&finalized);
            // Source-side fan-out staggering can create a temporary inward hook
            // (primary-axis reversal) on multi-bend forward paths in LR/RL.
            // Fall back to full normalization only for that case.
            if forward::has_forward_primary_axis_reversal(&finalized, direction) {
                finalized = normalize_orthogonal_route_contracts(&finalized, direction);
            }
        } else {
            finalized = normalize_orthogonal_route_contracts(&finalized, direction);
        }
    }
    if !is_backward {
        collapse_tiny_cross_axis_jog(&mut finalized, direction);
    }
    if !is_backward
        && let Some(policy_face) = overflow_policy_target_face
        && policy_face != flow_target_face_for_direction(direction)
        && endpoint_is_on_policy_face(
            &finalized,
            edge,
            geometry,
            map_face_to_rect_face(policy_face),
        )
    {
        enforce_terminal_support_normal_to_face(&mut finalized, policy_face, 8.0);
        collapse_collinear_interior_points(&mut finalized);
    }
    if !is_backward {
        forward::avoid_forward_td_bt_primary_lane_node_intrusion(
            &mut finalized,
            edge,
            geometry,
            direction,
            target_primary_channel_depth,
        );
        forward::prefer_secondary_axis_departure_for_angular_sources(
            &mut finalized,
            edge,
            geometry,
            direction,
        );
    }
    if !is_backward
        && forward::collapse_forward_source_primary_turnback_hooks(&mut finalized, direction)
    {
        finalized = light_normalize(&finalized);
    }
    if is_backward {
        backward::finalize_backward_path(
            &mut finalized,
            edge,
            geometry,
            direction,
            backward::BackwardFinalizeOptions {
                target_overflowed,
                source_face_override: backward_source_face_override,
                target_face_override: backward_target_face_override,
                base_finalized: &base_finalized,
            },
        );
    }
    let skip_or_backward_candidate = is_backward || rank_span >= 2;
    if skip_or_backward_candidate
        && backward::reroute_skip_backward_lane_for_node_clearance(
            &mut finalized,
            edge,
            geometry,
            direction,
            8.0,
            8.0,
            16.0,
        )
    {
        finalized = normalize_orthogonal_route_contracts(&finalized, direction);
        if backward::reroute_skip_backward_lane_for_node_clearance(
            &mut finalized,
            edge,
            geometry,
            direction,
            12.0,
            8.0,
            16.0,
        ) {
            finalized = normalize_orthogonal_route_contracts(&finalized, direction);
        }
    }
    finalized
}

/// Collapse tiny cross-axis jogs in forward orthogonal paths.
///
/// When the orthogonal path has a small gathering segment (4-point L-shape with
/// a cross-axis step smaller than the threshold), collapse it to a straight
/// 2-point path. This removes visually distracting micro-jogs that arise when
/// the layout engine produces nearly-collinear waypoints for edges between
/// adjacent nodes.
fn collapse_tiny_cross_axis_jog(path: &mut Vec<FPoint>, direction: Direction) {
    const EPS: f64 = 0.000_001;
    const MAX_JOG: f64 = 8.0;

    if path.len() != 4 {
        return;
    }

    // Identify the gathering segment (cross-axis interior segment) and check
    // whether the two bounding segments are primary-axis stems.
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    let (source_stem, gather_cross, target_stem) = if primary_vertical {
        // TD/BT: stems are vertical (same x), gathering is horizontal (same y).
        // The jog is the cross-axis (x) extent of the horizontal gathering segment.
        let s = (path[0].x - path[1].x).abs() <= EPS && (path[0].y - path[1].y).abs() > EPS;
        let g = (path[1].x - path[2].x).abs();
        let t = (path[2].x - path[3].x).abs() <= EPS && (path[2].y - path[3].y).abs() > EPS;
        (s, g, t)
    } else {
        // LR/RL: stems are horizontal (same y), gathering is vertical (same x).
        // The jog is the cross-axis (y) extent of the vertical gathering segment.
        let s = (path[0].y - path[1].y).abs() <= EPS && (path[0].x - path[1].x).abs() > EPS;
        let g = (path[1].y - path[2].y).abs();
        let t = (path[2].y - path[3].y).abs() <= EPS && (path[2].x - path[3].x).abs() > EPS;
        (s, g, t)
    };

    if source_stem && target_stem && gather_cross <= MAX_JOG {
        // Collapse to a straight line by averaging the cross-axis coordinates
        // of the source and target endpoints so the result is perfectly
        // axis-aligned (no diagonal).
        let mut start = path[0];
        let mut end = path[3];
        if primary_vertical {
            let mid_x = (start.x + end.x) / 2.0;
            start.x = mid_x;
            end.x = mid_x;
        } else {
            let mid_y = (start.y + end.y) / 2.0;
            start.y = mid_y;
            end.y = mid_y;
        }
        *path = vec![start, end];
    }
}

fn enforce_primary_axis_terminal_direction(
    points: &mut [FPoint],
    direction: Direction,
    min_terminal_support: f64,
    preferred_target_face: Option<Face>,
) {
    if points.len() < 2 || min_terminal_support <= 0.0 {
        return;
    }

    let n = points.len();
    let end_idx = n - 1;
    let penult_idx = n - 2;
    let flow_face = flow_target_face_for_direction(direction);
    let target_face = preferred_target_face.unwrap_or(flow_face);

    match target_face {
        Face::Top => {
            let target_penult_y = points[end_idx].y - min_terminal_support;
            if points[penult_idx].y > target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y > points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Face::Bottom => {
            let target_penult_y = points[end_idx].y + min_terminal_support;
            if points[penult_idx].y < target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y < points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Face::Left => {
            let target_penult_x = points[end_idx].x - min_terminal_support;
            if points[penult_idx].x > target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x > points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
        Face::Right => {
            let target_penult_x = points[end_idx].x + min_terminal_support;
            if points[penult_idx].x < target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x < points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
    }
}
