//! Edge normalization for hierarchical graph layout.
//!
//! This module implements the normalization step of the Sugiyama framework,
//! which breaks long edges (spanning multiple ranks) into chains of short
//! edges (spanning exactly 1 rank each) by inserting dummy nodes.
//!
//! The key benefit is that after normalization, all edges span exactly one
//! rank, which enables:
//! - Proper crossing reduction (dummies participate like real nodes)
//! - Waypoint generation for edge routing
//! - Label placement on isolated edge segments

use std::collections::{HashMap, HashSet};

use super::graph::LayoutGraph;
use super::types::{
    DummyChain, DummyNode, EdgeLabelInfo, LabelSide, NodeId, Point, WaypointWithRank,
};

/// Counter for generating unique dummy node IDs.
static DUMMY_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Generate a unique dummy node ID.
fn generate_dummy_id() -> NodeId {
    let id = DUMMY_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    NodeId::from(format!("_d{}", id))
}

/// Normalize long edges by inserting dummy nodes.
///
/// This function processes each edge and, if it spans more than one rank,
/// creates a chain of dummy nodes at intermediate ranks. The original edge
/// is replaced with a chain of edges connecting source -> dummies -> target.
///
/// After normalization, all edges span exactly one rank, which is required
/// for proper crossing reduction and coordinate assignment.
///
/// Uses a collect-and-rebuild strategy: long edges are identified and their
/// chain replacements are computed, then `edges`, `edge_weights`, and
/// `reversed_edges` are rebuilt in one pass to avoid index corruption.
///
/// # Arguments
/// * `graph` - The layout graph to normalize
/// * `edge_labels` - Optional label information for edges (keyed by original edge index)
pub(crate) fn run(
    graph: &mut LayoutGraph,
    edge_labels: &HashMap<usize, EdgeLabelInfo>,
    track_reversed_chains: bool,
) {
    // Clear any existing dummy data
    graph.dummy_nodes.clear();
    graph.dummy_chains.clear();

    // Get effective edges with reversals applied
    let effective = graph.effective_edges();

    // Phase 1: Identify long edges and generate chain data without mutating graph.edges
    let mut edges_to_remove: HashSet<usize> = HashSet::new();
    // Each chain replacement: Vec of (from_idx, to_idx, orig_edge_idx) edges + weights
    let mut new_chain_edges: Vec<(usize, usize, usize)> = Vec::new();
    let mut new_chain_weights: Vec<f64> = Vec::new();
    // Track which chain edges originated from reversed long edges
    let mut new_chain_is_reversed: Vec<bool> = Vec::new();

    for (edge_pos, &(from_idx, to_idx, orig_edge_idx)) in graph.edges.iter().enumerate() {
        // Skip excluded edges (nesting edges removed during compound graph cleanup)
        if graph.excluded_edges.contains(&edge_pos) {
            continue;
        }

        let (eff_from, eff_to) = effective[edge_pos];
        let from_rank = graph.ranks[eff_from];
        let to_rank = graph.ranks[eff_to];

        if to_rank <= from_rank + 1 {
            continue;
        }

        edges_to_remove.insert(edge_pos);

        let is_reversed = graph.reversed_edges.contains(&edge_pos);

        // For reversed edges, build chain in effective direction (eff_from -> eff_to)
        // so chain edges flow from lower rank to higher rank.
        // For normal edges, use stored direction (same as effective).
        let (chain_start, chain_end) = if is_reversed {
            (to_idx, from_idx) // effective direction: to_idx has lower rank
        } else {
            (from_idx, to_idx)
        };

        // Calculate label rank (midpoint) if edge has a label
        let label_info = edge_labels.get(&orig_edge_idx);
        let label_rank = label_info.map(|_| (from_rank + to_rank) / 2);

        let mut chain = DummyChain::new(orig_edge_idx);
        let mut prev_idx = chain_start;

        for rank in (from_rank + 1)..to_rank {
            let dummy_id = generate_dummy_id();
            let dummy_idx = graph.node_ids.len();

            let is_label_dummy = label_rank == Some(rank);

            let (dummy_node, width, height) =
                if let (true, Some(info)) = (is_label_dummy, label_info) {
                    let node = DummyNode::edge_label(
                        orig_edge_idx,
                        rank,
                        info.width,
                        info.height,
                        info.label_pos,
                    );
                    (node, info.width, info.height)
                } else {
                    (DummyNode::edge(orig_edge_idx, rank), 0.0, 0.0)
                };

            // Add dummy to the graph (node arrays are append-only, safe to mutate)
            graph.node_ids.push(dummy_id.clone());
            graph.node_index.insert(dummy_id.clone(), dummy_idx);
            graph.ranks.push(rank);
            graph.order.push(dummy_idx);
            graph.positions.push(Point::default());
            graph.dimensions.push((width, height));
            graph.original_has_predecessor.push(false);
            graph.parents.push(None);
            graph.model_order.push(None);
            graph.dummy_nodes.insert(dummy_id.clone(), dummy_node);

            if is_label_dummy {
                chain.label_dummy_index = Some(chain.dummy_ids.len());
            }
            chain.dummy_ids.push(dummy_id);

            // Collect chain edge — chain flows in effective direction, but if the
            // original edge was reversed, mark these chain edges as reversed too
            // so they are excluded from forward edge counts.
            new_chain_edges.push((prev_idx, dummy_idx, orig_edge_idx));
            new_chain_weights.push(1.0);
            new_chain_is_reversed.push(is_reversed);
            prev_idx = dummy_idx;
        }

        // Final edge to chain end
        new_chain_edges.push((prev_idx, chain_end, orig_edge_idx));
        new_chain_weights.push(1.0);
        new_chain_is_reversed.push(is_reversed);

        graph.dummy_chains.push(chain);
    }

    // Phase 2: Rebuild edges, edge_weights, edge_minlens, and reversed_edges
    if !edges_to_remove.is_empty() {
        let mut rebuilt_edges = Vec::new();
        let mut rebuilt_weights = Vec::new();
        let mut rebuilt_minlens = Vec::new();
        let mut old_to_new: HashMap<usize, usize> = HashMap::new();

        for (old_pos, &edge) in graph.edges.iter().enumerate() {
            if !edges_to_remove.contains(&old_pos) {
                old_to_new.insert(old_pos, rebuilt_edges.len());
                rebuilt_edges.push(edge);
                rebuilt_weights.push(graph.edge_weights[old_pos]);
                rebuilt_minlens.push(graph.edge_minlens[old_pos]);
            }
        }

        // Append chain edges — minlen=1 for chain edges.
        // Chain edges from reversed long edges are marked as reversed so they
        // are excluded from forward edge counts (count_forward_edges_per_gap).
        let chain_start_pos = rebuilt_edges.len();
        let chain_count = new_chain_edges.len();
        rebuilt_edges.extend(new_chain_edges);
        rebuilt_weights.extend(new_chain_weights);
        rebuilt_minlens.extend(std::iter::repeat_n(1, chain_count));

        // Remap reversed_edges: removed edges drop out, surviving edges get new indices
        graph.reversed_edges = graph
            .reversed_edges
            .iter()
            .filter_map(|&old_pos| old_to_new.get(&old_pos).copied())
            .collect();

        // Add chain edges from reversed long edges to reversed_edges.
        // Only when track_reversed_chains is enabled — dagre v0.8.5 does not
        // track these, so mermaid-layered leaves them out.
        if track_reversed_chains {
            for (i, &is_rev) in new_chain_is_reversed.iter().enumerate() {
                if is_rev {
                    graph.reversed_edges.insert(chain_start_pos + i);
                }
            }
        }

        // Remap excluded_edges so nesting edges remain excluded after rebuild.
        graph.excluded_edges = graph
            .excluded_edges
            .iter()
            .filter_map(|&old_pos| old_to_new.get(&old_pos).copied())
            .collect();

        graph.edges = rebuilt_edges;
        graph.edge_weights = rebuilt_weights;
        graph.edge_minlens = rebuilt_minlens;
    }
}

/// Extract waypoints from dummy node positions after coordinate assignment.
///
/// This should be called after the position phase to convert dummy positions
/// into edge waypoints for routing.
///
/// # Returns
/// A map from original edge index to a list of waypoints with rank information.
/// The rank is needed to transform waypoints from layout coordinates to draw coordinates.
pub(crate) fn denormalize(graph: &LayoutGraph) -> HashMap<usize, Vec<WaypointWithRank>> {
    let mut waypoints: HashMap<usize, Vec<WaypointWithRank>> = HashMap::new();

    for chain in &graph.dummy_chains {
        let mut points = Vec::new();

        for dummy_id in &chain.dummy_ids {
            if let Some(&dummy_idx) = graph.node_index.get(dummy_id) {
                let pos = graph.positions[dummy_idx];
                let dims = graph.dimensions[dummy_idx];

                // Get the rank from the dummy node metadata
                let rank = graph
                    .dummy_nodes
                    .get(dummy_id)
                    .map(|d| d.rank)
                    .unwrap_or(graph.ranks[dummy_idx]);

                // Use center of dummy (for label dummies with non-zero size)
                points.push(WaypointWithRank {
                    point: Point {
                        x: pos.x + dims.0 / 2.0,
                        y: pos.y + dims.1 / 2.0,
                    },
                    rank,
                });
            }
        }

        waypoints.insert(chain.edge_index, points);
    }

    waypoints
}

/// Get the label position for an edge if it has a label dummy.
///
/// # Returns
/// The center position of the label with rank information, or None if the edge has no label.
/// The rank is needed so the render layer can snap the primary axis to `layer_starts`.
#[cfg(test)]
pub(crate) fn get_label_position(
    graph: &LayoutGraph,
    edge_index: usize,
) -> Option<WaypointWithRank> {
    get_label_position_with_thickness(graph, edge_index, 0.0, 0.0)
}

/// Get the label position with edge thickness and spacing offset.
///
/// For Above labels, subtracts `edge_thickness/2 + edge_label_spacing`.
/// For Below labels, adds `edge_thickness/2 + edge_label_spacing`.
/// Center labels are not affected by thickness.
pub(crate) fn get_label_position_with_thickness(
    graph: &LayoutGraph,
    edge_index: usize,
    edge_thickness: f64,
    edge_label_spacing: f64,
) -> Option<WaypointWithRank> {
    for chain in &graph.dummy_chains {
        if chain.edge_index == edge_index
            && let Some(label_idx) = chain.label_dummy_index
        {
            let dummy_id = &chain.dummy_ids[label_idx];
            if let Some(&idx) = graph.node_index.get(dummy_id) {
                let pos = graph.positions[idx];
                let dims = graph.dimensions[idx];
                let rank = graph.ranks[idx];
                let dummy = graph.dummy_nodes.get(dummy_id);
                let side = dummy.map(|d| d.label_side).unwrap_or_default();
                let thickness_offset = match side {
                    LabelSide::Above => -(edge_thickness / 2.0 + edge_label_spacing),
                    LabelSide::Below => edge_thickness / 2.0 + edge_label_spacing,
                    LabelSide::Center => 0.0,
                };
                let y = match side {
                    LabelSide::Above => pos.y + thickness_offset,
                    LabelSide::Below => pos.y + dims.1 + thickness_offset,
                    LabelSide::Center => pos.y + dims.1 / 2.0,
                };
                return Some(WaypointWithRank {
                    point: Point {
                        x: pos.x + dims.0 / 2.0,
                        y,
                    },
                    rank,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::{DiGraph, LayoutGraph};
    use crate::engines::graph::algorithms::layered::types::{DummyType, LabelPos};
    use crate::engines::graph::algorithms::layered::{LayoutConfig, acyclic, rank};

    /// Helper to create a layout graph for testing.
    fn create_test_graph(nodes: &[&str], edges: &[(&str, &str)]) -> LayoutGraph {
        let mut graph: DiGraph<()> = DiGraph::new();
        for node in nodes {
            graph.add_node(*node, ());
        }
        for (from, to) in edges {
            graph.add_edge(*from, *to);
        }
        LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0))
    }

    #[test]
    fn label_side_variants_exist() {
        let _ = LabelSide::Above;
        let _ = LabelSide::Below;
        let _ = LabelSide::Center;
    }

    #[test]
    fn dummy_node_label_side_defaults_to_center() {
        let dummy = DummyNode::edge(0, 1);
        assert_eq!(dummy.label_side, LabelSide::Center);

        let label_dummy = DummyNode::edge_label(0, 1, 30.0, 14.0, LabelPos::Center);
        assert_eq!(label_dummy.label_side, LabelSide::Center);
    }

    #[test]
    fn test_normalize_short_edge() {
        // A -> B (spans 1 rank, should not be normalized)
        let mut lg = create_test_graph(&["A", "B"], &[("A", "B")]);
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let edge_labels = HashMap::new();
        run(&mut lg, &edge_labels, false);

        // No dummies should be created
        assert!(lg.dummy_chains.is_empty());
        assert!(lg.dummy_nodes.is_empty());
        // Original edge should still exist
        assert_eq!(lg.edges.len(), 1);
    }

    #[test]
    fn test_normalize_long_edge() {
        // A -> B -> C, but also A -> C (spans 2 ranks)
        let mut lg = create_test_graph(&["A", "B", "C"], &[("A", "B"), ("B", "C"), ("A", "C")]);
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Verify ranks: A=0, B=1, C=2
        let a_idx = lg.node_index[&NodeId::from("A")];
        let b_idx = lg.node_index[&NodeId::from("B")];
        let c_idx = lg.node_index[&NodeId::from("C")];
        assert_eq!(lg.ranks[a_idx], 0);
        assert_eq!(lg.ranks[b_idx], 1);
        assert_eq!(lg.ranks[c_idx], 2);

        let edge_labels = HashMap::new();
        run(&mut lg, &edge_labels, false);

        // A->C should be normalized (spans 2 ranks, needs 1 dummy)
        assert_eq!(lg.dummy_chains.len(), 1);
        assert_eq!(lg.dummy_chains[0].dummy_ids.len(), 1);

        // Should now have 4 nodes (A, B, C, + 1 dummy)
        assert_eq!(lg.node_ids.len(), 4);

        // The dummy should be at rank 1
        let dummy_id = &lg.dummy_chains[0].dummy_ids[0];
        let dummy_idx = lg.node_index[dummy_id];
        assert_eq!(lg.ranks[dummy_idx], 1);
    }

    #[test]
    fn test_normalize_with_label() {
        // A -> B -> C -> D, and A -> D (spans 3 ranks, needs 2 dummies)
        let mut lg = create_test_graph(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("B", "C"), ("C", "D"), ("A", "D")],
        );
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Create label info for edge A->D (which should be edge index 3)
        let mut edge_labels = HashMap::new();
        edge_labels.insert(3, EdgeLabelInfo::new(20.0, 10.0));

        run(&mut lg, &edge_labels, false);

        // A->D needs 2 dummies (rank 1 and rank 2)
        assert_eq!(lg.dummy_chains.len(), 1);
        assert_eq!(lg.dummy_chains[0].dummy_ids.len(), 2);

        // Label should be at the midpoint (rank 1 or 2)
        assert!(lg.dummy_chains[0].label_dummy_index.is_some());

        // Label dummy should have the specified dimensions
        let label_idx = lg.dummy_chains[0].label_dummy_index.unwrap();
        let label_dummy_id = &lg.dummy_chains[0].dummy_ids[label_idx];
        let label_dummy = lg.dummy_nodes.get(label_dummy_id).unwrap();
        assert!(label_dummy.is_label());
        assert_eq!(label_dummy.width, 20.0);
        assert_eq!(label_dummy.height, 10.0);
    }

    #[test]
    fn test_denormalize() {
        // A -> B -> C, and A -> C
        let mut lg = create_test_graph(&["A", "B", "C"], &[("A", "B"), ("B", "C"), ("A", "C")]);
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let edge_labels = HashMap::new();
        run(&mut lg, &edge_labels, false);

        // Set dummy position manually for testing
        let dummy_id = &lg.dummy_chains[0].dummy_ids[0];
        let dummy_idx = lg.node_index[dummy_id];
        lg.positions[dummy_idx] = Point { x: 50.0, y: 100.0 };

        let waypoints = denormalize(&lg);

        // Should have waypoints for the normalized edge
        assert!(waypoints.contains_key(&lg.dummy_chains[0].edge_index));
        let points = &waypoints[&lg.dummy_chains[0].edge_index];
        assert_eq!(points.len(), 1);
        // Dummy has zero dimensions, so center is the position itself
        assert_eq!(points[0].point.x, 50.0);
        assert_eq!(points[0].point.y, 100.0);
        // Dummy should be at rank 1 (between A=0 and C=2)
        assert_eq!(points[0].rank, 1);
    }

    #[test]
    fn test_dummy_node_edge() {
        let dummy = DummyNode::edge(0, 2);
        assert_eq!(dummy.dummy_type, DummyType::Edge);
        assert_eq!(dummy.edge_index, 0);
        assert_eq!(dummy.rank, 2);
        assert_eq!(dummy.width, 0.0);
        assert_eq!(dummy.height, 0.0);
        assert!(!dummy.is_label());
    }

    #[test]
    fn test_dummy_node_edge_label() {
        let dummy = DummyNode::edge_label(1, 3, 10.0, 5.0, LabelPos::Center);
        assert_eq!(dummy.dummy_type, DummyType::EdgeLabel);
        assert_eq!(dummy.edge_index, 1);
        assert_eq!(dummy.rank, 3);
        assert_eq!(dummy.width, 10.0);
        assert_eq!(dummy.height, 5.0);
        assert_eq!(dummy.label_pos, LabelPos::Center);
        assert!(dummy.is_label());
    }

    #[test]
    fn test_dummy_chain() {
        let mut chain = DummyChain::new(0);
        assert_eq!(chain.edge_index, 0);
        assert!(chain.dummy_ids.is_empty());
        assert!(!chain.has_label());

        chain.dummy_ids.push(NodeId::from("_d0"));
        chain.dummy_ids.push(NodeId::from("_d1"));
        chain.label_dummy_index = Some(1);

        assert_eq!(chain.dummy_ids.len(), 2);
        assert!(chain.has_label());
    }

    #[test]
    fn test_edge_label_info() {
        let info = EdgeLabelInfo::new(20.0, 10.0).with_pos(LabelPos::Left);
        assert_eq!(info.width, 20.0);
        assert_eq!(info.height, 10.0);
        assert_eq!(info.label_pos, LabelPos::Left);
        assert_eq!(info.thickness, 1.0); // default
    }

    #[test]
    fn edge_label_info_includes_thickness() {
        let mut info = EdgeLabelInfo::new(30.0, 14.0);
        info.thickness = 3.0;
        assert_eq!(info.thickness, 3.0);
    }

    #[test]
    fn test_label_pos_default() {
        let pos = LabelPos::default();
        assert_eq!(pos, LabelPos::Center);
    }

    #[test]
    fn test_dummy_chain_multiple_dummies() {
        // Simulate an edge spanning 4 ranks (needs 3 dummies)
        let mut chain = DummyChain::new(5);
        chain.dummy_ids.push(NodeId::from("_d0"));
        chain.dummy_ids.push(NodeId::from("_d1")); // This is the label dummy
        chain.dummy_ids.push(NodeId::from("_d2"));
        chain.label_dummy_index = Some(1);

        assert_eq!(chain.edge_index, 5);
        assert_eq!(chain.dummy_ids.len(), 3);
        assert!(chain.has_label());
        assert_eq!(
            chain.dummy_ids[chain.label_dummy_index.unwrap()],
            NodeId::from("_d1")
        );
    }

    /// Build a minimal LayoutGraph with a single label dummy at the given position/dimensions/side.
    /// Returns (graph, edge_index) for use with get_label_position.
    fn build_graph_with_positioned_label_dummy(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        side: LabelSide,
    ) -> (LayoutGraph, usize) {
        // Minimal graph: two real nodes + one label dummy in a chain
        let mut graph: crate::engines::graph::algorithms::layered::graph::DiGraph<()> =
            crate::engines::graph::algorithms::layered::graph::DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("A", "B");
        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));

        // Set up ranks so A=0, B=2 (long edge needing dummy)
        let a_idx = lg.node_index[&NodeId::from("A")];
        let b_idx = lg.node_index[&NodeId::from("B")];
        lg.ranks[a_idx] = 0;
        lg.ranks[b_idx] = 2;

        // Add a label dummy node manually
        let dummy_id = NodeId::from("_label_test");
        let dummy_idx = lg.node_ids.len();
        lg.node_ids.push(dummy_id.clone());
        lg.node_index.insert(dummy_id.clone(), dummy_idx);
        lg.ranks.push(1);
        lg.order.push(dummy_idx);
        lg.positions.push(Point { x, y });
        lg.dimensions.push((w, h));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);

        let mut dummy_node = DummyNode::edge_label(0, 1, w, h, LabelPos::Center);
        dummy_node.label_side = side;
        lg.dummy_nodes.insert(dummy_id.clone(), dummy_node);

        // Create dummy chain pointing to this label dummy
        let mut chain = DummyChain::new(0); // edge_index = 0
        chain.dummy_ids.push(dummy_id);
        chain.label_dummy_index = Some(0);
        lg.dummy_chains.push(chain);

        (lg, 0)
    }

    #[test]
    fn label_above_offset_above_center() {
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Above);
        let pos = get_label_position(&lg, edge_index).unwrap();
        let center_y = 50.0 + 14.0 / 2.0; // 57.0
        assert!(
            pos.point.y < center_y,
            "Above label should be above center, got y={}, center={}",
            pos.point.y,
            center_y
        );
    }

    #[test]
    fn label_below_offset_below_center() {
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Below);
        let pos = get_label_position(&lg, edge_index).unwrap();
        let center_y = 50.0 + 14.0 / 2.0; // 57.0
        assert!(
            pos.point.y > center_y,
            "Below label should be below center, got y={}, center={}",
            pos.point.y,
            center_y
        );
    }

    #[test]
    fn label_center_no_offset() {
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Center);
        let pos = get_label_position(&lg, edge_index).unwrap();
        let center_y = 50.0 + 14.0 / 2.0; // 57.0
        assert!(
            (pos.point.y - center_y).abs() < 0.01,
            "Center label should be at center, got y={}, expected={}",
            pos.point.y,
            center_y
        );
    }

    #[test]
    fn thick_edge_above_label_offset() {
        // Thick edge (3.0px) with Above label, spacing=2.0
        // Above: y = pos.y + (-(3.0/2 + 2.0)) = 50.0 + (-3.5) = 46.5
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Above);
        let pos = get_label_position_with_thickness(&lg, edge_index, 3.0, 2.0).unwrap();
        let expected_y = 50.0 + (-(3.0 / 2.0 + 2.0)); // 46.5
        assert!(
            (pos.point.y - expected_y).abs() < 0.01,
            "Above+thick: expected y={}, got y={}",
            expected_y,
            pos.point.y
        );
    }

    #[test]
    fn thick_edge_below_label_offset() {
        // Below: y = pos.y + dims.1 + (3.0/2 + 2.0) = 50.0 + 14.0 + 3.5 = 67.5
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Below);
        let pos = get_label_position_with_thickness(&lg, edge_index, 3.0, 2.0).unwrap();
        let expected_y = 50.0 + 14.0 + (3.0 / 2.0 + 2.0); // 67.5
        assert!(
            (pos.point.y - expected_y).abs() < 0.01,
            "Below+thick: expected y={}, got y={}",
            expected_y,
            pos.point.y
        );
    }

    #[test]
    fn thin_edge_center_label_no_thickness_offset() {
        let (lg, edge_index) =
            build_graph_with_positioned_label_dummy(100.0, 50.0, 40.0, 14.0, LabelSide::Center);
        let pos = get_label_position_with_thickness(&lg, edge_index, 1.0, 2.0).unwrap();
        let center_y = 50.0 + 14.0 / 2.0; // 57.0
        assert!(
            (pos.point.y - center_y).abs() < 0.01,
            "Center: expected y={}, got y={}",
            center_y,
            pos.point.y
        );
    }

    #[test]
    fn test_dummy_type_equality() {
        assert_eq!(DummyType::Edge, DummyType::Edge);
        assert_eq!(DummyType::EdgeLabel, DummyType::EdgeLabel);
        assert_ne!(DummyType::Edge, DummyType::EdgeLabel);
    }

    #[test]
    fn test_edge_label_info_default() {
        let info = EdgeLabelInfo::default();
        assert_eq!(info.width, 0.0);
        assert_eq!(info.height, 0.0);
        assert_eq!(info.label_pos, LabelPos::Center);
    }

    #[test]
    fn test_short_edge_with_label_gets_dummy() {
        // A -> B, 1-rank span, but with label and minlen=2
        // After ranking: A=0, B=2 (due to minlen=2)
        // After normalization: one dummy at rank 1 with EdgeLabel type
        let mut lg = create_test_graph(&["A", "B"], &[("A", "B")]);
        acyclic::run(&mut lg);

        // Simulate make_space_for_edge_labels: set minlen=2 for edge 0
        lg.edge_minlens[0] = 2;

        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a_idx = lg.node_index[&NodeId::from("A")];
        let b_idx = lg.node_index[&NodeId::from("B")];
        assert_eq!(lg.ranks[a_idx], 0);
        assert_eq!(lg.ranks[b_idx], 2, "B should be at rank 2 due to minlen=2");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, EdgeLabelInfo::new(5.0, 1.0));
        run(&mut lg, &edge_labels, false);

        // Should have one dummy chain for A->B
        assert_eq!(lg.dummy_chains.len(), 1);
        assert_eq!(lg.dummy_chains[0].dummy_ids.len(), 1);

        // The chain should have a label dummy
        assert!(lg.dummy_chains[0].label_dummy_index.is_some());

        // Label dummy should have the correct dimensions
        let label_idx = lg.dummy_chains[0].label_dummy_index.unwrap();
        let label_dummy_id = &lg.dummy_chains[0].dummy_ids[label_idx];
        let label_dummy = lg.dummy_nodes.get(label_dummy_id).unwrap();
        assert!(label_dummy.is_label());
        assert_eq!(label_dummy.width, 5.0);
        assert_eq!(label_dummy.height, 1.0);

        // Label dummy should be at rank 1 (midpoint of 0 and 2)
        let dummy_idx = lg.node_index[label_dummy_id];
        assert_eq!(lg.ranks[dummy_idx], 1);
    }

    #[test]
    fn test_long_edge_with_label_gets_midpoint_dummy() {
        // A -> B -> C -> D, and A -> D with label (originally spans 3 ranks)
        // With minlen=2, A->D spans 4 ranks (A=0, D=4)
        let mut lg = create_test_graph(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("B", "C"), ("C", "D"), ("A", "D")],
        );
        acyclic::run(&mut lg);

        // edge index 3 is A->D, set minlen=2 for label
        lg.edge_minlens[3] = 2;

        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let mut edge_labels = HashMap::new();
        edge_labels.insert(3, EdgeLabelInfo::new(7.0, 1.0));
        run(&mut lg, &edge_labels, false);

        // A->D should produce a chain with a label dummy
        let labeled_chain = lg
            .dummy_chains
            .iter()
            .find(|c| c.edge_index == 3 && c.label_dummy_index.is_some());
        assert!(
            labeled_chain.is_some(),
            "Should have a chain with label dummy for A->D"
        );

        let chain = labeled_chain.unwrap();
        let label_idx = chain.label_dummy_index.unwrap();
        let label_dummy_id = &chain.dummy_ids[label_idx];
        let label_dummy = lg.dummy_nodes.get(label_dummy_id).unwrap();
        assert!(label_dummy.is_label());
        assert_eq!(label_dummy.width, 7.0);
        assert_eq!(label_dummy.height, 1.0);
    }

    #[test]
    fn test_dummy_node_clone() {
        let dummy = DummyNode::edge_label(2, 5, 15.0, 8.0, LabelPos::Right);
        let cloned = dummy.clone();

        assert_eq!(cloned.dummy_type, DummyType::EdgeLabel);
        assert_eq!(cloned.edge_index, 2);
        assert_eq!(cloned.rank, 5);
        assert_eq!(cloned.width, 15.0);
        assert_eq!(cloned.height, 8.0);
        assert_eq!(cloned.label_pos, LabelPos::Right);
    }

    #[test]
    fn test_reversed_long_edge_chain_edges_are_in_reversed_edges() {
        // Graph: A -> B -> C -> D, and D -> A (backward edge spanning 3 ranks).
        // After acyclic, D -> A is reversed to A -> D (marked in reversed_edges).
        // After normalization, A -> D becomes chain edges A -> d1 -> d2 -> D.
        // Those chain edges must also be in reversed_edges.
        let mut lg = create_test_graph(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("B", "C"), ("C", "D"), ("D", "A")],
        );
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Verify ranks: A=0, B=1, C=2, D=3
        let a_idx = lg.node_index[&NodeId::from("A")];
        let d_idx = lg.node_index[&NodeId::from("D")];
        assert_eq!(lg.ranks[a_idx], 0);
        assert_eq!(lg.ranks[d_idx], 3);

        // Before normalize, D->A should be reversed and span 3 ranks
        assert!(
            !lg.reversed_edges.is_empty(),
            "Should have at least one reversed edge"
        );

        let edge_labels = HashMap::new();
        run(&mut lg, &edge_labels, true);

        // The reversed long edge should produce a chain with 2 dummies
        assert_eq!(lg.dummy_chains.len(), 1);
        assert_eq!(lg.dummy_chains[0].dummy_ids.len(), 2);

        // Chain edges from the reversed original should be in reversed_edges.
        // After rebuild, the 3 short forward edges (A->B, B->C, C->D) keep their
        // positions, and the 3 chain edges are appended after them.
        // All 3 chain edges should be in reversed_edges.
        let chain_edge_count = lg
            .edges
            .iter()
            .enumerate()
            .filter(|(pos, _)| lg.reversed_edges.contains(pos))
            .count();
        assert_eq!(
            chain_edge_count, 3,
            "All 3 chain edges from the reversed long edge should be in reversed_edges, \
             got {} reversed edges total",
            chain_edge_count
        );
    }
}
