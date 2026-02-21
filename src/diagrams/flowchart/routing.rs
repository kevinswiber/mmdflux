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
use crate::diagram::EdgeRouting;
use crate::graph::Diagram;

/// Route graph geometry to produce fully-routed edge paths.
///
/// Consumes engine-agnostic `GraphGeometry` and produces `RoutedGraphGeometry`
/// with polyline paths for every edge.
pub fn route_graph_geometry(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> RoutedGraphGeometry {
    let edges: Vec<RoutedEdgeGeometry> = match edge_routing {
        EdgeRouting::OrthogonalRoute => {
            route_edges_orthogonal(diagram, geometry, OrthogonalRoutingOptions::preview())
        }
        EdgeRouting::DirectRoute | EdgeRouting::EngineProvided | EdgeRouting::PolylineRoute => {
            geometry
                .edges
                .iter()
                .map(|edge| {
                    let path = match edge_routing {
                        EdgeRouting::DirectRoute => {
                            build_direct_path(edge, geometry, diagram.direction)
                        }
                        EdgeRouting::EngineProvided => edge
                            .layout_path_hint
                            .clone()
                            .unwrap_or_else(|| build_path_from_hints(edge, geometry)),
                        EdgeRouting::PolylineRoute => build_path_from_hints(edge, geometry),
                        EdgeRouting::OrthogonalRoute => unreachable!(),
                    };
                    let is_backward = geometry.reversed_edges.contains(&edge.index);
                    RoutedEdgeGeometry {
                        index: edge.index,
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        path,
                        label_position: edge.label_position,
                        is_backward,
                        from_subgraph: edge.from_subgraph.clone(),
                        to_subgraph: edge.to_subgraph.clone(),
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

    RoutedGraphGeometry {
        nodes: geometry.nodes.clone(),
        edges,
        subgraphs: geometry.subgraphs.clone(),
        self_edges,
        direction: geometry.direction,
        bounds: geometry.bounds,
    }
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

    if direct_segment_crosses_non_endpoint_nodes(start, end, edge, geometry) {
        return build_path_from_hints(edge, geometry);
    }

    vec![start, end]
}

fn direct_segment_crosses_non_endpoint_nodes(
    start: FPoint,
    end: FPoint,
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) -> bool {
    // TODO: This is currently O(V) per direct-routed edge (overall O(E*V)).
    // If large graphs make this hot, replace with a spatial index over node rects.
    geometry.nodes.iter().any(|(id, node)| {
        if id == &edge.from || id == &edge.to {
            return false;
        }
        segment_crosses_rect_interior(start, end, node.rect)
    })
}

fn segment_crosses_rect_interior(start: FPoint, end: FPoint, rect: FRect) -> bool {
    const EPS: f64 = 1e-6;
    let left = rect.x + EPS;
    let right = rect.x + rect.width - EPS;
    let top = rect.y + EPS;
    let bottom = rect.y + rect.height - EPS;
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
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: Some(vec![FPoint::new(50.0, 35.0), FPoint::new(50.0, 65.0)]),
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
        assert_eq!(edge.path[0].x, 50.0);
        assert_eq!(edge.path[0].y, 35.0);
        assert_eq!(edge.path[1].x, 50.0);
        assert_eq!(edge.path[1].y, 65.0);
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
        // Should be: A center → waypoint → B center
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].x, 70.0); // A center_x
        assert_eq!(path[0].y, 35.0); // A center_y
        assert_eq!(path[1].x, 50.0);
        assert_eq!(path[1].y, 50.0); // waypoint
        assert_eq!(path[2].x, 70.0); // B center_x
        assert_eq!(path[2].y, 85.0); // B center_y
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
        assert_eq!(path[0], FPoint::new(70.0, 35.0));
        assert_eq!(path[1], FPoint::new(70.0, 85.0));
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
        assert_eq!(
            routed.edges[0].path,
            vec![FPoint::new(60.0, 35.0), FPoint::new(80.0, 35.0)]
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
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: Some(direct_hint.clone()),
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: crate::graph::Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 200.0, 200.0),
            reversed_edges: vec![],
            engine_hints: None,
            rerouted_edges: std::collections::HashSet::new(),
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        assert_eq!(routed.edges[0].path, direct_hint);
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
}
