//! Proves the direct graph-in / geometry-out layout facade is reachable and
//! usable through the crate's public API alone (no Mermaid round-trip).

use mmdflux::graph::space::FPoint;
use mmdflux::graph::{Direction, Edge, GeometryLevel, Graph, Node};
use mmdflux::layout::{LaidOutGraph, LayoutOptions, layout_graph};

/// Supersession DAG with competing heads and an isolated node, mirroring the
/// consuming use case: B1 and B2 both supersede A; C supersedes B1; D isolated.
fn supersession_graph() -> Graph {
    let mut g = Graph::new(Direction::TopDown);
    g.add_node(Node::new("rev:sha256:A").with_label("A"));
    g.add_node(Node::new("rev:sha256:B1").with_label("B1"));
    g.add_node(Node::new("rev:sha256:B2").with_label("B2"));
    g.add_node(Node::new("rev:sha256:C").with_label("C"));
    g.add_node(Node::new("rev:sha256:D").with_label("D"));
    g.add_edge(Edge::new("rev:sha256:B1", "rev:sha256:A"));
    g.add_edge(Edge::new("rev:sha256:B2", "rev:sha256:A"));
    g.add_edge(Edge::new("rev:sha256:C", "rev:sha256:B1"));
    g
}

#[test]
fn layout_graph_returns_positions_polylines_and_bounds() {
    let laid: LaidOutGraph =
        layout_graph(&supersession_graph(), &LayoutOptions::default()).expect("layout succeeds");

    assert_eq!(laid.nodes.len(), 5);
    assert_eq!(laid.edges.len(), 3);
    assert!(laid.width > 0.0 && laid.height > 0.0);

    // Per-node position + size are present; centers are real points.
    let b1 = laid
        .nodes
        .iter()
        .find(|n| n.id == "rev:sha256:B1")
        .expect("B1 present");
    let _center: FPoint = b1.center;
    assert!(b1.width > 0.0 && b1.height > 0.0);

    // Routed polylines available for every edge.
    for edge in &laid.edges {
        assert!(edge.points.len() >= 2);
    }
}

#[test]
fn opaque_ids_round_trip_through_public_api() {
    let laid =
        layout_graph(&supersession_graph(), &LayoutOptions::default()).expect("layout succeeds");
    assert!(
        laid.edges
            .iter()
            .any(|e| e.from == "rev:sha256:C" && e.to == "rev:sha256:B1")
    );
}

#[test]
fn public_layout_is_deterministic() {
    let g = supersession_graph();
    let first = layout_graph(&g, &LayoutOptions::default()).expect("layout succeeds");
    let second = layout_graph(&g, &LayoutOptions::default()).expect("layout succeeds");
    assert_eq!(first, second);
}

#[test]
fn layout_level_option_is_selectable_via_public_api() {
    let options = LayoutOptions::default().with_geometry_level(GeometryLevel::Layout);
    let laid = layout_graph(&supersession_graph(), &options).expect("layout succeeds");
    assert!(laid.edges.iter().all(|e| e.points.is_empty()));
}
