use std::collections::HashMap;

use super::{DiGraph, LayoutConfig, layout};

#[test]
fn model_order_owner_local_smoke_runs_next_to_kernel() {
    let positions = layout_x_positions(smoke_model_order_graph());

    assert!(positions["B"] < positions["C"]);
}

fn layout_x_positions(
    graph_def: (
        &'static [&'static str],
        &'static [(&'static str, &'static str)],
    ),
) -> HashMap<String, f64> {
    let (nodes, edges) = graph_def;
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for &node in nodes {
        graph.add_node(node, (10.0, 10.0));
    }
    for &(from, to) in edges {
        graph.add_edge(from, to);
    }

    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
    result
        .nodes
        .iter()
        .map(|(id, rect)| (id.0.clone(), rect.x + rect.width / 2.0))
        .collect()
}

fn smoke_model_order_graph() -> (
    &'static [&'static str],
    &'static [(&'static str, &'static str)],
) {
    static NODES: [&str; 3] = ["A", "B", "C"];
    static EDGES: [(&str, &str); 2] = [("A", "B"), ("A", "C")];
    (&NODES, &EDGES)
}
