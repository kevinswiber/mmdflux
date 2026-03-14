//! Phase 2: Assign nodes to ranks (layers).
//!
//! Dispatches to the configured ranking algorithm:
//! - NetworkSimplex (default): optimal rank assignment minimizing total edge length
//! - LongestPath: fast heuristic via Kahn's topological sort

use super::graph::LayoutGraph;
use super::rank_core;
use super::types::{LayoutConfig, Ranker};

/// Assign ranks to nodes by dispatching to the configured ranker.
pub fn run(graph: &mut LayoutGraph, config: &LayoutConfig) {
    match config.ranker {
        Ranker::NetworkSimplex => super::network_simplex::run(graph),
        Ranker::LongestPath => rank_core::longest_path(graph),
    }
}

/// Assign ranks to nodes using longest-path algorithm.
#[cfg(test)]
pub fn longest_path(graph: &mut LayoutGraph) {
    rank_core::longest_path(graph);
}

/// Normalize ranks so minimum is 0.
pub fn normalize(graph: &mut LayoutGraph) {
    rank_core::normalize(graph);
}

/// Remove empty ranks that were introduced by nesting minlen multiplication.
///
/// Matches dagre.js `util.removeEmptyRanks()`. After nesting multiplies edge
/// minlens by `nodeRankFactor`, ranking creates large gaps. This function
/// compresses out empty ranks at positions that aren't multiples of
/// `nodeRankFactor`, keeping border nodes on separate ranks from content.
pub fn remove_empty_ranks(graph: &mut LayoutGraph) {
    let node_rank_factor = match graph.node_rank_factor {
        Some(f) if f > 1 => f,
        _ => return,
    };

    // Find the offset (minimum rank)
    let offset = graph.ranks.iter().copied().min().unwrap_or(0);

    // Build layers array
    let max_rank = graph.ranks.iter().copied().max().unwrap_or(0);
    let layer_count = (max_rank - offset + 1) as usize;
    let mut layers: Vec<Option<Vec<usize>>> = vec![None; layer_count];

    for (node, &rank) in graph.ranks.iter().enumerate() {
        let idx = (rank - offset) as usize;
        layers[idx].get_or_insert_with(Vec::new).push(node);
    }

    // Compute delta: for each empty layer at a non-factor position, decrement delta
    let mut delta: i32 = 0;
    for (i, layer) in layers.iter().enumerate() {
        match layer {
            None if (i as i32) % node_rank_factor != 0 => {
                delta -= 1;
            }
            Some(nodes) if delta != 0 => {
                for &node in nodes {
                    graph.ranks[node] += delta;
                }
            }
            _ => {}
        }
    }
}

/// Get nodes grouped by rank.
pub fn by_rank(graph: &LayoutGraph) -> Vec<Vec<usize>> {
    let max_rank = graph.ranks.iter().max().copied().unwrap_or(0) as usize;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];

    for (node, &rank) in graph.ranks.iter().enumerate() {
        layers[rank as usize].push(node);
    }

    layers
}

/// Get nodes grouped by rank, filtered by a predicate.
pub fn by_rank_filtered<F>(graph: &LayoutGraph, mut predicate: F) -> Vec<Vec<usize>>
where
    F: FnMut(usize) -> bool,
{
    // Collect matching nodes with their ranks
    let matching: Vec<(usize, i32)> = graph
        .ranks
        .iter()
        .enumerate()
        .filter(|&(node, _)| predicate(node))
        .map(|(node, &rank)| (node, rank))
        .collect();

    let Some(&max_rank) = matching.iter().map(|(_, r)| r).max() else {
        return Vec::new();
    };

    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank as usize + 1];
    for (node, rank) in matching {
        layers[rank as usize].push(node);
    }

    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::DiGraph;

    #[test]
    fn test_run_with_longest_path_config() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("A", "B");
        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let config = LayoutConfig {
            ranker: Ranker::LongestPath,
            ..Default::default()
        };
        run(&mut lg, &config);
        normalize(&mut lg);
        assert_eq!(lg.ranks[lg.node_index[&"A".into()]], 0);
        assert_eq!(lg.ranks[lg.node_index[&"B".into()]], 1);
    }

    #[test]
    fn test_rank_linear_chain() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg, &LayoutConfig::default());
        normalize(&mut lg);

        // A=0, B=1, C=2
        assert_eq!(lg.ranks[lg.node_index[&"A".into()]], 0);
        assert_eq!(lg.ranks[lg.node_index[&"B".into()]], 1);
        assert_eq!(lg.ranks[lg.node_index[&"C".into()]], 2);
    }

    #[test]
    fn test_rank_diamond() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg, &LayoutConfig::default());
        normalize(&mut lg);

        // A=0, B=C=1, D=2
        assert_eq!(lg.ranks[lg.node_index[&"A".into()]], 0);
        assert_eq!(lg.ranks[lg.node_index[&"B".into()]], 1);
        assert_eq!(lg.ranks[lg.node_index[&"C".into()]], 1);
        assert_eq!(lg.ranks[lg.node_index[&"D".into()]], 2);
    }

    #[test]
    fn test_rank_disconnected() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        // No edges - all disconnected

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg, &LayoutConfig::default());
        normalize(&mut lg);

        // All should be at rank 0
        assert_eq!(lg.ranks[0], 0);
        assert_eq!(lg.ranks[1], 0);
        assert_eq!(lg.ranks[2], 0);
    }

    #[test]
    fn test_longest_path_respects_minlen() {
        // A -> B with minlen=2, B -> C with minlen=1
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        lg.edge_minlens[0] = 2; // A->B needs minlen=2

        run(&mut lg, &LayoutConfig::default());
        normalize(&mut lg);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        assert_eq!(lg.ranks[a], 0);
        assert_eq!(lg.ranks[b], 2); // minlen=2 from A
        assert_eq!(lg.ranks[c], 3); // minlen=1 from B
    }

    #[test]
    fn test_by_rank() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        run(&mut lg, &LayoutConfig::default());
        normalize(&mut lg);

        let layers = by_rank(&lg);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].len(), 1); // A
        assert_eq!(layers[1].len(), 2); // B, C
        assert_eq!(layers[2].len(), 1); // D
    }
}
