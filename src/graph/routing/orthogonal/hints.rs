use super::endpoints::{endpoint_rect, point_on_or_inside_rect};
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::{FPoint, FRect};

pub(crate) fn build_path_from_hints(edge: &LayoutEdge, geometry: &GraphGeometry) -> Vec<FPoint> {
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

fn build_path_from_nodes_and_waypoints(edge: &LayoutEdge, geometry: &GraphGeometry) -> Vec<FPoint> {
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
    edge: &LayoutEdge,
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

fn rect_center(rect: &FRect) -> FPoint {
    FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}
