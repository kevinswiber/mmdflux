use std::collections::{HashMap, HashSet};

use crate::graph::geometry::{FPoint, FRect, GraphGeometry, LayoutEdge, PositionedNode};
use crate::graph::{Direction, Edge, Graph, Node, Shape};
use crate::render::graph::{TextRenderOptions, render_text_from_geometry};

#[test]
fn text_owner_local_smoke_renders_text_output() {
    let (diagram, geometry) = smoke_graph_geometry();
    let text = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());

    assert!(text.contains("Start"));
    assert!(text.contains("End"));
}

fn smoke_graph_geometry() -> (Graph, GraphGeometry) {
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));

    let nodes = HashMap::from([
        (
            "A".to_string(),
            PositionedNode {
                id: "A".to_string(),
                rect: FRect::new(50.0, 25.0, 40.0, 20.0),
                shape: Shape::Rectangle,
                label: "Start".to_string(),
                parent: None,
            },
        ),
        (
            "B".to_string(),
            PositionedNode {
                id: "B".to_string(),
                rect: FRect::new(50.0, 75.0, 40.0, 20.0),
                shape: Shape::Rectangle,
                label: "End".to_string(),
                parent: None,
            },
        ),
    ]);

    let geometry = GraphGeometry {
        nodes,
        edges: vec![LayoutEdge {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: Some(vec![FPoint::new(50.0, 45.0), FPoint::new(50.0, 75.0)]),
            preserve_orthogonal_topology: false,
        }],
        subgraphs: HashMap::new(),
        self_edges: vec![],
        direction: Direction::TopDown,
        node_directions: HashMap::from([
            ("A".to_string(), Direction::TopDown),
            ("B".to_string(), Direction::TopDown),
        ]),
        bounds: FRect::new(0.0, 0.0, 100.0, 100.0),
        reversed_edges: vec![],
        engine_hints: None,
        grid_projection: None,
        rerouted_edges: HashSet::new(),
        enhanced_backward_routing: false,
    };

    (diagram, geometry)
}
