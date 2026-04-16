//! Graph-family route execution and path shaping helpers.

use std::collections::HashMap;

use super::float_core::compute_port_attachments_from_geometry;
use super::labels::{arc_length_midpoint, compute_end_labels_for_edge};
use super::orthogonal::{OrthogonalRoutingOptions, build_path_from_hints, route_edges_orthogonal};
use super::{backward_corridor, label_clamp, label_lanes};
use crate::graph::direction_policy::effective_edge_direction;
use crate::graph::geometry::{
    EdgeLabelGeometry, EdgeLabelSide, GraphGeometry, LayoutEdge, RoutedEdgeGeometry,
    RoutedGraphGeometry, RoutedSelfEdge,
};
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Graph, Shape};

/// Graph-family routed-path ownership mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRouting {
    /// Build a single direct path from source to target.
    DirectRoute,
    /// Build a polyline from layout hints.
    PolylineRoute,
    /// Use complete edge paths supplied by the solve stage.
    EngineProvided,
    /// Build an axis-aligned path.
    OrthogonalRoute,
}

/// Route graph geometry to produce fully-routed edge paths.
///
/// Consumes engine-agnostic `GraphGeometry` and produces `RoutedGraphGeometry`
/// with polyline paths for every edge.
pub fn route_graph_geometry(
    diagram: &Graph,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
    metrics: &ProportionalTextMetrics,
) -> RoutedGraphGeometry {
    let port_attachments =
        compute_port_attachments_from_geometry(&diagram.edges, geometry, diagram.direction);

    let edges: Vec<RoutedEdgeGeometry> = match edge_routing {
        EdgeRouting::OrthogonalRoute => {
            let mut edges =
                route_edges_orthogonal(diagram, geometry, OrthogonalRoutingOptions::preview());
            for edge in &mut edges {
                if let Some((sp, tp)) = port_attachments.get(&edge.index) {
                    edge.source_port = sp.clone();
                    edge.target_port = tp.clone();
                }
            }
            edges
        }
        EdgeRouting::DirectRoute | EdgeRouting::EngineProvided | EdgeRouting::PolylineRoute => {
            let backward_corridor_ctx = backward_corridor::compute_direct_backward_corridor_context(
                geometry,
                diagram.direction,
            );

            geometry
                .edges
                .iter()
                .map(|edge| {
                    let edge_direction = effective_edge_direction(
                        &geometry.node_directions,
                        &edge.from,
                        &edge.to,
                        diagram.direction,
                    );
                    let path = match edge_routing {
                        EdgeRouting::DirectRoute => {
                            build_direct_path(edge, geometry, edge_direction)
                        }
                        EdgeRouting::EngineProvided => edge
                            .layout_path_hint
                            .clone()
                            .unwrap_or_else(|| build_path_from_hints(edge, geometry)),
                        EdgeRouting::PolylineRoute => build_path_from_hints(edge, geometry),
                        EdgeRouting::OrthogonalRoute => unreachable!(),
                    };
                    let is_backward = geometry.reversed_edges.contains(&edge.index);
                    let path = if !is_backward && path.len() >= 2 {
                        snap_path_endpoints_to_faces(&path, edge, geometry, edge_direction)
                    } else {
                        path
                    };
                    let corridor_slot = backward_corridor_ctx.slot_for(edge.index);
                    let needs_channel = is_backward
                        && geometry.enhanced_backward_routing
                        && (backward_corridor::has_direct_corridor_obstructions(
                            edge,
                            geometry,
                            edge_direction,
                        ) || corridor_slot.is_some());
                    let needs_short_offset = is_backward
                        && (geometry.enhanced_backward_routing
                            || edge_direction != diagram.direction);
                    let path = if needs_channel {
                        build_backward_channel_path(
                            path,
                            edge,
                            geometry,
                            edge_direction,
                            corridor_slot,
                        )
                    } else if needs_short_offset {
                        apply_short_backward_port_offset(path, edge, geometry, edge_direction)
                    } else {
                        path
                    };
                    let label_position = if needs_channel && path.len() >= 2 {
                        arc_length_midpoint(&path)
                    } else {
                        edge.label_position
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
                        source_port: port_attachments
                            .get(&edge.index)
                            .and_then(|(sp, _)| sp.clone()),
                        target_port: port_attachments
                            .get(&edge.index)
                            .and_then(|(_, tp)| tp.clone()),
                        preserve_orthogonal_topology: false,
                        label_geometry: None,
                    }
                })
                .collect::<Vec<_>>()
        }
    };

    // Spread co-located backward edge ports for non-orthogonal presets.
    // The orthogonal path does this internally.  EngineProvided is excluded
    // because its contract is to preserve engine-supplied geometry.
    let mut edges = edges;
    if matches!(
        edge_routing,
        EdgeRouting::DirectRoute | EdgeRouting::PolylineRoute
    ) {
        super::orthogonal::fan::spread_colocated_backward_source_ports(&mut edges, geometry);
        super::orthogonal::fan::spread_colocated_backward_target_ports(&mut edges, geometry);

        // Recompute label anchors for backward edges whose paths were
        // mutated by the spreading pass.
        for edge in edges.iter_mut().filter(|e| e.is_backward) {
            edge.label_position = if edge.path.len() >= 2 {
                arc_length_midpoint(&edge.path)
            } else {
                edge.label_position
            };
            let (head, tail) = compute_end_labels_for_edge(diagram, edge.index, &edge.path);
            edge.head_label_position = head;
            edge.tail_label_position = tail;
        }
    }

    // Populate label_geometry on every routed edge with a non-empty label.
    // This is the single source of truth for padded label rectangles consumed
    // by SVG, MMDS, and bounds downstream. track: 0 until the lane assignment
    // pass (plan 0145 PR 3 task 3.5).
    populate_label_geometry(&mut edges, diagram, metrics);

    // Run label lane assignment pass. This shifts labels and middle path
    // segments to resolve overlaps within compartments. Per Q7 — labels
    // packed against final routed paths, after backward corridors and fan
    // spreading and after the placeholder track:0 label_geometry has been
    // populated, but before self-edge routing and bounds recomputation.
    let paths_by_index: HashMap<usize, Vec<FPoint>> =
        edges.iter().map(|e| (e.index, e.path.clone())).collect();
    let backward_flags: HashMap<usize, bool> =
        edges.iter().map(|e| (e.index, e.is_backward)).collect();
    let lane_outcomes = label_lanes::assign_label_tracks(
        diagram,
        geometry,
        &paths_by_index,
        &backward_flags,
        metrics,
        diagram.direction,
    );
    for routed_edge in edges.iter_mut() {
        let Some(outcome) = lane_outcomes.get(&routed_edge.index) else {
            continue;
        };
        // Skip the wire-up for singleton compartments on track 0 — the
        // lane pass had nothing to displace and nothing to coordinate
        // with, so preserve whatever populate_label_geometry placed for
        // the edge (the orchestrator's arc-midpoint can differ from the
        // engine's label_position by a few pixels for unrelated edges,
        // and we don't want that churn). For multi-member compartments
        // (track 0 or otherwise), apply the outcome unconditionally so
        // every compartment member shares a consistent reference point —
        // otherwise a track-0 forward at the engine's anchor and a
        // track±1 reverse at the descriptor midpoint can end up only a
        // few pixels apart when the engine pre-shifted the forward.
        if outcome.compartment_size == 1 {
            continue;
        }
        // Preserve padding/side from populate_label_geometry's output so the
        // lane pass only updates center/rect/track. Fall back to metric
        // defaults if label_geometry is somehow absent (should not happen
        // because populate_label_geometry ran first for every labeled edge).
        let (existing_padding, existing_side) = routed_edge
            .label_geometry
            .as_ref()
            .map(|g| (g.padding, g.side))
            .unwrap_or((
                (metrics.label_padding_x, metrics.label_padding_y),
                EdgeLabelSide::Center,
            ));
        routed_edge.label_position = Some(outcome.label_center);
        routed_edge.label_geometry = Some(EdgeLabelGeometry {
            center: outcome.label_center,
            rect: outcome.label_rect,
            padding: existing_padding,
            side: existing_side,
            track: outcome.track,
        });
        // Note: Algorithm C produces an `adjusted_path` that bows the path
        // around lane-displaced labels. We deliberately do NOT apply it
        // here. Bending routed paths corrupts the text grid's corridor
        // closure for backward edges (text renderer reads routed paths
        // directly), and reciprocal pairs are already separated at the
        // routing layer (backward corridors place reverse edges to the
        // side, not collinear with the forward edge). Label-only shifts
        // are sufficient to resolve overlap. See Q9 tests + finding
        // `task-3.9-text-renderer-coupling.md`.
        //
        // The adjusted_path field is kept in LabelTrackOutcome for
        // potential future use (e.g., a follow-on plan that adds a
        // text-aware path-bend pass).
        let _ = &outcome.adjusted_path;
    }

    let self_edges: Vec<RoutedSelfEdge> = geometry
        .self_edges
        .iter()
        .map(|se| {
            let path = if let Some(node) = geometry.nodes.get(&se.node_id) {
                canonical_self_loop_path(&node.rect, &se.points, geometry.direction, &node.shape)
            } else {
                se.points.clone()
            };
            RoutedSelfEdge {
                node_id: se.node_id.clone(),
                edge_index: se.edge_index,
                path,
            }
        })
        .collect();

    // Plan 0146 Task 2.1: clamp label rects so they sit beyond source/target
    // node faces (plus marker avoidance) along the edge-parallel axis.
    // Records unfit cases on `unfit_label_overlaps` for downstream consumers
    // (MMDS diagnostics, CLI stderr — Task 2.3) instead of silently shipping
    // overlapping output.
    let mut unfit_label_overlaps = Vec::new();
    label_clamp::clamp_label_geometry_to_node_bounds(
        &mut edges,
        &geometry.nodes,
        diagram,
        geometry.direction,
        metrics,
        &mut unfit_label_overlaps,
    );

    let bounds = recompute_routed_bounds(geometry, &edges, &self_edges);

    RoutedGraphGeometry {
        nodes: geometry.nodes.clone(),
        edges,
        subgraphs: geometry.subgraphs.clone(),
        self_edges,
        direction: geometry.direction,
        bounds,
        unfit_label_overlaps,
    }
}

pub(crate) fn recompute_routed_bounds(
    geometry: &GraphGeometry,
    edges: &[RoutedEdgeGeometry],
    self_edges: &[RoutedSelfEdge],
) -> FRect {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    let b = geometry.bounds;
    min_x = min_x.min(b.x);
    min_y = min_y.min(b.y);
    max_x = max_x.max(b.x + b.width);
    max_y = max_y.max(b.y + b.height);

    for edge in edges {
        for p in &edge.path {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        // Extend by the full padded label rectangle, not just the center
        // anchor. After the label-lane pass (plan 0145), labels can be
        // shifted into positions whose padded extent reaches outside the
        // original anchor — including only the center would clip the
        // viewBox.
        if let Some(rect) = edge.label_geometry.as_ref().map(|g| g.rect) {
            min_x = min_x.min(rect.x);
            min_y = min_y.min(rect.y);
            max_x = max_x.max(rect.x + rect.width);
            max_y = max_y.max(rect.y + rect.height);
        }
    }

    for se in self_edges {
        for p in &se.path {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    FRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

pub(crate) fn build_direct_path(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Vec<FPoint> {
    if edge.from == edge.to {
        return build_path_from_hints(edge, geometry);
    }

    let Some(from_node) = geometry.nodes.get(&edge.from) else {
        return build_path_from_hints(edge, geometry);
    };
    let Some(to_node) = geometry.nodes.get(&edge.to) else {
        return build_path_from_hints(edge, geometry);
    };

    let start = FPoint::new(from_node.rect.center_x(), from_node.rect.center_y());
    let mut end = FPoint::new(to_node.rect.center_x(), to_node.rect.center_y());

    if points_are_same(start, end) {
        if let Some(hint) = edge.layout_path_hint.as_ref()
            && path_has_non_degenerate_span(hint)
        {
            return hint.clone();
        }
        end = nudge_for_direction(start, direction);
    }

    let start = snap_to_primary_face(start, &from_node.rect, direction, true);
    let end = snap_to_primary_face(end, &to_node.rect, direction, false);

    if direct_segment_crosses_non_endpoint_nodes(start, end, edge, geometry) {
        return build_path_from_hints(edge, geometry);
    }

    vec![start, end]
}

pub(crate) fn apply_short_backward_port_offset(
    path: Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Vec<FPoint> {
    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return path;
    };

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let max_offset = (sr.width.min(tr.width) / 3.0).min(20.0);
            let offset = max_offset.max(8.0);
            let src_x = sr.center_x() + offset;
            let tgt_x = tr.center_x() + offset;
            let src_y = sr.center_y();
            let tgt_y = tr.center_y();
            let mid_y = (src_y + tgt_y) / 2.0;
            vec![
                FPoint::new(src_x, src_y),
                FPoint::new(src_x.max(tgt_x), mid_y),
                FPoint::new(tgt_x, tgt_y),
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            let max_offset = (sr.height.min(tr.height) / 3.0).min(20.0);
            let offset = max_offset.max(8.0);
            let src_x = match direction {
                Direction::LeftRight => sr.x,
                Direction::RightLeft => sr.x + sr.width,
                _ => sr.center_x(),
            };
            let tgt_x = match direction {
                Direction::LeftRight => tr.x + tr.width,
                Direction::RightLeft => tr.x,
                _ => tr.center_x(),
            };
            let src_y = (sr.center_y() + offset).clamp(sr.y + 1.0, sr.y + sr.height - 1.0);
            let tgt_y = (tr.center_y() + offset).clamp(tr.y + 1.0, tr.y + tr.height - 1.0);
            let mid_x = (src_x + tgt_x) / 2.0;
            vec![
                FPoint::new(src_x, src_y),
                FPoint::new(mid_x, src_y.max(tgt_y)),
                FPoint::new(tgt_x, tgt_y),
            ]
        }
    }
}

pub(crate) fn build_backward_channel_path(
    path: Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    corridor_slot: Option<&backward_corridor::BackwardCorridorSlot>,
) -> Vec<FPoint> {
    use super::backward_corridor::LANE_SPACING;

    const CHANNEL_CLEARANCE: f64 = 8.0;

    let from_node = geometry.nodes.get(&edge.from);
    let to_node = geometry.nodes.get(&edge.to);
    let from_rect = from_node.map(|n| n.rect);
    let to_rect = to_node.map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return path;
    };

    let scope_parent = from_node.and_then(|n| n.parent.as_deref());
    let sg_rect = backward_corridor::shared_parent_subgraph_rect(edge, geometry);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let source_face_x = sr.x + sr.width;
            let target_face_x = tr.x + tr.width;
            let source_cy = sr.center_y();
            let target_cy = tr.center_y();

            let lane_x = if let Some(slot) = corridor_slot {
                // Use pre-computed compartment base lane + per-edge slot offset.
                let mut lx = slot.base_lane + (slot.slot as f64) * LANE_SPACING;
                if let Some(sg) = sg_rect {
                    lx = lx.min(sg.x + sg.width - CHANNEL_CLEARANCE);
                }
                lx
            } else {
                // Fallback: independent computation (single backward edge).
                let face_envelope = source_face_x.max(target_face_x);
                let (min_y, max_y) = source_target_rank_range_y(from_rect, to_rect);
                let mut lx = face_envelope + CHANNEL_CLEARANCE;
                for node in geometry.nodes.values() {
                    if node.id == edge.from || node.id == edge.to {
                        continue;
                    }
                    if !backward_corridor::node_in_scope(&node.id, scope_parent, geometry) {
                        continue;
                    }
                    let cy = node.rect.center_y();
                    let node_right = node.rect.x + node.rect.width;
                    if cy >= min_y && cy <= max_y {
                        lx = lx.max(node_right + CHANNEL_CLEARANCE);
                    }
                }
                if let Some(sg) = sg_rect {
                    lx = lx.min(sg.x + sg.width - CHANNEL_CLEARANCE);
                }
                lx
            };

            vec![
                FPoint::new(source_face_x, source_cy),
                FPoint::new(lane_x, source_cy),
                FPoint::new(lane_x, target_cy),
                FPoint::new(target_face_x, target_cy),
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            let source_face_y = sr.y + sr.height;
            let target_face_y = tr.y + tr.height;
            let source_cx = sr.center_x();
            let target_cx = tr.center_x();

            let lane_y = if let Some(slot) = corridor_slot {
                let mut ly = slot.base_lane + (slot.slot as f64) * LANE_SPACING;
                if let Some(sg) = sg_rect {
                    ly = ly.min(sg.y + sg.height - CHANNEL_CLEARANCE);
                }
                ly
            } else {
                let face_envelope = source_face_y.max(target_face_y);
                let corridor_top = sr.y.min(tr.y);
                let (min_x, max_x) = source_target_rank_range_x(from_rect, to_rect);
                let mut ly = face_envelope + CHANNEL_CLEARANCE;
                for node in geometry.nodes.values() {
                    if node.id == edge.from || node.id == edge.to {
                        continue;
                    }
                    if !backward_corridor::node_in_scope(&node.id, scope_parent, geometry) {
                        continue;
                    }
                    let cx = node.rect.center_x();
                    let node_bottom = node.rect.y + node.rect.height;
                    if cx >= min_x && cx <= max_x && node.rect.y < ly && node_bottom > corridor_top
                    {
                        ly = ly.max(node_bottom + CHANNEL_CLEARANCE);
                    }
                }
                if let Some(sg) = sg_rect {
                    ly = ly.min(sg.y + sg.height - CHANNEL_CLEARANCE);
                }
                ly
            };

            vec![
                FPoint::new(source_cx, source_face_y),
                FPoint::new(source_cx, lane_y),
                FPoint::new(target_cx, lane_y),
                FPoint::new(target_cx, target_face_y),
            ]
        }
    }
}

pub(crate) fn snap_path_endpoints_to_faces(
    path: &[FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Vec<FPoint> {
    let mut result = path.to_vec();

    let source_rect = if let Some(sg_id) = &edge.from_subgraph {
        geometry.subgraphs.get(sg_id).map(|sg| sg.rect)
    } else {
        geometry.nodes.get(&edge.from).map(|n| n.rect)
    };
    if let Some(rect) = source_rect {
        result[0] = snap_to_primary_face(result[0], &rect, direction, true);
    }

    let target_rect = if let Some(sg_id) = &edge.to_subgraph {
        geometry.subgraphs.get(sg_id).map(|sg| sg.rect)
    } else {
        geometry.nodes.get(&edge.to).map(|n| n.rect)
    };
    if let Some(rect) = target_rect {
        let last = result.len() - 1;
        result[last] = snap_to_primary_face(result[last], &rect, direction, false);
    }

    result
}

fn source_target_rank_range_y(from_rect: Option<FRect>, to_rect: Option<FRect>) -> (f64, f64) {
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for rect in [from_rect, to_rect].iter().flatten() {
        min_y = min_y.min(rect.y);
        max_y = max_y.max(rect.y + rect.height);
    }
    (min_y, max_y)
}

fn source_target_rank_range_x(from_rect: Option<FRect>, to_rect: Option<FRect>) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for rect in [from_rect, to_rect].iter().flatten() {
        min_x = min_x.min(rect.x);
        max_x = max_x.max(rect.x + rect.width);
    }
    (min_x, max_x)
}

fn direct_segment_crosses_non_endpoint_nodes(
    start: FPoint,
    end: FPoint,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) -> bool {
    const BORDER_CLEARANCE_MARGIN: f64 = -0.5;
    geometry.nodes.iter().any(|(id, node)| {
        if id == &edge.from || id == &edge.to {
            return false;
        }
        segment_crosses_rect_interior(start, end, node.rect, BORDER_CLEARANCE_MARGIN)
    })
}

fn segment_crosses_rect_interior(start: FPoint, end: FPoint, rect: FRect, margin: f64) -> bool {
    const EPS: f64 = 1e-6;
    let left = rect.x + margin + EPS;
    let right = rect.x + rect.width - margin - EPS;
    let top = rect.y + margin + EPS;
    let bottom = rect.y + rect.height - margin - EPS;
    if left >= right || top >= bottom {
        return false;
    }

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let mut t0 = 0.0;
    let mut t1 = 1.0;

    if !clip_test(-dx, start.x - left, &mut t0, &mut t1) {
        return false;
    }
    if !clip_test(dx, right - start.x, &mut t0, &mut t1) {
        return false;
    }
    if !clip_test(-dy, start.y - top, &mut t0, &mut t1) {
        return false;
    }
    if !clip_test(dy, bottom - start.y, &mut t0, &mut t1) {
        return false;
    }

    t0 < t1
}

fn clip_test(p: f64, q: f64, t0: &mut f64, t1: &mut f64) -> bool {
    const EPS: f64 = 1e-12;
    if p.abs() <= EPS {
        return q >= 0.0;
    }

    let r = q / p;
    if p < 0.0 {
        if r > *t1 {
            return false;
        }
        if r > *t0 {
            *t0 = r;
        }
    } else {
        if r < *t0 {
            return false;
        }
        if r < *t1 {
            *t1 = r;
        }
    }
    true
}

fn points_are_same(a: FPoint, b: FPoint) -> bool {
    const EPS: f64 = 1e-6;
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS
}

fn path_has_non_degenerate_span(path: &[FPoint]) -> bool {
    path.windows(2)
        .any(|segment| !points_are_same(segment[0], segment[1]))
}

fn nudge_for_direction(point: FPoint, direction: Direction) -> FPoint {
    const DIRECT_STUB: f64 = 1.0;
    match direction {
        Direction::TopDown | Direction::BottomTop => FPoint::new(point.x, point.y + DIRECT_STUB),
        Direction::LeftRight | Direction::RightLeft => FPoint::new(point.x + DIRECT_STUB, point.y),
    }
}

/// Compute a canonical 4-point self-loop path.
///
/// Both exit and entry are offset from the corners along the node face.
/// The loop extends between their positions so it does not reach the
/// node border.  The terminal segment is axis-aligned (horizontal for
/// TD/BT, vertical for LR/RL).
///
/// For diamond and hexagon shapes, attachment points are placed on the actual
/// shape border rather than the bounding rect corners.
fn canonical_self_loop_path(
    rect: &FRect,
    raw_points: &[FPoint],
    direction: Direction,
    shape: &Shape,
) -> Vec<FPoint> {
    const MIN_PAD: f64 = 8.0;

    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;

    let (exit, entry) = self_loop_anchor_points(rect, direction, shape);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let loop_x = raw_points
                .iter()
                .map(|p| p.x)
                .fold(right, f64::max)
                .max(right + MIN_PAD);
            vec![
                exit,
                FPoint::new(loop_x, exit.y),
                FPoint::new(loop_x, entry.y),
                entry,
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            let loop_y = raw_points
                .iter()
                .map(|p| p.y)
                .fold(bottom, f64::max)
                .max(bottom + MIN_PAD);
            vec![
                exit,
                FPoint::new(exit.x, loop_y),
                FPoint::new(entry.x, loop_y),
                entry,
            ]
        }
    }
}

/// Compute exit and entry attachment points for a self-loop.
///
/// Both points are offset from the bounding-rect corners along the node
/// face so the loop does not touch the border.
fn self_loop_anchor_points(rect: &FRect, direction: Direction, shape: &Shape) -> (FPoint, FPoint) {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    // Default face offset for rectangular shapes.
    let face_offset = |face_len: f64| (8.0_f64).min(face_len / 4.0);

    match shape {
        Shape::Diamond => {
            // Diamond edge parameter t=0.25 (exit) / t=0.75 (entry) gives
            // 75 % height span while staying on the border.
            let w8 = rect.width / 8.0;
            let h8 = rect.height / 8.0;
            match direction {
                Direction::TopDown => (
                    FPoint::new(right - 3.0 * w8, top + h8),
                    FPoint::new(right - 3.0 * w8, bottom - h8),
                ),
                Direction::BottomTop => (
                    FPoint::new(right - 3.0 * w8, bottom - h8),
                    FPoint::new(right - 3.0 * w8, top + h8),
                ),
                Direction::LeftRight => (
                    FPoint::new(right - w8, bottom - 3.0 * h8),
                    FPoint::new(left + w8, bottom - 3.0 * h8),
                ),
                Direction::RightLeft => (
                    FPoint::new(left + w8, bottom - 3.0 * h8),
                    FPoint::new(right - w8, bottom - 3.0 * h8),
                ),
            }
        }
        Shape::Hexagon => {
            // Hexagon right face: upper-right edge (right-indent,top)→(right,cy)
            // and lower-right edge (right,cy)→(right-indent,bottom).
            // At y = top+h8 (t=0.25), border x = right - 3*indent/4.
            let indent = rect.width * 0.2;
            let border_inset = 3.0 * indent / 4.0;
            let h8 = rect.height / 8.0;
            match direction {
                Direction::TopDown => (
                    FPoint::new(right - border_inset, top + h8),
                    FPoint::new(right - border_inset, bottom - h8),
                ),
                Direction::BottomTop => (
                    FPoint::new(right - border_inset, bottom - h8),
                    FPoint::new(right - border_inset, top + h8),
                ),
                Direction::LeftRight => (
                    FPoint::new(right - border_inset, bottom - h8),
                    FPoint::new(left + border_inset, bottom - h8),
                ),
                Direction::RightLeft => (
                    FPoint::new(left + border_inset, bottom - h8),
                    FPoint::new(right - border_inset, bottom - h8),
                ),
            }
        }
        _ => match direction {
            Direction::TopDown => {
                let fo = face_offset(rect.height);
                (
                    FPoint::new(right, top + fo),
                    FPoint::new(right, bottom - fo),
                )
            }
            Direction::BottomTop => {
                let fo = face_offset(rect.height);
                (
                    FPoint::new(right, bottom - fo),
                    FPoint::new(right, top + fo),
                )
            }
            Direction::LeftRight => {
                let fo = face_offset(rect.width);
                (
                    FPoint::new(right - fo, bottom),
                    FPoint::new(left + fo, bottom),
                )
            }
            Direction::RightLeft => {
                let fo = face_offset(rect.width);
                (
                    FPoint::new(left + fo, bottom),
                    FPoint::new(right - fo, bottom),
                )
            }
        },
    }
}

fn snap_to_primary_face(
    point: FPoint,
    rect: &FRect,
    direction: Direction,
    is_source: bool,
) -> FPoint {
    match direction {
        Direction::TopDown => {
            let y = if is_source {
                rect.y + rect.height
            } else {
                rect.y
            };
            FPoint::new(point.x, y)
        }
        Direction::BottomTop => {
            let y = if is_source {
                rect.y
            } else {
                rect.y + rect.height
            };
            FPoint::new(point.x, y)
        }
        Direction::LeftRight => {
            let x = if is_source {
                rect.x + rect.width
            } else {
                rect.x
            };
            FPoint::new(x, point.y)
        }
        Direction::RightLeft => {
            let x = if is_source {
                rect.x
            } else {
                rect.x + rect.width
            };
            FPoint::new(x, point.y)
        }
    }
}

/// Populate `label_geometry` on every routed edge that has a non-empty label.
///
/// Uses the diagram's edge list to look up label text (by `routed_edge.index`
/// into `diagram.edges`), then measures the padded rectangle via
/// `metrics.edge_label_dimensions`. The `track` field is always `0` — lane
/// assignment is deferred to the label-lane pass (plan 0145 PR 3, task 3.5).
fn populate_label_geometry(
    edges: &mut [RoutedEdgeGeometry],
    diagram: &Graph,
    metrics: &ProportionalTextMetrics,
) {
    for routed_edge in edges.iter_mut() {
        let Some(center) = routed_edge.label_position else {
            continue;
        };
        let diagram_edge = diagram.edges.get(routed_edge.index);
        let Some(label) = diagram_edge.and_then(|e| e.label.as_deref()) else {
            continue;
        };
        if label.is_empty() {
            continue;
        }
        // Plan 0147 Task 1.6: prefer the pre-engine wrap artifact when present
        // so the reserved rect matches what SVG text and the MMDS replay emit.
        let (w, h) = match diagram_edge.and_then(|e| e.wrapped_label_lines.as_deref()) {
            Some(lines) => metrics.edge_label_dimensions_wrapped(lines),
            None => metrics.edge_label_dimensions(label),
        };
        let side = routed_edge.label_side.unwrap_or(EdgeLabelSide::Center);
        routed_edge.label_geometry = Some(EdgeLabelGeometry {
            center,
            rect: FRect::new(center.x - w / 2.0, center.y - h / 2.0, w, h),
            padding: (metrics.label_padding_x, metrics.label_padding_y),
            side,
            track: 0,
        });
    }
}
