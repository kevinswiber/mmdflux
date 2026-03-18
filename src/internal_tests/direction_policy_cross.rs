//! Direction-policy tests that require cross-boundary imports (mermaid + diagrams).
//! Moved from graph::direction_policy to respect module boundary rules.

use crate::diagrams::flowchart::compile_to_graph;
use crate::graph::Direction;
use crate::graph::direction_policy::{
    build_node_directions, build_override_node_map, cross_boundary_edge_direction,
};
use crate::mermaid::parse_flowchart;

#[test]
fn cross_boundary_direction_uses_ancestor_override() {
    let input = "graph TD\nsubgraph outer\ndirection LR\nA\nsubgraph inner\ndirection BT\nB\nend\nA --> B\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let dirs = build_node_directions(&diagram);
    let override_nodes = build_override_node_map(&diagram);

    let direction = cross_boundary_edge_direction(
        &diagram,
        &dirs,
        override_nodes.get("A"),
        override_nodes.get("B"),
        "A",
        "B",
        Direction::TopDown,
    );

    assert_eq!(direction, Direction::LeftRight);
}
