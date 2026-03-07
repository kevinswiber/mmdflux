//! Graph-family routing stage.
//!
//! Produces `RoutedGraphGeometry` (Layer 2) from `GraphGeometry` (Layer 1).
//! Supports four modes:
//! - `DirectRoute`: Build source→target direct paths.
//! - `PolylineRoute`: Build edge paths from layout hints + node positions.
//! - `EngineProvided`: Use engine-provided paths directly.
//! - `OrthogonalRoute`: Produce axis-aligned (right-angle) edge paths.

use super::geometry::*;
use super::render::orthogonal_router::{
    OrthogonalRoutingOptions, build_path_from_hints, route_edges_orthogonal, snap_path_to_grid,
};
use super::render::route_policy::effective_edge_direction;
use crate::diagram::EdgeRouting;
use crate::graph::{Diagram, Direction};

/// Route graph geometry to produce fully-routed edge paths.
///
/// Consumes engine-agnostic `GraphGeometry` and produces `RoutedGraphGeometry`
/// with polyline paths for every edge.
pub fn route_graph_geometry(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> RoutedGraphGeometry {
    let port_attachments = super::render::text_routing_core::compute_port_attachments_from_geometry(
        &diagram.edges,
        geometry,
        diagram.direction,
    );

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
            // Pre-compute per-edge backward lane indices for staggering.
            let backward_lane_indices: Vec<usize> = {
                let mut counter = 0usize;
                geometry
                    .edges
                    .iter()
                    .map(|edge| {
                        if geometry.reversed_edges.contains(&edge.index)
                            && geometry.enhanced_backward_routing
                        {
                            let idx = counter;
                            counter += 1;
                            idx
                        } else {
                            0
                        }
                    })
                    .collect()
            };

            geometry
                .edges
                .iter()
                .enumerate()
                .map(|(i, edge)| {
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
                    // Snap forward-edge endpoints to primary departure/arrival faces
                    // so intersect_svg_rect sees a strong primary-axis approach angle.
                    // Use the effective edge direction (accounting for subgraph overrides).
                    let path = if !is_backward && path.len() >= 2 {
                        snap_path_endpoints_to_faces(&path, edge, geometry, edge_direction)
                    } else {
                        path
                    };
                    // Only use face-based channel routing for backward edges that
                    // have intermediate nodes obstructing the corridor. Short backward
                    // edges (adjacent nodes, no obstructions) get a small port offset
                    // to differentiate from the forward edge without detouring.
                    let needs_channel = is_backward
                        && geometry.enhanced_backward_routing
                        && has_corridor_obstructions(edge, geometry, edge_direction);
                    let needs_short_offset = is_backward
                        && (geometry.enhanced_backward_routing
                            || edge_direction != diagram.direction);
                    let path = if needs_channel {
                        build_backward_channel_path(
                            path,
                            edge,
                            geometry,
                            edge_direction,
                            backward_lane_indices[i],
                        )
                    } else if needs_short_offset {
                        apply_short_backward_port_offset(path, edge, geometry, edge_direction)
                    } else {
                        path
                    };
                    // Recompute label position for channel-routed backward edges
                    // so labels track the routed path, not the stale
                    // layout_path_hint midpoint.
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
                    }
                })
                .collect()
        }
    };

    let self_edges: Vec<RoutedSelfEdge> = geometry
        .self_edges
        .iter()
        .map(|se| RoutedSelfEdge {
            node_id: se.node_id.clone(),
            edge_index: se.edge_index,
            path: se.points.clone(),
        })
        .collect();

    let bounds = recompute_routed_bounds(geometry, &edges, &self_edges);

    RoutedGraphGeometry {
        nodes: geometry.nodes.clone(),
        edges,
        subgraphs: geometry.subgraphs.clone(),
        self_edges,
        direction: geometry.direction,
        bounds,
    }
}

/// Recompute bounds as the union of the original layout bounds with all
/// routed edge paths and self-edge paths.
///
/// The layout bounds seed already covers node rects and subgraph rects.
/// This expands that envelope to include any path points that routing
/// pushed beyond the layout box (e.g. backward channels).
fn recompute_routed_bounds(
    geometry: &GraphGeometry,
    edges: &[RoutedEdgeGeometry],
    self_edges: &[RoutedSelfEdge],
) -> FRect {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    // Seed from layout bounds (covers nodes and subgraphs).
    let b = geometry.bounds;
    min_x = min_x.min(b.x);
    min_y = min_y.min(b.y);
    max_x = max_x.max(b.x + b.width);
    max_y = max_y.max(b.y + b.height);

    // Expand for all routed edge path points.
    for edge in edges {
        for p in &edge.path {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    // Expand for self-edge paths.
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

fn build_direct_path(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: crate::graph::Direction,
) -> Vec<FPoint> {
    // Self loops already have dedicated geometry in `self_edges`.
    // If they appear in regular edges, keep the existing hint-driven behavior.
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

    // Snap to primary faces before collision detection so the line-of-sight
    // check uses face-to-face geometry (not center-to-center).
    let start = snap_to_primary_face(start, &from_node.rect, direction, true);
    let end = snap_to_primary_face(end, &to_node.rect, direction, false);

    if direct_segment_crosses_non_endpoint_nodes(start, end, edge, geometry) {
        return build_path_from_hints(edge, geometry);
    }

    vec![start, end]
}

/// Apply a small port offset to short backward edges so they don't overlap
/// the forward edge. Instead of routing through a face-based channel,
/// this shifts the endpoints slightly to one side of center, creating
/// a visually distinct path while keeping the edge compact.
///
/// **TD/BT:** Shifts endpoints right of center.
/// **LR/RL:** Shifts endpoints to lower side-face lanes to avoid centerline overlap.
fn apply_short_backward_port_offset(
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
            // Offset to the right of center, capped at 1/3 of the narrower node.
            let max_offset = (sr.width.min(tr.width) / 3.0).min(20.0);
            let offset = max_offset.max(8.0);
            let src_x = sr.center_x() + offset;
            let tgt_x = tr.center_x() + offset;
            let src_y = sr.center_y();
            let tgt_y = tr.center_y();
            // Build a 3-point zigzag: source → midpoint → target.
            // The midpoint sits between the two ranks at the average x,
            // creating a subtle diagonal that signals backward direction.
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

/// Check if a backward edge has intermediate nodes in its routing corridor
/// that would obstruct a direct path between source and target.
///
/// A "short" backward edge (no obstructions) can use the layout hint path
/// which provides natural port offset. A "long" backward edge (with
/// obstructing intermediate nodes) needs face-based channel routing.
fn has_corridor_obstructions(
    edge: &LayoutEdge,
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
            let (min_y, max_y) = source_target_rank_range_y(from_rect, to_rect);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                // Node is vertically between source and target AND
                // horizontally overlaps the routing corridor.
                cy > min_y
                    && cy < max_y
                    && node.rect.x < corridor_right
                    && node_right > corridor_left
            })
        }
        Direction::LeftRight | Direction::RightLeft => {
            let corridor_top = sr.y.min(tr.y);
            let corridor_bottom = (sr.y + sr.height).max(tr.y + tr.height);
            let (min_x, max_x) = source_target_rank_range_x(from_rect, to_rect);
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

/// Build a backward edge path using face-based attachment points.
///
/// Instead of modifying the layout engine's hint path (which typically
/// overlaps the forward edge), this builds a 4-point channel path from
/// scratch with endpoints on the canonical backward face:
///
/// **TD/BT:** Right face → channel lane → right face
/// **LR/RL:** Bottom face → channel lane → bottom face
///
/// This mirrors the orthogonal router's backward routing approach,
/// keeping backward edges compact and consistent across all edge styles.
fn build_backward_channel_path(
    _path: Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    backward_lane_index: usize,
) -> Vec<FPoint> {
    /// Clearance between node face and backward channel lane.
    const CHANNEL_CLEARANCE: f64 = 8.0;
    /// Spacing between staggered backward edge lanes.
    const LANE_SPACING: f64 = 8.0;

    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return _path;
    };

    let lane_offset = CHANNEL_CLEARANCE + (backward_lane_index as f64) * LANE_SPACING;

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            // Exit source from right face, enter target from right face.
            let source_face_x = sr.x + sr.width;
            let target_face_x = tr.x + tr.width;
            let source_cy = sr.center_y();
            let target_cy = tr.center_y();

            // Channel lane clears source and target right faces plus any
            // intermediate nodes that sit between the source/target ranks.
            // This keeps long backward loops outside the working column and
            // avoids avoidable edge crossings with forward diagonals.
            let face_envelope = source_face_x.max(target_face_x);
            let (min_y, max_y) = source_target_rank_range_y(from_rect, to_rect);
            let mut lane_x = face_envelope + lane_offset;
            for node in geometry.nodes.values() {
                if node.id == edge.from || node.id == edge.to {
                    continue;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                if cy >= min_y && cy <= max_y {
                    lane_x = lane_x.max(node_right + lane_offset);
                }
            }

            vec![
                FPoint::new(source_face_x, source_cy),
                FPoint::new(lane_x, source_cy),
                FPoint::new(lane_x, target_cy),
                FPoint::new(target_face_x, target_cy),
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            // Exit source from bottom face, enter target from bottom face.
            let source_face_y = sr.y + sr.height;
            let target_face_y = tr.y + tr.height;
            let source_cx = sr.center_x();
            let target_cx = tr.center_x();

            let face_envelope = source_face_y.max(target_face_y);
            let corridor_top = sr.y.min(tr.y);
            let (min_x, max_x) = source_target_rank_range_x(from_rect, to_rect);
            let mut lane_y = face_envelope + lane_offset;
            for node in geometry.nodes.values() {
                if node.id == edge.from || node.id == edge.to {
                    continue;
                }
                let cx = node.rect.center_x();
                let node_bottom = node.rect.y + node.rect.height;
                if cx >= min_x && cx <= max_x && node.rect.y < lane_y && node_bottom > corridor_top
                {
                    lane_y = lane_y.max(node_bottom + lane_offset);
                }
            }

            vec![
                FPoint::new(source_cx, source_face_y),
                FPoint::new(source_cx, lane_y),
                FPoint::new(target_cx, lane_y),
                FPoint::new(target_cx, target_face_y),
            ]
        }
    }
}

/// Get the y-range spanned by the source and target nodes (for TD/BT).
fn source_target_rank_range_y(from_rect: Option<FRect>, to_rect: Option<FRect>) -> (f64, f64) {
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for rect in [from_rect, to_rect].iter().flatten() {
        min_y = min_y.min(rect.y);
        max_y = max_y.max(rect.y + rect.height);
    }
    (min_y, max_y)
}

/// Get the x-range spanned by the source and target nodes (for LR/RL).
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
    // Treat near-border grazing as a collision so "straight" lines do not
    // visually ride along unrelated node borders after rasterization.
    const BORDER_CLEARANCE_MARGIN: f64 = -0.5;
    // TODO: This is currently O(V) per direct-routed edge (overall O(E*V)).
    // If large graphs make this hot, replace with a spatial index over node rects.
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

/// Compute the arc-length midpoint of a polyline path.
///
/// Walks the path segments, accumulates distances, and interpolates to
/// the point at 50% of the total arc length. This produces a visually
/// centered position along the path regardless of point distribution.
fn arc_length_midpoint(path: &[FPoint]) -> Option<FPoint> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return Some(path[0]);
    }
    let total_len: f64 = path
        .windows(2)
        .map(|seg| point_distance(seg[0], seg[1]))
        .sum();
    if total_len <= 1e-6 {
        return path.get(path.len() / 2).copied();
    }
    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for seg in path.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let seg_len = point_distance(a, b);
        if seg_len <= 1e-6 {
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

/// Interpolate a point along a polyline path at a given arc-length distance.
fn interpolate_at_distance(path: &[FPoint], distance: f64) -> Option<FPoint> {
    if path.len() < 2 {
        return path.first().copied();
    }
    let mut traversed = 0.0;
    for seg in path.windows(2) {
        let seg_len = point_distance(seg[0], seg[1]);
        if seg_len <= 1e-6 {
            continue;
        }
        if traversed + seg_len >= distance {
            let t = (distance - traversed) / seg_len;
            return Some(FPoint::new(
                seg[0].x + (seg[1].x - seg[0].x) * t,
                seg[0].y + (seg[1].y - seg[0].y) * t,
            ));
        }
        traversed += seg_len;
    }
    path.last().copied()
}

/// Compute head and tail label positions from a routed edge path.
///
/// Head labels are positioned near the path end (target), tail labels near
/// the start (source), both offset perpendicular to the edge direction.
pub(crate) fn compute_end_label_positions(path: &[FPoint]) -> (Option<FPoint>, Option<FPoint>) {
    if path.len() < 2 {
        return (None, None);
    }

    let perpendicular_offset = 12.0; // px offset from path
    let along_fraction = 0.12; // 12% from endpoint

    let total_len: f64 = path
        .windows(2)
        .map(|seg| point_distance(seg[0], seg[1]))
        .sum();
    if total_len <= 1e-6 {
        return (None, None);
    }

    // Tail: near path start
    let tail = interpolate_at_distance(path, total_len * along_fraction).map(|p| {
        // Get direction at start
        let (dx, dy) = (path[1].x - path[0].x, path[1].y - path[0].y);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        // Perpendicular: rotate direction 90° (right side)
        FPoint::new(
            p.x + dy / len * perpendicular_offset,
            p.y - dx / len * perpendicular_offset,
        )
    });

    // Head: near path end
    let n = path.len();
    let head = interpolate_at_distance(path, total_len * (1.0 - along_fraction)).map(|p| {
        let (dx, dy) = (path[n - 1].x - path[n - 2].x, path[n - 1].y - path[n - 2].y);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        FPoint::new(
            p.x + dy / len * perpendicular_offset,
            p.y - dx / len * perpendicular_offset,
        )
    });

    (head, tail)
}

/// Look up the diagram edge and compute end label positions if head/tail labels are present.
pub(crate) fn compute_end_labels_for_edge(
    diagram: &Diagram,
    edge_index: usize,
    path: &[FPoint],
) -> (Option<FPoint>, Option<FPoint>) {
    let diagram_edge = diagram.edges.get(edge_index);
    let has_head = diagram_edge
        .map(|e| e.head_label.is_some())
        .unwrap_or(false);
    let has_tail = diagram_edge
        .map(|e| e.tail_label.is_some())
        .unwrap_or(false);
    if !has_head && !has_tail {
        return (None, None);
    }
    let (head_pos, tail_pos) = compute_end_label_positions(path);
    (
        if has_head { head_pos } else { None },
        if has_tail { tail_pos } else { None },
    )
}

fn points_are_same(a: FPoint, b: FPoint) -> bool {
    const EPS: f64 = 1e-6;
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS
}

fn path_has_non_degenerate_span(path: &[FPoint]) -> bool {
    path.windows(2)
        .any(|segment| !points_are_same(segment[0], segment[1]))
}

fn nudge_for_direction(point: FPoint, direction: crate::graph::Direction) -> FPoint {
    const DIRECT_STUB: f64 = 1.0;
    match direction {
        crate::graph::Direction::TopDown | crate::graph::Direction::BottomTop => {
            FPoint::new(point.x, point.y + DIRECT_STUB)
        }
        crate::graph::Direction::LeftRight | crate::graph::Direction::RightLeft => {
            FPoint::new(point.x + DIRECT_STUB, point.y)
        }
    }
}

/// Snap a point's primary-axis coordinate to the departure/arrival face of a node rect.
///
/// For sources, the primary face is the downstream face (TD→bottom, LR→right).
/// For targets, the primary face is the upstream face (TD→top, LR→left).
///
/// Only the primary axis is modified. The cross-axis coordinate is preserved
/// so the SVG renderer's `intersect_svg_rect` can compute proper port
/// distribution along the face. When the cross-axis falls outside the rect
/// bounds, `endpoint_attachment_is_invalid` triggers reclipping, which
/// naturally distributes fan-in/fan-out arrival points.
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

/// Snap edge path endpoints to primary departure/arrival faces.
///
/// Ensures that the first point sits on the source node's departure face
/// and the last point sits on the target node's arrival face. This gives
/// `intersect_svg_rect` a strong primary-axis approach angle so it reliably
/// finds the correct face (instead of clipping to a side face on fan-in edges).
fn snap_path_endpoints_to_faces(
    path: &[FPoint],
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Vec<FPoint> {
    let mut result = path.to_vec();

    // Snap source endpoint
    let source_rect = if let Some(sg_id) = &edge.from_subgraph {
        geometry.subgraphs.get(sg_id).map(|sg| sg.rect)
    } else {
        geometry.nodes.get(&edge.from).map(|n| n.rect)
    };
    if let Some(rect) = source_rect {
        result[0] = snap_to_primary_face(result[0], &rect, direction, true);
    }

    // Snap target endpoint
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

/// Preview helper: snap a float path to a deterministic grid.
///
/// Exposed for routed-geometry contract tests while orthogonal text integration
/// is still behind preview rollout.
pub fn snap_path_to_grid_preview(path: &[FPoint], scale_x: f64, scale_y: f64) -> Vec<FPoint> {
    snap_path_to_grid(path, scale_x, scale_y)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::diagram::EdgeRouting;

    fn simple_geometry() -> (Diagram, GraphGeometry) {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_edge(crate::graph::Edge::new("A", "B"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(50.0, 25.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(50.0, 75.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );

        let edges = vec![LayoutEdge {
            index: 0,
            from: "A".into(),
            to: "B".into(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: Some(vec![FPoint::new(50.0, 35.0), FPoint::new(50.0, 65.0)]),
            preserve_orthogonal_topology: false,
        }];

        let geom = GraphGeometry {
            nodes,
            edges,
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 100.0, 100.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        (diagram, geom)
    }

    #[test]
    fn polyline_route_produces_routed_edges() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

        assert_eq!(routed.nodes.len(), 2);
        assert_eq!(routed.edges.len(), 1);
        assert!(routed.edges[0].path.len() >= 2);
        assert!(!routed.edges[0].is_backward);
    }

    #[test]
    fn engine_provided_uses_layout_path_hints() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::EngineProvided);

        let edge = &routed.edges[0];
        assert_eq!(edge.path.len(), 2);
        // Face-snapped: x stays at 50 (clamped within rect), y snapped to faces
        assert_eq!(edge.path[0].x, 50.0);
        assert_eq!(edge.path[0].y, 45.0); // A bottom face
        assert_eq!(edge.path[1].x, 50.0);
        assert_eq!(edge.path[1].y, 75.0); // B top face
    }

    #[test]
    fn self_edges_are_routed() {
        let (diagram, mut geom) = simple_geometry();
        geom.self_edges.push(SelfEdgeGeometry {
            node_id: "A".into(),
            edge_index: 1,
            points: vec![
                FPoint::new(70.0, 15.0),
                FPoint::new(80.0, 15.0),
                FPoint::new(80.0, 35.0),
                FPoint::new(70.0, 35.0),
            ],
        });

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert_eq!(routed.self_edges.len(), 1);
        assert_eq!(routed.self_edges[0].path.len(), 4);
        assert_eq!(routed.self_edges[0].node_id, "A");
    }

    #[test]
    fn backward_edges_are_marked() {
        let (diagram, mut geom) = simple_geometry();
        geom.reversed_edges = vec![0];

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert!(routed.edges[0].is_backward);
    }

    #[test]
    fn fallback_path_from_node_centers_and_waypoints() {
        let (diagram, mut geom) = simple_geometry();
        // Remove layout_path_hint to force fallback
        geom.edges[0].layout_path_hint = None;
        geom.edges[0].waypoints = vec![FPoint::new(50.0, 50.0)];

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let path = &routed.edges[0].path;
        // Should be: A bottom face → waypoint → B top face (face-snapped)
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].x, 70.0); // A center_x (within rect bounds)
        assert_eq!(path[0].y, 45.0); // A bottom face
        assert_eq!(path[1].x, 50.0);
        assert_eq!(path[1].y, 50.0); // waypoint (unchanged)
        assert_eq!(path[2].x, 70.0); // B center_x (within rect bounds)
        assert_eq!(path[2].y, 75.0); // B top face
    }

    #[test]
    fn label_positions_are_preserved() {
        let (diagram, mut geom) = simple_geometry();
        geom.edges[0].label_position = Some(FPoint::new(55.0, 50.0));

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let lp = routed.edges[0].label_position.unwrap();
        assert_eq!(lp.x, 55.0);
        assert_eq!(lp.y, 50.0);
    }

    #[test]
    fn nodes_and_subgraphs_are_preserved() {
        let (diagram, mut geom) = simple_geometry();
        geom.subgraphs.insert(
            "sg1".into(),
            SubgraphGeometry {
                id: "sg1".into(),
                rect: FRect::new(10.0, 5.0, 80.0, 90.0),
                title: "Group".into(),
                depth: 0,
            },
        );

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert_eq!(routed.nodes.len(), 2);
        assert_eq!(routed.subgraphs.len(), 1);
        assert_eq!(routed.subgraphs["sg1"].title, "Group");
        assert_eq!(routed.direction, crate::graph::Direction::TopDown);
    }

    #[test]
    fn direct_route_produces_two_point_path() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        let path = &routed.edges[0].path;
        assert_eq!(path.len(), 2);
        // Face-snapped: A bottom face y=45, B top face y=75
        assert_eq!(path[0], FPoint::new(70.0, 45.0));
        assert_eq!(path[1], FPoint::new(70.0, 75.0));
    }

    #[test]
    fn direct_route_uses_effective_direction_for_override_nodes() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_edge(crate::graph::Edge::new("A", "B"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(0.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(100.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );

        let mut node_directions = HashMap::new();
        node_directions.insert("A".into(), crate::graph::Direction::LeftRight);
        node_directions.insert("B".into(), crate::graph::Direction::LeftRight);

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions,
            bounds: FRect::new(0.0, 0.0, 140.0, 20.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        assert_eq!(
            routed.edges[0].path,
            vec![FPoint::new(40.0, 10.0), FPoint::new(100.0, 10.0)]
        );
    }

    #[test]
    fn backward_short_offset_uses_effective_direction_for_override_nodes() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_edge(crate::graph::Edge::new("B", "A"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(0.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(100.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );

        let mut node_directions = HashMap::new();
        node_directions.insert("A".into(), crate::graph::Direction::LeftRight);
        node_directions.insert("B".into(), crate::graph::Direction::LeftRight);

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "B".into(),
                to: "A".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions,
            bounds: FRect::new(0.0, 0.0, 140.0, 20.0),
            reversed_edges: vec![0],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: true,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert_eq!(
            routed.edges[0].path,
            vec![
                FPoint::new(100.0, 18.0),
                FPoint::new(70.0, 18.0),
                FPoint::new(40.0, 18.0),
            ]
        );
    }

    #[test]
    fn orthogonal_backward_override_uses_side_faces() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_edge(crate::graph::Edge::new("B", "A"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(0.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(100.0, 0.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );

        let mut node_directions = HashMap::new();
        node_directions.insert("A".into(), crate::graph::Direction::LeftRight);
        node_directions.insert("B".into(), crate::graph::Direction::LeftRight);

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "B".into(),
                to: "A".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions,
            bounds: FRect::new(0.0, 0.0, 140.0, 20.0),
            reversed_edges: vec![0],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let path = &routed.edges[0].path;
        assert!(path.len() >= 2);
        assert!(
            (path[0].x - 100.0).abs() <= 0.001,
            "source should leave left face"
        );
        assert!(
            (path[path.len() - 1].x - 40.0).abs() <= 0.001,
            "target should enter right face"
        );
        assert!(
            path[0].y < 20.0 && path[path.len() - 1].y < 20.0,
            "compact short path should stay below center but on side faces"
        );
    }

    #[test]
    fn direct_route_uses_hint_when_endpoints_coincide() {
        let (diagram, mut geom) = simple_geometry();
        geom.nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(50.0, 25.0, 40.0, 20.0), // same center as A
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );
        geom.edges[0].layout_path_hint =
            Some(vec![FPoint::new(60.0, 35.0), FPoint::new(80.0, 35.0)]);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        // Hint path is used but face-snapped: A bottom=45, B top=25
        assert_eq!(
            routed.edges[0].path,
            vec![FPoint::new(60.0, 45.0), FPoint::new(80.0, 25.0)]
        );
    }

    #[test]
    fn direct_route_nudges_when_endpoints_coincide_without_hint() {
        let (diagram, mut geom) = simple_geometry();
        geom.nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(50.0, 25.0, 40.0, 20.0), // same center as A
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );
        geom.edges[0].layout_path_hint = None;
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        let path = &routed.edges[0].path;
        assert_eq!(path.len(), 2);
        assert_ne!(path[0], path[1]);
    }

    #[test]
    fn direct_route_falls_back_when_straight_segment_crosses_node_interior() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_node(crate::graph::Node::new("C"));
        diagram.add_edge(crate::graph::Edge::new("A", "C"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(0.0, 0.0, 20.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(60.0, 60.0, 40.0, 40.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );
        nodes.insert(
            "C".into(),
            PositionedNode {
                id: "C".into(),
                rect: FRect::new(120.0, 120.0, 20.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "C".into(),
                parent: None,
            },
        );

        let direct_hint = vec![
            FPoint::new(10.0, 20.0),
            FPoint::new(170.0, 20.0),
            FPoint::new(170.0, 120.0),
            FPoint::new(130.0, 120.0),
        ];

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "C".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: Some(direct_hint.clone()),
                preserve_orthogonal_topology: false,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 200.0, 200.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        assert_eq!(routed.edges[0].path, direct_hint);
    }

    #[test]
    fn direct_route_falls_back_when_straight_segment_grazes_node_border() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_node(crate::graph::Node::new("C"));
        diagram.add_edge(crate::graph::Edge::new("A", "C"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(0.0, 0.0, 20.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        // Left border sits exactly on the direct A->C centerline at x=10.
        nodes.insert(
            "B".into(),
            PositionedNode {
                id: "B".into(),
                rect: FRect::new(10.0, 60.0, 40.0, 40.0),
                shape: crate::graph::Shape::Rectangle,
                label: "B".into(),
                parent: None,
            },
        );
        nodes.insert(
            "C".into(),
            PositionedNode {
                id: "C".into(),
                rect: FRect::new(0.0, 120.0, 20.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "C".into(),
                parent: None,
            },
        );

        let fallback_hint = vec![
            FPoint::new(0.0, 20.0),
            FPoint::new(0.0, 70.0),
            FPoint::new(0.0, 120.0),
        ];

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "C".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: Some(fallback_hint.clone()),
                preserve_orthogonal_topology: false,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 200.0, 200.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        assert_eq!(routed.edges[0].path, fallback_hint);
    }

    #[test]
    fn orthogonal_router_preview_paths_are_axis_aligned() {
        let (diagram, geom) = simple_geometry();
        let orthogonal =
            route_edges_orthogonal(&diagram, &geom, OrthogonalRoutingOptions::preview());

        assert!(!orthogonal.is_empty());
        for edge in orthogonal.iter().filter(|edge| !edge.is_backward) {
            assert!(
                edge.path
                    .windows(2)
                    .all(|seg| seg[0].x == seg[1].x || seg[0].y == seg[1].y)
            );
        }
    }

    #[test]
    fn snap_path_to_grid_deterministic_and_preserves_endpoints() {
        let input = vec![
            FPoint::new(5.4, 8.6),
            FPoint::new(5.4, 12.3),
            FPoint::new(14.7, 12.3),
        ];
        let snapped = snap_path_to_grid(&input, 1.0, 1.0);

        assert_eq!(snapped.first(), Some(&FPoint::new(5.0, 9.0)));
        assert_eq!(snapped.last(), Some(&FPoint::new(15.0, 12.0)));
        assert_eq!(snapped, snap_path_to_grid(&input, 1.0, 1.0));
    }

    #[test]
    fn head_label_near_path_end() {
        // Vertical path from (50, 0) to (50, 100)
        let path = vec![FPoint::new(50.0, 0.0), FPoint::new(50.0, 100.0)];
        let (head, _tail) = compute_end_label_positions(&path);

        let head = head.unwrap();
        assert!(head.y > 80.0, "head near end, got y={}", head.y);
        // Perpendicular offset: for vertical path, offset is in x direction
        assert!(
            (head.x - 50.0).abs() > 5.0,
            "head offset from path, got x={}",
            head.x
        );
    }

    #[test]
    fn tail_label_near_path_start() {
        let path = vec![FPoint::new(50.0, 0.0), FPoint::new(50.0, 100.0)];
        let (_head, tail) = compute_end_label_positions(&path);

        let tail = tail.unwrap();
        assert!(tail.y < 20.0, "tail near start, got y={}", tail.y);
    }

    #[test]
    fn empty_path_returns_none() {
        let (head, tail) = compute_end_label_positions(&[]);
        assert!(head.is_none());
        assert!(tail.is_none());
    }

    #[test]
    fn single_point_path_returns_none() {
        let (head, tail) = compute_end_label_positions(&[FPoint::new(50.0, 50.0)]);
        assert!(head.is_none());
        assert!(tail.is_none());
    }

    #[test]
    fn routing_populates_head_label_position() {
        let (mut diagram, geom) = simple_geometry();
        diagram.edges[0].head_label = Some("1..*".to_string());
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert!(
            routed.edges[0].head_label_position.is_some(),
            "head_label_position should be populated when edge has head_label"
        );
        assert!(
            routed.edges[0].tail_label_position.is_none(),
            "tail_label_position should be None when edge has no tail_label"
        );
    }

    #[test]
    fn routing_populates_tail_label_position() {
        let (mut diagram, geom) = simple_geometry();
        diagram.edges[0].tail_label = Some("source".to_string());
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert!(
            routed.edges[0].tail_label_position.is_some(),
            "tail_label_position should be populated when edge has tail_label"
        );
        assert!(
            routed.edges[0].head_label_position.is_none(),
            "head_label_position should be None when edge has no head_label"
        );
    }

    #[test]
    fn routing_no_end_labels_by_default() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        assert!(routed.edges[0].head_label_position.is_none());
        assert!(routed.edges[0].tail_label_position.is_none());
    }

    #[test]
    fn route_graph_geometry_includes_ports_polyline() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let edge = &routed.edges[0];
        let src = edge
            .source_port
            .as_ref()
            .expect("source_port should be populated");
        let tgt = edge
            .target_port
            .as_ref()
            .expect("target_port should be populated");
        assert_eq!(src.face, PortFace::Bottom);
        assert!((src.fraction - 0.5).abs() < 0.01);
        assert_eq!(tgt.face, PortFace::Top);
        assert!((tgt.fraction - 0.5).abs() < 0.01);
    }

    #[test]
    fn self_edge_routed_separately_without_ports() {
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_edge(crate::graph::Edge::new("A", "A"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            PositionedNode {
                id: "A".into(),
                rect: FRect::new(50.0, 50.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "A".into(),
                parent: None,
            },
        );
        let geom = GraphGeometry {
            nodes,
            edges: vec![],
            subgraphs: HashMap::new(),
            self_edges: vec![SelfEdgeGeometry {
                node_id: "A".into(),
                edge_index: 0,
                points: vec![
                    FPoint::new(70.0, 40.0),
                    FPoint::new(80.0, 40.0),
                    FPoint::new(80.0, 60.0),
                    FPoint::new(70.0, 60.0),
                ],
            }],
            direction: crate::graph::Direction::TopDown,
            node_directions: {
                let mut m = HashMap::new();
                m.insert("A".to_string(), crate::graph::Direction::TopDown);
                m
            },
            bounds: FRect::new(0.0, 0.0, 100.0, 100.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        // Self-edges go to self_edges, not edges
        assert_eq!(routed.self_edges.len(), 1);
        assert_eq!(routed.edges.len(), 0);
        // RoutedSelfEdge has no port fields - confirmed by the type system
    }

    #[test]
    fn route_graph_geometry_includes_ports_orthogonal() {
        let (diagram, geom) = simple_geometry();
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let edge = &routed.edges[0];
        assert!(
            edge.source_port.is_some(),
            "source_port should be populated for orthogonal"
        );
        assert!(
            edge.target_port.is_some(),
            "target_port should be populated for orthogonal"
        );
    }

    /// Routed bounds must cover all edge path points, even when routing
    /// pushes paths beyond the original layout bounds (e.g. backward channels).
    #[test]
    fn routed_bounds_cover_all_edge_path_points() {
        // Build a 3-node TD diagram with a backward edge whose channel
        // extends beyond the tight layout bounds.
        let mut diagram = Diagram::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("B"));
        diagram.add_node(crate::graph::Node::new("C"));
        diagram.add_edge(crate::graph::Edge::new("A", "B"));
        diagram.add_edge(crate::graph::Edge::new("B", "C"));
        diagram.add_edge(crate::graph::Edge::new("C", "A")); // backward

        let mut nodes = HashMap::new();
        for (id, y) in [("A", 10.0), ("B", 50.0), ("C", 90.0)] {
            nodes.insert(
                id.to_string(),
                PositionedNode {
                    id: id.to_string(),
                    rect: FRect::new(10.0, y, 40.0, 20.0), // right edge at 50
                    shape: crate::graph::Shape::Rectangle,
                    label: id.to_string(),
                    parent: None,
                },
            );
        }

        let edges = vec![
            LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            },
            LayoutEdge {
                index: 1,
                from: "B".into(),
                to: "C".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            },
            LayoutEdge {
                index: 2,
                from: "C".into(),
                to: "A".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
            },
        ];

        let geom = GraphGeometry {
            nodes,
            edges,
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions: HashMap::new(),
            // Tight bounds: right edge of nodes is at x=50, channel needs x>=58
            bounds: FRect::new(0.0, 0.0, 55.0, 120.0),
            reversed_edges: vec![2], // C->A is backward
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: true,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

        // Verify all path points are within the recomputed bounds.
        let b = routed.bounds;
        let eps = 0.001;
        for edge in &routed.edges {
            for p in &edge.path {
                assert!(
                    p.x >= b.x - eps
                        && p.x <= b.x + b.width + eps
                        && p.y >= b.y - eps
                        && p.y <= b.y + b.height + eps,
                    "path point ({:.1}, {:.1}) outside bounds {:?} for edge {}->{}",
                    p.x,
                    p.y,
                    b,
                    edge.from,
                    edge.to
                );
            }
        }
    }
}
