//! Phase 1: Make the graph acyclic by identifying back-edges.
//!
//! Uses a DFS-based approach to identify back-edges - edges that point
//! to ancestors in the DFS tree. This preserves the natural forward flow
//! of the graph better than minimum feedback arc set algorithms.

use std::collections::BTreeSet;

use super::graph::LayoutGraph;

/// Identify back-edges that need to be reversed for acyclicity.
/// Marks edges in the LayoutGraph's reversed_edges set.
///
/// Uses DFS to find back-edges (edges pointing to ancestors in the DFS tree).
/// This preserves the natural forward flow of the graph better than
/// greedy_feedback_arc_set which may reverse arbitrary edges.
pub fn run(graph: &mut LayoutGraph) {
    let n = graph.node_ids.len();
    if n == 0 {
        return;
    }

    // Build adjacency list: node -> [(edge_idx, target_node)]
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (edge_idx, &(from, to, _)) in graph.edges.iter().enumerate() {
        adj[from].push((edge_idx, to));
    }

    // DFS state
    let mut visited = vec![false; n];
    let mut in_stack = vec![false; n]; // Nodes currently in the recursion stack
    let mut back_edges: BTreeSet<usize> = BTreeSet::new();

    // Run DFS from each unvisited node (handles disconnected components)
    for start in 0..n {
        if !visited[start] {
            dfs_find_back_edges(start, &adj, &mut visited, &mut in_stack, &mut back_edges);
        }
    }

    graph.reversed_edges = back_edges;
}

/// DFS helper to find back-edges.
fn dfs_find_back_edges(
    node: usize,
    adj: &[Vec<(usize, usize)>],
    visited: &mut [bool],
    in_stack: &mut [bool],
    back_edges: &mut BTreeSet<usize>,
) {
    visited[node] = true;
    in_stack[node] = true;

    for &(edge_idx, target) in &adj[node] {
        if !visited[target] {
            // Tree edge - recurse
            dfs_find_back_edges(target, adj, visited, in_stack, back_edges);
        } else if in_stack[target] {
            // Back edge - target is an ancestor in the current DFS path
            back_edges.insert(edge_idx);
        }
        // Cross edges (visited but not in stack) are fine, don't reverse
    }

    in_stack[node] = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::DiGraph;

    #[test]
    fn test_acyclic_no_cycle() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg);

        assert!(lg.reversed_edges.is_empty());
    }

    #[test]
    fn test_acyclic_simple_cycle() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("A", "B");
        graph.add_edge("B", "A"); // Cycle

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg);

        // One edge should be marked for reversal
        assert_eq!(lg.reversed_edges.len(), 1);
    }

    #[test]
    fn test_acyclic_triangle_cycle() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");
        graph.add_edge("C", "A"); // Creates cycle

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg);

        // One edge should be reversed to break the cycle
        assert_eq!(lg.reversed_edges.len(), 1);
    }

    #[test]
    fn test_acyclic_self_loop() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_edge("A", "A"); // Self-loop

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg);

        // Self-loop should be marked for reversal
        assert_eq!(lg.reversed_edges.len(), 1);
    }

    #[test]
    fn test_acyclic_disconnected() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");
        // Two disconnected components, no cycles

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg);

        assert!(lg.reversed_edges.is_empty());
    }
}
