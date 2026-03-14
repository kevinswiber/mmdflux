//! Cross-pipeline tests for the layered engine → GraphGeometry adapter.
//!
//! These tests verify that `from_layered_layout` correctly converts
//! engine-owned `LayoutResult` into graph-owned `GraphGeometry`.
//! They intentionally span the engine→graph boundary and belong in
//! internal_tests rather than in either owner module.

use std::collections::HashMap;

use crate::engines::graph::algorithms::layered;
use crate::engines::graph::algorithms::layered::adapter::from_layered_layout;
use crate::engines::graph::algorithms::layered::types::WaypointWithRank;
use crate::engines::graph::algorithms::layered::{EdgeLayout, NodeId, Point, Rect, SelfEdgeLayout};
use crate::graph::geometry::EngineHints;
use crate::graph::{Direction, Edge, Graph, Node, Subgraph};

/// Build a simple LayoutResult with two nodes and one edge.
fn sample_layout_result() -> layered::LayoutResult {
    let mut nodes = HashMap::new();
    nodes.insert(
        NodeId::from("A"),
        Rect {
            x: 50.0,
            y: 25.0,
            width: 40.0,
            height: 20.0,
        },
    );
    nodes.insert(
        NodeId::from("B"),
        Rect {
            x: 50.0,
            y: 75.0,
            width: 40.0,
            height: 20.0,
        },
    );

    let edges = vec![EdgeLayout {
        from: NodeId::from("A"),
        to: NodeId::from("B"),
        points: vec![Point { x: 50.0, y: 35.0 }, Point { x: 50.0, y: 65.0 }],
        index: 0,
    }];

    let mut node_ranks = HashMap::new();
    node_ranks.insert(NodeId::from("A"), 0);
    node_ranks.insert(NodeId::from("B"), 2);

    let mut rank_to_position = HashMap::new();
    rank_to_position.insert(0, (15.0, 35.0));
    rank_to_position.insert(2, (65.0, 85.0));

    layered::LayoutResult {
        nodes,
        edges,
        reversed_edges: vec![],
        width: 100.0,
        height: 100.0,
        edge_waypoints: HashMap::new(),
        label_positions: HashMap::new(),
        label_sides: HashMap::new(),
        subgraph_bounds: HashMap::new(),
        self_edges: vec![],
        rank_to_position,
        node_ranks,
    }
}

/// Build a matching Diagram for the sample layout result.
fn sample_diagram() -> Graph {
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(Node::new("A"));
    diagram.add_node(Node::new("B"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram
}

#[test]
fn layered_adapter_produces_nodes_and_edges() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    assert_eq!(geom.nodes.len(), 2);
    assert_eq!(geom.edges.len(), 1);
    assert!(geom.engine_hints.is_some());
    assert!(geom.grid_projection.is_some());
    assert_eq!(geom.direction, Direction::TopDown);
}

#[test]
fn layered_adapter_maps_node_rect() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    let node_a = &geom.nodes["A"];
    assert_eq!(node_a.rect.x, 50.0);
    assert_eq!(node_a.rect.y, 25.0);
    assert_eq!(node_a.rect.width, 40.0);
    assert_eq!(node_a.rect.height, 20.0);
    assert_eq!(node_a.label, "A");
    assert_eq!(node_a.shape, crate::graph::Shape::Rectangle);
    assert!(node_a.parent.is_none());
}

#[test]
fn layered_adapter_maps_edge() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    let edge = &geom.edges[0];
    assert_eq!(edge.index, 0);
    assert_eq!(edge.from, "A");
    assert_eq!(edge.to, "B");
    assert!(edge.waypoints.is_empty()); // short edge, no waypoints
    assert!(edge.label_position.is_none());
    assert!(edge.from_subgraph.is_none());
    assert!(edge.to_subgraph.is_none());
    // layout_path_hint is populated from layout edge points
    let path = edge.layout_path_hint.as_ref().unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].x, 50.0);
    assert_eq!(path[0].y, 35.0);
}

#[test]
fn layered_adapter_maps_waypoints_with_ranks() {
    let mut result = sample_layout_result();
    result.edge_waypoints.insert(
        0,
        vec![WaypointWithRank {
            point: Point { x: 50.0, y: 50.0 },
            rank: 1,
        }],
    );
    result.label_positions.insert(
        0,
        WaypointWithRank {
            point: Point { x: 50.0, y: 48.0 },
            rank: 1,
        },
    );

    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    // Check geometry IR waypoints (positions only)
    assert_eq!(geom.edges[0].waypoints.len(), 1);
    assert_eq!(geom.edges[0].waypoints[0].x, 50.0);
    assert_eq!(geom.edges[0].waypoints[0].y, 50.0);
    assert!(geom.edges[0].label_position.is_some());

    // Check layered hints preserve rank info
    let hints = match &geom.engine_hints {
        Some(EngineHints::Layered(h)) => h,
        _ => panic!("expected layered hints"),
    };
    let projection = geom.grid_projection.as_ref().expect("grid projection");
    let wp_ranks = &hints.edge_waypoints[&0];
    assert_eq!(wp_ranks.len(), 1);
    assert_eq!(wp_ranks[0].1, 1); // rank = 1
    let (lp, lr) = &hints.label_positions[&0];
    assert_eq!(lp.x, 50.0);
    assert_eq!(*lr, 1);
    assert_eq!(projection.edge_waypoints[&0][0].1, 1);
    assert_eq!(projection.label_positions[&0].1, 1);
}

#[test]
fn layered_adapter_maps_layout_hints() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    let hints = match &geom.engine_hints {
        Some(EngineHints::Layered(h)) => h,
        _ => panic!("expected layered hints"),
    };
    assert_eq!(hints.node_ranks["A"], 0);
    assert_eq!(hints.node_ranks["B"], 2);
    assert_eq!(hints.rank_to_position[&0], (15.0, 35.0));
    assert_eq!(hints.rank_to_position[&2], (65.0, 85.0));
}

#[test]
fn layered_adapter_maps_reversed_edges() {
    let mut result = sample_layout_result();
    result.reversed_edges = vec![0];
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);
    assert_eq!(geom.reversed_edges, vec![0]);
}

#[test]
fn layered_adapter_maps_bounds() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);
    assert_eq!(geom.bounds.width, 100.0);
    assert_eq!(geom.bounds.height, 100.0);
}

#[test]
fn layered_adapter_maps_self_edges() {
    let mut result = sample_layout_result();
    result.self_edges.push(SelfEdgeLayout {
        node: NodeId::from("A"),
        edge_index: 1,
        points: vec![
            Point { x: 70.0, y: 15.0 },
            Point { x: 80.0, y: 15.0 },
            Point { x: 80.0, y: 35.0 },
            Point { x: 70.0, y: 35.0 },
        ],
    });
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    assert_eq!(geom.self_edges.len(), 1);
    assert_eq!(geom.self_edges[0].node_id, "A");
    assert_eq!(geom.self_edges[0].edge_index, 1);
    assert_eq!(geom.self_edges[0].points.len(), 4);
}

#[test]
fn layered_adapter_maps_subgraph_bounds() {
    let mut result = sample_layout_result();
    result.subgraph_bounds.insert(
        "sg1".into(),
        Rect {
            x: 10.0,
            y: 5.0,
            width: 80.0,
            height: 90.0,
        },
    );

    let mut diagram = sample_diagram();
    diagram.subgraphs.insert(
        "sg1".into(),
        Subgraph {
            id: "sg1".into(),
            title: "Group 1".into(),
            nodes: vec!["A".into(), "B".into()],
            parent: None,
            dir: None,
        },
    );

    let geom = from_layered_layout(&result, &diagram);
    assert_eq!(geom.subgraphs.len(), 1);
    let sg = &geom.subgraphs["sg1"];
    assert_eq!(sg.title, "Group 1");
    assert_eq!(sg.rect.x, 10.0);
    assert_eq!(sg.depth, 0);
}

#[test]
fn layered_adapter_node_directions() {
    let result = sample_layout_result();
    let diagram = sample_diagram();
    let geom = from_layered_layout(&result, &diagram);

    // Both nodes should have root direction (no subgraph overrides)
    assert_eq!(geom.node_directions["A"], Direction::TopDown);
    assert_eq!(geom.node_directions["B"], Direction::TopDown);
}

#[test]
fn layered_adapter_skips_compound_nodes() {
    // If layout result has a subgraph as a node entry (compound graph),
    // it should not appear in geometry nodes (only in subgraphs).
    let mut result = sample_layout_result();
    result.nodes.insert(
        NodeId::from("sg1"),
        Rect {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
        },
    );
    result.subgraph_bounds.insert(
        "sg1".into(),
        Rect {
            x: 10.0,
            y: 5.0,
            width: 80.0,
            height: 90.0,
        },
    );

    let mut diagram = sample_diagram();
    diagram.subgraphs.insert(
        "sg1".into(),
        Subgraph {
            id: "sg1".into(),
            title: "SG".into(),
            nodes: vec!["A".into()],
            parent: None,
            dir: None,
        },
    );

    let geom = from_layered_layout(&result, &diagram);
    // sg1 should NOT appear in nodes (not in diagram.nodes)
    assert!(!geom.nodes.contains_key("sg1"));
    // sg1 SHOULD appear in subgraphs
    assert!(geom.subgraphs.contains_key("sg1"));
}
