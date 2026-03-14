//! Graph-family routing stage.
//!
//! Produces `RoutedGraphGeometry` (Layer 2) from `GraphGeometry` (Layer 1).
//! Supports four modes:
//! - `DirectRoute`: Build source→target direct paths.
//! - `PolylineRoute`: Build edge paths from layout hints + node positions.
//! - `EngineProvided`: Use engine-provided paths directly.
//! - `OrthogonalRoute`: Produce axis-aligned (right-angle) edge paths.

mod float_core;
mod labels;
mod orthogonal;
mod stage;

pub(crate) use self::float_core::{
    build_orthogonal_path_float, hexagon_vertices, intersect_convex_polygon,
};
pub(crate) use self::labels::compute_end_label_positions;
pub(crate) use self::orthogonal::{OrthogonalRoutingOptions, route_edges_orthogonal};
pub use self::stage::{EdgeRouting, route_graph_geometry};
#[cfg(test)]
use crate::graph::Graph;
#[cfg(test)]
use crate::graph::geometry::*;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::graph::attachment::PortFace;
    use crate::graph::routing::EdgeRouting;

    fn simple_geometry() -> (Graph, GraphGeometry) {
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
    fn backward_channel_path_routes_outside_intermediate_td_nodes() {
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
        diagram.add_node(crate::graph::Node::new("A"));
        diagram.add_node(crate::graph::Node::new("Mid"));
        diagram.add_node(crate::graph::Node::new("C"));
        diagram.add_edge(crate::graph::Edge::new("C", "A"));

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
            "Mid".into(),
            PositionedNode {
                id: "Mid".into(),
                rect: FRect::new(20.0, 45.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "Mid".into(),
                parent: None,
            },
        );
        nodes.insert(
            "C".into(),
            PositionedNode {
                id: "C".into(),
                rect: FRect::new(0.0, 100.0, 40.0, 20.0),
                shape: crate::graph::Shape::Rectangle,
                label: "C".into(),
                parent: None,
            },
        );

        let geom = GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "C".into(),
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
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 80.0, 120.0),
            reversed_edges: vec![0],
            engine_hints: None,
            grid_projection: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: true,
        };

        let path = super::orthogonal::backward::build_backward_orthogonal_channel_path(
            &geom.edges[0],
            &geom,
            crate::graph::Direction::TopDown,
        )
        .expect("backward channel path should be constructed");

        assert_eq!(
            path,
            vec![
                FPoint::new(40.0, 110.0),
                FPoint::new(72.0, 110.0),
                FPoint::new(72.0, 10.0),
                FPoint::new(40.0, 10.0),
            ]
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
            rerouted_edges: std::collections::HashSet::new(),
            enhanced_backward_routing: false,
        };

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::DirectRoute);
        assert_eq!(routed.edges[0].path, direct_hint);
    }

    #[test]
    fn direct_route_falls_back_when_straight_segment_grazes_node_border() {
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
        let snapped = orthogonal::path_utils::snap_path_to_grid(&input, 1.0, 1.0);

        assert_eq!(snapped.first(), Some(&FPoint::new(5.0, 9.0)));
        assert_eq!(snapped.last(), Some(&FPoint::new(15.0, 12.0)));
        assert_eq!(
            snapped,
            orthogonal::path_utils::snap_path_to_grid(&input, 1.0, 1.0)
        );
    }

    #[test]
    fn build_path_from_hints_falls_back_to_nodes_and_waypoints_when_layout_hint_is_degenerate() {
        let (_diagram, mut geom) = simple_geometry();
        geom.edges[0].layout_path_hint =
            Some(vec![FPoint::new(70.0, 35.0), FPoint::new(70.0, 35.0)]);
        geom.edges[0].waypoints = vec![FPoint::new(60.0, 55.0)];

        let path = orthogonal::hints::build_path_from_hints(&geom.edges[0], &geom);

        assert_eq!(
            path,
            vec![
                FPoint::new(70.0, 35.0),
                FPoint::new(60.0, 55.0),
                FPoint::new(70.0, 85.0),
            ]
        );
    }

    #[test]
    fn light_normalize_dedups_and_removes_collinear_points() {
        let normalized = orthogonal::path_utils::light_normalize(&[
            FPoint::new(0.0, 0.0),
            FPoint::new(0.0, 0.0),
            FPoint::new(0.0, 10.0),
            FPoint::new(0.0, 20.0),
            FPoint::new(15.0, 20.0),
        ]);

        assert_eq!(
            normalized,
            vec![
                FPoint::new(0.0, 0.0),
                FPoint::new(0.0, 20.0),
                FPoint::new(15.0, 20.0),
            ]
        );
    }

    #[test]
    fn anchor_path_endpoints_to_endpoint_faces_projects_simple_td_route() {
        let (_diagram, geom) = simple_geometry();
        let edge = &geom.edges[0];
        let mut path = vec![
            FPoint::new(70.0, 35.0),
            FPoint::new(70.0, 55.0),
            FPoint::new(70.0, 85.0),
        ];

        orthogonal::endpoints::anchor_path_endpoints_to_endpoint_faces(
            &mut path,
            edge,
            &geom,
            crate::graph::Direction::TopDown,
            false,
            None,
            None,
            None,
            false,
            false,
        );

        assert_eq!(path[0], FPoint::new(70.0, 45.0));
        assert_eq!(path[path.len() - 1], FPoint::new(70.0, 75.0));
    }

    #[test]
    fn pairwise_parallel_clearance_measures_criss_cross_channel_spacing() {
        let path_a = vec![
            FPoint::new(0.0, 0.0),
            FPoint::new(0.0, 10.0),
            FPoint::new(12.0, 10.0),
        ];
        let path_b = vec![
            FPoint::new(4.0, 0.0),
            FPoint::new(4.0, 10.0),
            FPoint::new(16.0, 10.0),
        ];

        assert_eq!(
            orthogonal::overlap::pairwise_parallel_clearance(&path_a, &path_b),
            Some(4.0)
        );
    }

    #[test]
    fn symmetric_side_band_depth_spreads_outer_and_inner_fan_channels() {
        let outer = orthogonal::fan::symmetric_side_band_depth(0, 3);
        let middle = orthogonal::fan::symmetric_side_band_depth(1, 3);
        let inner = orthogonal::fan::symmetric_side_band_depth(2, 3);

        assert!(outer < middle && middle < inner);
        assert!(outer >= 0.0 && inner <= 1.0);
    }

    #[test]
    fn collapse_forward_source_primary_turnback_hooks_flattens_inward_lr_hook() {
        let mut path = vec![
            FPoint::new(0.0, 0.0),
            FPoint::new(10.0, 0.0),
            FPoint::new(10.0, 5.0),
            FPoint::new(5.0, 5.0),
            FPoint::new(5.0, 10.0),
            FPoint::new(20.0, 10.0),
        ];

        let changed = orthogonal::forward::collapse_forward_source_primary_turnback_hooks(
            &mut path,
            crate::graph::Direction::LeftRight,
        );

        assert!(changed);
        assert_eq!(path[3].x, 10.0);
        assert_eq!(path[4].x, 10.0);
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
        let mut diagram = Graph::new(crate::graph::Direction::TopDown);
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
            grid_projection: None,
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
