//! Phase 4: Assign x, y coordinates to nodes.
//!
//! Implements coordinate assignment using the Brandes-Köpf algorithm for
//! optimal horizontal positioning, with y-coordinates based on layer rank.

use std::collections::HashMap;

use super::bk::{BKConfig, get_width, position_x};
use super::graph::LayoutGraph;
use super::rank;
use super::types::{Direction, LayoutConfig, Point};

/// Assign positions to all nodes.
pub fn run(graph: &mut LayoutGraph, config: &LayoutConfig) {
    let layers = rank::by_rank_filtered(graph, |node| graph.is_position_node(node));

    // Sort each layer by the computed order
    let sorted_layers: Vec<Vec<usize>> = layers
        .iter()
        .map(|layer| {
            let mut sorted = layer.clone();
            sorted.sort_by_key(|&n| graph.order[n]);
            sorted
        })
        .collect();

    // Assign coordinates based on direction
    match config.direction {
        Direction::TopBottom | Direction::BottomTop => {
            assign_vertical(graph, &sorted_layers, config);
        }
        Direction::LeftRight | Direction::RightLeft => {
            assign_horizontal(graph, &sorted_layers, config);
        }
    }

    // Reverse coordinates if needed
    if config.direction.is_reversed() {
        reverse_positions(graph, config);
    }
}

fn assign_vertical(graph: &mut LayoutGraph, layers: &[Vec<usize>], config: &LayoutConfig) {
    if layers.is_empty() {
        return;
    }

    // Use Brandes-Köpf algorithm for x-coordinate assignment
    let bk_config = BKConfig {
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        direction: config.direction,
    };
    let mut x_coords = position_x(graph, &bk_config);

    // Post-BK enforcement: ensure adjacent nodes in each layer respect
    // minimum separation (node_sep for real nodes, edge_sep for dummies).
    // The BK algorithm optimizes for alignment with neighbors in adjacent
    // layers, which can sometimes produce node overlaps when the graph
    // structure has fewer dummy nodes (e.g., per-edge label spacing).
    enforce_minimum_separation(graph, layers, &bk_config, &mut x_coords);

    // Find minimum x to shift everything to start at 0.
    // Dagre applies margin later in translateGraph; we do the same.
    let min_x = (0..graph.node_ids.len())
        .filter_map(|node| {
            x_coords
                .get(&node)
                .map(|&cx| cx - graph.dimensions[node].0 / 2.0)
        })
        .reduce(f64::min)
        .unwrap_or(0.0);

    let x_shift = -min_x;

    // Assign Y based on rank, X from BK algorithm
    let mut y = 0.0;

    for layer in layers.iter() {
        for &node in layer {
            let (w, _h) = graph.dimensions[node];
            // BK returns center x, convert to top-left corner
            let center_x = x_coords.get(&node).copied().unwrap_or(0.0);
            let x = center_x - w / 2.0 + x_shift;
            graph.positions[node] = Point { x, y };
        }

        // Y advances by max height in this layer + gap to next layer.
        // Use per-gap override if available, otherwise base rank_sep.
        let max_height = layer
            .iter()
            .map(|&n| graph.dimensions[n].1)
            .reduce(f64::max)
            .unwrap_or(0.0);
        let current_rank = layer.first().map(|&n| graph.ranks[n]).unwrap_or(0);
        let gap_spacing = config.rank_sep_for_gap(current_rank);
        y += max_height + gap_spacing;
    }
}

fn assign_horizontal(graph: &mut LayoutGraph, layers: &[Vec<usize>], config: &LayoutConfig) {
    if layers.is_empty() {
        return;
    }

    // Use Brandes-Köpf algorithm for y-coordinate assignment (perpendicular to rank)
    // BK always optimizes the "horizontal" axis (perpendicular to layer direction)
    let bk_config = BKConfig {
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        direction: config.direction,
    };
    let mut y_coords = position_x(graph, &bk_config);

    enforce_minimum_separation(graph, layers, &bk_config, &mut y_coords);

    // Find minimum y to shift everything to start at 0.
    // Dagre applies margin later in translateGraph; we do the same.
    let min_y = (0..graph.node_ids.len())
        .filter_map(|node| {
            y_coords
                .get(&node)
                .map(|&cy| cy - graph.dimensions[node].1 / 2.0)
        })
        .reduce(f64::min)
        .unwrap_or(0.0);

    let y_shift = -min_y;

    // Assign X based on rank, Y from BK algorithm
    let mut x = 0.0;

    for layer in layers.iter() {
        for &node in layer {
            let (_w, h) = graph.dimensions[node];
            // BK returns center position, convert to top-left corner
            let center_y = y_coords.get(&node).copied().unwrap_or(0.0);
            let y = center_y - h / 2.0 + y_shift;
            graph.positions[node] = Point { x, y };
        }

        // X advances by max width in this layer + gap to next layer.
        // Use per-gap override if available, otherwise base rank_sep.
        let max_width = layer
            .iter()
            .map(|&n| graph.dimensions[n].0)
            .reduce(f64::max)
            .unwrap_or(0.0);
        let current_rank = layer.first().map(|&n| graph.ranks[n]).unwrap_or(0);
        let gap_spacing = config.rank_sep_for_gap(current_rank);
        x += max_width + gap_spacing;
    }
}

fn reverse_positions(graph: &mut LayoutGraph, config: &LayoutConfig) {
    match config.direction {
        Direction::BottomTop => {
            let max_y = graph
                .positions
                .iter()
                .zip(graph.dimensions.iter())
                .map(|(p, (_, h))| p.y + h)
                .reduce(f64::max)
                .unwrap_or(0.0);
            for (pos, (_, h)) in graph.positions.iter_mut().zip(graph.dimensions.iter()) {
                pos.y = max_y - pos.y - h;
            }
        }
        Direction::RightLeft => {
            let max_x = graph
                .positions
                .iter()
                .zip(graph.dimensions.iter())
                .map(|(p, (w, _))| p.x + w)
                .reduce(f64::max)
                .unwrap_or(0.0);
            for (pos, (w, _)) in graph.positions.iter_mut().zip(graph.dimensions.iter()) {
                pos.x = max_x - pos.x - w;
            }
        }
        Direction::TopBottom | Direction::LeftRight => {}
    }
}

/// Enforce minimum separation between real nodes in each layer.
///
/// The BK algorithm optimizes horizontal positions by aligning nodes with
/// neighbors in adjacent layers. With fewer dummy nodes (e.g., per-edge
/// label spacing), this can place real nodes too close together. This pass
/// checks each pair of adjacent real nodes (skipping dummies) and enforces
/// `node_sep` between them. Dummy nodes are skipped because they represent
/// edge routing points, not visible boxes.
fn enforce_minimum_separation(
    graph: &LayoutGraph,
    layers: &[Vec<usize>],
    config: &BKConfig,
    coords: &mut HashMap<usize, f64>,
) {
    use super::bk::is_dummy_like;

    for layer in layers {
        // Only check adjacent pairs of real (non-dummy) nodes.
        let real_nodes: Vec<usize> = layer
            .iter()
            .copied()
            .filter(|&n| !is_dummy_like(graph, n))
            .collect();
        if real_nodes.len() < 2 {
            continue;
        }
        for i in 1..real_nodes.len() {
            let left = real_nodes[i - 1];
            let right = real_nodes[i];
            let left_cx = coords.get(&left).copied().unwrap_or(0.0);
            let right_cx = coords.get(&right).copied().unwrap_or(0.0);
            let left_half = get_width(graph, left, config.direction) / 2.0;
            let right_half = get_width(graph, right, config.direction) / 2.0;
            let min_center_dist = left_half + config.node_sep + right_half;
            if right_cx - left_cx < min_center_dist {
                coords.insert(right, left_cx + min_center_dist);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::DiGraph;
    use crate::engines::graph::algorithms::layered::{acyclic, order};

    fn run_full_layout(
        nodes: &[(&str, f64, f64)],
        edges: &[(&str, &str)],
        config: &LayoutConfig,
    ) -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for &(id, w, h) in nodes {
            graph.add_node(id, (w, h));
        }
        for &(from, to) in edges {
            graph.add_edge(from, to);
        }

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        order::run(&mut lg, false);
        run(&mut lg, config);
        lg
    }

    #[test]
    fn test_position_vertical_linear() {
        let config = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0), ("C", 50.0, 30.0)],
            &[("A", "B"), ("B", "C")],
            &config,
        );

        // Verify A is above B is above C
        let a_y = lg.positions[lg.node_index[&"A".into()]].y;
        let b_y = lg.positions[lg.node_index[&"B".into()]].y;
        let c_y = lg.positions[lg.node_index[&"C".into()]].y;

        assert!(a_y < b_y);
        assert!(b_y < c_y);
    }

    #[test]
    fn test_position_horizontal_linear() {
        let config = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0), ("C", 50.0, 30.0)],
            &[("A", "B"), ("B", "C")],
            &config,
        );

        // Verify A is left of B is left of C
        let a_x = lg.positions[lg.node_index[&"A".into()]].x;
        let b_x = lg.positions[lg.node_index[&"B".into()]].x;
        let c_x = lg.positions[lg.node_index[&"C".into()]].x;

        assert!(a_x < b_x);
        assert!(b_x < c_x);
    }

    #[test]
    fn test_position_bottom_top() {
        let config = LayoutConfig {
            direction: Direction::BottomTop,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0)],
            &[("A", "B")],
            &config,
        );

        // In BT, A should be below B (higher y)
        let a_y = lg.positions[lg.node_index[&"A".into()]].y;
        let b_y = lg.positions[lg.node_index[&"B".into()]].y;

        assert!(a_y > b_y);
    }

    #[test]
    fn test_position_right_left() {
        let config = LayoutConfig {
            direction: Direction::RightLeft,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0)],
            &[("A", "B")],
            &config,
        );

        // In RL, A should be right of B (higher x)
        let a_x = lg.positions[lg.node_index[&"A".into()]].x;
        let b_x = lg.positions[lg.node_index[&"B".into()]].x;

        assert!(a_x > b_x);
    }

    #[test]
    fn test_position_diamond_centering() {
        let config = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[
                ("A", 50.0, 30.0),
                ("B", 50.0, 30.0),
                ("C", 50.0, 30.0),
                ("D", 50.0, 30.0),
            ],
            &[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")],
            &config,
        );

        // A and D should be centered horizontally (same x or close)
        let a_x = lg.positions[lg.node_index[&"A".into()]].x;
        let d_x = lg.positions[lg.node_index[&"D".into()]].x;

        // They should be relatively centered
        let a_center = a_x + 25.0; // half of width
        let d_center = d_x + 25.0;
        assert!((a_center - d_center).abs() < 1.0);
    }

    #[test]
    fn test_position_skips_compound_parents() {
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("sg", ());
        g.add_node("A", ());
        g.set_parent("A", "sg");

        let mut lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));

        // Put A on a different rank so it gets a non-zero position,
        // while the compound parent remains unpositioned.
        lg.ranks[lg.node_index[&"sg".into()]] = 0;
        lg.ranks[lg.node_index[&"A".into()]] = 1;

        let config = LayoutConfig::default();
        run(&mut lg, &config);

        let sg_idx = lg.node_index[&"sg".into()];
        let a_idx = lg.node_index[&"A".into()];

        assert_eq!(lg.positions[sg_idx], Point::default());
        assert_ne!(lg.positions[a_idx], Point::default());
    }

    #[test]
    fn test_position_vertical_per_gap_spacing() {
        // A -> B -> C with rank_sep_overrides making the A->B gap wider
        let mut config = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };
        // Override gap at rank 0 to be 50.0 instead of 20.0.
        config.rank_sep_overrides.insert(0, 50.0);

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0), ("C", 50.0, 30.0)],
            &[("A", "B"), ("B", "C")],
            &config,
        );

        let a_y = lg.positions[lg.node_index[&"A".into()]].y;
        let b_y = lg.positions[lg.node_index[&"B".into()]].y;
        let c_y = lg.positions[lg.node_index[&"C".into()]].y;

        // A->B gap should be wider than B->C gap
        let gap_ab = b_y - a_y;
        let gap_bc = c_y - b_y;

        assert!(
            gap_ab > gap_bc,
            "A->B gap ({}) should be wider than B->C gap ({}) due to override",
            gap_ab,
            gap_bc,
        );
    }

    #[test]
    fn test_position_horizontal_per_gap_spacing() {
        let mut config = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };
        config.rank_sep_overrides.insert(0, 50.0);

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0), ("C", 50.0, 30.0)],
            &[("A", "B"), ("B", "C")],
            &config,
        );

        let a_x = lg.positions[lg.node_index[&"A".into()]].x;
        let b_x = lg.positions[lg.node_index[&"B".into()]].x;
        let c_x = lg.positions[lg.node_index[&"C".into()]].x;

        let gap_ab = b_x - a_x;
        let gap_bc = c_x - b_x;

        assert!(
            gap_ab > gap_bc,
            "A->B gap ({}) should be wider than B->C gap ({}) due to override",
            gap_ab,
            gap_bc,
        );
    }

    #[test]
    fn test_position_no_overrides_unchanged() {
        // Without overrides, all gaps should be equal (same as current behavior)
        let config = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 10.0,
            rank_sep: 20.0,
            margin: 5.0,
            ..Default::default()
        };

        let lg = run_full_layout(
            &[("A", 50.0, 30.0), ("B", 50.0, 30.0), ("C", 50.0, 30.0)],
            &[("A", "B"), ("B", "C")],
            &config,
        );

        let a_y = lg.positions[lg.node_index[&"A".into()]].y;
        let b_y = lg.positions[lg.node_index[&"B".into()]].y;
        let c_y = lg.positions[lg.node_index[&"C".into()]].y;

        let gap_ab = b_y - a_y;
        let gap_bc = c_y - b_y;

        assert!(
            (gap_ab - gap_bc).abs() < 0.001,
            "Without overrides, gaps should be equal: ab={}, bc={}",
            gap_ab,
            gap_bc,
        );
    }
}
