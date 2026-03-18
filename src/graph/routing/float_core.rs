//! Graph-owned float routing primitives shared by engine and render code.

use std::collections::HashMap;

use crate::graph::attachment::{
    AttachmentCandidate, AttachmentSide, EdgePort, Face, LARGE_HORIZONTAL_OFFSET_THRESHOLD,
    edge_faces, plan_attachment_candidates, point_on_face_float,
};
use crate::graph::geometry::GraphGeometry;
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Edge, Shape, Stroke};

/// Compute port attachments for all edges using float-coordinate `GraphGeometry`.
///
/// Called from the routing stage to make port data available in
/// `RoutedEdgeGeometry` for MMDS serialization.
pub(crate) fn compute_port_attachments_from_geometry(
    edges: &[Edge],
    geometry: &GraphGeometry,
    fallback_direction: Direction,
) -> HashMap<usize, (Option<EdgePort>, Option<EdgePort>)> {
    let mut candidates: Vec<AttachmentCandidate> = Vec::with_capacity(edges.len() * 2);

    for (idx, edge) in edges.iter().enumerate() {
        if edge.from == edge.to || edge.stroke == Stroke::Invisible {
            continue;
        }

        let src_node = match geometry.nodes.get(&edge.from) {
            Some(n) => n,
            None => continue,
        };
        let tgt_node = match geometry.nodes.get(&edge.to) {
            Some(n) => n,
            None => continue,
        };

        let tgt_center = FPoint::new(tgt_node.rect.center_x(), tgt_node.rect.center_y());
        let src_center = FPoint::new(src_node.rect.center_x(), src_node.rect.center_y());

        // Find the matching LayoutEdge for waypoint info
        let layout_edge = geometry.edges.iter().find(|e| e.index == idx);
        let waypoints = layout_edge.map(|le| &le.waypoints);

        let src_approach = waypoints
            .and_then(|wps| wps.first().copied())
            .unwrap_or(tgt_center);
        let tgt_approach = waypoints
            .and_then(|wps| wps.last().copied())
            .unwrap_or(src_center);

        let edge_dir = geometry
            .node_directions
            .get(&edge.from)
            .copied()
            .unwrap_or(fallback_direction);

        let is_backward = geometry.reversed_edges.contains(&idx);

        let (src_face, tgt_face) = edge_faces(edge_dir, is_backward);

        let src_cross = match src_face {
            Face::Top | Face::Bottom => src_approach.x,
            Face::Left | Face::Right => src_approach.y,
        };
        let tgt_cross = match tgt_face {
            Face::Top | Face::Bottom => tgt_approach.x,
            Face::Left | Face::Right => tgt_approach.y,
        };

        let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
        let tgt_id = edge.to_subgraph.as_deref().unwrap_or(edge.to.as_str());

        candidates.push(AttachmentCandidate {
            edge_index: idx,
            node_id: src_id.to_string(),
            side: AttachmentSide::Source,
            face: src_face,
            cross_axis: src_cross,
        });
        candidates.push(AttachmentCandidate {
            edge_index: idx,
            node_id: tgt_id.to_string(),
            side: AttachmentSide::Target,
            face: tgt_face,
            cross_axis: tgt_cross,
        });
    }

    let plan = plan_attachment_candidates(candidates);

    let mut result: HashMap<usize, (Option<EdgePort>, Option<EdgePort>)> = HashMap::new();
    for (edge_index, attachments) in plan.attachments() {
        let edge = &edges[*edge_index];

        let source_port = attachments.source.map(|att| {
            let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
            let src_rect = geometry
                .nodes
                .get(src_id)
                .map(|n| n.rect)
                .unwrap_or(FRect::new(0.0, 0.0, 0.0, 0.0));
            let group_size = plan.group_size(src_id, att.face);
            EdgePort {
                face: att.face.to_port_face(),
                fraction: att.fraction,
                position: point_on_face_float(src_rect, att.face, att.fraction),
                group_size,
            }
        });

        let target_port = attachments.target.map(|att| {
            let tgt_id = edge.to_subgraph.as_deref().unwrap_or(edge.to.as_str());
            let tgt_rect = geometry
                .nodes
                .get(tgt_id)
                .map(|n| n.rect)
                .unwrap_or(FRect::new(0.0, 0.0, 0.0, 0.0));
            let group_size = plan.group_size(tgt_id, att.face);
            EdgePort {
                face: att.face.to_port_face(),
                fraction: att.fraction,
                position: point_on_face_float(tgt_rect, att.face, att.fraction),
                group_size,
            }
        });

        result.insert(*edge_index, (source_port, target_port));
    }

    result
}

/// Build an orthogonal polyline in float space from start to end through optional waypoints.
///
/// Diagonal spans are split into two elbows using midpoint routing on the
/// diagram's primary axis to keep paths axis-aligned and symmetric.
pub(crate) const ROUTE_ALIGN_EPS: f64 = 0.5;
pub(crate) const ROUTE_POINT_EPS: f64 = 0.000_001;
const MIN_TERMINAL_SUPPORT: f64 = 8.0;

pub fn build_orthogonal_path_float(
    start: FPoint,
    end: FPoint,
    direction: Direction,
    waypoints: &[FPoint],
) -> Vec<FPoint> {
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let mut control_points: Vec<FPoint> = Vec::with_capacity(waypoints.len() + 2);
    control_points.push(start);
    control_points.extend_from_slice(waypoints);
    control_points.push(end);

    let mut output: Vec<FPoint> = Vec::with_capacity(control_points.len() * 3);
    output.push(start);
    let span_count = control_points.len().saturating_sub(1);

    for (span_idx, target) in control_points.into_iter().skip(1).enumerate() {
        let current = output.last().copied().unwrap_or(start);
        let is_first_span = span_idx == 0;
        let is_last_span = span_idx + 1 == span_count;

        if (current.x - target.x).abs() < ROUTE_POINT_EPS
            && (current.y - target.y).abs() < ROUTE_POINT_EPS
        {
            continue;
        }

        let x_aligned = (current.x - target.x).abs() < ROUTE_ALIGN_EPS;
        let y_aligned = (current.y - target.y).abs() < ROUTE_ALIGN_EPS;
        if x_aligned && y_aligned {
            continue;
        }

        if x_aligned {
            output.push(FPoint::new(current.x, target.y));
            continue;
        }

        if y_aligned {
            output.push(FPoint::new(target.x, current.y));
            continue;
        }

        // For diagonal spans, choose elbow orientation by span role:
        // - first span: preserve source-face normal support
        // - last span: preserve target-face normal support
        // - single diagonal span: keep balanced V-H-V / H-V-H fallback
        if primary_vertical && is_first_span && is_last_span {
            let mid_y = (current.y + target.y) / 2.0;
            output.push(FPoint::new(current.x, mid_y));
            output.push(FPoint::new(target.x, mid_y));
        } else if !primary_vertical && is_first_span && is_last_span {
            let mid_x = (current.x + target.x) / 2.0;
            output.push(FPoint::new(mid_x, current.y));
            output.push(FPoint::new(mid_x, target.y));
        } else if primary_vertical {
            if is_first_span {
                output.push(FPoint::new(current.x, target.y));
            } else {
                output.push(FPoint::new(target.x, current.y));
            }
        } else if is_first_span {
            output.push(FPoint::new(target.x, current.y));
        } else {
            output.push(FPoint::new(current.x, target.y));
        }
        output.push(target);
    }

    output.dedup_by(|a, b| {
        (a.x - b.x).abs() < ROUTE_POINT_EPS && (a.y - b.y).abs() < ROUTE_POINT_EPS
    });
    output
}

/// Enforce shared polyline contracts used by routed-preview outputs.
///
/// Contracts:
/// - no adjacent duplicate points
/// - no zero-length segments
/// - no redundant collinear interior points
/// - non-zero terminal support segment on the primary axis
pub(crate) fn normalize_orthogonal_route_contracts(
    points: &[FPoint],
    direction: Direction,
) -> Vec<FPoint> {
    if points.len() <= 1 {
        return points.to_vec();
    }

    let mut normalized = dedupe_adjacent_points(points);
    if normalized.len() <= 1 {
        return normalized;
    }

    normalized = remove_collinear_points(&normalized);
    normalized = reduce_midfield_jogs_for_large_horizontal_offset(&normalized, direction);
    normalized = compact_terminal_staircase(&normalized, direction);
    normalized = remove_axial_turnbacks(&normalized);
    ensure_terminal_support_segment(&mut normalized, direction);
    normalized = remove_axial_turnbacks(&normalized);
    normalized = dedupe_adjacent_points(&normalized);
    remove_collinear_points(&normalized)
}

fn dedupe_adjacent_points(points: &[FPoint]) -> Vec<FPoint> {
    let mut deduped = Vec::with_capacity(points.len());
    for point in points {
        let keep = deduped.last().is_none_or(|prev: &FPoint| {
            (prev.x - point.x).abs() > ROUTE_POINT_EPS || (prev.y - point.y).abs() > ROUTE_POINT_EPS
        });
        if keep {
            deduped.push(*point);
        }
    }
    deduped
}

fn remove_collinear_points(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(points.len());
    result.push(points[0]);
    for idx in 1..(points.len() - 1) {
        let prev = result.last().copied().expect("result is non-empty");
        let curr = points[idx];
        let next = points[idx + 1];

        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= ROUTE_POINT_EPS && dot >= -ROUTE_POINT_EPS;

        if !collinear_same_direction {
            result.push(curr);
        }
    }
    result.push(*points.last().expect("points has at least two elements"));
    result
}

fn remove_axial_turnbacks(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut current = points.to_vec();
    loop {
        let mut changed = false;
        let mut result = Vec::with_capacity(current.len());
        result.push(current[0]);

        for idx in 1..(current.len() - 1) {
            let prev = *result.last().expect("result is non-empty");
            let curr = current[idx];
            let next = current[idx + 1];
            let dx1 = curr.x - prev.x;
            let dy1 = curr.y - prev.y;
            let dx2 = next.x - curr.x;
            let dy2 = next.y - curr.y;
            let cross = dx1 * dy2 - dy1 * dx2;
            let dot = dx1 * dx2 + dy1 * dy2;
            let is_collinear = cross.abs() <= ROUTE_POINT_EPS;
            let reverses_direction = dot < -ROUTE_POINT_EPS;
            if is_collinear && reverses_direction {
                changed = true;
                continue;
            }
            result.push(curr);
        }

        result.push(*current.last().expect("points has at least two elements"));
        let deduped = dedupe_adjacent_points(&result);
        if !changed {
            return deduped;
        }
        current = deduped;
        if current.len() <= 2 {
            return current;
        }
    }
}

fn ensure_terminal_support_segment(points: &mut Vec<FPoint>, direction: Direction) {
    if points.len() < 2 {
        return;
    }

    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let end = *points.last().expect("len >= 2");
    let prev = points[points.len() - 2];

    if primary_vertical {
        let already_supported =
            (prev.x - end.x).abs() <= ROUTE_POINT_EPS && (prev.y - end.y).abs() > ROUTE_POINT_EPS;
        if already_supported {
            return;
        }

        if (prev.y - end.y).abs() > ROUTE_POINT_EPS {
            points.insert(points.len() - 1, FPoint::new(end.x, prev.y));
            return;
        }

        let support_y = match direction {
            Direction::TopDown => end.y - MIN_TERMINAL_SUPPORT,
            Direction::BottomTop => end.y + MIN_TERMINAL_SUPPORT,
            _ => end.y - MIN_TERMINAL_SUPPORT,
        };
        points.insert(points.len() - 1, FPoint::new(prev.x, support_y));
        points.insert(points.len() - 1, FPoint::new(end.x, support_y));
    } else {
        let already_supported =
            (prev.y - end.y).abs() <= ROUTE_POINT_EPS && (prev.x - end.x).abs() > ROUTE_POINT_EPS;
        if already_supported {
            return;
        }

        if (prev.x - end.x).abs() > ROUTE_POINT_EPS {
            points.insert(points.len() - 1, FPoint::new(prev.x, end.y));
            return;
        }

        let support_x = match direction {
            Direction::LeftRight => end.x - MIN_TERMINAL_SUPPORT,
            Direction::RightLeft => end.x + MIN_TERMINAL_SUPPORT,
            _ => end.x - MIN_TERMINAL_SUPPORT,
        };
        points.insert(points.len() - 1, FPoint::new(support_x, prev.y));
        points.insert(points.len() - 1, FPoint::new(support_x, end.y));
    }
}

fn reduce_midfield_jogs_for_large_horizontal_offset(
    points: &[FPoint],
    direction: Direction,
) -> Vec<FPoint> {
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || points.len() <= 4 {
        return points.to_vec();
    }

    let start = points[0];
    let end = *points.last().expect("points has at least two elements");
    let horizontal_offset = (start.x - end.x).abs();
    if horizontal_offset <= LARGE_HORIZONTAL_OFFSET_THRESHOLD as f64 {
        return points.to_vec();
    }

    let mid_y = preferred_mid_y_for_vertical_layout(start, end, direction);
    vec![
        start,
        FPoint::new(start.x, mid_y),
        FPoint::new(end.x, mid_y),
        end,
    ]
}

fn compact_terminal_staircase(points: &[FPoint], direction: Direction) -> Vec<FPoint> {
    // Keep short 4-point routes intact so source-face support is not converted
    // into a border-slide start segment.
    if points.len() <= 4 {
        return points.to_vec();
    }

    let mut compacted = points.to_vec();
    let len = compacted.len();
    let a = compacted[len - 4];
    let b = compacted[len - 3];
    let c = compacted[len - 2];
    let d = compacted[len - 1];

    if matches!(direction, Direction::TopDown | Direction::BottomTop) {
        if segment_is_vertical(a, b)
            && segment_is_horizontal(b, c)
            && segment_is_vertical(c, d)
            && segment_sign(b.y - a.y) == segment_sign(d.y - c.y)
            && segment_sign(b.y - a.y) != 0
        {
            let elbow = FPoint::new(c.x, a.y);
            let would_reverse_with_prefix =
                would_introduce_axial_turnback_with_prefix(&compacted, len - 4, a, elbow);
            if !points_equal(a, elbow) && !points_equal(elbow, d) && !would_reverse_with_prefix {
                compacted.truncate(len - 3);
                compacted.push(elbow);
                compacted.push(d);
            }
        }
    } else if segment_is_horizontal(a, b)
        && segment_is_vertical(b, c)
        && segment_is_horizontal(c, d)
        && segment_sign(b.x - a.x) == segment_sign(d.x - c.x)
        && segment_sign(b.x - a.x) != 0
    {
        let elbow = FPoint::new(a.x, c.y);
        let would_reverse_with_prefix =
            would_introduce_axial_turnback_with_prefix(&compacted, len - 4, a, elbow);
        if !points_equal(a, elbow) && !points_equal(elbow, d) && !would_reverse_with_prefix {
            compacted.truncate(len - 3);
            compacted.push(elbow);
            compacted.push(d);
        }
    }

    compacted
}

fn would_introduce_axial_turnback_with_prefix(
    points: &[FPoint],
    anchor_idx: usize,
    anchor: FPoint,
    elbow: FPoint,
) -> bool {
    if anchor_idx == 0 || anchor_idx >= points.len() {
        return false;
    }

    let prefix = points[anchor_idx - 1];
    let dx1 = anchor.x - prefix.x;
    let dy1 = anchor.y - prefix.y;
    let dx2 = elbow.x - anchor.x;
    let dy2 = elbow.y - anchor.y;
    let cross = dx1 * dy2 - dy1 * dx2;
    let dot = dx1 * dx2 + dy1 * dy2;
    cross.abs() <= ROUTE_POINT_EPS && dot < -ROUTE_POINT_EPS
}

fn segment_is_vertical(start: FPoint, end: FPoint) -> bool {
    (start.x - end.x).abs() <= ROUTE_POINT_EPS && (start.y - end.y).abs() > ROUTE_POINT_EPS
}

fn segment_is_horizontal(start: FPoint, end: FPoint) -> bool {
    (start.y - end.y).abs() <= ROUTE_POINT_EPS && (start.x - end.x).abs() > ROUTE_POINT_EPS
}

fn points_equal(a: FPoint, b: FPoint) -> bool {
    (a.x - b.x).abs() <= ROUTE_POINT_EPS && (a.y - b.y).abs() <= ROUTE_POINT_EPS
}

fn segment_sign(delta: f64) -> i8 {
    if delta.abs() <= ROUTE_POINT_EPS {
        0
    } else if delta.is_sign_positive() {
        1
    } else {
        -1
    }
}

/// Compute the point on a node's shape boundary closest to the approach ray
/// from `approach` toward the node center.
///
/// This is the single source of truth for endpoint geometry used by both the
/// orthogonal router and downstream float-space consumers.
///
/// Coordinate convention: `FRect` uses top-left origin `(rect.x, rect.y)`
/// with `width`/`height` extending right and down, matching the layered layout rect convention.
pub(crate) fn intersect_shape_boundary_float(
    rect: FRect,
    shape: Shape,
    approach: FPoint,
) -> FPoint {
    match shape {
        Shape::Hexagon => {
            let verts = hexagon_vertices(rect);
            let center = FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            intersect_convex_polygon(&verts, approach, center)
        }
        Shape::Diamond => intersect_diamond_boundary(rect, approach),
        _ => intersect_rect_boundary(rect, approach),
    }
}

/// Diamond boundary intersection using closed-form `|dx|/w + |dy|/h = 1`.
/// Verified equivalent to `intersect_convex_polygon` by oracle property test.
fn intersect_diamond_boundary(rect: FRect, approach: FPoint) -> FPoint {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = approach.x - cx;
    let dy = approach.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return FPoint::new(cx, cy + h);
    }

    let t = 1.0 / (dx.abs() / w + dy.abs() / h);
    FPoint::new(cx + t * dx, cy + t * dy)
}

fn intersect_rect_boundary(rect: FRect, approach: FPoint) -> FPoint {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = approach.x - cx;
    let dy = approach.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return FPoint::new(cx, cy + h);
    }

    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        let signed_h = if dy < 0.0 { -h } else { h };
        (signed_h * dx / dy, signed_h)
    } else {
        let signed_w = if dx < 0.0 { -w } else { w };
        (signed_w, signed_w * dy / dx)
    };

    FPoint::new(cx + sx, cy + sy)
}

/// Indent fraction for hexagon flat top/bottom edges.
pub(crate) const HEXAGON_INDENT_FACTOR: f64 = 0.2;

/// Return the 6 vertices of a hexagon inscribed in `rect`.
/// Order: top-left, top-right, right, bottom-right, bottom-left, left (clockwise).
pub fn hexagon_vertices(rect: FRect) -> [FPoint; 6] {
    let indent = rect.width * HEXAGON_INDENT_FACTOR;
    let cy = rect.y + rect.height / 2.0;
    [
        FPoint::new(rect.x + indent, rect.y),              // top-left
        FPoint::new(rect.x + rect.width - indent, rect.y), // top-right
        FPoint::new(rect.x + rect.width, cy),              // right
        FPoint::new(rect.x + rect.width - indent, rect.y + rect.height), // bottom-right
        FPoint::new(rect.x + indent, rect.y + rect.height), // bottom-left
        FPoint::new(rect.x, cy),                           // left
    ]
}

/// Intersect a ray from `center` toward `approach` with a convex polygon.
///
/// Returns the point where the ray first crosses the polygon boundary.
/// For degenerate cases (approach == center), returns the bottom-most vertex.
/// Vertices must be in order (clockwise or counter-clockwise).
pub fn intersect_convex_polygon(vertices: &[FPoint], approach: FPoint, center: FPoint) -> FPoint {
    let dx = approach.x - center.x;
    let dy = approach.y - center.y;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        // Degenerate: return bottom-most vertex
        return vertices
            .iter()
            .copied()
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap_or(center);
    }

    let n = vertices.len();
    let mut best_t = f64::INFINITY;
    let mut best_point = center;

    for i in 0..n {
        let a = vertices[i];
        let b = vertices[(i + 1) % n];

        // Edge vector
        let ex = b.x - a.x;
        let ey = b.y - a.y;

        // Solve: center + t * (dx, dy) = a + s * (ex, ey)
        let denom = dx * ey - dy * ex;
        if denom.abs() < f64::EPSILON {
            continue; // Parallel
        }

        let t = ((a.x - center.x) * ey - (a.y - center.y) * ex) / denom;
        let s = ((a.x - center.x) * dy - (a.y - center.y) * dx) / denom;

        if t > 0.0 && (0.0..=1.0).contains(&s) && t < best_t {
            best_t = t;
            best_point = FPoint::new(center.x + t * dx, center.y + t * dy);
        }
    }

    best_point
}

fn preferred_mid_y_for_vertical_layout(start: FPoint, end: FPoint, direction: Direction) -> f64 {
    let mut mid_y = (start.y + end.y) / 2.0;

    if (start.x - end.x).abs() > LARGE_HORIZONTAL_OFFSET_THRESHOLD as f64 {
        let target_mid = match direction {
            Direction::TopDown => end.y - (MIN_TERMINAL_SUPPORT * 2.0),
            Direction::BottomTop => end.y + (MIN_TERMINAL_SUPPORT * 2.0),
            _ => mid_y,
        };
        mid_y = match direction {
            Direction::TopDown => target_mid.max(mid_y),
            Direction::BottomTop => target_mid.min(mid_y),
            _ => mid_y,
        };
    }

    if (mid_y - end.y).abs() <= ROUTE_POINT_EPS {
        mid_y = if start.y > end.y {
            end.y + MIN_TERMINAL_SUPPORT
        } else {
            end.y - MIN_TERMINAL_SUPPORT
        };
    }

    mid_y
}
