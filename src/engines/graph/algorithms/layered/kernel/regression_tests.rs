//! Internal regression tests for the shared layered kernel.
//!
//! These are not the external dagre.js fixture comparisons from
//! `tests/dagre_parity.rs`. Some cases pin dagre-compatible semantics
//! required by Mermaid output; others protect shared kernel behavior used
//! by both Mermaid and Flux engines.

use std::collections::HashMap;

use super::graph::LayoutGraph;
use super::support::{
    assign_node_intersects, compute_rank_sep_overrides, count_forward_edges_per_gap,
    extract_self_edges, insert_self_edge_dummies, make_space_for_edge_labels,
    make_space_for_labeled_edges, position_self_edges, select_label_sides, switch_label_dummies,
    translate_layout_result,
};
use super::types::{
    DummyChain, DummyNode, DummyType, EdgeLabelInfo, LabelPos, LabelSide, WaypointWithRank,
};
use super::*;

/// Test helper: run the layout pipeline up to rank assignment and count forward edges per gap.
fn count_edges_per_gap_for_test(
    graph: &DiGraph<(f64, f64)>,
    config: &LayoutConfig,
) -> HashMap<i32, usize> {
    let mut lg = LayoutGraph::from_digraph(graph, |_, dims| *dims);
    extract_self_edges(&mut lg);
    if config.acyclic {
        acyclic::run(&mut lg);
    }
    make_space_for_edge_labels(&mut lg);
    let mut config = config.clone();
    config.rank_sep /= 2.0;
    rank::run(&mut lg, &config);
    rank::normalize(&mut lg);
    count_forward_edges_per_gap(&lg)
}

#[test]
fn count_edges_per_gap_linear_chain() {
    // A -> B -> C: 1 edge in each gap
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");

    let config = LayoutConfig::default();
    let counts = count_edges_per_gap_for_test(&graph, &config);
    // Each gap should have exactly 1 edge
    assert!(
        counts.values().all(|&c| c <= 1),
        "Linear chain should have at most 1 edge per gap, got {:?}",
        counts
    );
}

#[test]
fn count_edges_per_gap_fan_out() {
    // A -> B, A -> C, A -> D: 3 edges in gap between A's rank and B/C/D's rank
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_node("D", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "C");
    graph.add_edge("A", "D");

    let config = LayoutConfig::default();
    let counts = count_edges_per_gap_for_test(&graph, &config);
    let max_count = *counts.values().max().unwrap_or(&0);
    assert!(
        max_count >= 3,
        "fan-out should have 3+ edges in densest gap, got {}",
        max_count
    );
}

#[test]
fn count_edges_per_gap_five_fan_in() {
    // A,B,C,D,E -> F: 5 edges in the gap before F
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D", "E", "F"] {
        graph.add_node(id, (10.0, 10.0));
    }
    for src in ["A", "B", "C", "D", "E"] {
        graph.add_edge(src, "F");
    }

    let config = LayoutConfig::default();
    let counts = count_edges_per_gap_for_test(&graph, &config);
    let max_count = *counts.values().max().unwrap_or(&0);
    assert!(
        max_count >= 5,
        "5-fan-in should have 5 edges in densest gap, got {}",
        max_count
    );
}

#[test]
fn count_edges_per_gap_excludes_backward() {
    // A -> B, B -> A (cycle): only A -> B is forward, B -> A is reversed
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "A");

    let config = LayoutConfig::default();
    let counts = count_edges_per_gap_for_test(&graph, &config);
    let max_count = *counts.values().max().unwrap_or(&0);
    // Only the forward edge (A->B) should be counted; B->A is reversed
    assert!(
        max_count <= 1,
        "backward edge should not be counted, got max {}",
        max_count
    );
}

#[test]
fn count_edges_per_gap_excludes_long_backward_chain() {
    // A -> B -> C -> D, D -> A (reversed long edge spanning 3 ranks).
    // After normalization, D -> A becomes 3 chain edges. These chain edges
    // must NOT be counted as forward edges — they represent a backward edge.
    // Only A->B, B->C, C->D (1 edge per gap) should be counted.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D"] {
        graph.add_node(id, (10.0, 10.0));
    }
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");
    graph.add_edge("C", "D");
    graph.add_edge("D", "A"); // backward edge

    let config = LayoutConfig::default();
    let counts = count_edges_per_gap_for_test(&graph, &config);
    let max_count = *counts.values().max().unwrap_or(&0);
    // Each gap should have exactly 1 forward edge (A->B, B->C, C->D).
    // The reversed D->A chain edges should NOT be counted.
    assert!(
        max_count <= 1,
        "reversed long edge chain should not inflate gap counts, got max {}; counts: {:?}",
        max_count,
        counts
    );
}

// Phase 1 (global inflation B1) was prototyped and discarded in favor of
// per-gap variable spacing (B2). The global approach inflated ALL gaps
// uniformly, which wasted space in sparse gaps of mixed-density diagrams
// and caused compound-graph test failures. Per-gap tests are below.

/// Test helper: run the pipeline up to rank assignment and compute rank_sep overrides.
fn compute_overrides_for_test(
    graph: &DiGraph<(f64, f64)>,
    config: &LayoutConfig,
) -> HashMap<i32, f64> {
    let mut lg = LayoutGraph::from_digraph(graph, |_, dims| *dims);
    extract_self_edges(&mut lg);
    if config.acyclic {
        acyclic::run(&mut lg);
    }
    make_space_for_edge_labels(&mut lg);
    let mut config = config.clone();
    config.rank_sep /= 2.0;
    rank::run(&mut lg, &config);
    rank::normalize(&mut lg);
    compute_rank_sep_overrides(&lg, &config)
}

#[test]
fn compute_overrides_five_fan_in_inflates_dense_gap() {
    // A,B,C,D,E -> F: 5 edges in one gap, all other gaps have 0-1
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D", "E", "F"] {
        graph.add_node(id, (10.0, 10.0));
    }
    for src in ["A", "B", "C", "D", "E"] {
        graph.add_edge(src, "F");
    }

    let config = LayoutConfig::default();
    let overrides = compute_overrides_for_test(&graph, &config);

    // There should be at least one override with a value > base rank_sep
    let base = config.rank_sep / 2.0; // after halving in layout_with_labels
    let has_inflation = overrides.values().any(|&v| v > base);
    assert!(
        has_inflation,
        "Should have at least one inflated gap, overrides: {:?}",
        overrides
    );
}

#[test]
fn compute_overrides_linear_chain_no_overrides() {
    // A -> B -> C: max 1 edge per gap, no inflation needed
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");

    let config = LayoutConfig::default();
    let overrides = compute_overrides_for_test(&graph, &config);

    // No gap has more than threshold edges, so no overrides needed
    assert!(
        overrides.is_empty(),
        "Linear chain should have no overrides, got: {:?}",
        overrides
    );
}

#[test]
fn compute_overrides_mixed_density() {
    // A -> B, A -> C (fan-out, 2 edges in gap)
    // B -> D, C -> D (fan-in, 2 edges in gap)
    // Plus E -> D, F -> D (adds to fan-in gap, exceeding threshold)
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D", "E", "F"] {
        graph.add_node(id, (10.0, 10.0));
    }
    graph.add_edge("A", "B");
    graph.add_edge("A", "C");
    graph.add_edge("B", "D");
    graph.add_edge("C", "D");
    graph.add_edge("E", "D");
    graph.add_edge("F", "D");

    let config = LayoutConfig::default();
    let overrides = compute_overrides_for_test(&graph, &config);

    // The fan-in gap before D should have an override (4+ edges),
    // but the fan-out gap after A has only 2 edges (at threshold)
    let has_some_override = !overrides.is_empty();
    assert!(
        has_some_override,
        "Mixed graph should have at least one override"
    );
}

#[test]
fn layout_five_fan_in_denser_gap_than_sparse() {
    // Two-layer graph: A,B,C,D,E -> F (5 edges in one gap).
    // With per-gap spacing, the gap between source and target ranks should
    // be wider than in a simple 1-edge graph.
    let mut graph_dense: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D", "E", "F"] {
        graph_dense.add_node(id, (10.0, 10.0));
    }
    for src in ["A", "B", "C", "D", "E"] {
        graph_dense.add_edge(src, "F");
    }

    let mut graph_sparse: DiGraph<(f64, f64)> = DiGraph::new();
    graph_sparse.add_node("X", (10.0, 10.0));
    graph_sparse.add_node("Y", (10.0, 10.0));
    graph_sparse.add_edge("X", "Y");

    let config = LayoutConfig {
        variable_rank_spacing: true,
        ..LayoutConfig::default()
    };
    let dense_result = layout(&graph_dense, &config, |_, dims| *dims);
    let sparse_result = layout(&graph_sparse, &config, |_, dims| *dims);

    // Dense layout should be taller because its gap is inflated
    assert!(
        dense_result.height > sparse_result.height,
        "Dense 5-fan-in (h={}) should be taller than sparse (h={})",
        dense_result.height,
        sparse_result.height,
    );
}

#[test]
fn layout_mixed_density_selective_inflation() {
    // A -> B -> C, A -> C (long edge), D -> C, E -> C
    // The gap before C should be denser than the gap after A
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    for id in ["A", "B", "C", "D", "E"] {
        graph.add_node(id, (10.0, 10.0));
    }
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");
    graph.add_edge("A", "C"); // long edge, crosses both gaps
    graph.add_edge("D", "C");
    graph.add_edge("E", "C");

    let config = LayoutConfig {
        variable_rank_spacing: true,
        ..LayoutConfig::default()
    };
    let result = layout(&graph, &config, |_, dims| *dims);

    // Should produce a valid layout without panicking
    assert!(result.height > 0.0);
    assert_eq!(result.nodes.len(), 5);
}

#[test]
fn layout_sparse_graph_unchanged_by_variable_spacing() {
    // A -> B -> C: no dense gaps, layout should be identical to fixed rank_sep
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    let a_y = result.nodes.get(&"A".into()).unwrap().y;
    let b_y = result.nodes.get(&"B".into()).unwrap().y;
    let c_y = result.nodes.get(&"C".into()).unwrap().y;

    // Gaps should be equal (no overrides applied)
    let gap_ab = b_y - a_y;
    let gap_bc = c_y - b_y;
    assert!(
        (gap_ab - gap_bc).abs() < 1.0,
        "Sparse chain gaps should be equal: ab={}, bc={}",
        gap_ab,
        gap_bc,
    );
}

#[test]
fn test_simple_layout() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    assert_eq!(result.nodes.len(), 2);
    assert_eq!(result.edges.len(), 1);

    // A should be above B in TopBottom layout
    let a_rect = result.nodes.get(&"A".into()).unwrap();
    let b_rect = result.nodes.get(&"B".into()).unwrap();
    assert!(a_rect.y < b_rect.y);
}

#[test]
fn test_layout_with_cycle() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "A"); // Cycle

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    // Should still produce a valid layout
    assert_eq!(result.nodes.len(), 2);
    // One edge should be reversed
    assert_eq!(result.reversed_edges.len(), 1);
}

#[test]
fn test_layout_directions() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B");

    // Test LR direction
    let config = LayoutConfig {
        direction: Direction::LeftRight,
        ..Default::default()
    };
    let result = layout(&graph, &config, |_, dims| *dims);

    let a_rect = result.nodes.get(&"A".into()).unwrap();
    let b_rect = result.nodes.get(&"B".into()).unwrap();
    // A should be left of B in LeftRight layout
    assert!(a_rect.x < b_rect.x);
}

#[test]
fn test_http_request_cycle() {
    // Simulates http_request.mmd graph
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("Client", (100.0, 50.0));
    graph.add_node("Server", (100.0, 50.0));
    graph.add_node("Auth", (100.0, 50.0));
    graph.add_node("Process", (100.0, 50.0));
    graph.add_node("Reject", (100.0, 50.0));
    graph.add_node("Response", (100.0, 50.0));

    // Edges in order from the mmd file
    graph.add_edge("Client", "Server");
    graph.add_edge("Server", "Auth");
    graph.add_edge("Auth", "Process");
    graph.add_edge("Auth", "Reject");
    graph.add_edge("Process", "Response");
    graph.add_edge("Reject", "Response");
    graph.add_edge("Response", "Client"); // Creates cycle

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    // Client should be at the top (smallest y)
    let client_y = result.nodes.get(&"Client".into()).unwrap().y;
    let server_y = result.nodes.get(&"Server".into()).unwrap().y;
    let auth_y = result.nodes.get(&"Auth".into()).unwrap().y;
    let process_y = result.nodes.get(&"Process".into()).unwrap().y;
    let response_y = result.nodes.get(&"Response".into()).unwrap().y;

    assert!(client_y < server_y, "Client should be above Server");
    assert!(server_y < auth_y, "Server should be above Auth");
    assert!(auth_y < process_y, "Auth should be above Process");
    assert!(process_y < response_y, "Process should be above Response");
}

#[test]
fn test_layout_with_long_edge() {
    // A -> B -> C -> D, and A -> D (long edge spanning 3 ranks)
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_node("C", (100.0, 50.0));
    graph.add_node("D", (100.0, 50.0));
    graph.add_edge("A", "B");
    graph.add_edge("B", "C");
    graph.add_edge("C", "D");
    graph.add_edge("A", "D"); // Long edge: spans 3 ranks

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    // Should have 4 nodes
    assert_eq!(result.nodes.len(), 4);

    // Should have 4 edges
    assert_eq!(result.edges.len(), 4);

    // The A->D edge should have waypoints
    let ad_edge = result
        .edges
        .iter()
        .find(|e| e.from.0 == "A" && e.to.0 == "D");
    assert!(ad_edge.is_some(), "Should have A->D edge");

    let ad_edge = ad_edge.unwrap();
    // A->D spans 3 ranks. Points include start, end, and intermediate waypoints.
    assert!(
        ad_edge.points.len() >= 4,
        "A->D edge should have at least 4 points, got {}",
        ad_edge.points.len()
    );

    // Verify waypoints were extracted
    assert!(
        result.edge_waypoints.contains_key(&ad_edge.index),
        "Should have waypoints for long edge"
    );
}

#[test]
fn test_make_space_doubles_all_minlens() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (5.0, 3.0));
    graph.add_node("B", (5.0, 3.0));
    graph.add_node("C", (5.0, 3.0));
    graph.add_edge("A", "B"); // edge 0: labeled
    graph.add_edge("B", "C"); // edge 1: unlabeled

    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

    make_space_for_edge_labels(&mut lg);

    // ALL edges should be doubled, not just the labeled one
    assert_eq!(lg.edge_minlens[0], 2); // labeled edge: 1 * 2 = 2
    assert_eq!(lg.edge_minlens[1], 2); // unlabeled edge: 1 * 2 = 2
}

#[test]
fn test_make_space_doubles_without_labels() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (5.0, 3.0));
    graph.add_node("B", (5.0, 3.0));
    graph.add_edge("A", "B");

    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
    make_space_for_edge_labels(&mut lg);

    assert_eq!(lg.edge_minlens[0], 2); // doubled even without labels
}

#[test]
fn make_space_for_labeled_edges_only_bumps_labeled() {
    // 3 edges: edge 0 labeled (minlen=1), edge 1 unlabeled (minlen=1),
    // edge 2 labeled (minlen=2, already sufficient)
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (5.0, 3.0));
    graph.add_node("B", (5.0, 3.0));
    graph.add_node("C", (5.0, 3.0));
    graph.add_node("D", (5.0, 3.0));
    graph.add_edge("A", "B"); // edge 0: labeled
    graph.add_edge("B", "C"); // edge 1: unlabeled
    graph.add_edge("C", "D"); // edge 2: labeled

    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
    // Set edge 2 minlen to 2 (already sufficient for a label)
    lg.edge_minlens[2] = 2;

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(50.0, 5.0));
    edge_labels.insert(2, EdgeLabelInfo::new(50.0, 5.0));

    make_space_for_labeled_edges(&mut lg, &edge_labels);

    assert_eq!(lg.edge_minlens[0], 2); // bumped: was 1, needs at least 2
    assert_eq!(lg.edge_minlens[1], 1); // unchanged: no label
    assert_eq!(lg.edge_minlens[2], 2); // unchanged: already >= 2
}

// switch_label_dummies tests

/// Build a LayoutGraph with a long labeled edge spanning `span` ranks.
/// Returns graph with A at rank 0, B at rank `span`, and `span-1` dummies between.
/// The label dummy is at the midpoint rank.
fn build_graph_with_long_labeled_edge(span: usize) -> LayoutGraph {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

    let a_idx = lg.node_index[&NodeId::from("A")];
    let b_idx = lg.node_index[&NodeId::from("B")];
    lg.ranks[a_idx] = 0;
    lg.ranks[b_idx] = span as i32;

    let midpoint_rank = (span / 2) as i32;

    let mut chain = DummyChain::new(0);
    for r in 1..span {
        let rank = r as i32;
        let dummy_id = NodeId::from(format!("_d{}", r));
        let dummy_idx = lg.node_ids.len();

        let is_label = rank == midpoint_rank;
        let (w, h) = if is_label { (30.0, 14.0) } else { (0.0, 0.0) };

        let dummy_node = if is_label {
            DummyNode::edge_label(0, rank, w, h, LabelPos::Center)
        } else {
            DummyNode::edge(0, rank)
        };

        lg.node_ids.push(dummy_id.clone());
        lg.node_index.insert(dummy_id.clone(), dummy_idx);
        lg.ranks.push(rank);
        lg.order.push(dummy_idx);
        lg.positions.push(Point::default());
        lg.dimensions.push((w, h));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);
        lg.dummy_nodes.insert(dummy_id.clone(), dummy_node);

        if is_label {
            chain.label_dummy_index = Some(chain.dummy_ids.len());
        }
        chain.dummy_ids.push(dummy_id);
    }
    lg.dummy_chains.push(chain);
    lg
}

#[test]
fn switch_midpoint_strategy_is_noop() {
    let mut lg = build_graph_with_long_labeled_edge(6);
    let original_idx = lg.dummy_chains[0].label_dummy_index;
    switch_label_dummies(&mut lg, LabelDummyStrategy::Midpoint);
    assert_eq!(lg.dummy_chains[0].label_dummy_index, original_idx);
}

#[test]
fn switch_to_widest_layer_stays_if_midpoint_is_widest() {
    let mut lg = build_graph_with_long_labeled_edge(6);
    // Label is at midpoint rank 3 (chain index 2). Make rank 3 the widest
    // by adding a wide "real" node at that rank.
    let wide_id = NodeId::from("Wide");
    let wide_idx = lg.node_ids.len();
    lg.node_ids.push(wide_id.clone());
    lg.node_index.insert(wide_id, wide_idx);
    lg.ranks.push(3);
    lg.order.push(wide_idx);
    lg.positions.push(Point::default());
    lg.dimensions.push((200.0, 20.0));
    lg.original_has_predecessor.push(false);
    lg.parents.push(None);
    lg.model_order.push(None);

    switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);
    // Should stay at the same position since rank 3 is widest
    assert_eq!(lg.dummy_chains[0].label_dummy_index, Some(2));
}

#[test]
fn switch_moves_label_to_wider_layer() {
    let mut lg = build_graph_with_long_labeled_edge(6);
    // Label is at midpoint rank 3 (chain index 2). Make rank 4 wider.
    let wide_id = NodeId::from("Wide");
    let wide_idx = lg.node_ids.len();
    lg.node_ids.push(wide_id.clone());
    lg.node_index.insert(wide_id, wide_idx);
    lg.ranks.push(4);
    lg.order.push(wide_idx);
    lg.positions.push(Point::default());
    lg.dimensions.push((200.0, 20.0));
    lg.original_has_predecessor.push(false);
    lg.parents.push(None);
    lg.model_order.push(None);

    switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);
    // Label should move to chain index 3 (rank 4)
    assert_eq!(lg.dummy_chains[0].label_dummy_index, Some(3));

    // Verify the old label dummy is now Edge type with 0x0 dimensions
    let old_label_id = &lg.dummy_chains[0].dummy_ids[2];
    let old_dummy = lg.dummy_nodes.get(old_label_id).unwrap();
    assert_eq!(old_dummy.dummy_type, DummyType::Edge);
    assert_eq!(lg.dimensions[lg.node_index[old_label_id]], (0.0, 0.0));

    // Verify the new label dummy is EdgeLabel type with label dimensions
    let new_label_id = &lg.dummy_chains[0].dummy_ids[3];
    let new_dummy = lg.dummy_nodes.get(new_label_id).unwrap();
    assert_eq!(new_dummy.dummy_type, DummyType::EdgeLabel);
    assert_eq!(lg.dimensions[lg.node_index[new_label_id]], (30.0, 14.0));
}

#[test]
fn get_label_position_uses_updated_chain_index() {
    let mut lg = build_graph_with_long_labeled_edge(6);
    // Make rank 4 wider to trigger a switch from midpoint (rank 3) to rank 4
    let wide_id = NodeId::from("Wide");
    let wide_idx = lg.node_ids.len();
    lg.node_ids.push(wide_id.clone());
    lg.node_index.insert(wide_id, wide_idx);
    lg.ranks.push(4);
    lg.order.push(wide_idx);
    lg.positions.push(Point::default());
    lg.dimensions.push((200.0, 20.0));
    lg.original_has_predecessor.push(false);
    lg.parents.push(None);
    lg.model_order.push(None);

    switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);

    // Set positions for all dummies so get_label_position returns meaningful values
    for (i, id) in lg.dummy_chains[0].dummy_ids.iter().enumerate() {
        let idx = lg.node_index[id];
        lg.positions[idx] = Point {
            x: 10.0,
            y: (i as f64) * 50.0,
        };
    }

    let pos = normalize::get_label_position(&lg, 0).unwrap();
    // After switch, label is at chain index 3 (rank 4), positioned at y=150.0
    // With label dimensions 30x14, center would be at y=150.0+7.0=157.0
    // (actually get_label_position uses LabelSide which defaults to Center)
    let new_label_idx = lg.node_index[&lg.dummy_chains[0].dummy_ids[3]];
    let expected_y = lg.positions[new_label_idx].y + lg.dimensions[new_label_idx].1 / 2.0;
    assert!(
        (pos.point.y - expected_y).abs() < 0.01,
        "Label position y={} should match switched dummy center y={}",
        pos.point.y,
        expected_y
    );
}

// select_label_sides tests

#[test]
fn select_label_sides_single_label_stays_center() {
    // A -> B -> C with only A->B labeled.
    // After normalization, there's one label dummy at the intermediate rank.
    // Single label in a layer should stay Center.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "C");

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(30.0, 10.0));

    let config = LayoutConfig {
        per_edge_label_spacing: true,
        ..Default::default()
    };
    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
    extract_self_edges(&mut lg);
    if config.acyclic {
        acyclic::run(&mut lg);
    }
    make_space_for_labeled_edges(&mut lg, &edge_labels);
    let mut config = config.clone();
    config.rank_sep /= 2.0;
    rank::run(&mut lg, &config);
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &edge_labels, false);
    order::run(&mut lg, false);
    select_label_sides(&mut lg);

    // Find the label dummy and check its side
    for dummy in lg.dummy_nodes.values() {
        if dummy.is_label() {
            assert_eq!(
                dummy.label_side,
                LabelSide::Center,
                "single label dummy should stay Center"
            );
        }
    }
}

#[test]
fn select_label_sides_two_parallel_labels_get_above_below() {
    // A -> C and B -> C, both labeled. The label dummies for both edges
    // should be in the same layer. One gets Above, the other Below.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "C");
    graph.add_edge("B", "C");

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(30.0, 10.0));
    edge_labels.insert(1, EdgeLabelInfo::new(30.0, 10.0));

    let config = LayoutConfig {
        per_edge_label_spacing: true,
        ..Default::default()
    };
    let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
    extract_self_edges(&mut lg);
    if config.acyclic {
        acyclic::run(&mut lg);
    }
    make_space_for_labeled_edges(&mut lg, &edge_labels);
    let mut config = config.clone();
    config.rank_sep /= 2.0;
    rank::run(&mut lg, &config);
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &edge_labels, false);
    order::run(&mut lg, false);
    select_label_sides(&mut lg);

    // Collect label dummy sides
    let mut sides: Vec<LabelSide> = lg
        .dummy_nodes
        .values()
        .filter(|d| d.is_label())
        .map(|d| d.label_side)
        .collect();
    sides.sort_by_key(|s| match s {
        LabelSide::Above => 0,
        LabelSide::Center => 1,
        LabelSide::Below => 2,
    });

    assert_eq!(
        sides,
        vec![LabelSide::Above, LabelSide::Below],
        "two label dummies in same layer should get Above and Below"
    );
}

#[test]
fn per_edge_spacing_unlabeled_edge_has_no_dummy() {
    // Graph: A -> B -> C, only A->B has a label.
    // With per_edge_label_spacing=true: B->C keeps minlen=1, so no dummy node
    // (edge_waypoints empty for edge 1). rank_sep is halved for both modes.
    // With per_edge_label_spacing=false: B->C is doubled to minlen=2, creating
    // a dummy node (edge_waypoints populated for edge 1).
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_node("C", (10.0, 10.0));
    graph.add_edge("A", "B"); // edge 0: labeled
    graph.add_edge("B", "C"); // edge 1: unlabeled

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(50.0, 5.0));

    // Per-edge mode: unlabeled edge should NOT get a dummy node
    let config_per_edge = LayoutConfig {
        per_edge_label_spacing: true,
        ..Default::default()
    };
    let result_per_edge =
        layout_with_labels(&graph, &config_per_edge, |_, dims| *dims, &edge_labels);

    // Edge 1 (B->C, unlabeled) should be a short edge — no waypoints
    assert!(
        !result_per_edge.edge_waypoints.contains_key(&1),
        "per-edge: unlabeled B->C should have no waypoints (short edge)"
    );
    // Edge 0 (A->B, labeled) should have waypoints from label dummy
    assert!(
        result_per_edge.edge_waypoints.contains_key(&0)
            || result_per_edge.label_positions.contains_key(&0),
        "per-edge: labeled A->B should have label position"
    );

    // Global mode: unlabeled edge DOES get a dummy node
    let config_global = LayoutConfig::default();
    let result_global = layout_with_labels(&graph, &config_global, |_, dims| *dims, &edge_labels);

    // Edge 1 (B->C, unlabeled) gets doubled to minlen=2, creating a dummy
    assert!(
        result_global.edge_waypoints.contains_key(&1),
        "global: unlabeled B->C should have waypoints (minlen doubled)"
    );
}

#[test]
fn test_ranksep_compensates_for_doubled_minlen() {
    // dagre.js halves ranksep when it doubles minlen (makeSpaceForEdgeLabels).
    // With doubled minlen, A→B spans 2 internal ranks with a gap rank between.
    // Halved ranksep (25) means the total spacing = height + 2*(ranksep/2) = 10 + 50 = 60.
    // Without halving, spacing would be height + 2*ranksep = 10 + 100 = 110.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_edge("A", "B");

    let config = LayoutConfig::default(); // rank_sep = 50, margin = 10
    let result = layout(&graph, &config, |_, dims| *dims);

    let a = result.nodes.get(&"A".into()).unwrap();
    let b = result.nodes.get(&"B".into()).unwrap();

    // Expected: dy = height + 2*(rank_sep/2) = 10 + 2*25 = 60
    let dy = b.y - a.y;
    assert!(
        (dy - 60.0).abs() < 0.01,
        "Expected dy=60 (ranksep halved to 25, 2 rank gaps), got dy={}",
        dy
    );
}

#[test]
fn test_bk_allocates_space_for_label_dummy() {
    // Verify that label dummies with non-zero width influence layout spacing
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 3.0));
    graph.add_node("B", (10.0, 3.0));
    graph.add_node("C", (10.0, 3.0));
    // A -> B and A -> C: two parallel edges on same ranks
    graph.add_edge("A", "B");
    graph.add_edge("A", "C");

    // Label on A->B with significant width
    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(50.0, 5.0));

    let config = LayoutConfig::default();
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    // The label position should exist and have a valid x coordinate
    assert!(result.label_positions.contains_key(&0));
    let label_pos = result.label_positions.get(&0).unwrap();

    // Label dummy width (50.0) should be accounted for — the label
    // position should be at a reasonable x coordinate
    let a_rect = result.nodes.get(&"A".into()).unwrap();
    // Label should be in the general vicinity of the edge path
    assert!(
        label_pos.point.x >= 0.0,
        "Label x should be non-negative, got {}",
        label_pos.point.x
    );
    assert!(
        label_pos.point.y > a_rect.y,
        "Label should be below A in TD layout"
    );
}

#[test]
fn test_denorm_extracts_label_position_between_nodes() {
    // A -> B with label: verify label position is geometrically between A and B
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B");

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(50.0, 20.0));

    let config = LayoutConfig::default();
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    assert!(result.label_positions.contains_key(&0));
    let label_pos = result.label_positions.get(&0).unwrap();

    let a_y = result.nodes.get(&"A".into()).unwrap().y;
    let b_y = result.nodes.get(&"B".into()).unwrap().y;
    assert!(
        label_pos.point.y > a_y && label_pos.point.y < b_y,
        "Label y={} should be between A y={} and B y={}",
        label_pos.point.y,
        a_y,
        b_y
    );
}

#[test]
fn test_layout_with_labels_short_edge_gets_label_position() {
    // A -> B (short edge, 1-rank span) with label
    // After make_space, it should span 2 ranks and get a label dummy
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_edge("A", "B"); // Edge 0 - labeled

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(50.0, 20.0));

    let config = LayoutConfig::default();
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    // Short labeled edge should now have a label position
    assert!(
        result.label_positions.contains_key(&0),
        "Short labeled edge should have a label position"
    );
}

#[test]
fn test_layout_with_labels() {
    // A -> B -> C, and A -> C with label
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (100.0, 50.0));
    graph.add_node("B", (100.0, 50.0));
    graph.add_node("C", (100.0, 50.0));
    graph.add_edge("A", "B"); // Edge 0
    graph.add_edge("B", "C"); // Edge 1
    graph.add_edge("A", "C"); // Edge 2 - long edge with label

    let mut edge_labels = HashMap::new();
    edge_labels.insert(2, EdgeLabelInfo::new(50.0, 20.0));

    let config = LayoutConfig::default();
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    // Should have label position for edge 2
    assert!(
        result.label_positions.contains_key(&2),
        "Should have label position for edge 2"
    );

    let label_pos = result.label_positions.get(&2).unwrap();
    // Label should be at an intermediate y position
    let a_y = result.nodes.get(&"A".into()).unwrap().y;
    let c_y = result.nodes.get(&"C".into()).unwrap().y;
    assert!(
        label_pos.point.y > a_y && label_pos.point.y < c_y,
        "Label should be between A and C"
    );
}

#[test]
fn parallel_labeled_edges_get_distinct_sides() {
    // Two edges landing on the same target, both labeled.
    // A -->|left| C
    // B -->|right| C
    // With side selection enabled, labels should have different y offsets.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_node("C", (40.0, 20.0));
    graph.add_edge("A", "C"); // Edge 0 - labeled "left"
    graph.add_edge("B", "C"); // Edge 1 - labeled "right"

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(30.0, 14.0));
    edge_labels.insert(1, EdgeLabelInfo::new(30.0, 14.0));

    let config = LayoutConfig {
        per_edge_label_spacing: true,
        label_side_selection: true,
        ..Default::default()
    };
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    let left_pos = result
        .label_positions
        .get(&0)
        .expect("edge 0 should have label position");
    let right_pos = result
        .label_positions
        .get(&1)
        .expect("edge 1 should have label position");

    // Labels should have different y-coordinates (one above, one below)
    assert!(
        (left_pos.point.y - right_pos.point.y).abs() > 1.0,
        "labels should be offset from each other: left y={}, right y={}",
        left_pos.point.y,
        right_pos.point.y,
    );

    // Without label_side_selection, labels should be at the same y (both Center)
    let config_no_side = LayoutConfig {
        per_edge_label_spacing: true,
        label_side_selection: false,
        ..Default::default()
    };
    let result_no_side = layout_with_labels(&graph, &config_no_side, |_, dims| *dims, &edge_labels);
    let left_no = result_no_side.label_positions.get(&0).unwrap();
    let right_no = result_no_side.label_positions.get(&1).unwrap();
    // Both Center: same rank → same y (within tolerance for different x positions)
    assert!(
        (left_no.point.y - right_no.point.y).abs() < 1.0,
        "without side selection, labels should be near same y: left={}, right={}",
        left_no.point.y,
        right_no.point.y,
    );
}

#[test]
fn test_layout_compound_graph_end_to_end() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("sg1", (0.0, 0.0));
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    graph.set_parent("A", "sg1");
    graph.set_parent("B", "sg1");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    // Nodes should be laid out
    assert!(result.nodes.contains_key(&"A".into()));
    assert!(result.nodes.contains_key(&"B".into()));

    // Subgraph bounds should exist
    assert!(
        result.subgraph_bounds.contains_key("sg1"),
        "Should have subgraph bounds for sg1"
    );
    let bounds = &result.subgraph_bounds["sg1"];
    assert!(
        bounds.width > 0.0,
        "Subgraph width should be positive, got bounds={:?}, all_nodes={:?}",
        bounds,
        result.nodes
    );
    assert!(bounds.height > 0.0, "Subgraph height should be positive");
}

#[test]
fn test_layout_compound_titled_end_to_end() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("sg1", (0.0, 0.0));
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    graph.set_parent("A", "sg1");
    graph.set_parent("B", "sg1");
    graph.set_has_title("sg1");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    assert!(result.nodes.contains_key(&"A".into()));
    assert!(result.nodes.contains_key(&"B".into()));
    assert!(result.subgraph_bounds.contains_key("sg1"));
}

#[test]
fn test_layout_multi_level_compound_nesting() {
    let mut g: DiGraph<(f64, f64)> = DiGraph::new();
    g.add_node("A", (40.0, 20.0));
    g.add_node("B", (40.0, 20.0));
    g.add_node("inner", (0.0, 0.0));
    g.add_node("outer", (0.0, 0.0));
    g.add_edge("A", "B");
    g.set_parent("A", "inner");
    g.set_parent("B", "inner");
    g.set_parent("inner", "outer");

    let config = LayoutConfig::default();
    let result = layout(&g, &config, |_, dims| *dims);

    assert!(result.nodes.contains_key(&"A".into()));
    assert!(result.nodes.contains_key(&"B".into()));
}

#[test]
fn test_layout_simple_graph_no_subgraph_bounds() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    assert!(
        result.subgraph_bounds.is_empty(),
        "Simple graph should have no subgraph bounds"
    );
}

// --- Self-edge extraction tests ---

fn build_lg_from_edges(edges: &[(&str, &str)]) -> LayoutGraph {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    let mut seen = std::collections::HashSet::new();
    for (from, to) in edges {
        if seen.insert(*from) {
            graph.add_node(*from, (10.0, 5.0));
        }
        if seen.insert(*to) {
            graph.add_node(*to, (10.0, 5.0));
        }
        graph.add_edge(*from, *to);
    }
    LayoutGraph::from_digraph(&graph, |_, dims| *dims)
}

#[test]
fn test_extract_self_edges_single() {
    let mut lg = build_lg_from_edges(&[("A", "A")]);
    assert_eq!(lg.edges.len(), 1);
    extract_self_edges(&mut lg);
    assert_eq!(lg.self_edges.len(), 1);
    assert_eq!(lg.self_edges[0].node_index, lg.node_index[&"A".into()]);
    assert!(lg.edges.is_empty(), "self-edge should be removed");
}

#[test]
fn test_extract_self_edges_mixed() {
    let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A"), ("B", "C")]);
    assert_eq!(lg.edges.len(), 3);
    extract_self_edges(&mut lg);
    assert_eq!(lg.self_edges.len(), 1);
    assert_eq!(lg.edges.len(), 2, "only non-self edges remain");
    // Parallel arrays should be in sync
    assert_eq!(lg.edge_weights.len(), 2);
    assert_eq!(lg.edge_minlens.len(), 2);
}

#[test]
fn test_extract_self_edges_none() {
    let mut lg = build_lg_from_edges(&[("A", "B")]);
    extract_self_edges(&mut lg);
    assert!(lg.self_edges.is_empty());
    assert_eq!(lg.edges.len(), 1);
}

#[test]
fn test_extract_self_edges_multiple() {
    let mut lg = build_lg_from_edges(&[("A", "A"), ("B", "B"), ("A", "B")]);
    extract_self_edges(&mut lg);
    assert_eq!(lg.self_edges.len(), 2);
    assert_eq!(lg.edges.len(), 1);
}

#[test]
fn test_layout_with_self_edge_does_not_panic() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 5.0));
    graph.add_edge("A", "A");
    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);
    assert!(result.nodes.contains_key(&"A".into()));
}

#[test]
fn test_insert_self_edge_dummy_creates_node() {
    let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
    extract_self_edges(&mut lg);
    // Simulate ranking and ordering
    rank::run(&mut lg, &LayoutConfig::default());
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &HashMap::new(), false);
    order::run(&mut lg, false);

    let node_count_before = lg.node_ids.len();
    assert_eq!(lg.self_edges.len(), 1);

    insert_self_edge_dummies(&mut lg);

    assert_eq!(lg.node_ids.len(), node_count_before + 1);
    assert!(lg.self_edges[0].dummy_index.is_some());
}

#[test]
fn test_insert_self_edge_dummy_same_rank() {
    let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
    let a_idx = lg.node_index[&"A".into()];
    extract_self_edges(&mut lg);
    rank::run(&mut lg, &LayoutConfig::default());
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &HashMap::new(), false);
    order::run(&mut lg, false);

    let node_rank = lg.ranks[a_idx];
    insert_self_edge_dummies(&mut lg);

    let dummy_idx = lg.self_edges[0].dummy_index.unwrap();
    assert_eq!(lg.ranks[dummy_idx], node_rank);
}

#[test]
fn test_insert_self_edge_dummy_order_after_node() {
    let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
    let a_idx = lg.node_index[&"A".into()];
    extract_self_edges(&mut lg);
    rank::run(&mut lg, &LayoutConfig::default());
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &HashMap::new(), false);
    order::run(&mut lg, false);

    let node_order = lg.order[a_idx];
    insert_self_edge_dummies(&mut lg);

    let dummy_idx = lg.self_edges[0].dummy_index.unwrap();
    assert_eq!(lg.order[dummy_idx], node_order + 1);
}

#[test]
fn test_layout_result_contains_self_edge_layout() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 5.0));
    graph.add_edge("A", "A");
    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
    assert_eq!(result.self_edges.len(), 1);
    assert_eq!(result.self_edges[0].node, "A".into());
    assert_eq!(result.self_edges[0].points.len(), 6);
}

#[test]
fn test_layout_result_no_self_edges_when_none_exist() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 5.0));
    graph.add_node("B", (10.0, 5.0));
    graph.add_edge("A", "B");
    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
    assert!(result.self_edges.is_empty());
}

#[test]
fn test_position_self_edges_td_produces_6_points() {
    let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
    let a_idx = lg.node_index[&"A".into()];
    extract_self_edges(&mut lg);
    rank::run(&mut lg, &LayoutConfig::default());
    rank::normalize(&mut lg);
    normalize::run(&mut lg, &HashMap::new(), false);
    order::run(&mut lg, false);
    insert_self_edge_dummies(&mut lg);
    let config = LayoutConfig::default(); // TopBottom
    position::run(&mut lg, &config);
    let layouts = position_self_edges(&lg, &config);
    assert_eq!(layouts.len(), 1);
    assert_eq!(layouts[0].points.len(), 6);

    // Verify exit from bottom, enter from top (TD)
    let node_pos = lg.positions[a_idx];
    let (_nw, nh) = lg.dimensions[a_idx];
    let bot = node_pos.y + nh;
    let top = node_pos.y;
    assert!(
        layouts[0].points[0].y >= bot - 0.1,
        "first point should exit bottom"
    );
    assert!(
        layouts[0].points[5].y <= top + 0.1,
        "last point should enter top"
    );
}

#[test]
fn test_layout_self_edge_dummy_not_in_result_nodes() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 5.0));
    graph.add_node("B", (10.0, 5.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "A");
    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
    // Dummy should not appear in result nodes
    assert_eq!(result.nodes.len(), 2);
    assert!(result.nodes.contains_key(&"A".into()));
    assert!(result.nodes.contains_key(&"B".into()));
}

#[test]
fn test_layout_self_edge_not_in_reversed_edges() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 5.0));
    graph.add_node("B", (10.0, 5.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "A");
    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
    // Self-edge orig index is 1. It should not appear in reversed_edges.
    assert!(
        !result.reversed_edges.contains(&1),
        "self-edge should not be in reversed_edges"
    );
}

#[test]
fn test_reversed_edge_endpoints_match_original_direction() {
    // Edge 0: A→B (forward), Edge 1: B→A (reversed for acyclic).
    // After layout, the reversed edge should have from=B, to=A
    // (original direction) and points going from B toward A.
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_edge("A", "B"); // edge 0
    graph.add_edge("B", "A"); // edge 1: will be reversed

    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);

    assert!(!result.reversed_edges.is_empty());
    let rev_idx = result.reversed_edges[0];
    let edge = result.edges.iter().find(|e| e.index == rev_idx).unwrap();

    // The reversed edge's original direction is B→A.
    // After acyclic undo, from/to should reflect that.
    assert_eq!(
        edge.from,
        "B".into(),
        "reversed edge from should be B (original source)"
    );
    assert_eq!(
        edge.to,
        "A".into(),
        "reversed edge to should be A (original target)"
    );

    // Points should be oriented from B toward A.
    // In TD layout, A is above B. B→A goes upward, so first point
    // should be near B (lower y) and last point near A (higher y).
    let b_rect = result.nodes.get(&"B".into()).unwrap();
    let a_rect = result.nodes.get(&"A".into()).unwrap();
    let p_first = edge.points.first().unwrap();
    let p_last = edge.points.last().unwrap();
    let b_cy = b_rect.y + b_rect.height / 2.0;
    let a_cy = a_rect.y + a_rect.height / 2.0;
    assert!(
        (p_first.y - b_cy).abs() < (p_first.y - a_cy).abs(),
        "first point should be closer to B (original source), \
             p_first.y={}, b_cy={}, a_cy={}",
        p_first.y,
        b_cy,
        a_cy
    );
    assert!(
        (p_last.y - a_cy).abs() < (p_last.y - b_cy).abs(),
        "last point should be closer to A (original target), \
             p_last.y={}, a_cy={}, b_cy={}",
        p_last.y,
        a_cy,
        b_cy
    );
}

#[test]
fn test_translate_layout_result_uses_nodes_not_edge_points() {
    // dagre.js translateGraph uses node bounding boxes and edge labels for
    // min/max, NOT individual edge points. Edge points still get shifted,
    // but don't influence the bounding box calculation.
    let mut result = LayoutResult {
        nodes: HashMap::from([(
            "A".into(),
            Rect {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 10.0,
            },
        )]),
        edges: vec![EdgeLayout {
            from: "A".into(),
            to: "A".into(),
            points: vec![Point { x: -5.0, y: 12.0 }],
            index: 0,
        }],
        reversed_edges: vec![],
        width: 0.0,
        height: 0.0,
        edge_waypoints: HashMap::new(),
        label_positions: HashMap::new(),
        label_sides: HashMap::new(),
        subgraph_bounds: HashMap::new(),
        self_edges: vec![],
        rank_to_position: HashMap::new(),
        node_ranks: HashMap::new(),
    };

    translate_layout_result(&mut result, 10.0, 10.0, Direction::TopBottom);

    // Min X = 10 (from node, not edge point at -5). dagre: minX -= marginX => 0.
    // dx = -minX = -0 = 0. Wait: minX=10, minX -= 10 => 0, dx = -0 = 0.
    // Actually: minX=10 (node left), marginX=10. minX -= marginX => 0. dx = 0.
    // Node stays at x=10, edge point stays at -5.
    let rect = result.nodes.get(&"A".into()).unwrap();
    assert!(
        (rect.x - 10.0).abs() < 0.001,
        "node x should stay at 10.0 (min already at margin), got {}",
        rect.x
    );
    // Edge point shifts by same dx=0
    assert!(
        (result.edges[0].points[0].x - (-5.0)).abs() < 0.001,
        "edge point x should stay at -5.0, got {}",
        result.edges[0].points[0].x
    );

    // Width: maxX=20 (node right), minX after margin reduction = 0.
    // width = maxX - minX + marginX = 20 - 0 + 10 = 30
    assert!(
        (result.width - 30.0).abs() < 0.001,
        "width should be 30.0 (margin on both sides), got {}",
        result.width
    );
}

#[test]
fn test_translate_layout_result_shifts_all_fields() {
    let mut result = LayoutResult {
        nodes: HashMap::from([(
            "A".into(),
            Rect {
                x: 5.0,
                y: 5.0,
                width: 10.0,
                height: 10.0,
            },
        )]),
        edges: vec![EdgeLayout {
            from: "A".into(),
            to: "A".into(),
            points: vec![Point { x: 5.0, y: 5.0 }, Point { x: 20.0, y: 20.0 }],
            index: 0,
        }],
        reversed_edges: vec![],
        width: 0.0,
        height: 0.0,
        edge_waypoints: HashMap::from([(
            0,
            vec![WaypointWithRank {
                point: Point { x: 12.0, y: 12.0 },
                rank: 1,
            }],
        )]),
        label_positions: HashMap::from([(
            0,
            WaypointWithRank {
                point: Point { x: 12.0, y: 12.0 },
                rank: 1,
            },
        )]),
        label_sides: HashMap::new(),
        subgraph_bounds: HashMap::from([(
            "sg1".to_string(),
            Rect {
                x: 3.0,
                y: 3.0,
                width: 20.0,
                height: 20.0,
            },
        )]),
        self_edges: vec![],
        rank_to_position: HashMap::new(),
        node_ranks: HashMap::new(),
    };

    translate_layout_result(&mut result, 10.0, 10.0, Direction::TopBottom);

    // Min from nodes: x=5, subgraph: x=3 => minX=3. label: x=12 (not smaller).
    // dagre-style: minX -= marginX => 3 - 10 = -7. dx = -minX = 7.
    // All coords += 7.
    let sg = result.subgraph_bounds.get("sg1").unwrap();
    assert!(
        (sg.x - 10.0).abs() < 0.001,
        "subgraph x should be 10.0, got {}",
        sg.x
    );
    assert!(
        (sg.y - 10.0).abs() < 0.001,
        "subgraph y should be 10.0, got {}",
        sg.y
    );

    // Edge waypoints should be shifted by dx=7
    let wp = &result.edge_waypoints[&0][0];
    assert!(
        (wp.point.x - 19.0).abs() < 0.001,
        "waypoint x should be 19.0, got {}",
        wp.point.x
    );

    // Label position should be shifted by dx=7
    let lp = &result.label_positions[&0];
    assert!(
        (lp.point.x - 19.0).abs() < 0.001,
        "label x should be 19.0, got {}",
        lp.point.x
    );

    // Width/height with margin on both sides:
    // maxX from node = 5+10=15, subgraph = 3+20=23, label = 12. Max=23.
    // minX = 3 (before margin reduction).
    // width = maxX - (minX - marginX) + marginX = 23 - (3 - 10) + 10 = 23 + 7 + 10 = 40
    assert!(
        (result.width - 40.0).abs() < 0.001,
        "width should be 40.0 (margin on both sides), got {}",
        result.width
    );
    assert!(
        (result.height - 40.0).abs() < 0.001,
        "height should be 40.0 (margin on both sides), got {}",
        result.height
    );
}

#[test]
fn test_compound_node_rect_matches_subgraph_bounds() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("sg1", (0.0, 0.0));
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    graph.set_parent("A", "sg1");
    graph.set_parent("B", "sg1");
    graph.set_has_title("sg1");

    let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);

    let bounds = &result.subgraph_bounds["sg1"];
    let rect = result.nodes.get(&"sg1".into()).unwrap();

    assert!(
        (rect.x - bounds.x).abs() < 1e-6,
        "compound node x={} should match bounds x={}",
        rect.x,
        bounds.x
    );
    assert!(
        (rect.y - bounds.y).abs() < 1e-6,
        "compound node y={} should match bounds y={}",
        rect.y,
        bounds.y
    );
    assert!(
        (rect.width - bounds.width).abs() < 1e-6,
        "compound node width={} should match bounds width={}",
        rect.width,
        bounds.width
    );
    assert!(
        (rect.height - bounds.height).abs() < 1e-6,
        "compound node height={} should match bounds height={}",
        rect.height,
        bounds.height
    );
}

#[test]
fn test_assign_node_intersects_updates_edge_endpoints() {
    let mut result = LayoutResult {
        nodes: HashMap::from([
            (
                "A".into(),
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            ),
            (
                "B".into(),
                Rect {
                    x: 0.0,
                    y: 30.0,
                    width: 10.0,
                    height: 10.0,
                },
            ),
        ]),
        edges: vec![EdgeLayout {
            from: "A".into(),
            to: "B".into(),
            points: vec![
                Point { x: 5.0, y: 5.0 },
                Point { x: 5.0, y: 20.0 },
                Point { x: 5.0, y: 35.0 },
            ],
            index: 0,
        }],
        reversed_edges: vec![],
        width: 0.0,
        height: 0.0,
        edge_waypoints: HashMap::new(),
        label_positions: HashMap::new(),
        label_sides: HashMap::new(),
        subgraph_bounds: HashMap::new(),
        self_edges: vec![],
        rank_to_position: HashMap::new(),
        node_ranks: HashMap::new(),
    };

    assign_node_intersects(&mut result);

    let edge = &result.edges[0];
    let p_first = edge.points.first().unwrap();
    let p_last = edge.points.last().unwrap();

    // Bottom of A (center y=5, h/2=5) and top of B (center y=35, h/2=5).
    assert!((p_first.x - 5.0).abs() < 0.001);
    assert!((p_first.y - 10.0).abs() < 0.001);
    assert!((p_last.x - 5.0).abs() < 0.001);
    assert!((p_last.y - 30.0).abs() < 0.001);
}

#[test]
fn test_layout_multi_edge_produces_two_edge_layouts() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "B");

    let config = LayoutConfig::default();
    let result = layout(&graph, &config, |_, dims| *dims);

    assert_eq!(
        result.edges.len(),
        2,
        "Should have 2 edge layouts for 2 edges between A and B"
    );
    assert_ne!(
        result.edges[0].index, result.edges[1].index,
        "Edge indices should differ"
    );
}

#[test]
fn test_layout_multi_edge_with_labels() {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
    graph.add_node("A", (40.0, 20.0));
    graph.add_node("B", (40.0, 20.0));
    graph.add_edge("A", "B");
    graph.add_edge("A", "B");

    let mut edge_labels = HashMap::new();
    edge_labels.insert(0, EdgeLabelInfo::new(30.0, 10.0));
    edge_labels.insert(1, EdgeLabelInfo::new(30.0, 10.0));

    let config = LayoutConfig::default();
    let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

    assert_eq!(result.edges.len(), 2);
    assert!(
        result.label_positions.contains_key(&0),
        "Edge 0 should have a label position"
    );
    assert!(
        result.label_positions.contains_key(&1),
        "Edge 1 should have a label position"
    );
}
