//! Model-order tie-breaking tests.
//!
//! These invariants belong next to the layered kernel because the ordering
//! passes and their tie-break behavior live here.

use std::collections::HashMap;

use super::{DiGraph, LayoutConfig, layout};

/// Build a graph, run full layout, return node center-x positions.
/// In TD layout, x position reflects the within-rank ordering.
fn layout_and_get_x_positions(nodes: &[&str], edges: &[(&str, &str)]) -> HashMap<String, f64> {
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

/// Check if nodes appear in declaration order by x-coordinate.
/// In TD layout, lower x = further left = lower order index.
fn is_x_order_preserved(positions: &HashMap<String, f64>, expected_order: &[&str]) -> bool {
    for i in 0..expected_order.len() - 1 {
        let a = positions.get(expected_order[i]).unwrap();
        let b = positions.get(expected_order[i + 1]).unwrap();
        if a >= b {
            return false;
        }
    }
    true
}

/// Count nodes whose center-x position changed significantly between two layouts.
fn count_x_position_changes(
    before: &HashMap<String, f64>,
    after: &HashMap<String, f64>,
    tolerance: f64,
) -> usize {
    before
        .iter()
        .filter(|&(id, &x)| {
            after
                .get(id.as_str())
                .is_some_and(|&new_x| (new_x - x).abs() > tolerance)
        })
        .count()
}

// =========================================================================
// Phase 0: Baseline measurements (eprintln only, no assertions)
// =========================================================================

#[test]
fn baseline_fan_out_3() {
    let positions =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("A", "D")]);
    let preserved = is_x_order_preserved(&positions, &["B", "C", "D"]);
    eprintln!("fan_out_3 source order preserved: {preserved}");
    eprintln!(
        "  x-positions: B={:.1}, C={:.1}, D={:.1}",
        positions["B"], positions["C"], positions["D"]
    );
}

#[test]
fn baseline_fan_out_5() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E", "F"],
        &[("A", "B"), ("A", "C"), ("A", "D"), ("A", "E"), ("A", "F")],
    );
    let preserved = is_x_order_preserved(&positions, &["B", "C", "D", "E", "F"]);
    eprintln!("fan_out_5 source order preserved: {preserved}");
    eprintln!(
        "  x-positions: B={:.1}, C={:.1}, D={:.1}, E={:.1}, F={:.1}",
        positions["B"], positions["C"], positions["D"], positions["E"], positions["F"]
    );
}

#[test]
fn baseline_diamond_fan_out() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D"],
        &[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")],
    );
    let preserved = is_x_order_preserved(&positions, &["B", "C"]);
    eprintln!("diamond_fan source order preserved: {preserved}");
    eprintln!(
        "  x-positions: B={:.1}, C={:.1}",
        positions["B"], positions["C"]
    );
}

#[test]
fn baseline_fan_out_then_converge() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[
            ("A", "B"),
            ("A", "C"),
            ("A", "D"),
            ("B", "E"),
            ("C", "E"),
            ("D", "E"),
        ],
    );
    let preserved = is_x_order_preserved(&positions, &["B", "C", "D"]);
    eprintln!("fan_out_then_converge source order preserved: {preserved}");
    eprintln!(
        "  x-positions: B={:.1}, C={:.1}, D={:.1}",
        positions["B"], positions["C"], positions["D"]
    );
}

#[test]
fn baseline_two_independent_fan_outs() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E", "F"],
        &[("A", "B"), ("A", "C"), ("D", "E"), ("D", "F")],
    );
    let preserved_1 = is_x_order_preserved(&positions, &["B", "C"]);
    let preserved_2 = is_x_order_preserved(&positions, &["E", "F"]);
    eprintln!("two_fan_outs: group1={preserved_1}, group2={preserved_2}");
}

// =========================================================================
// Phase 4.1: Source-order fidelity validation (assertions)
// =========================================================================

#[test]
fn fan_out_3_source_order_preserved() {
    let positions =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("A", "D")]);
    assert!(
        is_x_order_preserved(&positions, &["B", "C", "D"]),
        "Fan-out targets should follow declaration order. Got B={:.1}, C={:.1}, D={:.1}",
        positions["B"],
        positions["C"],
        positions["D"]
    );
}

#[test]
fn fan_out_5_source_order_preserved() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E", "F"],
        &[("A", "B"), ("A", "C"), ("A", "D"), ("A", "E"), ("A", "F")],
    );
    assert!(
        is_x_order_preserved(&positions, &["B", "C", "D", "E", "F"]),
        "5-way fan-out should follow declaration order"
    );
}

#[test]
fn diamond_fan_source_order_preserved() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D"],
        &[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")],
    );
    assert!(
        is_x_order_preserved(&positions, &["B", "C"]),
        "Diamond fan-out should preserve B before C"
    );
}

#[test]
fn fan_out_then_converge_order_preserved() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[
            ("A", "B"),
            ("A", "C"),
            ("A", "D"),
            ("B", "E"),
            ("C", "E"),
            ("D", "E"),
        ],
    );
    assert!(
        is_x_order_preserved(&positions, &["B", "C", "D"]),
        "Fan-out-then-converge should preserve declaration order"
    );
}

#[test]
fn two_independent_fan_outs_both_preserved() {
    let positions = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E", "F"],
        &[("A", "B"), ("A", "C"), ("D", "E"), ("D", "F")],
    );
    assert!(
        is_x_order_preserved(&positions, &["B", "C"]),
        "First fan-out should preserve order"
    );
    assert!(
        is_x_order_preserved(&positions, &["E", "F"]),
        "Second fan-out should preserve order"
    );
}

// =========================================================================
// Phase 0.2: Edit stability baselines
// =========================================================================

#[test]
fn baseline_edit_stability_add_sibling() {
    let before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("A", "C")]);
    let after =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("A", "D")]);
    let changes = count_x_position_changes(&before, &after, 1.0);
    eprintln!("edit_stability_add_sibling: {changes} nodes changed position");
    eprintln!("  before: B={:.1}, C={:.1}", before["B"], before["C"]);
    eprintln!("  after:  B={:.1}, C={:.1}", after["B"], after["C"]);
}

#[test]
fn baseline_edit_stability_add_unrelated_node() {
    let before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("B", "C")]);
    let after = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[("A", "B"), ("B", "C"), ("D", "E")],
    );
    let changes = count_x_position_changes(&before, &after, 1.0);
    eprintln!("edit_stability_add_unrelated: {changes} nodes changed position");
}

#[test]
fn baseline_edit_stability_add_middle_edge() {
    let before = layout_and_get_x_positions(
        &["A", "B", "D", "E"],
        &[("A", "B"), ("A", "D"), ("B", "E"), ("D", "E")],
    );
    let after = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[
            ("A", "B"),
            ("A", "C"),
            ("A", "D"),
            ("B", "E"),
            ("C", "E"),
            ("D", "E"),
        ],
    );
    let changes = count_x_position_changes(&before, &after, 1.0);
    eprintln!("edit_stability_add_middle: {changes} nodes changed position");
    eprintln!("  before: B={:.1}, D={:.1}", before["B"], before["D"]);
    eprintln!(
        "  after:  B={:.1}, C={:.1}, D={:.1}",
        after["B"], after["C"], after["D"]
    );
}

#[test]
fn baseline_edit_stability_add_downstream() {
    let before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("A", "C")]);
    let after =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("B", "D")]);
    let changes = count_x_position_changes(&before, &after, 1.0);
    eprintln!("edit_stability_add_downstream: {changes} nodes changed position");
}

// =========================================================================
// Phase 4.2: Edit stability validation (assertions)
// =========================================================================

#[test]
fn edit_stability_add_sibling_preserves_relative_order() {
    let _before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("A", "C")]);
    let after =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("A", "D")]);
    assert!(
        after["B"] < after["C"],
        "B should still be before C after adding sibling D. B={:.1}, C={:.1}",
        after["B"],
        after["C"]
    );
}

#[test]
fn edit_stability_add_unrelated_no_changes() {
    let before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("B", "C")]);
    let after = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[("A", "B"), ("B", "C"), ("D", "E")],
    );
    let changes = count_x_position_changes(&before, &after, 1.0);
    assert_eq!(
        changes, 0,
        "Adding disconnected nodes should not change existing positions"
    );
}

#[test]
fn edit_stability_add_downstream_preserves_fan_out() {
    let _before = layout_and_get_x_positions(&["A", "B", "C"], &[("A", "B"), ("A", "C")]);
    let after =
        layout_and_get_x_positions(&["A", "B", "C", "D"], &[("A", "B"), ("A", "C"), ("B", "D")]);
    assert!(
        after["B"] < after["C"],
        "B should still be before C after extending B's branch"
    );
}

#[test]
fn edit_stability_relative_order_preserved_across_fan_out_extension() {
    let _before = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E"],
        &[
            ("A", "B"),
            ("A", "C"),
            ("A", "D"),
            ("B", "E"),
            ("C", "E"),
            ("D", "E"),
        ],
    );
    let after = layout_and_get_x_positions(
        &["A", "B", "C", "D", "E", "F"],
        &[
            ("A", "B"),
            ("A", "C"),
            ("A", "D"),
            ("A", "F"),
            ("B", "E"),
            ("C", "E"),
            ("D", "E"),
            ("F", "E"),
        ],
    );
    assert!(after["B"] < after["C"], "B before C after adding F");
    assert!(after["C"] < after["D"], "C before D after adding F");
}

// =========================================================================
// Phase 0.3: Scan fixtures for equal barycenters
// =========================================================================

#[test]
fn baseline_scan_fixtures_for_equal_barycenters() {
    use std::fs;
    use std::path::PathBuf;

    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowchart");

    let mut fixtures_with_ties = Vec::new();
    let mut fixtures_without_ties = Vec::new();

    let mut dir_entries: Vec<_> = fs::read_dir(&fixture_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "mmd") {
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let flowchart = match crate::mermaid::parse_flowchart(&content) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);

            let mut out_targets: HashMap<String, Vec<String>> = HashMap::new();
            for edge in &diagram.edges {
                out_targets
                    .entry(edge.from.clone())
                    .or_default()
                    .push(edge.to.clone());
            }

            let fan_outs: Vec<(String, Vec<String>)> = out_targets
                .into_iter()
                .filter(|(_, targets)| targets.len() >= 2)
                .collect();

            if fan_outs.is_empty() {
                fixtures_without_ties.push(name);
            } else {
                fixtures_with_ties.push((name, fan_outs));
            }
        }
    }

    eprintln!(
        "\n=== Fixtures with fan-out patterns (potential equal barycenters) ({}) ===",
        fixtures_with_ties.len()
    );
    for (name, candidates) in &fixtures_with_ties {
        eprintln!("  {name}:");
        for (source, targets) in candidates {
            eprintln!("    {source} --> {:?}", targets);
        }
    }

    eprintln!(
        "\n=== Fixtures without fan-out patterns ({}) ===",
        fixtures_without_ties.len()
    );
    for name in &fixtures_without_ties {
        eprintln!("  {name}");
    }
}
