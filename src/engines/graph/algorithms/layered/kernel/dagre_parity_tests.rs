//! Dagre parity tests.
//!
//! These tests compare mmdflux's layered-kernel layout output against captured
//! dagre.js output to preserve the parity cases that still define behavior.

use std::fs;
use std::path::Path;

use serde::Deserialize;

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

// =============================================================================
// Test Data Types
// =============================================================================

/// Input graph format matching `mmdflux-dagre-input.json`.
#[derive(Debug, Deserialize)]
struct InputGraph {
    nodes: Vec<InputNode>,
    edges: Vec<InputEdge>,
}

#[derive(Debug, Deserialize)]
struct InputNode {
    id: String,
    width: f64,
    height: f64,
    parent: Option<String>,
    #[serde(rename = "is_subgraph")]
    _is_subgraph: bool,
}

#[derive(Debug, Deserialize)]
struct InputEdge {
    from: String,
    to: String,
}

/// Expected layout output format matching `dagre-layout.json`.
#[derive(Debug, Deserialize)]
struct DagreLayout {
    nodes: Vec<DagreNode>,
    #[serde(default)]
    edges: Vec<DagreEdge>,
}

#[derive(Debug, Deserialize)]
struct DagreNode {
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    is_compound: bool,
}

#[derive(Debug, Deserialize)]
struct DagreEdge {
    index: usize,
    #[serde(rename = "from")]
    _from: String,
    #[serde(rename = "to")]
    _to: String,
    points: Vec<[f64; 2]>,
}

// =============================================================================
// Helpers
// =============================================================================

fn load_json<T: for<'a> Deserialize<'a>>(path: &str) -> T {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let full_path = Path::new(manifest).join(path);
    let content =
        fs::read_to_string(&full_path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e))
}

fn build_digraph_from_input(input: &InputGraph) -> DiGraph<(f64, f64)> {
    let mut graph: DiGraph<(f64, f64)> = DiGraph::new();

    for node in &input.nodes {
        graph.add_node(node.id.as_str(), (node.width, node.height));
    }

    for node in &input.nodes {
        if let Some(parent) = &node.parent {
            graph.set_parent(node.id.as_str(), parent.as_str());
        }
    }

    for edge in &input.edges {
        graph.add_edge(edge.from.as_str(), edge.to.as_str());
    }

    graph
}

/// Border node info parsed from debug dump files.
#[derive(Debug, Clone)]
struct BorderNodeInfo {
    order: i32,
    x: f64,
    #[allow(dead_code)]
    y: f64,
}

/// Parse border node positions from a debug dump file.
/// Returns a map of compound_name -> (border_type -> BorderNodeInfo).
/// border_type is "top", "bottom", or "rank_N_left"/"rank_N_right".
fn parse_border_nodes(
    path: &str,
) -> std::collections::HashMap<String, std::collections::HashMap<String, BorderNodeInfo>> {
    use std::collections::HashMap;

    use regex::Regex;

    let manifest = env!("CARGO_MANIFEST_DIR");
    let full_path = Path::new(manifest).join(path);
    let content =
        fs::read_to_string(&full_path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));

    let mut result: HashMap<String, HashMap<String, BorderNodeInfo>> = HashMap::new();
    let mut current_compound: Option<String> = None;

    let compound_re = Regex::new(r"^\[border_nodes\] (\S+) min_rank").unwrap();
    let top_bottom_re = Regex::new(
        r"^\[border_nodes\]\s+(top|bottom) \S+ rank=\d+ order=(\d+) x=([\d.]+) y=([\d.]+)",
    )
    .unwrap();
    let rank_re = Regex::new(r"^\[border_nodes\]\s+rank (\d+): left \S+ order=(\d+) x=([\d.]+) y=([\d.]+) right \S+ order=(\d+) x=([\d.]+) y=([\d.]+)").unwrap();

    for line in content.lines() {
        if let Some(caps) = compound_re.captures(line) {
            current_compound = Some(caps[1].to_string());
            result.entry(caps[1].to_string()).or_default();
            continue;
        }

        if let Some(caps) = top_bottom_re.captures(line) {
            if let Some(ref compound) = current_compound {
                let border_type = caps[1].to_string();
                let info = BorderNodeInfo {
                    order: caps[2].parse().unwrap(),
                    x: caps[3].parse().unwrap(),
                    y: caps[4].parse().unwrap(),
                };
                result.get_mut(compound).unwrap().insert(border_type, info);
            }
            continue;
        }

        if let Some(caps) = rank_re.captures(line)
            && let Some(ref compound) = current_compound
        {
            let rank: i32 = caps[1].parse().unwrap();
            let left_info = BorderNodeInfo {
                order: caps[2].parse().unwrap(),
                x: caps[3].parse().unwrap(),
                y: caps[4].parse().unwrap(),
            };
            let right_info = BorderNodeInfo {
                order: caps[5].parse().unwrap(),
                x: caps[6].parse().unwrap(),
                y: caps[7].parse().unwrap(),
            };
            result
                .get_mut(compound)
                .unwrap()
                .insert(format!("rank_{}_left", rank), left_info);
            result
                .get_mut(compound)
                .unwrap()
                .insert(format!("rank_{}_right", rank), right_info);
        }
    }

    result
}

/// Assert that two slices of points are close within a tolerance.
fn assert_points_close(actual: &[(f64, f64)], expected: &[[f64; 2]], tolerance: f64, label: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{}: point count mismatch: actual {} vs expected {}",
        label,
        actual.len(),
        expected.len()
    );
    for (i, (act, exp)) in actual.iter().zip(expected.iter()).enumerate() {
        let dx = (act.0 - exp[0]).abs();
        let dy = (act.1 - exp[1]).abs();
        assert!(
            dx < tolerance && dy < tolerance,
            "{} point {}: actual ({:.6}, {:.6}) vs expected ({:.6}, {:.6}), diff ({:.6}, {:.6})",
            label,
            i,
            act.0,
            act.1,
            exp[0],
            exp[1],
            dx,
            dy
        );
    }
}

// =============================================================================
// Parity Tests
// =============================================================================

mod subgraph_bounds {
    use super::*;

    const INPUT_PATH: &str =
        "tests/parity-fixtures/external_node_subgraph/mmdflux-dagre-input.json";
    const EXPECTED_PATH: &str = "tests/parity-fixtures/external_node_subgraph/dagre-layout.json";

    #[test]
    fn external_node_subgraph_bounds_match_dagre() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);

        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 75.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);

        let expected_cloud = expected
            .nodes
            .iter()
            .find(|n| n.id == "Cloud" && n.is_compound)
            .expect("Should find Cloud compound node in expected output");

        let actual_cloud = result
            .subgraph_bounds
            .get("Cloud")
            .expect("Should have bounds for Cloud subgraph");

        let tolerance = 1.0;

        assert!(
            (actual_cloud.width - expected_cloud.width).abs() < tolerance,
            "Cloud width mismatch: actual={}, expected={} (diff={})",
            actual_cloud.width,
            expected_cloud.width,
            actual_cloud.width - expected_cloud.width
        );
        assert!(
            (actual_cloud.height - expected_cloud.height).abs() < tolerance,
            "Cloud height mismatch: actual={}, expected={} (diff={})",
            actual_cloud.height,
            expected_cloud.height,
            actual_cloud.height - expected_cloud.height
        );
        assert!(
            (actual_cloud.x - expected_cloud.x).abs() < tolerance,
            "Cloud x mismatch: actual={}, expected={} (diff={})",
            actual_cloud.x,
            expected_cloud.x,
            actual_cloud.x - expected_cloud.x
        );
        assert!(
            (actual_cloud.y - expected_cloud.y).abs() < tolerance,
            "Cloud y mismatch: actual={}, expected={} (diff={})",
            actual_cloud.y,
            expected_cloud.y,
            actual_cloud.y - expected_cloud.y
        );
    }

    #[test]
    fn external_node_subgraph_all_compound_dimensions_match_dagre() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);

        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 75.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);

        let tolerance = 1.0;

        for expected_node in expected.nodes.iter().filter(|n| n.is_compound) {
            let actual = result
                .subgraph_bounds
                .get(&expected_node.id)
                .unwrap_or_else(|| panic!("Missing bounds for compound {}", expected_node.id));

            let width_diff = (actual.width - expected_node.width).abs();
            let height_diff = (actual.height - expected_node.height).abs();

            assert!(
                width_diff < tolerance,
                "{} width mismatch: actual={}, expected={} (diff={})",
                expected_node.id,
                actual.width,
                expected_node.width,
                width_diff
            );
            assert!(
                height_diff < tolerance,
                "{} height mismatch: actual={}, expected={} (diff={})",
                expected_node.id,
                actual.height,
                expected_node.height,
                height_diff
            );
        }
    }

    #[test]
    fn external_node_subgraph_sibling_ordering_matches_dagre() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);

        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 75.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);

        let us_east_actual = result
            .subgraph_bounds
            .get("us-east")
            .expect("us-east bounds");
        let us_west_actual = result
            .subgraph_bounds
            .get("us-west")
            .expect("us-west bounds");

        let us_east_expected = expected
            .nodes
            .iter()
            .find(|n| n.id == "us-east")
            .expect("us-east expected");
        let us_west_expected = expected
            .nodes
            .iter()
            .find(|n| n.id == "us-west")
            .expect("us-west expected");

        let dagre_order_is_east_then_west = us_east_expected.x < us_west_expected.x;
        let mmdflux_order_is_east_then_west = us_east_actual.x < us_west_actual.x;

        assert_eq!(
            dagre_order_is_east_then_west, mmdflux_order_is_east_then_west,
            "Sibling ordering should match dagre layout ordering"
        );

        assert!(
            us_east_actual.x < us_west_actual.x,
            "mmdflux should have us-east on the left"
        );
        assert!(
            us_east_expected.x < us_west_expected.x,
            "dagre should have us-east on the left"
        );
    }
}

mod border_ordering {
    use super::*;

    const INPUT_PATH: &str =
        "tests/parity-fixtures/external_node_subgraph/mmdflux-dagre-input.json";
    const EXPECTED_PATH: &str = "tests/parity-fixtures/external_node_subgraph/dagre-layout.json";
    const MMDFLUX_BORDER_NODES_PATH: &str =
        "tests/parity-fixtures/external_node_subgraph/mmdflux-border-nodes.txt";
    const DAGRE_BORDER_NODES_PATH: &str =
        "tests/parity-fixtures/external_node_subgraph/dagre-border-nodes.txt";

    #[test]
    fn cloud_bottom_border_x_between_left_and_right() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);

        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 75.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);

        let expected_cloud = expected
            .nodes
            .iter()
            .find(|n| n.id == "Cloud" && n.is_compound)
            .expect("Should find Cloud compound node");

        let actual_cloud = result
            .subgraph_bounds
            .get("Cloud")
            .expect("Should have bounds for Cloud");

        let tolerance = 1.0;
        let width_diff = (actual_cloud.width - expected_cloud.width).abs();

        assert!(
            width_diff < tolerance,
            "Cloud bounds width divergence suggests border ordering issue: \
             actual width={}, expected width={}, diff={}\n\
             This is caused by _bb_Cloud being ordered AFTER _br_Cloud instead of BETWEEN \
             _bl_Cloud and _br_Cloud at rank 8.",
            actual_cloud.width,
            expected_cloud.width,
            width_diff
        );
    }

    #[test]
    fn cloud_bottom_border_x_matches_dagre() {
        let mmdflux = parse_border_nodes(MMDFLUX_BORDER_NODES_PATH);
        let dagre = parse_border_nodes(DAGRE_BORDER_NODES_PATH);

        let mmdflux_cloud = mmdflux.get("Cloud").expect("mmdflux should have Cloud");
        let dagre_cloud = dagre.get("Cloud").expect("dagre should have Cloud");

        let mmdflux_bottom = mmdflux_cloud
            .get("bottom")
            .expect("mmdflux Cloud should have bottom");
        let dagre_bottom = dagre_cloud
            .get("bottom")
            .expect("dagre Cloud should have bottom");

        let tolerance = 1.0;
        let x_diff = (mmdflux_bottom.x - dagre_bottom.x).abs();

        assert!(
            x_diff < tolerance,
            "Cloud bottom border x mismatch: mmdflux x={}, dagre x={}, diff={}\n\
             Expected bottom border to be centered between left/right borders.\n\
             dagre: left.order < bottom.order < right.order (bottom between left/right)\n\
             mmdflux: left.order < right.order < bottom.order (bottom after right)",
            mmdflux_bottom.x,
            dagre_bottom.x,
            x_diff
        );
    }

    #[test]
    fn cloud_bottom_border_order_after_right_for_top_level_compound() {
        let mmdflux = parse_border_nodes(MMDFLUX_BORDER_NODES_PATH);

        let mmdflux_cloud = mmdflux.get("Cloud").expect("mmdflux should have Cloud");

        let left = mmdflux_cloud
            .get("rank_8_left")
            .expect("Cloud should have rank_8_left");
        let right = mmdflux_cloud
            .get("rank_8_right")
            .expect("Cloud should have rank_8_right");
        let bottom = mmdflux_cloud
            .get("bottom")
            .expect("Cloud should have bottom");

        assert!(
            left.order < right.order && right.order < bottom.order,
            "Cloud bottom border should be ordered AFTER right at rank 8 (dagre 0.8.5):\n\
             Expected: left.order ({}) < right.order ({}) < bottom.order ({})",
            left.order,
            right.order,
            bottom.order
        );
    }
}

mod backward_edge_bends {
    use super::*;

    const INPUT_PATH: &str = "tests/parity-fixtures/backward_in_subgraph/mmdflux-dagre-input.json";
    const EXPECTED_PATH: &str = "tests/parity-fixtures/backward_in_subgraph/dagre-layout.json";

    #[test]
    fn backward_edge_bends_match_dagre() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);

        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 75.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);

        let expected_e0 = expected
            .edges
            .iter()
            .find(|e| e.index == 0)
            .expect("Should find edge 0 in expected output");
        let expected_e1 = expected
            .edges
            .iter()
            .find(|e| e.index == 1)
            .expect("Should find edge 1 in expected output");

        let actual_e0 = result
            .edges
            .iter()
            .find(|e| e.index == 0)
            .expect("Should find edge 0 in actual output");
        let actual_e1 = result
            .edges
            .iter()
            .find(|e| e.index == 1)
            .expect("Should find edge 1 in actual output");

        let actual_e0_points: Vec<(f64, f64)> =
            actual_e0.points.iter().map(|p| (p.x, p.y)).collect();
        let actual_e1_points: Vec<(f64, f64)> =
            actual_e1.points.iter().map(|p| (p.x, p.y)).collect();

        let tolerance = 11.0;

        assert_points_close(
            &actual_e0_points,
            &expected_e0.points,
            tolerance,
            "Edge 0 (A→B)",
        );
        assert_points_close(
            &actual_e1_points,
            &expected_e1.points,
            tolerance,
            "Edge 1 (B→A)",
        );
    }
}

mod feedback_cycle_ranking {
    use super::*;

    const INPUT_PATH: &str =
        "tests/parity-fixtures/callgraph_feedback_cycle/mmdflux-dagre-input.json";
    const EXPECTED_PATH: &str = "tests/parity-fixtures/callgraph_feedback_cycle/dagre-layout.json";

    #[test]
    fn callgraph_feedback_cycle_node_y_positions_match_dagre() {
        let input: InputGraph = load_json(INPUT_PATH);
        let expected: DagreLayout = load_json(EXPECTED_PATH);

        let graph = build_digraph_from_input(&input);
        let config = LayoutConfig {
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            margin: 8.0,
            ..Default::default()
        };

        let result = layout(&graph, &config, |_, dims| *dims);
        let tolerance = 1.0;

        for expected_node in &expected.nodes {
            let node_id = NodeId::from(expected_node.id.as_str());
            let actual = result
                .nodes
                .get(&node_id)
                .unwrap_or_else(|| panic!("Missing node {}", expected_node.id));

            let y_diff = (actual.y - expected_node.y).abs();
            assert!(
                y_diff < tolerance,
                "{} y mismatch: actual={}, expected={} (diff={})",
                expected_node.id,
                actual.y,
                expected_node.y,
                y_diff
            );
        }
    }
}
