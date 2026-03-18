//! Graph diagram tests that require cross-boundary imports (mermaid + diagrams).
//! Moved from graph::diagram to respect module boundary rules.

use crate::diagrams::flowchart::compile_to_graph;
use crate::mermaid::parse_flowchart;

#[test]
fn test_subgraph_children() {
    let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB\nend\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let children = diagram.subgraph_children("outer");
    assert_eq!(children.len(), 1);
    assert!(children.contains(&&"inner".to_string()));
    assert!(diagram.subgraph_children("inner").is_empty());
}

#[test]
fn test_subgraph_depth() {
    let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA\nend\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert_eq!(diagram.subgraph_depth("outer"), 0);
    assert_eq!(diagram.subgraph_depth("inner"), 1);
}

#[test]
fn subgraph_parse_order_is_postorder() {
    let input = include_str!("../../tests/fixtures/flowchart/external_node_subgraph.mmd");
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);

    assert_eq!(
        diagram.subgraph_order,
        vec![
            "us-east".to_string(),
            "us-west".to_string(),
            "Cloud".to_string(),
        ]
    );
}

#[test]
fn cross_boundary_edges_isolated_subgraph() {
    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> D";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert!(!diagram.subgraph_has_cross_boundary_edges("sg1"));
}

#[test]
fn cross_boundary_edges_non_isolated_subgraph() {
    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert!(diagram.subgraph_has_cross_boundary_edges("sg1"));
}

#[test]
fn cross_boundary_edges_outgoing() {
    let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nB --> C";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert!(diagram.subgraph_has_cross_boundary_edges("sg1"));
}

#[test]
fn cross_boundary_edges_nested_outer_has_inner_does_not() {
    let input = "graph TD\nsubgraph outer[Outer]\ndirection LR\nsubgraph inner[Inner]\ndirection BT\nA --> B\nend\nC --> D\nend\nE --> C";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    assert!(diagram.subgraph_has_cross_boundary_edges("outer"));
    assert!(!diagram.subgraph_has_cross_boundary_edges("inner"));
}
