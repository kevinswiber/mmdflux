//! Shared rank-manipulation helpers for layered layout algorithms.

use std::collections::VecDeque;

use super::graph::LayoutGraph;

/// Assign ranks to nodes using longest-path algorithm.
pub(crate) fn longest_path(graph: &mut LayoutGraph) {
    let n = graph.node_ids.len();
    if n == 0 {
        return;
    }

    // Get effective edges (with reversals applied)
    let edges = graph.effective_edges();

    // Build adjacency and compute in-degrees
    let mut in_degree = vec![0usize; n];
    let mut successors: Vec<Vec<(usize, i32)>> = vec![Vec::new(); n];

    for (edge_idx, &(from, to)) in edges.iter().enumerate() {
        let minlen = graph.edge_minlens[edge_idx];
        successors[from].push((to, minlen));
        in_degree[to] += 1;
    }

    // Kahn's algorithm with rank tracking
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut ranks = vec![0i32; n];

    // Start with nodes that have no predecessors
    for (node, &degree) in in_degree.iter().enumerate() {
        if degree == 0 {
            queue.push_back(node);
        }
    }

    // Handle disconnected nodes (no edges at all)
    if queue.is_empty() {
        // All nodes have incoming edges - must be cycles only
        // Start with the first node
        queue.push_back(0);
        ranks[0] = 0;
    }

    let mut processed = 0;
    while let Some(node) = queue.pop_front() {
        processed += 1;
        for &(succ, minlen) in &successors[node] {
            ranks[succ] = ranks[succ].max(ranks[node] + minlen);

            in_degree[succ] -= 1;
            if in_degree[succ] == 0 {
                queue.push_back(succ);
            }
        }
    }

    // Handle remaining unprocessed nodes (shouldn't happen after acyclic phase)
    if processed < n {
        let max_rank = *ranks.iter().max().unwrap_or(&0);
        for node in 0..n {
            if ranks[node] == 0 && in_degree[node] > 0 {
                ranks[node] = max_rank + 1;
            }
        }
    }

    graph.ranks = ranks;
}

/// Normalize ranks so minimum is 0.
pub(crate) fn normalize(graph: &mut LayoutGraph) {
    // Prefer minimum rank among position nodes, fall back to all nodes
    let min = graph
        .ranks
        .iter()
        .enumerate()
        .filter_map(|(idx, &rank)| graph.is_position_node(idx).then_some(rank))
        .min()
        .or_else(|| graph.ranks.iter().copied().min());

    if let Some(min) = min {
        for rank in &mut graph.ranks {
            *rank -= min;
        }
    }
}
