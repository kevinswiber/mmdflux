//! Border segment creation and removal for compound graph layout.
//!
//! Creates left/right border nodes at each rank within a compound node's span.
//! These nodes constrain ordering and, after positioning, define the subgraph
//! bounding box.

use std::collections::HashMap;

use super::debug;
use super::graph::{BorderType, LayoutGraph};
use super::types::{NodeId, Rect};

fn debug_border_nodes(lg: &LayoutGraph) {
    if !debug::border_nodes_enabled() {
        return;
    }

    let mut compounds: Vec<usize> = lg.compound_nodes.iter().copied().collect();
    compounds.sort_by_key(|&idx| lg.node_ids[idx].0.clone());

    eprintln!("[border_nodes] layout positions");
    for compound_idx in compounds {
        let left = match lg.border_left.get(&compound_idx) {
            Some(nodes) => nodes,
            None => continue,
        };
        let right = match lg.border_right.get(&compound_idx) {
            Some(nodes) => nodes,
            None => continue,
        };

        let name = &lg.node_ids[compound_idx].0;
        let min_rank = lg.min_rank.get(&compound_idx).copied();
        let max_rank = lg.max_rank.get(&compound_idx).copied();
        eprintln!(
            "[border_nodes] {} min_rank={:?} max_rank={:?}",
            name, min_rank, max_rank
        );

        if let Some(&top_idx) = lg.border_top.get(&compound_idx) {
            let pos = lg.positions[top_idx];
            eprintln!(
                "[border_nodes]   top {} rank={} order={} x={:.2} y={:.2}",
                lg.node_ids[top_idx].0, lg.ranks[top_idx], lg.order[top_idx], pos.x, pos.y
            );
        }

        if let Some(&bot_idx) = lg.border_bottom.get(&compound_idx) {
            let pos = lg.positions[bot_idx];
            eprintln!(
                "[border_nodes]   bottom {} rank={} order={} x={:.2} y={:.2}",
                lg.node_ids[bot_idx].0, lg.ranks[bot_idx], lg.order[bot_idx], pos.x, pos.y
            );
        }

        let count = left.len().min(right.len());
        for i in 0..count {
            let left_idx = left[i];
            let right_idx = right[i];
            let rank = lg.ranks[left_idx];
            let left_pos = lg.positions[left_idx];
            let right_pos = lg.positions[right_idx];
            eprintln!(
                "[border_nodes]   rank {}: left {} order={} x={:.2} y={:.2} right {} order={} x={:.2} y={:.2}",
                rank,
                lg.node_ids[left_idx].0,
                lg.order[left_idx],
                left_pos.x,
                left_pos.y,
                lg.node_ids[right_idx].0,
                lg.order[right_idx],
                right_pos.x,
                right_pos.y
            );
        }
    }
}

/// Create left and right border nodes for each rank in each compound node's span.
///
/// Border nodes are linked vertically (consecutive ranks) and assigned the
/// appropriate border type. They participate in ordering to ensure they are
/// placed at the edges of their parent's children.
pub fn add_segments(lg: &mut LayoutGraph) {
    let compound_indices: Vec<usize> = lg.compound_nodes.iter().copied().collect();

    for compound_idx in compound_indices {
        let min_r = match lg.min_rank.get(&compound_idx) {
            Some(&r) => r,
            None => continue,
        };
        let max_r = match lg.max_rank.get(&compound_idx) {
            Some(&r) => r,
            None => continue,
        };

        let compound_id = lg.node_ids[compound_idx].0.clone();
        let mut left_nodes = Vec::new();
        let mut right_nodes = Vec::new();

        for rank in min_r..=max_r {
            // Left border node
            let left_id = NodeId(format!("_bl_{}_{}", compound_id, rank));
            let left_idx = lg.add_nesting_node(left_id);
            lg.ranks[left_idx] = rank;
            lg.border_type.insert(left_idx, BorderType::Left);
            lg.parents[left_idx] = Some(compound_idx);
            left_nodes.push(left_idx);

            // Right border node
            let right_id = NodeId(format!("_br_{}_{}", compound_id, rank));
            let right_idx = lg.add_nesting_node(right_id);
            lg.ranks[right_idx] = rank;
            lg.border_type.insert(right_idx, BorderType::Right);
            lg.parents[right_idx] = Some(compound_idx);
            right_nodes.push(right_idx);
        }

        // Link consecutive border nodes vertically
        for i in 0..left_nodes.len().saturating_sub(1) {
            lg.add_nesting_edge(left_nodes[i], left_nodes[i + 1], 1.0);
            lg.add_nesting_edge(right_nodes[i], right_nodes[i + 1], 1.0);
        }

        lg.border_left.insert(compound_idx, left_nodes);
        lg.border_right.insert(compound_idx, right_nodes);
    }
}

/// Extract subgraph bounding boxes from border node positions and remove border nodes.
///
/// Returns a map from compound node ID to its bounding rectangle. The bounding box
/// is computed from the positioned border nodes.
pub fn remove_nodes(lg: &mut LayoutGraph) -> HashMap<String, Rect> {
    debug_border_nodes(lg);

    let debug_bounds = debug::subgraph_bounds_enabled();

    let mut bounds = HashMap::new();

    let compound_indices: Vec<usize> = lg.compound_nodes.iter().copied().collect();
    for compound_idx in compound_indices {
        let left = match lg.border_left.get(&compound_idx) {
            Some(l) => l.clone(),
            None => continue,
        };
        let right = match lg.border_right.get(&compound_idx) {
            Some(r) => r.clone(),
            None => continue,
        };

        if left.is_empty() || right.is_empty() {
            continue;
        }

        // Compute bounding box from left/right border nodes only.
        //
        // Left/right borders span all ranks and together define the full rectangle
        // in both x and y dimensions for any layout direction (TD, LR, etc.).
        //
        // We intentionally exclude border_top and border_bottom because:
        // 1. They can be mispositioned in x due to BK alignment (they form their
        //    own blocks with no neighbors and may end up outside the left/right range)
        // 2. Their y positions don't reliably span the content height (they're
        //    centered in their rank, not at the content edges)
        //
        // Note: dagre.js uses top/bottom y for height, but this requires those nodes
        // to be correctly positioned at content edges. Our BK alignment doesn't
        // guarantee this, so we use left/right which span the full y range.
        //
        // NOTE: Extra vertical padding in LR layouts matches current layout output.
        // If we ever want to tighten this, it will be a deliberate parity deviation
        // and should be validated against Mermaid rendering expectations.
        let mut x_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_min = f64::INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        for &idx in left.iter().chain(right.iter()) {
            let pos = lg.positions[idx];
            x_min = x_min.min(pos.x);
            x_max = x_max.max(pos.x);
            y_min = y_min.min(pos.y);
            y_max = y_max.max(pos.y);
        }

        // Bounds come from border nodes only, matching dagre.js removeBorderNodes
        // (layout.js:309-330) which uses borderLeft/Right/Top/Bottom exclusively.
        if debug_bounds {
            let name = &lg.node_ids[compound_idx].0;
            eprintln!(
                "[subgraph_bounds] {} border=({:.2},{:.2})-({:.2},{:.2})",
                name, x_min, y_min, x_max, y_max
            );
        }

        let width = (x_max - x_min).max(0.0);
        let height = (y_max - y_min).max(0.0);

        let name = lg.node_ids[compound_idx].0.clone();
        bounds.insert(
            name,
            Rect {
                x: x_min,
                y: y_min,
                width,
                height,
            },
        );
    }

    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::DiGraph;
    use crate::engines::graph::algorithms::layered::{LayoutConfig, nesting, rank};

    fn build_ranked_compound_graph() -> LayoutGraph {
        let mut g: DiGraph<(f64, f64)> = DiGraph::new();
        g.add_node("A", (10.0, 10.0));
        g.add_node("B", (10.0, 10.0));
        g.add_node("C", (10.0, 10.0));
        g.add_node("sg1", (0.0, 0.0));
        g.add_edge("A", "B");
        g.add_edge("B", "C");
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");
        g.set_parent("C", "sg1");

        let mut lg = LayoutGraph::from_digraph(&g, |_, dims| *dims);
        nesting::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        nesting::cleanup(&mut lg);
        nesting::assign_rank_minmax(&mut lg);
        lg
    }

    #[test]
    fn test_add_segments_creates_border_nodes() {
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];
        let min_r = lg.min_rank[&sg1_idx];
        let max_r = lg.max_rank[&sg1_idx];
        let expected_count = (max_r - min_r + 1) as usize;

        add_segments(&mut lg);

        assert!(lg.border_left.contains_key(&sg1_idx));
        assert!(lg.border_right.contains_key(&sg1_idx));
        assert_eq!(lg.border_left[&sg1_idx].len(), expected_count);
        assert_eq!(lg.border_right[&sg1_idx].len(), expected_count);
    }

    #[test]
    fn test_border_nodes_have_correct_type() {
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];

        add_segments(&mut lg);

        for &left_idx in &lg.border_left[&sg1_idx] {
            assert_eq!(lg.border_type[&left_idx], BorderType::Left);
        }
        for &right_idx in &lg.border_right[&sg1_idx] {
            assert_eq!(lg.border_type[&right_idx], BorderType::Right);
        }
    }

    #[test]
    fn test_border_nodes_linked_vertically() {
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];

        add_segments(&mut lg);

        let left = &lg.border_left[&sg1_idx];
        for i in 0..left.len().saturating_sub(1) {
            assert!(
                lg.has_edge(left[i], left[i + 1]),
                "Left border nodes at ranks {} and {} should be connected",
                i,
                i + 1
            );
        }
    }

    #[test]
    fn test_remove_nodes_extracts_bounds() {
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];
        add_segments(&mut lg);

        // Give border nodes some positions
        for &idx in lg.border_left.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point {
                x: 10.0,
                y: lg.ranks[idx] as f64 * 50.0,
            };
        }
        for &idx in lg.border_right.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point {
                x: 100.0,
                y: lg.ranks[idx] as f64 * 50.0,
            };
        }
        if let Some(&top) = lg.border_top.get(&sg1_idx) {
            lg.positions[top] = super::super::types::Point { x: 50.0, y: 0.0 };
        }
        if let Some(&bot) = lg.border_bottom.get(&sg1_idx) {
            lg.positions[bot] = super::super::types::Point { x: 50.0, y: 100.0 };
        }

        let bounds = remove_nodes(&mut lg);

        assert!(bounds.contains_key("sg1"));
        let b = &bounds["sg1"];
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn test_remove_nodes_uses_border_only_bounds() {
        // dagre.js computes bounds from border nodes only, never expanding
        // to include children. Place a child outside border range and verify
        // bounds are not affected.
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];
        add_segments(&mut lg);

        // Set all border nodes to a known box: x=10..100, y=5..95
        for &idx in lg.border_left.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point { x: 10.0, y: 5.0 };
        }
        for &idx in lg.border_right.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point { x: 100.0, y: 95.0 };
        }
        if let Some(&top) = lg.border_top.get(&sg1_idx) {
            lg.positions[top] = super::super::types::Point { x: 10.0, y: 5.0 };
        }
        if let Some(&bot) = lg.border_bottom.get(&sg1_idx) {
            lg.positions[bot] = super::super::types::Point { x: 100.0, y: 95.0 };
        }

        // Place child A far outside border bounds
        let child_idx = lg.node_index[&"A".into()];
        lg.positions[child_idx] = super::super::types::Point { x: -50.0, y: -20.0 };
        lg.dimensions[child_idx] = (10.0, 10.0);

        let bounds = remove_nodes(&mut lg);
        let b = &bounds["sg1"];

        // Bounds should come from borders only, not expand to child at (-50, -20)
        assert!((b.x - 10.0).abs() < 0.001, "x should be 10.0, got {}", b.x);
        assert!(
            (b.width - 90.0).abs() < 0.001,
            "width should be 90.0, got {}",
            b.width
        );
        assert!((b.y - 5.0).abs() < 0.001, "y should be 5.0, got {}", b.y);
        assert!(
            (b.height - 90.0).abs() < 0.001,
            "height should be 90.0, got {}",
            b.height
        );
    }

    #[test]
    fn test_x_bounds_from_left_right_only() {
        // dagre.js computes x bounds from left/right borders only.
        // border_top and border_bottom x positions should NOT affect x bounds.
        // This matches dagre.js removeBorderNodes which uses:
        //   node.width = Math.abs(r.x - l.x)
        //   node.x = l.x + node.width / 2
        let mut lg = build_ranked_compound_graph();
        let sg1_idx = lg.node_index[&"sg1".into()];
        add_segments(&mut lg);

        // Set left borders at x=10, right borders at x=100
        for &idx in lg.border_left.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point {
                x: 10.0,
                y: lg.ranks[idx] as f64 * 50.0,
            };
        }
        for &idx in lg.border_right.get(&sg1_idx).unwrap() {
            lg.positions[idx] = super::super::types::Point {
                x: 100.0,
                y: lg.ranks[idx] as f64 * 50.0,
            };
        }

        // Place border_top and border_bottom at x positions OUTSIDE the left/right range
        // This simulates the bug where these nodes end up mispositioned
        if let Some(&top) = lg.border_top.get(&sg1_idx) {
            lg.positions[top] = super::super::types::Point {
                x: 200.0, // Far right of the right border
                y: 0.0,
            };
        }
        if let Some(&bot) = lg.border_bottom.get(&sg1_idx) {
            lg.positions[bot] = super::super::types::Point {
                x: 150.0, // Also outside the left/right range
                y: 100.0,
            };
        }

        let bounds = remove_nodes(&mut lg);
        let b = &bounds["sg1"];

        // X bounds should come from left/right only (10..100), not top/bottom
        assert!(
            (b.x - 10.0).abs() < 0.001,
            "x should be 10.0 (from left border), got {}",
            b.x
        );
        assert!(
            (b.width - 90.0).abs() < 0.001,
            "width should be 90.0 (100-10 from left/right), got {}",
            b.width
        );
    }
}
