use super::{DiGraph, LayoutConfig, NodeId, layout};

#[test]
fn graph_layered_pipeline_entrypoint_preserves_current_layout_contract() {
    let graph = simple_graph_input();
    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    assert!(result.nodes.contains_key(&NodeId::from("A")));
    assert!(!result.edges.is_empty());
}

fn simple_graph_input() -> DiGraph<(f64, f64)> {
    let mut graph = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B");
    graph
}
