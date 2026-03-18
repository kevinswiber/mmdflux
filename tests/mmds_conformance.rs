//! MMDS conformance harness.
//!
//! Tiered conformance checks comparing the direct render pipeline
//! (Mermaid text → Diagram → render) against the MMDS roundtrip pipeline
//! (Mermaid text → Diagram → MMDS JSON → hydrate → Diagram → render).
//!
//! Three tiers:
//! - **Semantic**: graph structure equivalence (nodes, edges, subgraphs, direction)
//! - **Layout**: MMDS geometry export equivalence (node positions, edge topology)
//! - **Visual**: rendered text output equivalence

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::graph::Graph;
use mmdflux::mmds::{
    Bounds, Edge, Node, Output, Port, Position, Subgraph, from_str, generate_mermaid_from_str,
    parse_input,
};
use mmdflux::payload::Diagram as Payload;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

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
fn check_semantic(direct: &Graph, roundtrip: &Graph) -> TierResult {
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
                    if d_sg.id != r_sg.id
                        || d_sg.title != r_sg.title
                        || d_sg.parent != r_sg.parent
                        || d_sg.dir != r_sg.dir
                        || direct_children != roundtrip_children
                    {
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

fn compare_visual_outputs(
    direct_text: String,
    roundtrip_text: String,
    direct_svg: String,
    roundtrip_svg: String,
) -> TierResult {
    let mut mismatches = Vec::new();
    if direct_text != roundtrip_text {
        mismatches.push("text output differs".to_string());
    }
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

/// Compare direct Mermaid rendering against a generated Mermaid roundtrip.
fn check_visual_generated_mermaid(input: &str, roundtrip_input: &str) -> TierResult {
    let direct_text = render_diagram(input, OutputFormat::Text, &RenderConfig::default())
        .expect("direct text render should succeed");
    let roundtrip_text = render_diagram(
        roundtrip_input,
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .expect("roundtrip text render should succeed");
    let direct_svg = render_diagram(input, OutputFormat::Svg, &RenderConfig::default())
        .expect("direct SVG render should succeed");
    let roundtrip_svg =
        render_diagram(roundtrip_input, OutputFormat::Svg, &RenderConfig::default())
            .expect("roundtrip SVG render should succeed");

    compare_visual_outputs(direct_text, roundtrip_text, direct_svg, roundtrip_svg)
}

/// Ensure MMDS replay can emit both text and SVG for diagrams whose generated
/// Mermaid roundtrip is not yet supported.
fn check_visual_replay_smoke(mmds_json: &str) -> TierResult {
    let text =
        match mmdflux::render_diagram(mmds_json, OutputFormat::Text, &RenderConfig::default()) {
            Ok(text) => text,
            Err(error) => {
                return TierResult {
                    tier: "visual",
                    status: TierStatus::Fail(error.message),
                };
            }
        };
    let svg = match mmdflux::render_diagram(mmds_json, OutputFormat::Svg, &RenderConfig::default())
    {
        Ok(svg) => svg,
        Err(error) => {
            return TierResult {
                tier: "visual",
                status: TierStatus::Fail(error.message),
            };
        }
    };

    if text.is_empty() {
        return TierResult {
            tier: "visual",
            status: TierStatus::Fail("text replay output is empty".to_string()),
        };
    }
    if !svg.contains("<svg") {
        return TierResult {
            tier: "visual",
            status: TierStatus::Fail("svg replay output is missing <svg".to_string()),
        };
    }

    TierResult {
        tier: "visual",
        status: TierStatus::Pass,
    }
}

/// Float tolerance for geometry comparison.
const GEOMETRY_TOLERANCE: f64 = 0.01;

fn floats_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < GEOMETRY_TOLERANCE
}

/// Compare layout geometry for equivalence.
///
/// Compares direct routed MMDS output against MMDS regenerated Mermaid rerender.
fn check_layout(direct: &Output, roundtrip: &Output) -> TierResult {
    let mismatches = compare_layout_payloads(direct, roundtrip);

    TierResult {
        tier: "layout",
        status: if mismatches.is_empty() {
            TierStatus::Pass
        } else {
            TierStatus::Fail(mismatches.join("; "))
        },
    }
}

fn compare_layout_payloads(direct: &Output, roundtrip: &Output) -> Vec<String> {
    let mut mismatches = Vec::new();

    if direct.metadata.direction != roundtrip.metadata.direction {
        mismatches.push(format!(
            "direction: {} vs {}",
            direct.metadata.direction, roundtrip.metadata.direction
        ));
    }
    if direct.geometry_level != roundtrip.geometry_level {
        mismatches.push(format!(
            "geometry_level: {} vs {}",
            direct.geometry_level, roundtrip.geometry_level
        ));
    }
    if !bounds_eq(&direct.metadata.bounds, &roundtrip.metadata.bounds) {
        mismatches.push(format!(
            "bounds: ({:.1}x{:.1}) vs ({:.1}x{:.1})",
            direct.metadata.bounds.width,
            direct.metadata.bounds.height,
            roundtrip.metadata.bounds.width,
            roundtrip.metadata.bounds.height
        ));
    }

    let direct_nodes = sorted_nodes(direct);
    let roundtrip_nodes = sorted_nodes(roundtrip);
    if direct_nodes.len() != roundtrip_nodes.len() {
        mismatches.push(format!(
            "node count: {} vs {}",
            direct_nodes.len(),
            roundtrip_nodes.len()
        ));
    } else {
        for (direct_node, roundtrip_node) in direct_nodes.iter().zip(&roundtrip_nodes) {
            if direct_node.id != roundtrip_node.id {
                mismatches.push(format!(
                    "node id mismatch: {} vs {}",
                    direct_node.id, roundtrip_node.id
                ));
                continue;
            }
            if !position_eq(&direct_node.position, &roundtrip_node.position) {
                mismatches.push(format!(
                    "node {} position: ({:.1},{:.1}) vs ({:.1},{:.1})",
                    direct_node.id,
                    direct_node.position.x,
                    direct_node.position.y,
                    roundtrip_node.position.x,
                    roundtrip_node.position.y
                ));
            }
            if !size_eq(&direct_node.size, &roundtrip_node.size) {
                mismatches.push(format!(
                    "node {} size: ({:.1}x{:.1}) vs ({:.1}x{:.1})",
                    direct_node.id,
                    direct_node.size.width,
                    direct_node.size.height,
                    roundtrip_node.size.width,
                    roundtrip_node.size.height
                ));
            }
        }
    }

    let direct_edges = sorted_edges(direct);
    let roundtrip_edges = sorted_edges(roundtrip);
    if direct_edges.len() != roundtrip_edges.len() {
        mismatches.push(format!(
            "edge count: {} vs {}",
            direct_edges.len(),
            roundtrip_edges.len()
        ));
    } else {
        for (direct_edge, roundtrip_edge) in direct_edges.iter().zip(&roundtrip_edges) {
            if direct_edge.id != roundtrip_edge.id {
                mismatches.push(format!(
                    "edge id mismatch: {} vs {}",
                    direct_edge.id, roundtrip_edge.id
                ));
                continue;
            }
            if direct_edge.source != roundtrip_edge.source
                || direct_edge.target != roundtrip_edge.target
            {
                mismatches.push(format!(
                    "edge {} endpoints: {}->{} vs {}->{}",
                    direct_edge.id,
                    direct_edge.source,
                    direct_edge.target,
                    roundtrip_edge.source,
                    roundtrip_edge.target
                ));
            }
            if !path_eq(direct_edge.path.as_deref(), roundtrip_edge.path.as_deref()) {
                mismatches.push(format!("edge {} path differs", direct_edge.id));
            }
            if !optional_position_eq(
                direct_edge.label_position.as_ref(),
                roundtrip_edge.label_position.as_ref(),
            ) {
                mismatches.push(format!("edge {} label position differs", direct_edge.id));
            }
            if !optional_port_eq(
                direct_edge.source_port.as_ref(),
                roundtrip_edge.source_port.as_ref(),
            ) {
                mismatches.push(format!("edge {} source port differs", direct_edge.id));
            }
            if !optional_port_eq(
                direct_edge.target_port.as_ref(),
                roundtrip_edge.target_port.as_ref(),
            ) {
                mismatches.push(format!("edge {} target port differs", direct_edge.id));
            }
        }
    }

    let direct_subgraphs = sorted_subgraphs(direct);
    let roundtrip_subgraphs = sorted_subgraphs(roundtrip);
    if direct_subgraphs.len() != roundtrip_subgraphs.len() {
        mismatches.push(format!(
            "subgraph count: {} vs {}",
            direct_subgraphs.len(),
            roundtrip_subgraphs.len()
        ));
    } else {
        for (direct_subgraph, roundtrip_subgraph) in
            direct_subgraphs.iter().zip(&roundtrip_subgraphs)
        {
            let direct_id = normalize_roundtrip_identifier(&direct_subgraph.id);
            let roundtrip_id = normalize_roundtrip_identifier(&roundtrip_subgraph.id);
            if direct_id != roundtrip_id {
                mismatches.push(format!(
                    "subgraph id mismatch: {} vs {}",
                    direct_subgraph.id, roundtrip_subgraph.id
                ));
                continue;
            }
            let mut direct_children = direct_subgraph.children.clone();
            let mut roundtrip_children = roundtrip_subgraph.children.clone();
            direct_children.sort();
            roundtrip_children.sort();
            if direct_children != roundtrip_children {
                mismatches.push(format!("subgraph {} children differ", direct_subgraph.id));
            }
            if direct_subgraph.title != roundtrip_subgraph.title {
                mismatches.push(format!("subgraph {} title differs", direct_subgraph.id));
            }
            if direct_subgraph.direction != roundtrip_subgraph.direction {
                mismatches.push(format!("subgraph {} direction differs", direct_subgraph.id));
            }
            if direct_subgraph
                .parent
                .as_deref()
                .map(normalize_roundtrip_identifier)
                != roundtrip_subgraph
                    .parent
                    .as_deref()
                    .map(normalize_roundtrip_identifier)
            {
                mismatches.push(format!("subgraph {} parent differs", direct_subgraph.id));
            }
            if !optional_bounds_eq(
                direct_subgraph.bounds.as_ref(),
                roundtrip_subgraph.bounds.as_ref(),
            ) {
                mismatches.push(format!("subgraph {} bounds differ", direct_subgraph.id));
            }
        }
    }

    mismatches
}

fn position_eq(left: &Position, right: &Position) -> bool {
    floats_eq(left.x, right.x) && floats_eq(left.y, right.y)
}

fn size_eq(left: &mmdflux::mmds::Size, right: &mmdflux::mmds::Size) -> bool {
    floats_eq(left.width, right.width) && floats_eq(left.height, right.height)
}

fn bounds_eq(left: &Bounds, right: &Bounds) -> bool {
    floats_eq(left.width, right.width) && floats_eq(left.height, right.height)
}

fn optional_bounds_eq(left: Option<&Bounds>, right: Option<&Bounds>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => bounds_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn optional_position_eq(left: Option<&Position>, right: Option<&Position>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => position_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn optional_port_eq(left: Option<&Port>, right: Option<&Port>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.face == right.face
                && left.group_size == right.group_size
                && floats_eq(left.fraction, right.fraction)
                && position_eq(&left.position, &right.position)
        }
        (None, None) => true,
        _ => false,
    }
}

fn path_eq(left: Option<&[[f64; 2]]>, right: Option<&[[f64; 2]]>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left_point, right_point)| {
                    floats_eq(left_point[0], right_point[0])
                        && floats_eq(left_point[1], right_point[1])
                })
        }
        (None, None) => true,
        _ => false,
    }
}

fn normalize_roundtrip_identifier(value: &str) -> String {
    value.replace('-', "_")
}

fn sorted_nodes(output: &Output) -> Vec<&Node> {
    let mut nodes: Vec<_> = output.nodes.iter().collect();
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    nodes
}

fn sorted_edges(output: &Output) -> Vec<&Edge> {
    let mut edges: Vec<_> = output.edges.iter().collect();
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    edges
}

fn sorted_subgraphs(output: &Output) -> Vec<&Subgraph> {
    let mut subgraphs: Vec<_> = output.subgraphs.iter().collect();
    subgraphs.sort_by(|left, right| {
        left.title.cmp(&right.title).then(
            normalize_roundtrip_identifier(&left.id)
                .cmp(&normalize_roundtrip_identifier(&right.id)),
        )
    });
    subgraphs
}

fn flowchart_visual_uses_replay_smoke(name: &str) -> bool {
    matches!(
        name,
        "subgraph_as_node_edge.mmd" | "subgraph_to_subgraph_edge.mmd"
    )
}

fn prepare_graph_diagram(diagram_id: &str, input: &str) -> Graph {
    let payload = default_registry()
        .create(diagram_id)
        .unwrap_or_else(|| panic!("missing registry implementation for {diagram_id}"))
        .parse(input)
        .unwrap_or_else(|error| panic!("failed to parse {diagram_id} input: {error}"))
        .into_payload()
        .unwrap_or_else(|error| panic!("failed to build {diagram_id} payload: {error}"));
    match payload {
        Payload::Flowchart(graph) | Payload::Class(graph) => graph,
        _ => panic!("{diagram_id} input should build a graph payload"),
    }
}

fn render_mmds_output(input: &str, config: &RenderConfig) -> (String, Output) {
    let json =
        render_diagram(input, OutputFormat::Mmds, config).expect("MMDS render should succeed");
    let output = parse_input(&json).expect("MMDS output should parse");
    (json, output)
}

fn rerender_mmds_output_from_generated_mermaid(mmds_json: &str, config: &RenderConfig) -> Output {
    let generated =
        generate_mermaid_from_str(mmds_json).expect("MMDS Mermaid generation should succeed");
    let rerendered = render_diagram(&generated, OutputFormat::Mmds, config)
        .expect("generated Mermaid should render back to MMDS");
    parse_input(&rerendered).expect("rerendered MMDS should parse")
}

/// Run a full conformance case for a flowchart fixture.
fn run_flowchart_conformance(name: &str) -> ConformanceReport {
    let input = fixture_input("flowchart", name);
    let mmds_config = RenderConfig::default();

    let direct_diagram = prepare_graph_diagram("flowchart", &input);
    let (mmds_json, direct_output) = render_mmds_output(&input, &mmds_config);
    let generated = generate_mermaid_from_str(&mmds_json).unwrap();
    let roundtrip_diagram = from_str(&mmds_json).unwrap();
    let roundtrip_output = rerender_mmds_output_from_generated_mermaid(&mmds_json, &mmds_config);

    ConformanceReport {
        fixture_path: format!("flowchart/{name}"),
        semantic: check_semantic(&direct_diagram, &roundtrip_diagram),
        layout: check_layout(&direct_output, &roundtrip_output),
        visual: if flowchart_visual_uses_replay_smoke(name) {
            check_visual_replay_smoke(&mmds_json)
        } else {
            check_visual_generated_mermaid(&input, &generated)
        },
    }
}

/// Run a full conformance case for a class diagram fixture.
fn run_class_conformance(name: &str) -> ConformanceReport {
    let input = fixture_input("class", name);

    let direct_diagram = prepare_graph_diagram("class", &input);
    let (mmds_json, _) = render_mmds_output(&input, &RenderConfig::default());
    let roundtrip_diagram = from_str(&mmds_json).unwrap();

    ConformanceReport {
        fixture_path: format!("class/{name}"),
        semantic: check_semantic(&direct_diagram, &roundtrip_diagram),
        layout: TierResult {
            tier: "layout",
            status: TierStatus::Pass,
        },
        visual: check_visual_replay_smoke(&mmds_json),
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
