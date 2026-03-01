//! Float-first orthogonal routing preview helpers.
//!
//! This module routes edges in float space first, then optionally applies a
//! deterministic grid snap adapter for text-oriented consumption.

use std::collections::{HashMap, HashSet};

use super::backward_policy::{can_apply_td_bt_backward_hint_parity, prefer_backward_side_channel};
use super::route_policy::{build_override_node_map, effective_edge_direction};
use super::text_routing_core::{
    Face, OverflowSide, build_orthogonal_path_float, canonical_backward_channel_face,
    fan_in_overflow_face_for_slot, fan_in_primary_face_capacity, fan_in_primary_target_face,
    intersect_shape_boundary_float, normalize_orthogonal_route_contracts,
    resolve_overflow_backward_channel_conflict,
};
use crate::diagrams::flowchart::geometry::{
    EngineHints, FPoint, FRect, GraphGeometry, RoutedEdgeGeometry,
};
use crate::graph::{Diagram, Direction, Shape};

/// Preview options for orthogonal float-first routing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OrthogonalRoutingOptions {
    /// Keep existing behavior for backward edges while previewing forward routing.
    pub backward_fallback_to_hints: bool,
    /// Optional grid snap `(scale_x, scale_y)` applied after routing.
    pub grid_snap: Option<(f64, f64)>,
}

impl OrthogonalRoutingOptions {
    /// Conservative preview: orthogonal routing for forward edges only.
    pub(crate) fn preview() -> Self {
        Self {
            backward_fallback_to_hints: true,
            grid_snap: None,
        }
    }
}

/// Route all edges using float-first orthogonal routing.
pub(crate) fn route_edges_orthogonal(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    options: OrthogonalRoutingOptions,
) -> Vec<RoutedEdgeGeometry> {
    let fan_in_target_conflict =
        fan_in_target_overflow_context(geometry, geometry.direction, diagram.edges.len());
    let fan_out_source_stagger =
        fan_out_source_stagger_context(geometry, geometry.direction, diagram.edges.len());
    let override_nodes = build_override_node_map(diagram);
    geometry
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
            let rank_span = edge_rank_span(geometry, edge).unwrap_or(0);
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
                .is_some_and(|s| s != crate::layered::normalize::LabelSide::Center)
            {
                edge.label_position
            } else {
                revalidate_label_anchor(edge.label_position, &path)
            };

            let (head_label_position, tail_label_position) =
                crate::diagrams::flowchart::routing::compute_end_labels_for_edge(
                    diagram, edge.index, &path,
                );
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
            }
        })
        .collect()
}

fn orthogonal_edge_direction(
    diagram: &Diagram,
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

fn cross_boundary_edge_direction(
    diagram: &Diagram,
    node_directions: &HashMap<String, Direction>,
    from_sg: Option<&String>,
    to_sg: Option<&String>,
    from_node: &str,
    to_node: &str,
    fallback: Direction,
) -> Direction {
    if let (Some(sg_a), Some(sg_b)) = (from_sg, to_sg) {
        if is_ancestor_sg(diagram, sg_a, sg_b) {
            return diagram
                .subgraphs
                .get(sg_a.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        if is_ancestor_sg(diagram, sg_b, sg_a) {
            return diagram
                .subgraphs
                .get(sg_b.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        return fallback;
    }

    let outside_node = if from_sg.is_some() && to_sg.is_none() {
        to_node
    } else {
        from_node
    };

    node_directions
        .get(outside_node)
        .copied()
        .unwrap_or(fallback)
}

fn is_ancestor_sg(diagram: &Diagram, ancestor: &str, descendant: &str) -> bool {
    let mut current = descendant;
    while let Some(parent) = diagram
        .subgraphs
        .get(current)
        .and_then(|sg| sg.parent.as_deref())
    {
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

const LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE: f64 = 2.0;
const POINT_EPS: f64 = 0.000_001;
const MIN_PORT_CORNER_INSET_FORWARD: f64 = 8.0;
const MIN_PORT_CORNER_INSET_BACKWARD: f64 = 12.0;
const MIN_FAN_IN_PRIMARY_SLOT_SPACING: f64 = 16.0;
// Primary knob for TD/BT fan lane compaction near shared faces.
// Increase for longer endpoint stems and tighter shared lanes;
// decrease for wider lane spread.
const FAN_PRIMARY_SIDE_BAND_DEPTH_MARGIN: f64 = 0.1;

/// Lightweight normalization: dedup + remove collinear, without
/// `compact_terminal_staircase` which can collapse gathering columns.
fn light_normalize(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 1 {
        return points.to_vec();
    }
    let mut result: Vec<FPoint> = Vec::with_capacity(points.len());
    // Dedup adjacent
    for &p in points {
        let dominated = result.last().is_some_and(|prev: &FPoint| {
            (prev.x - p.x).abs() <= POINT_EPS && (prev.y - p.y).abs() <= POINT_EPS
        });
        if !dominated {
            result.push(p);
        }
    }
    // Remove collinear interior points
    if result.len() <= 2 {
        return result;
    }
    let mut compacted = Vec::with_capacity(result.len());
    compacted.push(result[0]);
    for idx in 1..result.len() - 1 {
        let prev = *compacted.last().unwrap();
        let curr = result[idx];
        let next = result[idx + 1];
        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = (dx1 * dy2 - dy1 * dx2).abs();
        if cross > POINT_EPS {
            compacted.push(curr);
        }
    }
    compacted.push(*result.last().unwrap());
    compacted
}

fn clamp_face_coordinate_with_corner_inset(value: f64, min: f64, max: f64, max_inset: f64) -> f64 {
    let lo = min.min(max);
    let hi = min.max(max);
    let span = hi - lo;
    if span <= POINT_EPS {
        (lo + hi) / 2.0
    } else {
        // Scale inset with face span so text-sized nodes keep usable side lanes,
        // while SVG-sized nodes still enforce a visibly distinct stem offset.
        let inset = (span * 0.2).clamp(1.0, max_inset);
        if span <= inset * 2.0 {
            (lo + hi) / 2.0
        } else {
            value.clamp(lo + inset, hi - inset)
        }
    }
}

fn revalidate_label_anchor(label_position: Option<FPoint>, path: &[FPoint]) -> Option<FPoint> {
    let Some(anchor) = label_position else {
        return route_derived_label_anchor(path);
    };
    let drift = distance_point_to_path(anchor, path);
    if drift <= LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE {
        return Some(anchor);
    }
    route_derived_label_anchor(path).or(Some(anchor))
}

fn route_derived_label_anchor(path: &[FPoint]) -> Option<FPoint> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return path.first().copied();
    }

    let total_len: f64 = path
        .windows(2)
        .map(|segment| point_distance(segment[0], segment[1]))
        .sum();
    if total_len <= POINT_EPS {
        return path.get(path.len() / 2).copied();
    }

    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let seg_len = point_distance(a, b);
        if seg_len <= POINT_EPS {
            continue;
        }
        if traversed + seg_len >= target {
            let t = (target - traversed) / seg_len;
            return Some(FPoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
        traversed += seg_len;
    }
    path.last().copied()
}

fn point_distance(a: FPoint, b: FPoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn distance_point_to_segment(point: FPoint, a: FPoint, b: FPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= POINT_EPS {
        return point_distance(point, a);
    }
    let projection = ((point.x - a.x) * dx + (point.y - a.y) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = FPoint::new(a.x + t * dx, a.y + t * dy);
    point_distance(point, closest)
}

fn distance_point_to_path(point: FPoint, path: &[FPoint]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return point_distance(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

#[allow(clippy::too_many_arguments)]
fn build_orthogonal_path(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
        backward_td_bt_face_overrides(
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
    ensure_primary_stem_for_flat_off_center_fanout_sources(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    ensure_endpoint_segments_axis_aligned(&mut path);
    ensure_primary_stem_for_flat_off_center_fanout_sources(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    ensure_primary_stem_for_td_bt_angular_fanout_source(
        &mut path,
        edge,
        geometry,
        direction,
        is_backward,
    );
    collapse_source_turnback_spikes(&mut path);
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
        ensure_backward_outer_lane_clearance(&mut normalized, direction, 12.0);
    }
    collapse_source_turnback_spikes(&mut normalized);
    let base_finalized = normalize_orthogonal_route_contracts(&normalized, direction);
    let mut finalized = base_finalized.clone();
    if !is_backward {
        let stagger_depth = target_primary_channel_depth.or(source_primary_channel_depth);
        let pre_stagger = finalized.clone();
        stagger_primary_face_shared_axis_segment(&mut finalized, direction, stagger_depth);
        // Use lighter normalization after stagger to avoid compact_terminal_staircase
        // collapsing the gathering column that stagger just created.
        if finalized != pre_stagger {
            finalized = light_normalize(&finalized);
            // Source-side fan-out staggering can create a temporary inward hook
            // (primary-axis reversal) on multi-bend forward paths in LR/RL.
            // Fall back to full normalization only for that case.
            if has_forward_primary_axis_reversal(&finalized, direction) {
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
        avoid_forward_td_bt_primary_lane_node_intrusion(
            &mut finalized,
            edge,
            geometry,
            direction,
            target_primary_channel_depth,
        );
        prefer_secondary_axis_departure_for_angular_sources(
            &mut finalized,
            edge,
            geometry,
            direction,
        );
    }
    if !is_backward && collapse_forward_source_primary_turnback_hooks(&mut finalized, direction) {
        finalized = light_normalize(&finalized);
    }
    if is_backward {
        let mut compact_short_backward = false;
        // For backward edges with corridor obstructions (intermediate nodes
        // between source and target), construct a clean channel path from
        // scratch rather than trying to fix the layout-hint-derived path.
        // This avoids node-border overlaps that the post-processing pipeline
        // cannot reliably fix for complex path shapes (e.g. 6-point V-H-V-H-V).
        let use_channel_path = geometry.enhanced_backward_routing
            && has_backward_corridor_obstructions(edge, geometry, direction);

        if use_channel_path
            && let Some(channel_path) =
                build_backward_orthogonal_channel_path(edge, geometry, direction)
        {
            finalized = channel_path;
        }

        if !use_channel_path {
            let use_compact_side_lane =
                matches!(direction, Direction::LeftRight | Direction::RightLeft)
                    && direction != geometry.direction;
            if use_compact_side_lane
                && let Some(compact_path) =
                    build_short_backward_side_lane_path(edge, geometry, direction)
            {
                finalized = compact_path;
                compact_short_backward = true;
            } else {
                enforce_backward_source_tangent_direction(
                    &mut finalized,
                    edge,
                    geometry,
                    direction,
                    backward_source_face_override,
                );
                ensure_backward_outer_lane_clearance(&mut finalized, direction, 12.0);
                align_backward_source_stem_to_outer_lane(&mut finalized, edge, geometry, direction);
                enforce_backward_terminal_tangent_direction(
                    &mut finalized,
                    edge,
                    geometry,
                    direction,
                    target_overflowed,
                    backward_target_face_override,
                );
                let parity_override_active = backward_source_face_override.is_some()
                    || backward_target_face_override.is_some();
                if parity_override_active {
                    finalized = normalize_orthogonal_route_contracts(&finalized, direction);
                }
                if parity_override_active && has_immediate_axial_turnback(&finalized) {
                    finalized = base_finalized;
                    enforce_backward_source_tangent_direction(
                        &mut finalized,
                        edge,
                        geometry,
                        direction,
                        None,
                    );
                    ensure_backward_outer_lane_clearance(&mut finalized, direction, 12.0);
                    align_backward_source_stem_to_outer_lane(
                        &mut finalized,
                        edge,
                        geometry,
                        direction,
                    );
                    enforce_backward_terminal_tangent_direction(
                        &mut finalized,
                        edge,
                        geometry,
                        direction,
                        target_overflowed,
                        None,
                    );
                }
                collapse_tiny_backward_terminal_staircase(&mut finalized, direction, 8.0);
                align_backward_outer_lane_to_hint(
                    &mut finalized,
                    edge.layout_path_hint.as_deref(),
                    direction,
                    edge,
                    geometry,
                );
                collapse_tiny_backward_terminal_staircase(&mut finalized, direction, 8.0);
                enforce_backward_minimum_channel_floor(
                    &mut finalized,
                    edge,
                    geometry,
                    direction,
                    12.0,
                );
                avoid_backward_td_bt_vertical_lane_node_intrusion(
                    &mut finalized,
                    edge,
                    geometry,
                    direction,
                );
                collapse_backward_terminal_node_intrusion(
                    &mut finalized,
                    edge,
                    geometry,
                    direction,
                );
            }
        }
        if !compact_short_backward {
            enforce_backward_terminal_corner_inset(&mut finalized, edge, geometry);
        }
        collapse_collinear_interior_points(&mut finalized);
        fix_backward_diagonal_node_collision(&mut finalized, edge, geometry, direction);
    }
    let skip_or_backward_candidate = is_backward || rank_span >= 2;
    if skip_or_backward_candidate
        && reroute_skip_backward_lane_for_node_clearance(
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
        if reroute_skip_backward_lane_for_node_clearance(
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

fn avoid_forward_td_bt_primary_lane_node_intrusion(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    target_primary_channel_depth: Option<f64>,
) {
    const EPS: f64 = 0.000_001;
    // Treat near-border segments as intrusions too so rendered strokes do not
    // visually overlap unrelated node borders after anti-aliasing.
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
        let first_crosses =
            axis_aligned_segment_crosses_rect_interior(p0, p1, rect, INTRUSION_MARGIN);
        let middle_crosses =
            axis_aligned_segment_crosses_rect_interior(p1, p2, rect, INTRUSION_MARGIN);
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

fn reroute_skip_backward_lane_for_node_clearance(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn axis_aligned_segment_crosses_rect_interior(
    a: FPoint,
    b: FPoint,
    rect: FRect,
    margin: f64,
) -> bool {
    let left = rect.x + margin;
    let right = rect.x + rect.width - margin;
    let top = rect.y + margin;
    let bottom = rect.y + rect.height - margin;
    if left >= right || top >= bottom {
        return false;
    }

    if (a.y - b.y).abs() <= POINT_EPS {
        let seg_y = a.y;
        if seg_y <= top || seg_y >= bottom {
            return false;
        }
        let seg_min_x = a.x.min(b.x);
        let seg_max_x = a.x.max(b.x);
        return seg_max_x > left && seg_min_x < right;
    }

    if (a.x - b.x).abs() <= POINT_EPS {
        let seg_x = a.x;
        if seg_x <= left || seg_x >= right {
            return false;
        }
        let seg_min_y = a.y.min(b.y);
        let seg_max_y = a.y.max(b.y);
        return seg_max_y > top && seg_min_y < bottom;
    }

    false
}

fn reroute_forward_td_bt_terminal_intrusion_with_safe_vertical_corridor(
    path: &[FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<Vec<FPoint>> {
    const MIN_TARGET_STEM: f64 = 16.0;
    const NODE_CLEARANCE: f64 = 8.0;
    // Include near-border grazing so terminal stems don't visually ride along
    // unrelated node borders after rasterization/anti-aliasing.
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

    if !segment_crosses_any_other_node_interior(edge, geometry, p2, p3, INTRUSION_MARGIN) {
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
        if !ranges_overlap(y_min, y_max, rect.y, rect.y + rect.height) {
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
            !segment_crosses_any_other_node_interior(
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

fn avoid_backward_td_bt_vertical_lane_node_intrusion(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

/// Check whether a backward edge has intermediate nodes in its routing corridor.
///
/// When nodes exist between source and target (horizontally overlapping the
/// corridor), the backward edge needs a full channel detour to avoid crossing
/// them. Otherwise, a simple port-offset suffices.
fn has_backward_corridor_obstructions(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

/// Build a clean orthogonal channel path for a backward edge with corridor
/// obstructions.
///
/// Instead of trying to fix the layout-hint-derived path (which overlaps
/// intermediate nodes), this constructs a 4-point right-angle path from scratch
/// using the canonical backward face:
///
/// **TD/BT:** source right face → channel lane → target right face
/// **LR/RL:** source bottom face → channel lane → target bottom face
///
/// This matches the non-orthogonal `build_backward_channel_path` approach and
/// is already axis-aligned, so it works directly for step/smooth-step/curved-step rendering.
fn build_backward_orthogonal_channel_path(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<Vec<FPoint>> {
    const CHANNEL_CLEARANCE: f64 = 12.0;

    let sr = geometry.nodes.get(&edge.from)?.rect;
    let tr = geometry.nodes.get(&edge.to)?.rect;

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            // Exit source from right face, enter target from right face.
            let source_right = sr.x + sr.width;
            let target_right = tr.x + tr.width;
            let source_cy = sr.center_y();
            let target_cy = tr.center_y();

            // Channel lane: to the right of all nodes between the source/target
            // rank span so backward returns stay outside forward-flow diagonals.
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
            // Exit source from bottom face, enter target from bottom face.
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

/// Build a compact short backward path for LR/RL override-direction edges.
///
/// For short reciprocal edges inside LR/RL override subgraphs, the canonical
/// bottom-channel policy can produce visual vertical stems that look like
/// incorrect attachments in step/smooth-step/curved-step. This compact mode keeps backward
/// endpoints on side faces (source leading-side, target trailing-side) and
/// routes along a lower lane, matching direct/polyline intent.
fn build_short_backward_side_lane_path(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
    if segment_crosses_any_other_node_interior(
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

fn prefer_secondary_axis_departure_for_angular_sources(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    let segments_clear =
        !segment_crosses_any_other_node_interior(edge, geometry, start, elbow, INTRUSION_MARGIN)
            && !segment_crosses_any_other_node_interior(
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

fn segment_crosses_any_other_node_interior(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    a: FPoint,
    b: FPoint,
    margin: f64,
) -> bool {
    geometry.nodes.iter().any(|(node_id, node)| {
        if node_id == &edge.from || node_id == &edge.to {
            return false;
        }
        axis_aligned_segment_crosses_rect_interior(a, b, node.rect, margin)
    })
}

fn ranges_overlap(a_min: f64, a_max: f64, b_min: f64, b_max: f64) -> bool {
    let low = a_min.max(b_min);
    let high = a_max.min(b_max);
    high > low + POINT_EPS
}

fn backward_td_bt_face_overrides(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    // Layout hint points for backward edges come from the reversed-edge layout,
    // so the source hint may sit on the forward-direction departure face
    // (Bottom for TD, Top for BT). Flip the source face when it matches the
    // forward departure face, since a backward edge must depart toward the
    // target (upward in TD, downward in BT).
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

    // Skip parity when the source node's center is entirely to the right of
    // the target's right edge. In that topology the forward target→source
    // edge runs rightward through the space between the two nodes; the
    // backward path's leftward approach to the target's top/bottom face would
    // have to cross that forward edge. Fall back to canonical side-channel
    // routing to avoid the crossing.
    // Note: when the source center is still within the target's x-span, any
    // parity approach is nearly vertical and does not cross the forward edge.
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

#[derive(Default)]
struct FanInTargetOverflowContext {
    target_face_for_edge: HashMap<usize, Face>,
    target_fraction_for_edge: HashMap<usize, f64>,
    target_primary_channel_depth_for_edge: HashMap<usize, f64>,
    overflow_targeted: HashSet<String>,
    targets_with_backward_inbound: HashSet<String>,
}

#[derive(Default)]
struct FanOutSourceStaggerContext {
    source_primary_channel_depth_for_edge: HashMap<usize, f64>,
    source_fraction_for_edge: HashMap<usize, f64>,
}

fn fan_in_target_overflow_context(
    geometry: &GraphGeometry,
    direction: Direction,
    visible_edge_count: usize,
) -> FanInTargetOverflowContext {
    let mut incoming_by_target: HashMap<
        String,
        Vec<&crate::diagrams::flowchart::geometry::LayoutEdge>,
    > = HashMap::new();
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| edge.index < visible_edge_count)
    {
        incoming_by_target
            .entry(edge.to.clone())
            .or_default()
            .push(edge);
    }

    let primary_face = fan_in_primary_target_face(direction);
    let mut target_face_for_edge: HashMap<usize, Face> = HashMap::new();
    let mut target_fraction_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut target_primary_channel_depth_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut overflow_targeted: HashSet<String> = HashSet::new();
    let mut targets_with_backward_inbound: HashSet<String> = HashSet::new();
    const CENTER_EPS: f64 = 0.5;

    for (target_id, mut incoming_edges) in incoming_by_target {
        incoming_edges.sort_unstable_by_key(|edge| edge.index);
        let mut forward_edges: Vec<&crate::diagrams::flowchart::geometry::LayoutEdge> = Vec::new();
        let mut backward_edge_count = 0usize;
        for edge in incoming_edges {
            if geometry.reversed_edges.contains(&edge.index) {
                backward_edge_count += 1;
            } else {
                forward_edges.push(edge);
            }
        }

        if backward_edge_count > 0 {
            targets_with_backward_inbound.insert(target_id.clone());
        }

        if forward_edges.len() <= 1 {
            continue;
        }

        let target_rect_and_shape = forward_edges.first().and_then(|edge| {
            endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
        });
        let target_rect = target_rect_and_shape.map(|(rect, _)| rect);
        let target_is_angular = target_rect_and_shape
            .is_some_and(|(_, shape)| matches!(shape, Shape::Diamond | Shape::Hexagon));
        let capacity = target_rect
            .as_ref()
            .map(|rect| adaptive_fan_in_primary_face_capacity(direction, rect))
            .unwrap_or_else(|| fan_in_primary_face_capacity(direction));

        forward_edges.sort_by(|a, b| {
            let a_cross = fan_in_source_cross_axis(geometry, a, direction);
            let b_cross = fan_in_source_cross_axis(geometry, b, direction);
            a_cross
                .total_cmp(&b_cross)
                .then_with(|| a.index.cmp(&b.index))
        });

        let primary_count = forward_edges.len().min(capacity);
        for edge in &forward_edges[..primary_count] {
            target_face_for_edge.insert(edge.index, primary_face);
        }

        if forward_edges.len() > capacity {
            overflow_targeted.insert(target_id);
            let overflow_edges = &forward_edges[capacity..];
            let target_cross = overflow_edges
                .first()
                .and_then(|edge| endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref()))
                .map(|rect| face_cross_axis(rect, direction))
                .unwrap_or(0.0);
            for (idx, edge) in overflow_edges.iter().enumerate() {
                let source_cross = fan_in_source_cross_axis(geometry, edge, direction);
                let overflow_slot = if source_cross < target_cross - CENTER_EPS {
                    OverflowSide::LeftOrTop
                } else if source_cross > target_cross + CENTER_EPS {
                    OverflowSide::RightOrBottom
                } else if idx % 2 == 0 {
                    OverflowSide::LeftOrTop
                } else {
                    OverflowSide::RightOrBottom
                };
                let face = fan_in_overflow_face_for_slot(direction, overflow_slot);
                target_face_for_edge.insert(edge.index, face);
            }
        }

        let mut edges_by_face: HashMap<Face, Vec<(usize, f64)>> = HashMap::new();
        for edge in &forward_edges {
            let Some(face) = target_face_for_edge.get(&edge.index).copied() else {
                continue;
            };
            let source_cross = fan_in_source_cross_axis(geometry, edge, direction);
            edges_by_face
                .entry(face)
                .or_default()
                .push((edge.index, source_cross));
        }

        for (face, mut face_edges) in edges_by_face {
            face_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            let count = face_edges.len();
            for (idx, (edge_index, _)) in face_edges.iter().enumerate() {
                let base_fraction = if count <= 1 {
                    0.5
                } else {
                    idx as f64 / (count - 1) as f64
                };
                let fraction = if target_is_angular
                    && face == primary_face
                    && matches!(direction, Direction::TopDown | Direction::BottomTop)
                {
                    remap_angular_fan_in_target_fraction(base_fraction, count)
                } else {
                    base_fraction
                };
                target_fraction_for_edge.insert(*edge_index, fraction);
            }
            if count > 1 {
                if face == primary_face {
                    let target_cross = target_rect
                        .as_ref()
                        .map(|rect| face_cross_axis(rect, direction))
                        .unwrap_or_else(|| {
                            if count % 2 == 1 {
                                face_edges[count / 2].1
                            } else {
                                (face_edges[count / 2 - 1].1 + face_edges[count / 2].1) / 2.0
                            }
                        });

                    let mut left_edges: Vec<(usize, f64)> = Vec::new();
                    let mut right_edges: Vec<(usize, f64)> = Vec::new();
                    let mut center_edges: Vec<(usize, f64)> = Vec::new();
                    for (edge_index, source_cross) in &face_edges {
                        if *source_cross < target_cross - CENTER_EPS {
                            left_edges.push((*edge_index, *source_cross));
                        } else if *source_cross > target_cross + CENTER_EPS {
                            right_edges.push((*edge_index, *source_cross));
                        } else {
                            center_edges.push((*edge_index, *source_cross));
                        }
                    }

                    left_edges.sort_by(|a, b| {
                        (target_cross - a.1)
                            .total_cmp(&(target_cross - b.1))
                            .then_with(|| a.0.cmp(&b.0))
                    });
                    right_edges.sort_by(|a, b| {
                        (a.1 - target_cross)
                            .total_cmp(&(b.1 - target_cross))
                            .then_with(|| a.0.cmp(&b.0))
                    });
                    center_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

                    let band_count = left_edges.len().max(right_edges.len());
                    for (band_index, (edge_index, _)) in left_edges.into_iter().enumerate() {
                        target_primary_channel_depth_for_edge.insert(
                            edge_index,
                            symmetric_side_band_depth(band_index, band_count),
                        );
                    }
                    for (band_index, (edge_index, _)) in right_edges.into_iter().enumerate() {
                        target_primary_channel_depth_for_edge.insert(
                            edge_index,
                            symmetric_side_band_depth(band_index, band_count),
                        );
                    }

                    if center_edges.len() == 1 {
                        target_primary_channel_depth_for_edge.insert(center_edges[0].0, 0.5);
                    } else if center_edges.len() > 1 {
                        let denom = center_edges.len() as f64 + 1.0;
                        for (idx, (edge_index, _)) in center_edges.into_iter().enumerate() {
                            target_primary_channel_depth_for_edge
                                .insert(edge_index, (idx as f64 + 1.0) / denom);
                        }
                    }
                } else {
                    // When overflow moves arrivals to cross-faces, spread their
                    // shared-axis channel depth so lanes do not collapse.
                    for (idx, (edge_index, _)) in face_edges.iter().enumerate() {
                        let depth = idx as f64 / (count - 1) as f64;
                        target_primary_channel_depth_for_edge.insert(*edge_index, depth);
                    }
                }
            }
        }

        if let Some(target_rect) = target_rect {
            apply_near_aligned_primary_face_fraction_override(
                geometry,
                direction,
                primary_face,
                &target_rect,
                &forward_edges,
                &target_face_for_edge,
                &mut target_fraction_for_edge,
            );
        }
    }

    FanInTargetOverflowContext {
        target_face_for_edge,
        target_fraction_for_edge,
        target_primary_channel_depth_for_edge,
        overflow_targeted,
        targets_with_backward_inbound,
    }
}

fn fan_out_source_stagger_context(
    geometry: &GraphGeometry,
    direction: Direction,
    visible_edge_count: usize,
) -> FanOutSourceStaggerContext {
    let mut outgoing_by_source: HashMap<
        String,
        Vec<&crate::diagrams::flowchart::geometry::LayoutEdge>,
    > = HashMap::new();
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| edge.index < visible_edge_count)
    {
        outgoing_by_source
            .entry(edge.from.clone())
            .or_default()
            .push(edge);
    }

    let mut source_primary_channel_depth_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut source_fraction_for_edge: HashMap<usize, f64> = HashMap::new();
    const CENTER_EPS: f64 = 0.5;

    for (source_id, mut outgoing_edges) in outgoing_by_source {
        outgoing_edges.sort_unstable_by_key(|edge| edge.index);
        let mut forward_edges: Vec<&crate::diagrams::flowchart::geometry::LayoutEdge> = Vec::new();
        for edge in outgoing_edges {
            if geometry.reversed_edges.contains(&edge.index) {
                continue;
            }
            forward_edges.push(edge);
        }
        if forward_edges.len() <= 1 {
            continue;
        }

        let source_cross = forward_edges
            .first()
            .and_then(|edge| endpoint_rect(geometry, &source_id, edge.from_subgraph.as_deref()))
            .map(|rect| face_cross_axis(rect, direction))
            .unwrap_or(0.0);

        let mut ordered_for_fraction: Vec<(usize, f64)> = forward_edges
            .iter()
            .map(|edge| {
                (
                    edge.index,
                    fan_out_target_cross_axis(geometry, edge, direction),
                )
            })
            .collect();
        ordered_for_fraction.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let angular_source = forward_edges
            .first()
            .and_then(|edge| {
                endpoint_rect_and_shape(geometry, &source_id, edge.from_subgraph.as_deref())
            })
            .is_some_and(|(_, shape)| matches!(shape, Shape::Diamond | Shape::Hexagon));
        let count = ordered_for_fraction.len();
        for (idx, (edge_index, _)) in ordered_for_fraction.iter().enumerate() {
            let base_fraction = if count <= 1 {
                0.5
            } else {
                idx as f64 / (count - 1) as f64
            };
            let fraction = if angular_source
                && matches!(direction, Direction::TopDown | Direction::BottomTop)
            {
                remap_angular_fan_out_source_fraction(base_fraction, count)
            } else {
                base_fraction
            };
            source_fraction_for_edge.insert(*edge_index, fraction);
        }

        let mut left_edges: Vec<(usize, f64)> = Vec::new();
        let mut right_edges: Vec<(usize, f64)> = Vec::new();
        let mut center_edges: Vec<(usize, f64)> = Vec::new();
        for edge in &forward_edges {
            let target_cross = fan_out_target_cross_axis(geometry, edge, direction);
            if target_cross < source_cross - CENTER_EPS {
                left_edges.push((edge.index, target_cross));
            } else if target_cross > source_cross + CENTER_EPS {
                right_edges.push((edge.index, target_cross));
            } else {
                center_edges.push((edge.index, target_cross));
            }
        }

        // Fan-out is source-centric: make outer branches shallower and inner
        // branches deeper so the bundle opens outward from the source.
        left_edges.sort_by(|a, b| {
            (source_cross - b.1)
                .total_cmp(&(source_cross - a.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        right_edges.sort_by(|a, b| {
            (b.1 - source_cross)
                .total_cmp(&(a.1 - source_cross))
                .then_with(|| a.0.cmp(&b.0))
        });
        center_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

        let band_count = left_edges.len().max(right_edges.len());
        for (band_index, (edge_index, _)) in left_edges.into_iter().enumerate() {
            source_primary_channel_depth_for_edge.insert(
                edge_index,
                symmetric_side_band_depth(band_index, band_count),
            );
        }
        for (band_index, (edge_index, _)) in right_edges.into_iter().enumerate() {
            source_primary_channel_depth_for_edge.insert(
                edge_index,
                symmetric_side_band_depth(band_index, band_count),
            );
        }

        if center_edges.len() == 1 {
            source_primary_channel_depth_for_edge.insert(center_edges[0].0, 0.5);
        } else if center_edges.len() > 1 {
            let denom = center_edges.len() as f64 + 1.0;
            for (idx, (edge_index, _)) in center_edges.into_iter().enumerate() {
                source_primary_channel_depth_for_edge
                    .insert(edge_index, (idx as f64 + 1.0) / denom);
            }
        }
    }

    FanOutSourceStaggerContext {
        source_primary_channel_depth_for_edge,
        source_fraction_for_edge,
    }
}

fn adaptive_fan_in_primary_face_capacity(direction: Direction, target_rect: &FRect) -> usize {
    let baseline_capacity = fan_in_primary_face_capacity(direction);
    let face_span = match direction {
        Direction::TopDown | Direction::BottomTop => target_rect.width.abs(),
        Direction::LeftRight | Direction::RightLeft => target_rect.height.abs(),
    };
    let usable_span = (face_span - 2.0 * MIN_PORT_CORNER_INSET_FORWARD).max(0.0);
    let dynamic_capacity = if usable_span <= f64::EPSILON {
        1
    } else {
        (usable_span / MIN_FAN_IN_PRIMARY_SLOT_SPACING).floor() as usize + 1
    };
    dynamic_capacity.max(baseline_capacity).max(1)
}

fn symmetric_side_band_depth(band_index: usize, band_count: usize) -> f64 {
    let margin = FAN_PRIMARY_SIDE_BAND_DEPTH_MARGIN.clamp(0.0, 0.49);
    if band_count <= 1 {
        margin
    } else {
        let raw = band_index as f64 / (band_count - 1) as f64;
        margin + (1.0 - 2.0 * margin) * raw
    }
}

fn remap_angular_fan_out_source_fraction(base_fraction: f64, edge_count: usize) -> f64 {
    if edge_count <= 3 {
        return base_fraction.clamp(0.0, 1.0);
    }

    // Pull interior slots toward center while preserving extremes.
    // This increases vertical separation between outer/inner lateral branches
    // on angular sources (diamond/hexagon) in TD/BT fan-out.
    let exponent = (1.0 + (edge_count as f64 - 3.0)).clamp(1.0, 4.0);
    let centered = (base_fraction.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let remapped = centered.signum() * centered.abs().powf(exponent);
    ((remapped + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn remap_angular_fan_in_target_fraction(base_fraction: f64, edge_count: usize) -> f64 {
    if edge_count <= 3 {
        return base_fraction.clamp(0.0, 1.0);
    }

    // Favor visual spread for smaller fan-ins, then progressively tighten as
    // port count grows (to preserve room for additional slots).
    let exponent = (8.0 / edge_count as f64).clamp(1.0, 2.5);
    let centered = (base_fraction.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let remapped = centered.signum() * centered.abs().powf(exponent);
    ((remapped + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn fan_in_source_cross_axis(
    geometry: &GraphGeometry,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    direction: Direction,
) -> f64 {
    let Some(rect) = endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref()) else {
        return edge.index as f64;
    };
    face_cross_axis(rect, direction)
}

fn fan_out_target_cross_axis(
    geometry: &GraphGeometry,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    direction: Direction,
) -> f64 {
    let Some(rect) = endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref()) else {
        return edge.index as f64;
    };
    face_cross_axis(rect, direction)
}

fn apply_near_aligned_primary_face_fraction_override(
    geometry: &GraphGeometry,
    direction: Direction,
    primary_face: Face,
    target_rect: &FRect,
    forward_edges: &[&crate::diagrams::flowchart::geometry::LayoutEdge],
    target_face_for_edge: &HashMap<usize, Face>,
    target_fraction_for_edge: &mut HashMap<usize, f64>,
) {
    let target_cross = face_cross_axis(target_rect, direction);
    let mut best: Option<(usize, f64, f64)> = None;

    for edge in forward_edges {
        if target_face_for_edge.get(&edge.index).copied() != Some(primary_face) {
            continue;
        }
        let Some(source_rect) = endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref())
        else {
            continue;
        };
        let source_cross = face_cross_axis(source_rect, direction);
        let delta = (source_cross - target_cross).abs();
        if delta > near_alignment_threshold(source_rect, target_rect, direction) {
            continue;
        }

        match best {
            Some((best_index, _, best_delta))
                if delta > best_delta
                    || ((delta - best_delta).abs() <= f64::EPSILON && edge.index >= best_index) => {
            }
            _ => {
                best = Some((edge.index, source_cross, delta));
            }
        }
    }

    if let Some((edge_index, source_cross, _)) = best {
        let aligned_fraction = cross_axis_to_face_fraction(source_cross, target_rect, direction);
        let aligned_slot_occupied = forward_edges.iter().any(|edge| {
            if edge.index == edge_index {
                return false;
            }
            if target_face_for_edge.get(&edge.index).copied() != Some(primary_face) {
                return false;
            }
            target_fraction_for_edge
                .get(&edge.index)
                .is_some_and(|fraction| (*fraction - aligned_fraction).abs() <= f64::EPSILON)
        });
        if aligned_slot_occupied {
            return;
        }
        target_fraction_for_edge.insert(edge_index, aligned_fraction);
    }
}

fn near_alignment_threshold(source_rect: &FRect, target_rect: &FRect, direction: Direction) -> f64 {
    match direction {
        Direction::TopDown | Direction::BottomTop => 0.5 * source_rect.width.min(target_rect.width),
        Direction::LeftRight | Direction::RightLeft => {
            0.5 * source_rect.height.min(target_rect.height)
        }
    }
}

fn cross_axis_to_face_fraction(cross: f64, rect: &FRect, direction: Direction) -> f64 {
    const EPS: f64 = 0.000_001;
    let raw = match direction {
        Direction::TopDown | Direction::BottomTop => {
            if rect.width.abs() <= EPS {
                0.5
            } else {
                (cross - rect.x) / rect.width
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if rect.height.abs() <= EPS {
                0.5
            } else {
                (cross - rect.y) / rect.height
            }
        }
    };
    raw.clamp(0.0, 1.0)
}

fn face_cross_axis(rect: &FRect, direction: Direction) -> f64 {
    match direction {
        Direction::TopDown | Direction::BottomTop => rect.x + rect.width / 2.0,
        Direction::LeftRight | Direction::RightLeft => rect.y + rect.height / 2.0,
    }
}

fn stagger_primary_face_shared_axis_segment(
    path: &mut [FPoint],
    direction: Direction,
    target_primary_channel_depth: Option<f64>,
) {
    const EPS: f64 = 0.000_001;
    const MIN_SOURCE_STEM: f64 = 8.0;
    const MIN_TARGET_STEM: f64 = 8.0;

    let Some(depth) = target_primary_channel_depth else {
        return;
    };
    if path.len() < 4 {
        return;
    }
    let depth = depth.clamp(0.0, 1.0);

    // Find the gathering segment: for TD/BT it's a horizontal segment
    // bounded by vertical segments; for LR/RL it's a vertical segment
    // bounded by horizontal segments. Search interior segments.
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    for i in 1..path.len().saturating_sub(2) {
        let seg_is_gathering = if primary_vertical {
            // TD/BT: gathering = horizontal segment (same y, different x)
            (path[i].y - path[i + 1].y).abs() <= EPS && (path[i].x - path[i + 1].x).abs() > EPS
        } else {
            // LR/RL: gathering = vertical segment (same x, different y)
            (path[i].x - path[i + 1].x).abs() <= EPS && (path[i].y - path[i + 1].y).abs() > EPS
        };
        if !seg_is_gathering {
            continue;
        }

        // Verify the adjacent segments are axis-normal (perpendicular to gathering)
        let prev_is_normal = if primary_vertical {
            (path[i - 1].x - path[i].x).abs() <= EPS && (path[i - 1].y - path[i].y).abs() > EPS
        } else {
            (path[i - 1].y - path[i].y).abs() <= EPS && (path[i - 1].x - path[i].x).abs() > EPS
        };
        let next_is_normal = if primary_vertical {
            (path[i + 1].x - path[i + 2].x).abs() <= EPS
                && (path[i + 1].y - path[i + 2].y).abs() > EPS
        } else {
            (path[i + 1].y - path[i + 2].y).abs() <= EPS
                && (path[i + 1].x - path[i + 2].x).abs() > EPS
        };
        if !prev_is_normal || !next_is_normal {
            continue;
        }

        // Stagger the gathering segment's shared-axis coordinate
        if primary_vertical {
            if let Some(y) = stagger_axis_value(
                path[0].y,
                path[path.len() - 1].y,
                depth,
                MIN_SOURCE_STEM,
                MIN_TARGET_STEM,
            ) {
                path[i].y = y;
                path[i + 1].y = y;
            }
        } else if let Some(x) = stagger_axis_value(
            path[0].x,
            path[path.len() - 1].x,
            depth,
            MIN_SOURCE_STEM,
            MIN_TARGET_STEM,
        ) {
            path[i].x = x;
            path[i + 1].x = x;
        }
        return;
    }
}

fn stagger_axis_value(
    start: f64,
    end: f64,
    depth: f64,
    min_source_stem: f64,
    min_target_stem: f64,
) -> Option<f64> {
    const EPS: f64 = 0.000_001;
    let delta = end - start;
    if delta.abs() <= min_source_stem + min_target_stem + EPS {
        return None;
    }

    let sign = delta.signum();
    let shallow = start + sign * min_source_stem;
    let deep = end - sign * min_target_stem;
    if (deep - shallow).abs() <= EPS {
        return None;
    }
    Some(shallow + (deep - shallow) * depth.clamp(0.0, 1.0))
}

fn edge_rank_span(
    geometry: &GraphGeometry,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
) -> Option<usize> {
    let EngineHints::Layered(hints) = geometry.engine_hints.as_ref()?;
    let src_rank = *hints.node_ranks.get(&edge.from)?;
    let dst_rank = *hints.node_ranks.get(&edge.to)?;
    Some(src_rank.abs_diff(dst_rank) as usize)
}

fn ensure_primary_stem_for_flat_off_center_fanout_sources(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    let fanout_outbound: Vec<&crate::diagrams::flowchart::geometry::LayoutEdge> = geometry
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

fn ensure_primary_stem_for_td_bt_angular_fanout_source(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn collapse_source_turnback_spikes(path: &mut Vec<FPoint>) {
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

fn has_immediate_axial_turnback(path: &[FPoint]) -> bool {
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

fn has_forward_primary_axis_reversal(path: &[FPoint], direction: Direction) -> bool {
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

fn collapse_forward_source_primary_turnback_hooks(
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
                    // Preserve outer lane spacing by pulling the inward hook
                    // out to the existing lane instead of collapsing lane x.
                    path[3].x = p2.x;
                    path[4].x = p2.x;
                } else {
                    // If p4 is terminal, avoid moving the endpoint-side segment.
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
                    // Preserve outer lane spacing by pulling the inward hook
                    // out to the existing lane instead of collapsing lane x.
                    path[3].x = p2.x;
                    path[4].x = p2.x;
                } else {
                    // If p4 is terminal, avoid moving the endpoint-side segment.
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
                    // Preserve outer lane spacing by pulling the inward hook
                    // out to the existing lane instead of collapsing lane y.
                    path[3].y = p2.y;
                    path[4].y = p2.y;
                } else {
                    // If p4 is terminal, avoid moving the endpoint-side segment.
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
                    // Preserve outer lane spacing by pulling the inward hook
                    // out to the existing lane instead of collapsing lane y.
                    path[3].y = p2.y;
                    path[4].y = p2.y;
                } else {
                    // If p4 is terminal, avoid moving the endpoint-side segment.
                    path[1].y = p3.y;
                    path[2].y = p3.y;
                }
                changed = true;
            }
        }
    }

    changed
}

fn ensure_backward_outer_lane_clearance(
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

/// After `align_backward_outer_lane_to_hint` pulls interior points to the layout's
/// channel hint, the channel lane may sit too close to the node envelope.
/// This function enforces a minimum clearance between the node faces and
/// the backward channel lane, matching R-BACK-8/9/10.
fn enforce_backward_minimum_channel_floor(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
            // Right-side channel: envelope = max right edge of source/target.
            // Only applies when interior points already sit beyond the envelope
            // (i.e. the edge uses side-face channel routing, not flow-face).
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
            // Bottom channel: envelope = max bottom edge of source/target
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

/// After `snap_backward_endpoints_to_shape`, diamond/hexagon source endpoints
/// may create diagonal segments.  When the SVG orthogonal renderer splits
/// these into axis-aligned steps (vertical-first), the vertical leg can cut
/// through an intermediate node.
///
/// Fix: detect diagonal source segments whose vertical-first orthogonalization
/// would cross an intermediate node and reroute through the outer lane so the
/// path goes horizontal-first (at source y) then vertical (at outer lane x).
/// The same logic applies symmetrically for target-side diagonals and LR/RL.
fn fix_backward_diagonal_node_collision(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

/// TD/BT source-side: if [0]→[1] is diagonal and the vertical-first
/// orthogonalization would cross an intermediate node, reroute through
/// the outer lane (max-x of interior points).
fn fix_backward_diagonal_source_td_bt(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    margin: f64,
    eps: f64,
) {
    if path.len() < 3 {
        return;
    }
    let source = path[0];
    let next = path[1];

    // Only act on diagonal segments.
    let dx = (source.x - next.x).abs();
    let dy = (source.y - next.y).abs();
    if dx <= eps || dy <= eps {
        return;
    }

    // Would a vertical-first step (at source.x) cross an intermediate node?
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

    // Find a safe x for the vertical: start from the outer lane (max-x of
    // interior points) and push past any node whose x-range contains safe_x.
    let last = path.len() - 1;
    let mut safe_x = path[1..last]
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);

    // Iteratively push safe_x past overlapping nodes until it converges.
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
            // Node must overlap the vertical y-range AND contain safe_x.
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

    // Replace diagonal [0]→[1] with orthogonal: [0] → (safe_x, source.y) → (safe_x, next.y).
    // Then drop old [1] since (safe_x, next.y) replaces it.
    path[1] = FPoint::new(safe_x, next.y);
    path.insert(1, FPoint::new(safe_x, source.y));
}

/// TD/BT target-side: same check for the last segment.
fn fix_backward_diagonal_target_td_bt(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    let last = path.len() - 1;
    path[last - 1] = FPoint::new(safe_x, prev.y);
    path.insert(last, FPoint::new(safe_x, target.y));
}

/// LR/RL source-side: if [0]→[1] is diagonal and the horizontal-first
/// orthogonalization would cross an intermediate node, reroute through
/// the outer lane (max-y of interior points).
fn fix_backward_diagonal_source_lr_rl(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

/// LR/RL target-side.
fn fix_backward_diagonal_target_lr_rl(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    let last = path.len() - 1;
    path[last - 1] = FPoint::new(prev.x, safe_y);
    path.insert(last, FPoint::new(target.x, safe_y));
}

fn align_backward_source_stem_to_outer_lane(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn align_backward_outer_lane_to_hint(
    path: &mut [FPoint],
    hint: Option<&[FPoint]>,
    direction: Direction,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

            // If the hint's outer lane x is inside either endpoint node, the
            // hint is not meaningful for backward lane alignment — skip.
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

            let mut aligned = false;
            for (idx, point) in path.iter_mut().enumerate() {
                if idx == 0 || idx == last {
                    continue;
                }
                if (point.x - route_outer).abs() <= EPS {
                    point.x = hint_outer;
                    aligned = true;
                }
            }
            if !aligned {}
        }
        Direction::LeftRight | Direction::RightLeft => {
            // The hint waypoints come from the layout engine and may pass through
            // node centers. If the hint's outer lane y is inside either endpoint
            // node, the hint is not meaningful for backward lane alignment — skip.
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

            let mut aligned = false;
            for (idx, point) in path.iter_mut().enumerate() {
                if idx == 0 || idx == last {
                    continue;
                }
                if (point.y - route_outer).abs() <= EPS {
                    point.y = hint_outer;
                    aligned = true;
                }
            }
            if !aligned {
                return;
            }

            // Keep residual terminal hooks from drifting too far below the
            // hint-derived backward lane envelope in LR/RL.
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

fn enforce_backward_terminal_tangent_direction(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

    // Pattern: pre -> turn -> support reverses along one axis before the
    // terminal support segment. Preserve the already-defined lane at `turn`.
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

    // Pattern: turn -> support -> end immediately reverses on the same axis.
    // Replace `turn` with an outer-corner elbow so the terminal approach
    // remains monotonic toward the endpoint.
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

fn collapse_tiny_backward_terminal_staircase(
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

fn collapse_backward_terminal_node_intrusion(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn enforce_backward_source_tangent_direction(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

pub(crate) fn build_path_from_hints(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
) -> Vec<FPoint> {
    if let Some(ref path) = edge.layout_path_hint {
        if hint_has_non_degenerate_span(path)
            && hint_endpoints_attach_to_layout_bounds(edge, geometry, path)
        {
            return path.clone();
        }

        let fallback = build_path_from_nodes_and_waypoints(edge, geometry);
        if fallback.len() >= 2 {
            return fallback;
        }

        return path.clone();
    }

    build_path_from_nodes_and_waypoints(edge, geometry)
}

fn build_path_from_nodes_and_waypoints(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
) -> Vec<FPoint> {
    let mut path = Vec::new();
    if let Some(from_node) = geometry.nodes.get(&edge.from) {
        let center = rect_center(&from_node.rect);
        path.push(FPoint::new(center.x, center.y));
    }
    path.extend(edge.waypoints.iter().copied());
    if let Some(to_node) = geometry.nodes.get(&edge.to) {
        let center = rect_center(&to_node.rect);
        path.push(FPoint::new(center.x, center.y));
    }
    path
}

fn hint_has_non_degenerate_span(path: &[FPoint]) -> bool {
    if path.len() < 2 {
        return false;
    }
    path.windows(2).any(|segment| {
        let a = segment[0];
        let b = segment[1];
        (a.x - b.x).abs() > f64::EPSILON || (a.y - b.y).abs() > f64::EPSILON
    })
}

fn hint_endpoints_attach_to_layout_bounds(
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
    path: &[FPoint],
) -> bool {
    const MAX_HINT_ENDPOINT_DRIFT: f64 = 20.0;
    if path.len() < 2 {
        return false;
    }

    let Some(from_rect) = endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref()) else {
        return false;
    };
    let Some(to_rect) = endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref()) else {
        return false;
    };

    let start = path[0];
    let end = path[path.len() - 1];
    point_on_or_inside_rect(start, from_rect, MAX_HINT_ENDPOINT_DRIFT)
        && point_on_or_inside_rect(end, to_rect, MAX_HINT_ENDPOINT_DRIFT)
}

fn endpoint_rect<'a>(
    geometry: &'a GraphGeometry,
    node_id: &str,
    subgraph_id: Option<&str>,
) -> Option<&'a crate::diagrams::flowchart::geometry::FRect> {
    if let Some(sg_id) = subgraph_id {
        geometry.subgraphs.get(sg_id).map(|sg| &sg.rect)
    } else {
        geometry.nodes.get(node_id).map(|node| &node.rect)
    }
}

fn rect_center(rect: &crate::diagrams::flowchart::geometry::FRect) -> FPoint {
    FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn point_on_or_inside_rect(
    point: FPoint,
    rect: &crate::diagrams::flowchart::geometry::FRect,
    eps: f64,
) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    point.x >= left - eps
        && point.x <= right + eps
        && point.y >= top - eps
        && point.y <= bottom + eps
}

/// Deterministically snap float path points onto a fixed grid.
pub(crate) fn snap_path_to_grid(path: &[FPoint], scale_x: f64, scale_y: f64) -> Vec<FPoint> {
    let sx = if scale_x.abs() < f64::EPSILON {
        1.0
    } else {
        scale_x.abs()
    };
    let sy = if scale_y.abs() < f64::EPSILON {
        1.0
    } else {
        scale_y.abs()
    };

    path.iter()
        .map(|p| FPoint::new((p.x / sx).round() * sx, (p.y / sy).round() * sy))
        .collect()
}

fn build_contracted_path(control_points: &[FPoint], direction: Direction) -> Vec<FPoint> {
    if control_points.len() < 2 {
        return control_points.to_vec();
    }

    let start = control_points[0];
    let end = control_points[control_points.len() - 1];
    let waypoints = &control_points[1..(control_points.len() - 1)];
    let orthogonal = build_orthogonal_path_float(start, end, direction, waypoints);
    normalize_orthogonal_route_contracts(&orthogonal, direction)
}

#[allow(clippy::too_many_arguments)]
fn anchor_path_endpoints_to_endpoint_faces(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
            // For non-rect sources, projecting via `next` can collapse multiple
            // slotted ports to the same apex (e.g. fan-out from diamond/hexagon
            // sources). Preserve the slotted rect ray for TD/BT, and for LR/RL
            // only when there is a true fan-out group (3+ forward outbound
            // edges) so two-edge diamonds can still prefer smooth lane entry.
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
            // For non-rect targets, projecting via `prev` can collapse distinct
            // fan-in slot fractions to the same boundary point. When a policy
            // fraction is active on forward routing, preserve that slot.
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

/// Offset a backward edge's source port from the primary flow face center
/// so it doesn't overlap with the forward edge's arrival port.
///
/// For TD, if the backward edge departs from the source's top face (where
/// forward edges arrive), shift the departure x right of center. This creates
/// distinct ports for forward arrival and backward departure on the same face.
fn offset_backward_source_from_primary_face(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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
            // Backward edge departing from source's top face (forward arrival face).
            if (start.y - sr.y).abs() <= FACE_EPS {
                let offset = (sr.width / 4.0).clamp(8.0, 20.0);
                let new_x = (sr.center_x() + offset).min(sr.x + sr.width - FACE_EPS);
                path[0].x = new_x;
                // If the source stem is vertical (points [0] and [1] share x),
                // also update the next point to preserve the vertical stem.
                if path.len() >= 2 && (path[1].x - start.x).abs() <= FACE_EPS {
                    path[1].x = new_x;
                }
            }
        }
        Direction::BottomTop => {
            // Backward edge departing from source's bottom face.
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
            // Backward edge departing from source's left face.
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
            // Backward edge departing from source's right face.
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

/// Re-project backward edge endpoints to actual shape boundaries.
///
/// Backward edge processing (tangent direction, lane clearance, corner inset)
/// snaps endpoints to rect faces. This function corrects non-rect shapes
/// (diamond, hexagon) by projecting the rect-face endpoint onto the actual
/// shape boundary using the adjacent path point as the approach direction.
fn snap_backward_endpoints_to_shape(
    path: &mut [FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
    geometry: &GraphGeometry,
) {
    if path.len() < 2 {
        return;
    }

    // Re-project source endpoint
    if let Some((from_rect, from_shape)) =
        endpoint_rect_and_shape(geometry, &edge.from, edge.from_subgraph.as_deref())
        && matches!(from_shape, Shape::Diamond | Shape::Hexagon)
    {
        let approach = path[1];
        path[0] = intersect_shape_boundary_float(from_rect, from_shape, approach);
    }

    // Re-project target endpoint
    let last = path.len() - 1;
    if let Some((to_rect, to_shape)) =
        endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
        && matches!(to_shape, Shape::Diamond | Shape::Hexagon)
    {
        let approach = path[last - 1];
        let boundary = intersect_shape_boundary_float(to_rect, to_shape, approach);
        // Add marker clearance: the SVG arrowhead marker has physical width
        // (8px base), and on angled diamond/hexagon edges the marker body
        // protrudes past the shape boundary. Push the endpoint outward along
        // the approach direction so the marker body clears the angled edge.
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

/// Project a rect-clipped endpoint to the actual shape boundary for non-rect shapes.
/// For rectangles, returns the rect-clipped point unchanged.
fn project_endpoint_to_shape(
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

fn endpoint_rect_and_shape(
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum RectFace {
    Top,
    Bottom,
    Left,
    Right,
}

fn boundary_face_excluding_corners(point: FPoint, rect: FRect, eps: f64) -> Option<RectFace> {
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

fn hint_face_for_td_bt_parity(point: FPoint, rect: FRect) -> Option<Face> {
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

    // Reject corner-ambiguous hints where side/top proximity is comparable.
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

        // For forward fan-in overflow, honor the assigned policy face even if
        // the incoming segment has not been reshaped yet. Endpoint axis-normal
        // support is enforced later in the pipeline.
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

    // For backward edges whose target has fan-in overflow, if the endpoint
    // landed on the canonical backward face (e.g. right for TD), flip to the
    // opposite side so the backward channel doesn't collide with forward
    // fan-in ports. Only apply to backward edges — forward overflow targets
    // use the overflow policy face system instead.
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

    // Backward TD/BT source endpoints: preserve side-face attachment only for
    // the canonical backward channel face (right for TD/BT). Left-face routing
    // is not supported for backward edges — it produces inverted tangent
    // directions and endpoint pull-back failures (see issue 0013).
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

    // Terminal segment is horizontal: anchor endpoint on left/right face.
    if dy.abs() <= EPS && dx.abs() > EPS {
        let face = if adjacent.x < endpoint.x {
            RectFace::Left
        } else {
            RectFace::Right
        };
        return clip_point_to_rect_face_with_inset(endpoint, rect, face, max_corner_inset);
    }

    // Terminal segment is vertical: anchor endpoint on top/bottom face.
    if dx.abs() <= EPS && dy.abs() > EPS {
        let face = if adjacent.y < endpoint.y {
            RectFace::Top
        } else {
            RectFace::Bottom
        };
        return clip_point_to_rect_face_with_inset(endpoint, rect, face, max_corner_inset);
    }

    // Fallback: clamp interior drift to the rectangle boundary box.
    FPoint::new(
        endpoint.x.clamp(x_min, x_max),
        endpoint.y.clamp(y_min, y_max),
    )
}

fn enforce_backward_terminal_corner_inset(
    path: &mut Vec<FPoint>,
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn map_face_to_rect_face(face: Face) -> RectFace {
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

fn clip_point_to_rect_face_with_inset(
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

fn boundary_face_including_corners(point: FPoint, rect: FRect, eps: f64) -> Option<RectFace> {
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

fn bias_face_coordinate_toward_center(
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

fn ensure_endpoint_segments_axis_aligned(path: &mut Vec<FPoint>) {
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

fn points_match(a: FPoint, b: FPoint) -> bool {
    const EPS: f64 = 0.000_001;
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS
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

fn collapse_collinear_interior_points(path: &mut Vec<FPoint>) {
    const EPS: f64 = 0.000_001;
    if path.len() <= 2 {
        return;
    }

    let mut collapsed = Vec::with_capacity(path.len());
    collapsed.push(path[0]);
    for idx in 1..(path.len() - 1) {
        let prev = *collapsed.last().expect("collapsed is non-empty");
        let curr = path[idx];
        let next = path[idx + 1];

        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= EPS && dot >= -EPS;
        if !collinear_same_direction {
            collapsed.push(curr);
        }
    }
    collapsed.push(*path.last().expect("path has at least two points"));
    *path = collapsed;
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

fn flow_target_face_for_direction(direction: Direction) -> Face {
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

fn endpoint_is_on_policy_face(
    path: &[FPoint],
    edge: &crate::diagrams::flowchart::geometry::LayoutEdge,
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

fn enforce_terminal_support_normal_to_face(path: &mut Vec<FPoint>, face: Face, min_support: f64) {
    const EPS: f64 = 0.000_001;
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
    let pre_to_penult_axis = (pre.x - penult.x).abs() <= EPS || (pre.y - penult.y).abs() <= EPS;
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
