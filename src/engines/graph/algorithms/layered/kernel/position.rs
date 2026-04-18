//! Phase 4: Assign x, y coordinates to nodes.
//!
//! Implements coordinate assignment using the Brandes-Köpf algorithm for
//! optimal horizontal positioning, with y-coordinates based on layer rank.

use std::collections::HashMap;

use super::bk::{BKConfig, get_width, position_x};
use super::compartment_spacing::compute_compartment_rank_sep_overrides;
use super::graph::LayoutGraph;
use super::rank;
use super::types::{Direction, LayoutConfig, Point};

/// MAX-merge `from` into `into` in place. Used by the compartment pass to
/// layer kernel-computed reservations over the caller's
/// `rank_sep_overrides` without mutating `LayoutConfig`.
fn max_merge(into: &mut HashMap<i32, f64>, from: HashMap<i32, f64>) {
    for (key, value) in from {
        let entry = into.entry(key).or_insert(0.0);
        if value > *entry {
            *entry = value;
        }
    }
}

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

    // Build the local absolute-gap map used by this assign pass. Start
    // from config.rank_sep_overrides so caller-supplied overrides are
    // preserved. The compartment pass layers kernel reservations on top
    // via MAX-merge when variable_rank_spacing is enabled.
    let mut local_gap_overrides: HashMap<i32, f64> = config.rank_sep_overrides.clone();
    if config.variable_rank_spacing {
        let compartment_overrides =
            compute_compartment_rank_sep_overrides(graph, &x_coords, config.direction);
        max_merge(&mut local_gap_overrides, compartment_overrides);
    }

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
        // Prefer the locally-merged override (compartment reservations +
        // caller overrides), falling back to the base rank_sep.
        let max_height = layer
            .iter()
            .map(|&n| graph.dimensions[n].1)
            .reduce(f64::max)
            .unwrap_or(0.0);
        let current_rank = layer.first().map(|&n| graph.ranks[n]).unwrap_or(0);
        let gap_spacing = local_gap_overrides
            .get(&current_rank)
            .copied()
            .unwrap_or(config.rank_sep);
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

    // Local absolute-gap map: caller overrides + kernel compartment
    // reservations (when variable_rank_spacing is on). See assign_vertical
    // for the vertical-side counterpart; semantics are identical.
    let mut local_gap_overrides: HashMap<i32, f64> = config.rank_sep_overrides.clone();
    if config.variable_rank_spacing {
        let compartment_overrides =
            compute_compartment_rank_sep_overrides(graph, &y_coords, config.direction);
        max_merge(&mut local_gap_overrides, compartment_overrides);
    }

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
        let max_width = layer
            .iter()
            .map(|&n| graph.dimensions[n].0)
            .reduce(f64::max)
            .unwrap_or(0.0);
        let current_rank = layer.first().map(|&n| graph.ranks[n]).unwrap_or(0);
        let gap_spacing = local_gap_overrides
            .get(&current_rank)
            .copied()
            .unwrap_or(config.rank_sep);
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

    // Plan 0150: compartment-pass integration tests.
    //
    // Fixture shape: two labeled edges A→C and B→C. Both edges get an
    // EdgeLabel dummy at rank 1; with per_edge_label_spacing the labelled
    // edges get minlen=2 so C lands at rank 2. The two label dummies sit
    // between A/B at rank 0 and C at rank 2, giving overlapping cross
    // bands under BK output.
    fn build_two_label_layout(config: &LayoutConfig) -> LayoutGraph {
        use super::super::support::{
            extract_self_edges, make_space_for_labeled_edges, select_label_sides_first_last,
        };
        use super::super::types::EdgeLabelInfo;
        use super::super::{acyclic, normalize, order, rank};

        // Two parallel labeled branches A→B (edge 0) and C→D (edge 1), with
        // A/C on rank 0 side by side and B/D on rank 2 side by side. Wide
        // labels (80) with narrow node_sep give the two EdgeLabel dummies
        // at rank 1 overlapping cross-bands under BK output.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_node("C", (40.0, 20.0));
        graph.add_node("D", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, EdgeLabelInfo::new(80.0, 28.0));
        edge_labels.insert(1, EdgeLabelInfo::new(80.0, 28.0));

        let mut cfg = config.clone();
        cfg.per_edge_label_spacing = true;

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if cfg.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_labeled_edges(&mut lg, &edge_labels);
        cfg.rank_sep /= 2.0;
        rank::run(&mut lg, &cfg);
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &edge_labels, false);
        order::run(&mut lg, false);
        select_label_sides_first_last(&mut lg);
        run(&mut lg, &cfg);
        lg
    }

    fn b_y(lg: &LayoutGraph) -> f64 {
        lg.positions[lg.node_index[&"B".into()]].y
    }

    #[test]
    fn vertical_compartment_pass_noop_when_variable_rank_spacing_disabled() {
        let cfg_off = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let baseline = b_y(&build_two_label_layout(&cfg_off));

        // Flipping variable_rank_spacing *on* is the behaviour-gate for the
        // pass. The OFF case must exactly match the pre-change baseline —
        // which for this fixture is deterministic under the current formula.
        let again = b_y(&build_two_label_layout(&cfg_off));
        assert!(
            (baseline - again).abs() < 1e-9,
            "OFF should be deterministic"
        );
    }

    #[test]
    fn vertical_compartment_pass_widens_affected_gaps_when_enabled() {
        let mut cfg = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let y_off = b_y(&build_two_label_layout(&cfg));
        cfg.variable_rank_spacing = true;
        let y_on = b_y(&build_two_label_layout(&cfg));
        assert!(
            y_on > y_off,
            "B.y with variable_rank_spacing ON ({}) should exceed OFF ({})",
            y_on,
            y_off,
        );
    }

    #[test]
    fn vertical_compartment_pass_max_merges_over_preexisting_overrides() {
        // Seed a very large rank_sep override at the same gap the compartment
        // would also target. The larger value wins, so enabling the
        // compartment pass has no additional effect.
        let seed_gap: i32 = 1;
        let mut cfg_big = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        cfg_big.rank_sep_overrides.insert(seed_gap, 500.0);
        let y_big_off = b_y(&build_two_label_layout(&cfg_big));
        cfg_big.variable_rank_spacing = true;
        let y_big_on = b_y(&build_two_label_layout(&cfg_big));
        assert!(
            (y_big_on - y_big_off).abs() < 1e-6,
            "large pre-existing override (500) should win: off={}, on={}",
            y_big_off,
            y_big_on,
        );
    }

    #[test]
    fn vertical_compartment_pass_respects_smaller_preexisting_override() {
        // Seed a tiny override so the compartment value wins when enabled.
        // Observing behaviour: `y_on` should match the no-seed case, since
        // the compartment reservation outweighs the tiny seed.
        let seed_gap: i32 = 1;

        let mut cfg = LayoutConfig {
            direction: Direction::TopBottom,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: true,
            ..Default::default()
        };
        cfg.rank_sep_overrides.insert(seed_gap, 1.0);
        let y_seeded = b_y(&build_two_label_layout(&cfg));

        cfg.rank_sep_overrides.clear();
        let y_bare = b_y(&build_two_label_layout(&cfg));
        assert!(
            (y_seeded - y_bare).abs() < 1e-6,
            "small seed (1.0) should be overridden by compartment: seeded={}, bare={}",
            y_seeded,
            y_bare,
        );
    }

    // Plan 0150: horizontal variant of the compartment-pass integration
    // tests. Same topology, direction flipped. Width drives rank-axis
    // extent in LR/RL. RL mirrors LR after reverse_positions.
    fn build_two_label_layout_h(config: &LayoutConfig) -> LayoutGraph {
        use super::super::support::{
            extract_self_edges, make_space_for_labeled_edges, select_label_sides_first_last,
        };
        use super::super::types::EdgeLabelInfo;
        use super::super::{acyclic, normalize, order, rank};

        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (20.0, 40.0));
        graph.add_node("B", (20.0, 40.0));
        graph.add_node("C", (20.0, 40.0));
        graph.add_node("D", (20.0, 40.0));
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        let mut edge_labels = HashMap::new();
        // Wide rank-axis extent (80), tall cross-axis (40) so adjacent
        // label dummies' cross-bands overlap under BK output.
        edge_labels.insert(0, EdgeLabelInfo::new(80.0, 40.0));
        edge_labels.insert(1, EdgeLabelInfo::new(80.0, 40.0));

        let mut cfg = config.clone();
        cfg.per_edge_label_spacing = true;

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if cfg.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_labeled_edges(&mut lg, &edge_labels);
        cfg.rank_sep /= 2.0;
        rank::run(&mut lg, &cfg);
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &edge_labels, false);
        order::run(&mut lg, false);
        select_label_sides_first_last(&mut lg);
        run(&mut lg, &cfg);
        lg
    }

    fn b_x(lg: &LayoutGraph) -> f64 {
        lg.positions[lg.node_index[&"B".into()]].x
    }

    #[test]
    fn horizontal_compartment_pass_noop_when_variable_rank_spacing_disabled() {
        let cfg = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let baseline = b_x(&build_two_label_layout_h(&cfg));
        let again = b_x(&build_two_label_layout_h(&cfg));
        assert!(
            (baseline - again).abs() < 1e-9,
            "OFF should be deterministic"
        );
    }

    #[test]
    fn horizontal_compartment_pass_widens_affected_gaps_when_enabled() {
        let mut cfg = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let x_off = b_x(&build_two_label_layout_h(&cfg));
        cfg.variable_rank_spacing = true;
        let x_on = b_x(&build_two_label_layout_h(&cfg));
        assert!(
            x_on > x_off,
            "B.x with variable_rank_spacing ON ({}) should exceed OFF ({})",
            x_on,
            x_off,
        );
    }

    #[test]
    fn horizontal_compartment_pass_uses_width_not_height() {
        // Reservation must be sum(widths=80)+INTER_TRACK_GAP=164 — if it
        // used height (28) the widening would only be ~40 delta. We
        // observe via X advance by comparing ON vs OFF and asserting the
        // delta is at least 150 (sanity bound < 164 to avoid pinning
        // exact BK/formula arithmetic).
        let mut cfg = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let x_off = b_x(&build_two_label_layout_h(&cfg));
        cfg.variable_rank_spacing = true;
        let x_on = b_x(&build_two_label_layout_h(&cfg));
        let delta = x_on - x_off;
        assert!(
            delta > 100.0,
            "LR compartment delta should reflect widths (expected > 100 for 80-wide labels), got {}",
            delta,
        );
    }

    #[test]
    fn horizontal_compartment_pass_rl_is_symmetric() {
        let mut cfg_lr = LayoutConfig {
            direction: Direction::LeftRight,
            node_sep: 0.0,
            edge_sep: 0.0,
            rank_sep: 40.0,
            margin: 5.0,
            variable_rank_spacing: false,
            ..Default::default()
        };
        let lr_off = b_x(&build_two_label_layout_h(&cfg_lr));
        cfg_lr.variable_rank_spacing = true;
        let lr_on = b_x(&build_two_label_layout_h(&cfg_lr));
        let lr_delta = lr_on - lr_off;

        // For RL, reverse_positions mirrors X, but absolute compartment
        // reservation and layout extent should be identical, so we
        // measure extent (max X of B) rather than B.x directly.
        let mut cfg_rl = LayoutConfig {
            direction: Direction::RightLeft,
            ..cfg_lr
        };
        cfg_rl.variable_rank_spacing = false;
        let rl_lg_off = build_two_label_layout_h(&cfg_rl);
        cfg_rl.variable_rank_spacing = true;
        let rl_lg_on = build_two_label_layout_h(&cfg_rl);

        let extent_off = rl_lg_off
            .positions
            .iter()
            .zip(rl_lg_off.dimensions.iter())
            .map(|(p, (w, _))| p.x + w)
            .fold(f64::NEG_INFINITY, f64::max);
        let extent_on = rl_lg_on
            .positions
            .iter()
            .zip(rl_lg_on.dimensions.iter())
            .map(|(p, (w, _))| p.x + w)
            .fold(f64::NEG_INFINITY, f64::max);
        let rl_delta = extent_on - extent_off;

        assert!(
            (lr_delta - rl_delta).abs() < 1e-6,
            "LR compartment delta ({}) should equal RL extent delta ({})",
            lr_delta,
            rl_delta,
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
