//! MMDS conformance harness.
//!
//! Tiered conformance checks comparing the direct render pipeline
//! (Mermaid text → Diagram → render) against the MMDS roundtrip pipeline
//! (Mermaid text → Diagram → MMDS JSON → hydrate → Diagram → render).
//!
//! Three tiers:
//! - **Semantic**: graph structure equivalence (nodes, edges, subgraphs, direction)
//! - **Layout**: geometry-level equivalence (node positions, edge topology)
//! - **Visual**: rendered text output equivalence

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use mmdflux::diagram::{EngineConfig, OutputFormat, RenderConfig};
use mmdflux::diagrams::flowchart::FlowchartInstance;
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::geometry::{GraphGeometry, LayoutEdge};
use mmdflux::diagrams::mmds::from_mmds_str;
use mmdflux::graph::{Diagram, Subgraph};
use mmdflux::registry::DiagramInstance;
use mmdflux::render::{RenderOptions, render};

// ---------------------------------------------------------------------------
// Conformance report model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TierStatus {
    Pass,
    Fail(String),
}

impl TierStatus {
    fn is_pass(&self) -> bool {
        matches!(self, TierStatus::Pass)
    }
}

#[derive(Debug, Clone)]
struct TierResult {
    tier: &'static str,
    status: TierStatus,
}

#[derive(Debug)]
struct ConformanceReport {
    fixture_path: String,
    semantic: TierResult,
    layout: TierResult,
    visual: TierResult,
}

impl ConformanceReport {
    fn tiers(&self) -> [&TierResult; 3] {
        [&self.semantic, &self.layout, &self.visual]
    }
}

// ---------------------------------------------------------------------------
// Harness: run a single conformance case
// ---------------------------------------------------------------------------

fn fixture_input(family: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(family)
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

/// Compare two Diagrams for semantic equivalence.
///
/// Checks direction, nodes (sorted by ID), edges (by index), and subgraphs.
fn check_semantic(direct: &Diagram, roundtrip: &Diagram) -> TierResult {
    let mut mismatches = Vec::new();

    if direct.direction != roundtrip.direction {
        mismatches.push(format!(
            "direction: {:?} vs {:?}",
            direct.direction, roundtrip.direction
        ));
    }

    // Compare nodes (sorted by ID for determinism)
    let direct_nodes: BTreeMap<_, _> = direct.nodes.iter().collect();
    let roundtrip_nodes: BTreeMap<_, _> = roundtrip.nodes.iter().collect();

    if direct_nodes.len() != roundtrip_nodes.len() {
        mismatches.push(format!(
            "node count: {} vs {}",
            direct_nodes.len(),
            roundtrip_nodes.len()
        ));
    } else {
        for (id, d_node) in &direct_nodes {
            match roundtrip_nodes.get(id) {
                None => mismatches.push(format!("node {id} missing in roundtrip")),
                Some(r_node) => {
                    if d_node != r_node {
                        mismatches.push(format!("node {id} differs"));
                    }
                }
            }
        }
    }

    // Compare edges (by index order)
    if direct.edges.len() != roundtrip.edges.len() {
        mismatches.push(format!(
            "edge count: {} vs {}",
            direct.edges.len(),
            roundtrip.edges.len()
        ));
    } else {
        for (i, (d_edge, r_edge)) in direct.edges.iter().zip(&roundtrip.edges).enumerate() {
            if d_edge != r_edge {
                mismatches.push(format!("edge {i} differs"));
            }
        }
    }

    // Compare subgraphs (sorted by ID).
    //
    // Normalize node lists to direct children only on both sides. Runtime
    // internals may carry descendant membership for compound layout parity,
    // but semantic parity compares the contract-level direct-children view.
    let direct_sgs: BTreeMap<_, _> = direct.subgraphs.iter().collect();
    let roundtrip_sgs: BTreeMap<_, _> = roundtrip.subgraphs.iter().collect();

    if direct_sgs.len() != roundtrip_sgs.len() {
        mismatches.push(format!(
            "subgraph count: {} vs {}",
            direct_sgs.len(),
            roundtrip_sgs.len()
        ));
    } else {
        for (id, d_sg) in &direct_sgs {
            match roundtrip_sgs.get(id) {
                None => mismatches.push(format!("subgraph {id} missing in roundtrip")),
                Some(r_sg) => {
                    let mut direct_children: Vec<String> = d_sg
                        .nodes
                        .iter()
                        .filter(|node_id| {
                            direct.nodes.get(*node_id).and_then(|n| n.parent.as_deref())
                                == Some(&d_sg.id)
                        })
                        .cloned()
                        .collect();
                    direct_children.sort();

                    let mut roundtrip_children: Vec<String> = r_sg
                        .nodes
                        .iter()
                        .filter(|node_id| {
                            roundtrip
                                .nodes
                                .get(*node_id)
                                .and_then(|n| n.parent.as_deref())
                                == Some(&r_sg.id)
                        })
                        .cloned()
                        .collect();
                    roundtrip_children.sort();
                    let normalized_d = Subgraph {
                        nodes: direct_children,
                        ..(*d_sg).clone()
                    };
                    let normalized_r = Subgraph {
                        nodes: roundtrip_children,
                        ..(*r_sg).clone()
                    };
                    if normalized_d != normalized_r {
                        mismatches.push(format!("subgraph {id} differs"));
                    }
                }
            }
        }
    }

    TierResult {
        tier: "semantic",
        status: if mismatches.is_empty() {
            TierStatus::Pass
        } else {
            TierStatus::Fail(mismatches.join("; "))
        },
    }
}

/// Compare rendered output for visual equivalence (text and SVG).
fn check_visual(direct: &Diagram, roundtrip: &Diagram) -> TierResult {
    let mut mismatches = Vec::new();

    // Text comparison
    let text_options = RenderOptions::default();
    let direct_text = render(direct, &text_options);
    let roundtrip_text = render(roundtrip, &text_options);
    if direct_text != roundtrip_text {
        mismatches.push("text output differs".to_string());
    }

    // SVG comparison
    let svg_options = RenderOptions::default_svg();
    let direct_svg = render(direct, &svg_options);
    let roundtrip_svg = render(roundtrip, &svg_options);
    if direct_svg != roundtrip_svg {
        mismatches.push("svg output differs".to_string());
    }

    TierResult {
        tier: "visual",
        status: if mismatches.is_empty() {
            TierStatus::Pass
        } else {
            TierStatus::Fail(mismatches.join("; "))
        },
    }
}

/// Float tolerance for geometry comparison.
const GEOMETRY_TOLERANCE: f64 = 0.01;

fn floats_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < GEOMETRY_TOLERANCE
}

/// Compare layout geometry for equivalence.
///
/// Runs layered layout on both diagrams and compares node positions/sizes,
/// edge count, and overall bounds.
fn check_layout(direct: &Diagram, roundtrip: &Diagram) -> TierResult {
    let engine_config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());

    let direct_geom = match run_layered_layout(&MeasurementMode::Text, direct, &engine_config) {
        Ok(geom) => geom,
        Err(e) => {
            return TierResult {
                tier: "layout",
                status: TierStatus::Fail(format!("direct layout failed: {e}")),
            };
        }
    };

    let roundtrip_geom = match run_layered_layout(&MeasurementMode::Text, roundtrip, &engine_config)
    {
        Ok(geom) => geom,
        Err(e) => {
            return TierResult {
                tier: "layout",
                status: TierStatus::Fail(format!("roundtrip layout failed: {e}")),
            };
        }
    };

    let mismatches = compare_geometry(&direct_geom, &roundtrip_geom);

    TierResult {
        tier: "layout",
        status: if mismatches.is_empty() {
            TierStatus::Pass
        } else {
            TierStatus::Fail(mismatches.join("; "))
        },
    }
}

fn compare_geometry(direct: &GraphGeometry, roundtrip: &GraphGeometry) -> Vec<String> {
    let mut mismatches = Vec::new();

    // Compare node count
    if direct.nodes.len() != roundtrip.nodes.len() {
        mismatches.push(format!(
            "node count: {} vs {}",
            direct.nodes.len(),
            roundtrip.nodes.len()
        ));
        return mismatches;
    }

    // Compare node positions and sizes
    let mut direct_nodes: Vec<_> = direct.nodes.iter().collect();
    direct_nodes.sort_by_key(|(id, _)| (*id).clone());
    for (id, d_node) in &direct_nodes {
        match roundtrip.nodes.get(*id) {
            None => mismatches.push(format!("node {id} missing in roundtrip geometry")),
            Some(r_node) => {
                if !floats_eq(d_node.rect.x, r_node.rect.x)
                    || !floats_eq(d_node.rect.y, r_node.rect.y)
                {
                    mismatches.push(format!(
                        "node {id} position: ({:.1},{:.1}) vs ({:.1},{:.1})",
                        d_node.rect.x, d_node.rect.y, r_node.rect.x, r_node.rect.y
                    ));
                }
                if !floats_eq(d_node.rect.width, r_node.rect.width)
                    || !floats_eq(d_node.rect.height, r_node.rect.height)
                {
                    mismatches.push(format!(
                        "node {id} size: ({:.1}x{:.1}) vs ({:.1}x{:.1})",
                        d_node.rect.width,
                        d_node.rect.height,
                        r_node.rect.width,
                        r_node.rect.height
                    ));
                }
            }
        }
    }

    // Compare edges (sorted by index for determinism).
    //
    // Filter to user-visible edges only. The layered layout's compound graph pipeline creates
    // internal border edges whose endpoints are synthetic nodes (_bt_*, _bb_*,
    // _bl_*, _br_*, _tt_*). These may differ between direct and roundtrip paths
    // due to the descendant-vs-direct-children difference without affecting the
    // user-visible layout.
    let is_user_edge =
        |e: &LayoutEdge| direct.nodes.contains_key(&e.from) && direct.nodes.contains_key(&e.to);
    let mut d_edges: Vec<_> = direct.edges.iter().filter(|e| is_user_edge(e)).collect();
    let mut r_edges: Vec<_> = roundtrip.edges.iter().filter(|e| is_user_edge(e)).collect();
    d_edges.sort_by_key(|e| e.index);
    r_edges.sort_by_key(|e| e.index);

    if d_edges.len() != r_edges.len() {
        mismatches.push(format!(
            "user edge count: {} vs {}",
            d_edges.len(),
            r_edges.len()
        ));
    } else {
        for (de, re) in d_edges.iter().zip(&r_edges) {
            let idx = de.index;
            if de.from != re.from || de.to != re.to {
                mismatches.push(format!(
                    "edge {idx} endpoints: {}->{} vs {}->{}",
                    de.from, de.to, re.from, re.to
                ));
            }
            if de.waypoints.len() != re.waypoints.len() {
                mismatches.push(format!(
                    "edge {idx} waypoint count: {} vs {}",
                    de.waypoints.len(),
                    re.waypoints.len()
                ));
            } else {
                for (i, (dw, rw)) in de.waypoints.iter().zip(&re.waypoints).enumerate() {
                    if !floats_eq(dw.x, rw.x) || !floats_eq(dw.y, rw.y) {
                        mismatches.push(format!(
                            "edge {idx} waypoint {i}: ({:.1},{:.1}) vs ({:.1},{:.1})",
                            dw.x, dw.y, rw.x, rw.y
                        ));
                    }
                }
            }
            match (&de.label_position, &re.label_position) {
                (Some(dl), Some(rl)) => {
                    if !floats_eq(dl.x, rl.x) || !floats_eq(dl.y, rl.y) {
                        mismatches.push(format!(
                            "edge {idx} label pos: ({:.1},{:.1}) vs ({:.1},{:.1})",
                            dl.x, dl.y, rl.x, rl.y
                        ));
                    }
                }
                (None, None) => {}
                _ => mismatches.push(format!("edge {idx} label_position presence mismatch")),
            }
        }
    }

    // Compare subgraph geometry
    if direct.subgraphs.len() != roundtrip.subgraphs.len() {
        mismatches.push(format!(
            "subgraph geometry count: {} vs {}",
            direct.subgraphs.len(),
            roundtrip.subgraphs.len()
        ));
    } else {
        for (id, d_sg) in &direct.subgraphs {
            match roundtrip.subgraphs.get(id) {
                None => mismatches.push(format!("subgraph {id} missing in roundtrip geometry")),
                Some(r_sg) => {
                    if !floats_eq(d_sg.rect.x, r_sg.rect.x)
                        || !floats_eq(d_sg.rect.y, r_sg.rect.y)
                        || !floats_eq(d_sg.rect.width, r_sg.rect.width)
                        || !floats_eq(d_sg.rect.height, r_sg.rect.height)
                    {
                        mismatches.push(format!("subgraph {id} geometry differs"));
                    }
                }
            }
        }
    }

    // Compare bounds
    if !floats_eq(direct.bounds.width, roundtrip.bounds.width)
        || !floats_eq(direct.bounds.height, roundtrip.bounds.height)
    {
        mismatches.push(format!(
            "bounds: ({:.1}x{:.1}) vs ({:.1}x{:.1})",
            direct.bounds.width,
            direct.bounds.height,
            roundtrip.bounds.width,
            roundtrip.bounds.height
        ));
    }

    mismatches
}

/// Run a full conformance case for a flowchart fixture.
fn run_flowchart_conformance(name: &str) -> ConformanceReport {
    let input = fixture_input("flowchart", name);

    // Direct path: parse → build → Diagram
    let direct_diagram = {
        let fc = mmdflux::parse_flowchart(&input).unwrap();
        mmdflux::build_diagram(&fc)
    };

    // MMDS roundtrip: parse → build → layout → JSON → hydrate → Diagram
    let roundtrip_diagram = {
        let mut instance = FlowchartInstance::new();
        instance.parse(&input).unwrap();
        let json = instance
            .render(OutputFormat::Mmds, &RenderConfig::default())
            .unwrap();
        from_mmds_str(&json).unwrap()
    };

    ConformanceReport {
        fixture_path: format!("flowchart/{name}"),
        semantic: check_semantic(&direct_diagram, &roundtrip_diagram),
        layout: check_layout(&direct_diagram, &roundtrip_diagram),
        visual: check_visual(&direct_diagram, &roundtrip_diagram),
    }
}

/// Run a full conformance case for a class diagram fixture.
fn run_class_conformance(name: &str) -> ConformanceReport {
    use mmdflux::diagrams::class::parser::parse_class_diagram;
    use mmdflux::diagrams::class::{ClassInstance, compiler};

    let input = fixture_input("class", name);

    // Direct path: parse → compile → Diagram
    let direct_diagram = {
        let model = parse_class_diagram(&input).unwrap();
        compiler::compile(&model)
    };

    // MMDS roundtrip: parse → compile → layout → JSON → hydrate → Diagram
    let roundtrip_diagram = {
        let mut instance = ClassInstance::new();
        instance.parse(&input).unwrap();
        let json = instance
            .render(OutputFormat::Mmds, &RenderConfig::default())
            .unwrap();
        from_mmds_str(&json).unwrap()
    };

    ConformanceReport {
        fixture_path: format!("class/{name}"),
        semantic: check_semantic(&direct_diagram, &roundtrip_diagram),
        layout: check_layout(&direct_diagram, &roundtrip_diagram),
        visual: check_visual(&direct_diagram, &roundtrip_diagram),
    }
}

fn assert_all_tiers_pass_for_fixture(fixture: &str, report: &ConformanceReport) {
    assert!(
        report.semantic.status.is_pass(),
        "{fixture} semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.layout.status.is_pass(),
        "{fixture} layout: {:?}",
        report.layout.status
    );
    assert!(
        report.visual.status.is_pass(),
        "{fixture} visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Conformance report assertions
// ---------------------------------------------------------------------------

#[test]
fn conformance_report_has_three_tiers() {
    let report = run_flowchart_conformance("simple.mmd");
    assert_eq!(report.tiers().len(), 3);
    assert_eq!(report.tiers()[0].tier, "semantic");
    assert_eq!(report.tiers()[1].tier, "layout");
    assert_eq!(report.tiers()[2].tier, "visual");
}

#[test]
fn conformance_report_contains_fixture_path() {
    let report = run_flowchart_conformance("simple.mmd");
    assert!(report.fixture_path.ends_with("simple.mmd"));
}

// ---------------------------------------------------------------------------
// Flowchart conformance: basic fixtures
// ---------------------------------------------------------------------------

#[test]
fn flowchart_simple_all_tiers_pass() {
    let report = run_flowchart_conformance("simple.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.layout.status.is_pass(),
        "layout: {:?}",
        report.layout.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_chain_all_tiers_pass() {
    let report = run_flowchart_conformance("chain.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_decision_all_tiers_pass() {
    let report = run_flowchart_conformance("decision.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_labeled_edges_all_tiers_pass() {
    let report = run_flowchart_conformance("labeled_edges.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_edge_styles_all_tiers_pass() {
    let report = run_flowchart_conformance("edge_styles.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Flowchart conformance: direction variants
// ---------------------------------------------------------------------------

#[test]
fn flowchart_left_right_all_tiers_pass() {
    let report = run_flowchart_conformance("left_right.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_right_left_all_tiers_pass() {
    let report = run_flowchart_conformance("right_left.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_bottom_top_all_tiers_pass() {
    let report = run_flowchart_conformance("bottom_top.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Flowchart conformance: subgraphs
// ---------------------------------------------------------------------------

#[test]
fn flowchart_simple_subgraph_all_tiers_pass() {
    let report = run_flowchart_conformance("simple_subgraph.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_subgraph_edges_all_tiers_pass() {
    let report = run_flowchart_conformance("subgraph_edges.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_nested_subgraph_all_tiers_pass() {
    let report = run_flowchart_conformance("nested_subgraph.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_nested_subgraph_only_all_tiers_pass_after_hardening() {
    let fixture = "nested_subgraph_only.mmd";
    let report = run_flowchart_conformance(fixture);
    assert_all_tiers_pass_for_fixture(fixture, &report);
}

#[test]
fn flowchart_external_node_subgraph_all_tiers_pass_after_hardening() {
    let fixture = "external_node_subgraph.mmd";
    let report = run_flowchart_conformance(fixture);
    assert_all_tiers_pass_for_fixture(fixture, &report);
}

// ---------------------------------------------------------------------------
// Flowchart conformance: cycles and backward edges
// ---------------------------------------------------------------------------

#[test]
fn flowchart_simple_cycle_all_tiers_pass() {
    let report = run_flowchart_conformance("simple_cycle.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Flowchart conformance: complex fixtures
// ---------------------------------------------------------------------------

#[test]
fn flowchart_complex_all_tiers_pass() {
    let report = run_flowchart_conformance("complex.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

#[test]
fn flowchart_shapes_all_tiers_pass() {
    let report = run_flowchart_conformance("shapes.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Conformance summary (for CI log output)
// ---------------------------------------------------------------------------

#[test]
fn conformance_summary_reports_tier_counts() {
    let mut fc_pass = [0usize; 3]; // semantic, layout, visual
    let mut fc_total = 0;
    let mut fc_failures: Vec<String> = Vec::new();

    for fixture in FLOWCHART_CONFORMANCE_MATRIX {
        let report = run_flowchart_conformance(fixture);
        fc_total += 1;
        if report.semantic.status.is_pass() {
            fc_pass[0] += 1;
        } else {
            fc_failures.push(format!(
                "  {fixture}: semantic {:?}",
                report.semantic.status
            ));
        }
        if report.layout.status.is_pass() {
            fc_pass[1] += 1;
        } else {
            fc_failures.push(format!("  {fixture}: layout {:?}", report.layout.status));
        }
        if report.visual.status.is_pass() {
            fc_pass[2] += 1;
        } else {
            fc_failures.push(format!("  {fixture}: visual {:?}", report.visual.status));
        }
    }

    // Class fixtures
    let class_report = run_class_conformance("simple.mmd");
    let class_pass = [
        usize::from(class_report.semantic.status.is_pass()),
        usize::from(class_report.layout.status.is_pass()),
        usize::from(class_report.visual.status.is_pass()),
    ];

    // Print structured summary for CI
    let tiers = ["Semantic", "Layout", "Visual"];
    eprintln!();
    eprintln!("╔══════════════════════════════════════════════╗");
    eprintln!("║        MMDS Conformance Summary              ║");
    eprintln!("╠══════════════════════════════════════════════╣");
    eprintln!("║ Tier     │ Flowchart     │ Class             ║");
    eprintln!("╟──────────┼───────────────┼───────────────────╢");
    for (i, tier) in tiers.iter().enumerate() {
        eprintln!(
            "║ {:<8} │ {:>2}/{:<2} ({:>3}%)  │ {}/1 ({:>3}%)          ║",
            tier,
            fc_pass[i],
            fc_total,
            fc_pass[i] * 100 / fc_total,
            class_pass[i],
            class_pass[i] * 100,
        );
    }
    eprintln!("╚══════════════════════════════════════════════╝");

    if !fc_failures.is_empty() {
        eprintln!("\nFailures:");
        for f in &fc_failures {
            eprintln!("{f}");
        }
    }

    // All fixtures in the main matrix should pass all tiers
    assert_eq!(
        fc_pass[0], fc_total,
        "semantic tier should have 100% pass rate"
    );
    assert_eq!(
        fc_pass[1], fc_total,
        "layout tier should have 100% pass rate"
    );
    assert_eq!(
        fc_pass[2], fc_total,
        "visual tier should have 100% pass rate"
    );
}

// ---------------------------------------------------------------------------
// Class diagram conformance
// ---------------------------------------------------------------------------

#[test]
fn class_simple_all_tiers_pass() {
    let report = run_class_conformance("simple.mmd");
    assert!(
        report.semantic.status.is_pass(),
        "semantic: {:?}",
        report.semantic.status
    );
    assert!(
        report.visual.status.is_pass(),
        "visual: {:?}",
        report.visual.status
    );
}

// ---------------------------------------------------------------------------
// Fixture matrix: broad coverage across flowchart fixtures
// ---------------------------------------------------------------------------

/// Flowchart fixtures expected to pass all three conformance tiers.
const FLOWCHART_CONFORMANCE_MATRIX: &[&str] = &[
    "simple.mmd",
    "chain.mmd",
    "decision.mmd",
    "shapes.mmd",
    "edge_styles.mmd",
    "labeled_edges.mmd",
    "left_right.mmd",
    "right_left.mmd",
    "bottom_top.mmd",
    "fan_in.mmd",
    "fan_out.mmd",
    "simple_cycle.mmd",
    "multiple_cycles.mmd",
    "simple_subgraph.mmd",
    "subgraph_edges.mmd",
    "nested_subgraph.mmd",
    "multi_subgraph.mmd",
    "complex.mmd",
    "ampersand.mmd",
    "diamond_fan.mmd",
    "self_loop.mmd",
    "bidirectional.mmd",
    "cross_circle_arrows.mmd",
    "subgraph_as_node_edge.mmd",
    "subgraph_to_subgraph_edge.mmd",
    "nested_subgraph_only.mmd",
    "external_node_subgraph.mmd",
    "inline_edge_labels.mmd",
    "fan_in_lr.mmd",
    "double_skip.mmd",
    "http_request.mmd",
    "ci_pipeline.mmd",
];

#[test]
fn flowchart_matrix_semantic_tier() {
    let mut failures = Vec::new();
    for fixture in FLOWCHART_CONFORMANCE_MATRIX {
        let report = run_flowchart_conformance(fixture);
        if !report.semantic.status.is_pass() {
            failures.push(format!("{}: {:?}", fixture, report.semantic.status));
        }
    }
    assert!(
        failures.is_empty(),
        "semantic tier failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn flowchart_matrix_layout_tier() {
    let mut failures = Vec::new();
    for fixture in FLOWCHART_CONFORMANCE_MATRIX {
        let report = run_flowchart_conformance(fixture);
        if !report.layout.status.is_pass() {
            failures.push(format!("{}: {:?}", fixture, report.layout.status));
        }
    }
    assert!(
        failures.is_empty(),
        "layout tier failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn flowchart_matrix_visual_tier() {
    let mut failures = Vec::new();
    for fixture in FLOWCHART_CONFORMANCE_MATRIX {
        let report = run_flowchart_conformance(fixture);
        if !report.visual.status.is_pass() {
            failures.push(format!("{}: {:?}", fixture, report.visual.status));
        }
    }
    assert!(
        failures.is_empty(),
        "visual tier failures:\n{}",
        failures.join("\n")
    );
}
