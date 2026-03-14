use std::collections::HashMap;

use super::super::{Point, Rect};
use super::point_inside_rect;
use crate::graph::direction_policy::effective_edge_direction;
use crate::graph::geometry::{FPoint, FRect, GraphGeometry};
use crate::graph::routing::{EdgeRouting, hexagon_vertices, intersect_convex_polygon};
use crate::graph::{Direction, Edge, Graph, Shape};

type EndpointShapeRect = (Rect, Shape);

pub(super) fn clip_points_to_rect_start(points: &[Point], rect: &Rect) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    if !point_inside_rect(rect, points[0]) {
        return points.to_vec();
    }

    let mut idx = 0usize;
    while idx + 1 < points.len() && point_inside_rect(rect, points[idx]) {
        idx += 1;
    }
    if idx == 0 || idx >= points.len() {
        return points.to_vec();
    }

    let inside = points[idx - 1];
    let outside = points[idx];
    let intersection = segment_rect_intersection(inside, outside, rect).unwrap_or(inside);

    let mut out = Vec::new();
    out.push(intersection);
    out.extend_from_slice(&points[idx..]);
    out
}

pub(super) fn clip_points_to_rect_end(points: &[Point], rect: &Rect) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let last_idx = points.len() - 1;
    if !point_inside_rect(rect, points[last_idx]) {
        return points.to_vec();
    }

    let mut idx = last_idx;
    while idx > 0 && point_inside_rect(rect, points[idx]) {
        idx -= 1;
    }
    if idx == last_idx || idx >= last_idx {
        return points.to_vec();
    }

    let outside = points[idx];
    let inside = points[idx + 1];
    let intersection = segment_rect_intersection(outside, inside, rect).unwrap_or(inside);

    let mut out = points[..=idx].to_vec();
    out.push(intersection);
    out
}

pub(super) fn orthogonal_route_edge_direction(
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
        _ => orthogonal_route_cross_boundary_direction(
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

pub(super) fn should_adjust_rerouted_edge_endpoints(
    diagram: &Graph,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
    direction: Direction,
) -> bool {
    const FACE_PROXIMITY: f64 = 6.0;
    const EPS: f64 = 0.5;
    if points.len() < 2 {
        return false;
    }

    let Some(((from_rect, _), (to_rect, _))) = edge_endpoint_shape_rects(diagram, geom, edge)
    else {
        return false;
    };

    // For orthogonal routing, the router produces authoritative endpoint geometry.
    // Keep intentional non-flow-face attachments (e.g. fan-in overflow) but
    // still re-adjust when endpoints drift inside/outside or violate expected
    // flow faces on the primary axis.
    if endpoint_drifted_inside_or_outside(points[0], from_rect, EPS)
        || endpoint_drifted_inside_or_outside(points[points.len() - 1], to_rect, EPS)
    {
        return true;
    }

    let is_backward = geom.reversed_edges.contains(&edge.index);
    if is_backward {
        return endpoint_attachment_is_invalid(points[0], from_rect, direction, true, true, EPS)
            || endpoint_attachment_is_invalid(
                points[points.len() - 1],
                to_rect,
                direction,
                false,
                true,
                EPS,
            );
    }

    if !endpoint_on_non_flow_face(points[0], from_rect, direction, FACE_PROXIMITY)
        && endpoint_attachment_is_invalid(points[0], from_rect, direction, true, false, EPS)
    {
        return true;
    }
    if !endpoint_on_non_flow_face(points[points.len() - 1], to_rect, direction, FACE_PROXIMITY)
        && endpoint_attachment_is_invalid(
            points[points.len() - 1],
            to_rect,
            direction,
            false,
            false,
            EPS,
        )
    {
        return true;
    }

    false
}

pub(super) fn adjust_edge_points_for_shapes(
    diagram: &Graph,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
    direction: Direction,
    is_backward: bool,
    edge_routing: EdgeRouting,
) -> Vec<Point> {
    const EPS: f64 = 0.5;
    if points.len() < 2 {
        return points.to_vec();
    }

    let Some(((from_rect, from_shape), (to_rect, to_shape))) =
        edge_endpoint_shape_rects(diagram, geom, edge)
    else {
        return points.to_vec();
    };

    let mut adjusted = points.to_vec();
    let is_self_loop = edge.from == edge.to;
    // In orthogonal routing mode the router already places non-rect shape
    // endpoints on the actual shape boundary (with marker clearance for
    // backward edges) — these are authoritative and must not be re-projected
    // (different approach angles would shift them).
    // In polyline routing mode the layout only clips to the bounding rect, so non-rect
    // shapes always need re-projection to the actual shape boundary.
    let router_placed_source = matches!(edge_routing, EdgeRouting::OrthogonalRoute)
        && !is_self_loop
        && matches!(from_shape, Shape::Diamond | Shape::Hexagon);
    let router_placed_target = matches!(edge_routing, EdgeRouting::OrthogonalRoute)
        && !is_self_loop
        && matches!(to_shape, Shape::Diamond | Shape::Hexagon);
    let source_needs_adjustment = !router_placed_source
        && (matches!(from_shape, Shape::Diamond | Shape::Hexagon)
            || endpoint_attachment_is_invalid(
                points[0],
                from_rect,
                direction,
                true,
                is_backward,
                EPS,
            ));
    let target_needs_adjustment = !router_placed_target
        && (matches!(to_shape, Shape::Diamond | Shape::Hexagon)
            || endpoint_attachment_is_invalid(
                points[points.len() - 1],
                to_rect,
                direction,
                false,
                is_backward,
                EPS,
            ));

    if source_needs_adjustment {
        let from_target = if points.len() > 1 {
            points[1]
        } else {
            from_rect.center()
        };
        adjusted[0] = intersect_svg_node(&from_rect, from_target, from_shape);
    }

    if target_needs_adjustment {
        let to_target = if points.len() > 1 {
            points[points.len() - 2]
        } else {
            to_rect.center()
        };
        let last = adjusted.len() - 1;
        adjusted[last] = intersect_svg_node(&to_rect, to_target, to_shape);
    }

    adjusted
}

pub(super) fn fix_corner_points(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut corner_positions = Vec::new();
    for i in 1..points.len() - 1 {
        let prev = points[i - 1];
        let curr = points[i];
        let next = points[i + 1];
        let dx_prev = (curr.x - prev.x).abs();
        let dy_prev = (curr.y - prev.y).abs();
        let dx_next = (next.x - curr.x).abs();
        let dy_next = (next.y - curr.y).abs();

        let is_corner =
            (prev.x == curr.x && (curr.y - next.y).abs() > 5.0 && dx_next > 5.0 && dy_prev > 5.0)
                || (prev.y == curr.y
                    && (curr.x - next.x).abs() > 5.0
                    && dx_prev > 5.0
                    && dy_next > 5.0);

        if is_corner {
            corner_positions.push(i);
        }
    }

    if corner_positions.is_empty() {
        return points.to_vec();
    }

    let mut out = Vec::new();
    for i in 0..points.len() {
        if !corner_positions.contains(&i) {
            out.push(points[i]);
            continue;
        }

        let prev = points[i - 1];
        let curr = points[i];
        let next = points[i + 1];

        let new_prev = find_adjacent_point(prev, curr, 5.0);
        let new_next = find_adjacent_point(next, curr, 5.0);

        let x_diff = new_next.x - new_prev.x;
        let y_diff = new_next.y - new_prev.y;
        out.push(new_prev);

        let mut new_corner = curr;
        let a = (2.0_f64).sqrt() * 2.0;
        if (next.x - prev.x).abs() > 10.0 && (next.y - prev.y).abs() >= 10.0 {
            let r = 5.0;
            if (curr.x - new_prev.x).abs() < f64::EPSILON {
                new_corner = Point {
                    x: if x_diff < 0.0 {
                        new_prev.x - r + a
                    } else {
                        new_prev.x + r - a
                    },
                    y: if y_diff < 0.0 {
                        new_prev.y - a
                    } else {
                        new_prev.y + a
                    },
                };
            } else {
                new_corner = Point {
                    x: if x_diff < 0.0 {
                        new_prev.x - a
                    } else {
                        new_prev.x + a
                    },
                    y: if y_diff < 0.0 {
                        new_prev.y - r + a
                    } else {
                        new_prev.y + r - a
                    },
                };
            }
        }

        out.push(new_corner);
        out.push(new_next);
    }

    out
}

pub(super) fn intersect_svg_node(rect: &Rect, point: Point, shape: Shape) -> Point {
    match shape {
        Shape::Diamond => intersect_svg_diamond(rect, point),
        Shape::Hexagon => intersect_svg_hexagon(rect, point),
        _ => intersect_svg_rect(rect, point),
    }
}

fn segment_rect_intersection(start: Point, end: Point, rect: &Rect) -> Option<Point> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return None;
    }

    let mut candidates: Vec<(f64, Point)> = Vec::new();

    let x_min = rect.x;
    let x_max = rect.x + rect.width;
    let y_min = rect.y;
    let y_max = rect.y + rect.height;

    if dx.abs() > f64::EPSILON {
        let t_left = (x_min - start.x) / dx;
        if (0.0..=1.0).contains(&t_left) {
            let y = start.y + t_left * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_left, Point { x: x_min, y }));
            }
        }
        let t_right = (x_max - start.x) / dx;
        if (0.0..=1.0).contains(&t_right) {
            let y = start.y + t_right * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_right, Point { x: x_max, y }));
            }
        }
    }

    if dy.abs() > f64::EPSILON {
        let t_top = (y_min - start.y) / dy;
        if (0.0..=1.0).contains(&t_top) {
            let x = start.x + t_top * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_top, Point { x, y: y_min }));
            }
        }
        let t_bottom = (y_max - start.y) / dy;
        if (0.0..=1.0).contains(&t_bottom) {
            let x = start.x + t_bottom * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_bottom, Point { x, y: y_max }));
            }
        }
    }

    candidates
        .into_iter()
        .filter(|(t, _)| *t >= 0.0 && *t <= 1.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

pub(super) fn edge_endpoint_shape_rects(
    diagram: &Graph,
    geom: &GraphGeometry,
    edge: &Edge,
) -> Option<(EndpointShapeRect, EndpointShapeRect)> {
    let from = if let Some(sg_id) = edge.from_subgraph.as_ref() {
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect;
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.from)?.rect;
        let node = diagram.nodes.get(&edge.from)?;
        (node_rect, node.shape)
    };

    let to = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect;
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.to)?.rect;
        let node = diagram.nodes.get(&edge.to)?;
        (node_rect, node.shape)
    };

    Some((from, to))
}

fn orthogonal_route_cross_boundary_direction(
    diagram: &Graph,
    node_directions: &HashMap<String, Direction>,
    from_sg: Option<&String>,
    to_sg: Option<&String>,
    from_node: &str,
    to_node: &str,
    fallback: Direction,
) -> Direction {
    if let (Some(sg_a), Some(sg_b)) = (from_sg, to_sg) {
        if is_ancestor_subgraph(diagram, sg_a, sg_b) {
            return diagram
                .subgraphs
                .get(sg_a.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        if is_ancestor_subgraph(diagram, sg_b, sg_a) {
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

fn is_ancestor_subgraph(diagram: &Graph, ancestor: &str, descendant: &str) -> bool {
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

fn endpoint_drifted_inside_or_outside(point: Point, rect: Rect, eps: f64) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if point.x < left - eps
        || point.x > right + eps
        || point.y < top - eps
        || point.y > bottom + eps
    {
        return true;
    }

    point_inside_rect(&rect, point)
}

fn endpoint_on_non_flow_face(
    point: Point,
    rect: Rect,
    proximity: Direction,
    face_tol: f64,
) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    let near_left = (point.x - left) <= face_tol;
    let near_right = (right - point.x) <= face_tol;
    let near_top = (point.y - top) <= face_tol;
    let near_bottom = (bottom - point.y) <= face_tol;

    match proximity {
        Direction::TopDown | Direction::BottomTop => near_left || near_right,
        Direction::LeftRight | Direction::RightLeft => near_top || near_bottom,
    }
}

fn endpoint_attachment_is_invalid(
    point: Point,
    rect: Rect,
    direction: Direction,
    is_source: bool,
    is_backward: bool,
    eps: f64,
) -> bool {
    const FACE_PROXIMITY: f64 = 6.0;
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    if point.x < left - eps
        || point.x > right + eps
        || point.y < top - eps
        || point.y > bottom + eps
    {
        return true;
    }

    if is_backward {
        let near_left = (point.x - left) <= FACE_PROXIMITY;
        let near_right = (right - point.x) <= FACE_PROXIMITY;
        let near_top = (point.y - top) <= FACE_PROXIMITY;
        let near_bottom = (bottom - point.y) <= FACE_PROXIMITY;
        return !(near_left || near_right || near_top || near_bottom);
    }

    let is_forward_source = is_source != is_backward;

    match direction {
        Direction::TopDown => {
            if is_forward_source {
                (bottom - point.y) > FACE_PROXIMITY
            } else {
                (point.y - top) > FACE_PROXIMITY
            }
        }
        Direction::BottomTop => {
            if is_forward_source {
                (point.y - top) > FACE_PROXIMITY
            } else {
                (bottom - point.y) > FACE_PROXIMITY
            }
        }
        Direction::LeftRight => {
            if is_forward_source {
                (right - point.x) > FACE_PROXIMITY
            } else {
                (point.x - left) > FACE_PROXIMITY
            }
        }
        Direction::RightLeft => {
            if is_forward_source {
                (point.x - left) > FACE_PROXIMITY
            } else {
                (right - point.x) > FACE_PROXIMITY
            }
        }
    }
}

fn find_adjacent_point(point_a: Point, point_b: Point, distance: f64) -> Point {
    let x_diff = point_b.x - point_a.x;
    let y_diff = point_b.y - point_a.y;
    let length = (x_diff * x_diff + y_diff * y_diff).sqrt();
    if length <= f64::EPSILON {
        return point_b;
    }
    let ratio = distance / length;
    Point {
        x: point_b.x - ratio * x_diff,
        y: point_b.y - ratio * y_diff,
    }
}

fn intersect_svg_hexagon(rect: &Rect, point: Point) -> Point {
    let frect = FRect::new(rect.x, rect.y, rect.width, rect.height);
    let verts = hexagon_vertices(frect);
    let center = FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let approach = FPoint::new(point.x, point.y);
    let result = intersect_convex_polygon(&verts, approach, center);
    Point {
        x: result.x,
        y: result.y,
    }
}

fn intersect_svg_rect(rect: &Rect, point: Point) -> Point {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = point.x - cx;
    let dy = point.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return Point { x: cx, y: cy + h };
    }

    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        let h = if dy < 0.0 { -h } else { h };
        (h * dx / dy, h)
    } else {
        let w = if dx < 0.0 { -w } else { w };
        (w, w * dy / dx)
    };

    Point {
        x: cx + sx,
        y: cy + sy,
    }
}

fn intersect_svg_diamond(rect: &Rect, point: Point) -> Point {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = point.x - cx;
    let dy = point.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return Point { x: cx, y: cy + h };
    }

    let t = 1.0 / (dx.abs() / w + dy.abs() / h);
    Point {
        x: cx + t * dx,
        y: cy + t * dy,
    }
}
