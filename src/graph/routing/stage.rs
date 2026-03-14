//! Graph-family route execution and path shaping helpers.

use super::float_core::compute_port_attachments_from_geometry;
use super::labels::{arc_length_midpoint, compute_end_labels_for_edge};
use super::orthogonal::{OrthogonalRoutingOptions, build_path_from_hints, route_edges_orthogonal};
use crate::graph::direction_policy::effective_edge_direction;
use crate::graph::geometry::{
    GraphGeometry, LayoutEdge, RoutedEdgeGeometry, RoutedGraphGeometry, RoutedSelfEdge,
};
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Graph};

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
                    let path = if !is_backward && path.len() >= 2 {
                        snap_path_endpoints_to_faces(&path, edge, geometry, edge_direction)
                    } else {
                        path
                    };
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

pub(crate) fn has_corridor_obstructions(
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

pub(crate) fn build_backward_channel_path(
    path: Vec<FPoint>,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
    backward_lane_index: usize,
) -> Vec<FPoint> {
    const CHANNEL_CLEARANCE: f64 = 8.0;
    const LANE_SPACING: f64 = 8.0;

    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return path;
    };

    let lane_offset = CHANNEL_CLEARANCE + (backward_lane_index as f64) * LANE_SPACING;

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let source_face_x = sr.x + sr.width;
            let target_face_x = tr.x + tr.width;
            let source_cy = sr.center_y();
            let target_cy = tr.center_y();

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
