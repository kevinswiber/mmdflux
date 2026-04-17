//! MMDS JSON contract tests.
//!
//! Verifies that `--format mmds` output (with `json` alias) matches the MMDS specification:
//! - Default output is `geometry_level: "layout"` with no edge geometry.
//! - Routed output is explicit opt-in with edge paths and bounds.

use std::path::Path;

use mmdflux::graph::GeometryLevel;
use mmdflux::mmds::{
    Edge as MmdsEdge, Output, Rect as MmdsRect, SUPPORTED_PROFILES, evaluate_profiles, parse_input,
};
use mmdflux::simplification::PathSimplification;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, TextColorMode};
use serde_json::Value;

const STYLED_MMDS_LAYOUT: &str = r##"{
  "version": 1,
  "profiles": ["mmds-core-v1", "mmdflux-node-style-v1"],
  "extensions": {
    "org.mmdflux.node-style.v1": {
      "nodes": {
        "A": {
          "fill": "#ffeeaa",
          "stroke": "#333",
          "color": "#111"
        }
      }
    }
  },
  "defaults": {
    "node": { "shape": "rectangle" },
    "edge": {
      "stroke": "solid",
      "arrow_start": "none",
      "arrow_end": "normal",
      "minlen": 1
    }
  },
  "geometry_level": "layout",
  "metadata": {
    "diagram_type": "flowchart",
    "direction": "TD",
    "bounds": { "width": 120.0, "height": 200.0 }
  },
  "nodes": [
    {
      "id": "A",
      "label": "Alpha",
      "position": { "x": 60.0, "y": 35.0 },
      "size": { "width": 99.16, "height": 54.0 }
    },
    {
      "id": "B",
      "label": "Beta",
      "position": { "x": 60.0, "y": 139.0 },
      "size": { "width": 88.0, "height": 54.0 }
    }
  ],
  "edges": [
    { "id": "e0", "source": "A", "target": "B" }
  ]
}"##;

fn flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read flowchart fixture {}: {e}", path.display()))
}

fn render_json(input: &str) -> String {
    render_json_with_config(input, &RenderConfig::default())
}

fn render_json_with_level(input: &str, level: GeometryLevel) -> String {
    let config = RenderConfig {
        geometry_level: level,
        ..RenderConfig::default()
    };
    render_json_with_config(input, &config)
}

fn render_routed_mmds_with_engine(input: &str, engine: &str) -> String {
    render_json_with_config(
        input,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: EngineAlgorithmId::parse(engine).ok(),
            ..RenderConfig::default()
        },
    )
}

fn render_json_with_config(input: &str, config: &RenderConfig) -> String {
    mmdflux::render_diagram(input, OutputFormat::Mmds, config).unwrap()
}

fn render_mmds_input(input: &str, format: OutputFormat, config: RenderConfig) -> String {
    mmdflux::render_diagram(input, format, &config).unwrap()
}

fn mmds_fixture(path: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(path);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("invalid fixture JSON: {err}"))
}

fn mmds_profile_fixture(name: &str) -> Value {
    mmds_fixture(&format!("profiles/{name}"))
}

fn mmds_profile_fixture_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join("profiles")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read profile fixture {}: {err}", path.display()))
}

fn mmds_contract_fixture_text(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join("contracts")
        .join(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read contract fixture {}: {err}", path.display()))
}

fn mmds_contract_fixture(path: &str) -> Value {
    mmds_fixture(&format!("contracts/{path}"))
}

fn assert_matches_contract_fixture(actual: &str, fixture_path: &str) {
    let actual: Value = serde_json::from_str(actual).expect("actual MMDS should be valid JSON");
    let expected = mmds_contract_fixture(fixture_path);
    assert_eq!(
        actual, expected,
        "MMDS output drifted from locked contract fixture {fixture_path}"
    );
}

fn mmds_schema_validator() -> jsonschema::Validator {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("mmds.schema.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read schema {}: {err}", path.display()));
    let schema: Value =
        serde_json::from_str(&raw).unwrap_or_else(|err| panic!("invalid schema JSON: {err}"));
    jsonschema::validator_for(&schema).expect("schema should compile")
}

fn assert_schema_valid(payload: Value) {
    let validator = mmds_schema_validator();
    let errors: Vec<String> = validator
        .iter_errors(&payload)
        .map(|error| error.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "expected schema-valid payload; errors: {errors:?}"
    );
}

fn assert_schema_invalid(payload: Value) {
    let validator = mmds_schema_validator();
    let errors: Vec<String> = validator
        .iter_errors(&payload)
        .map(|error| error.to_string())
        .collect();
    assert!(
        !errors.is_empty(),
        "expected schema-invalid payload but it validated"
    );
}

#[test]
fn top_level_mmds_contract_helpers_parse_and_negotiate_shared_fixture_profiles() {
    let payload = mmds_contract_fixture_text("flowchart-simple.layout.json");

    let parsed = parse_input(&payload).expect("shared contract fixture should parse");
    let negotiation =
        evaluate_profiles(&payload).expect("shared contract fixture profile evaluation");

    assert_eq!(parsed.metadata.diagram_type, "flowchart");
    assert_eq!(
        negotiation.supported,
        vec!["mmds-core-v1".to_string(), "mmdflux-text-v1".to_string()]
    );
    assert_eq!(negotiation.unknown, Vec::<String>::new());
}

// -----------------------------------------------------------------------
// Contract: MMDS envelope
// -----------------------------------------------------------------------

#[test]
fn mmds_default_has_version_1() {
    let json = render_json("graph TD\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.version, 1);
}

#[test]
fn mmds_default_geometry_level_is_layout() {
    let json = render_json("graph TD\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, "layout");
}

#[test]
fn mmds_has_metadata_with_direction() {
    let json = render_json("graph LR\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.metadata.diagram_type, "flowchart");
    assert_eq!(output.metadata.direction, "LR");
}

#[test]
fn mmds_has_nodes_and_edges() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.nodes.len(), 2);
    assert_eq!(output.edges.len(), 1);
}

#[test]
fn mmds_output_emits_node_style_extension_when_styles_exist() {
    let json = render_json(&flowchart_fixture("style-basic.mmd"));
    let value: Value = serde_json::from_str(&json).unwrap();

    assert!(
        value["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile == "mmdflux-node-style-v1")
    );
    assert_eq!(
        value["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]["fill"],
        "#ffeeaa"
    );
    assert_eq!(
        value["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]["stroke"],
        "#333"
    );
    assert_eq!(
        value["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]["color"],
        "#111"
    );
    assert_schema_valid(value);
}

#[test]
fn mmds_output_omits_node_style_extension_when_styles_absent() {
    let json = render_json("graph TD\nA-->B");
    let value: Value = serde_json::from_str(&json).unwrap();

    assert!(!value["profiles"].as_array().is_some_and(|profiles| {
        profiles
            .iter()
            .any(|profile| profile == "mmdflux-node-style-v1")
    }));
    assert!(
        value
            .get("extensions")
            .and_then(|extensions| extensions.get("org.mmdflux.node-style.v1"))
            .is_none()
    );
}

#[test]
fn mmds_output_emits_grid_projection_extension_when_available() {
    let json = render_json("graph TD\nA-->B");
    let value: Value = serde_json::from_str(&json).unwrap();

    assert!(
        value["profiles"]
            .as_array()
            .is_some_and(|profiles| profiles.iter().any(|profile| profile == "mmdflux-text-v1"))
    );
    let projection = &value["extensions"]["org.mmdflux.render.text.v1"]["projection"];
    assert!(projection["node_ranks"].get("A").is_some());
    assert!(projection["node_ranks"].get("B").is_some());
    assert!(projection["edge_waypoints"].is_object());
    assert!(projection["label_positions"].is_object());
    assert_schema_valid(value);
}

#[test]
fn mmds_output_matches_locked_simple_contract_fixture() {
    let json = render_json(&flowchart_fixture("simple.mmd"));
    assert_matches_contract_fixture(&json, "flowchart-simple.layout.json");
}

#[test]
fn mmds_output_matches_locked_node_style_contract_fixture() {
    let json = render_json(&flowchart_fixture("style-basic.mmd"));
    assert_matches_contract_fixture(&json, "flowchart-style.layout.json");
}

#[test]
fn mmds_hydration_replays_node_styles_into_svg_and_text_rendering() {
    let svg = render_mmds_input(
        STYLED_MMDS_LAYOUT,
        OutputFormat::Svg,
        RenderConfig::default(),
    );
    let text = render_mmds_input(
        STYLED_MMDS_LAYOUT,
        OutputFormat::Text,
        RenderConfig {
            text_color_mode: TextColorMode::Ansi,
            ..RenderConfig::default()
        },
    );

    assert!(
        svg.contains("fill=\"#ffeeaa\""),
        "styled MMDS SVG fill missing: {svg}"
    );
    assert!(
        svg.contains("stroke=\"#333\""),
        "styled MMDS SVG stroke missing: {svg}"
    );
    assert!(
        svg.contains("fill=\"#111\">Alpha</text>"),
        "styled MMDS SVG label color missing: {svg}"
    );
    assert!(
        text.contains("Alpha"),
        "styled MMDS text label missing: {text}"
    );
    assert!(
        text.contains("\u{1b}["),
        "styled MMDS text ANSI missing: {text}"
    );
}

// -----------------------------------------------------------------------
// Contract: layout-level node geometry
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_nodes_have_positions_and_sizes() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: Output = serde_json::from_str(&json).unwrap();

    let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
    assert_eq!(node_a.label, "Start");
    assert_eq!(node_a.shape, "rectangle");
    assert!(node_a.size.width > 0.0);
    assert!(node_a.size.height > 0.0);
}

#[test]
fn mmds_layout_nodes_sorted_by_id() {
    let json = render_json("graph TD\nC-->B\nB-->A");
    let output: Output = serde_json::from_str(&json).unwrap();
    let ids: Vec<&str> = output.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn mmds_layout_node_shapes() {
    let json = render_json("graph TD\nA[Rect]\nB(Round)\nC{Diamond}\nD([Stadium])");
    let output: Output = serde_json::from_str(&json).unwrap();

    let shapes: std::collections::HashMap<String, String> = output
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.shape.clone()))
        .collect();
    assert_eq!(shapes["A"], "rectangle");
    assert_eq!(shapes["B"], "round");
    assert_eq!(shapes["C"], "diamond");
    assert_eq!(shapes["D"], "stadium");
}

#[test]
fn mmds_lossless_path_simplification_sits_between_none_and_lossy() {
    let input = flowchart_fixture("multi_subgraph_direction_override.mmd");
    let render_for = |path_simplification: PathSimplification| {
        render_json_with_config(
            &input,
            &RenderConfig {
                geometry_level: GeometryLevel::Routed,
                path_simplification,
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
    };

    let full = render_for(PathSimplification::None);
    let compact = render_for(PathSimplification::Lossless);
    let simplified = render_for(PathSimplification::Lossy);

    let full: Output = serde_json::from_str(&full).unwrap();
    let compact: Output = serde_json::from_str(&compact).unwrap();
    let simplified: Output = serde_json::from_str(&simplified).unwrap();

    let full_len = full
        .edges
        .iter()
        .find(|edge| edge.source == "Bmid" && edge.target == "F")
        .and_then(|edge| edge.path.as_ref())
        .map(std::vec::Vec::len)
        .unwrap();
    let compact_len = compact
        .edges
        .iter()
        .find(|edge| edge.source == "Bmid" && edge.target == "F")
        .and_then(|edge| edge.path.as_ref())
        .map(std::vec::Vec::len)
        .unwrap();
    let simplified_len = simplified
        .edges
        .iter()
        .find(|edge| edge.source == "Bmid" && edge.target == "F")
        .and_then(|edge| edge.path.as_ref())
        .map(std::vec::Vec::len)
        .unwrap();

    assert!(
        full_len >= compact_len,
        "compact should not increase waypoints: full={full_len}, compact={compact_len}"
    );
    assert!(
        compact_len >= simplified_len,
        "compact should preserve more structure than simplified: compact={compact_len}, simplified={simplified_len}"
    );
    assert_eq!(simplified_len, 3);
}

#[test]
fn routed_mmds_defaults_to_lossless_path_simplification() {
    let input = flowchart_fixture("multi_subgraph_direction_override.mmd");
    let render_for = |path_simplification: Option<PathSimplification>| {
        let mut config = RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        };
        if let Some(path_simplification) = path_simplification {
            config.path_simplification = path_simplification;
        }
        render_json_with_config(&input, &config)
    };
    let edge_len = |json: &str| {
        let output: Output = serde_json::from_str(json).unwrap();
        output
            .edges
            .iter()
            .find(|edge| edge.source == "Bmid" && edge.target == "F")
            .and_then(|edge| edge.path.as_ref())
            .map(std::vec::Vec::len)
            .unwrap()
    };

    let default = render_for(None);
    let lossless = render_for(Some(PathSimplification::Lossless));
    let full = render_for(Some(PathSimplification::None));
    let simplified = render_for(Some(PathSimplification::Lossy));
    let default_len = edge_len(&default);
    let lossless_len = edge_len(&lossless);
    let full_len = edge_len(&full);
    let simplified_len = edge_len(&simplified);

    assert_eq!(
        default_len, lossless_len,
        "default routed MMDS path detail should match lossless output"
    );
    assert!(
        default_len <= full_len,
        "lossless default should have no more points than full: default={default_len}, full={full_len}"
    );
    assert!(
        default_len >= simplified_len,
        "lossless default should not have fewer points than simplified: default={default_len}, simplified={simplified_len}"
    );
    if default_len == simplified_len {
        assert!(
            default_len <= 3,
            "default/simplified point counts should only match when the routed path is already minimal: default={default_len}, simplified={simplified_len}"
        );
    }
}

#[test]
fn path_simplification_monotonicity_holds_none_lossless_lossy() {
    let input = flowchart_fixture("multi_subgraph_direction_override.mmd");
    let render_for = |path_simplification: PathSimplification| {
        render_json_with_config(
            &input,
            &RenderConfig {
                geometry_level: GeometryLevel::Routed,
                path_simplification,
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
    };
    let edge_len = |json: &str| {
        let output: Output = serde_json::from_str(json).unwrap();
        output
            .edges
            .iter()
            .find(|edge| edge.source == "Bmid" && edge.target == "F")
            .and_then(|edge| edge.path.as_ref())
            .map(std::vec::Vec::len)
            .unwrap()
    };

    let full = edge_len(&render_for(PathSimplification::None));
    let compact = edge_len(&render_for(PathSimplification::Lossless));
    let simplified = edge_len(&render_for(PathSimplification::Lossy));

    assert!(
        full >= compact && compact >= simplified,
        "path-detail monotonicity violated: full={full}, compact={compact}, simplified={simplified}"
    );
}

// -----------------------------------------------------------------------
// Plan 0147 Task 3.1: Tier A bent paths round-trip byte-stable through MMDS
// -----------------------------------------------------------------------

#[test]
fn tier_a_bent_paths_preserved_in_mmds_routed_output() {
    // User RL repro: reciprocal labeled edges with a wrapped label.
    // Flux Bend emits two waypoints around the label node; the routed MMDS
    // output must carry them (path length >= 4 for labeled edges).
    let src = "graph RL\n    A -->|x| B\n    A -->|yes<br>no| B\n";
    let json = render_json_with_config(
        src,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );
    let output: Output = serde_json::from_str(&json).expect("routed MMDS parses");

    let labeled: Vec<&MmdsEdge> = output
        .edges
        .iter()
        .filter(|edge| edge.label.as_deref().is_some_and(|s| !s.is_empty()))
        .collect();
    assert!(!labeled.is_empty(), "expected labeled edges in MMDS output");
    for edge in labeled {
        let path = edge
            .path
            .as_ref()
            .unwrap_or_else(|| panic!("labeled edge {} missing path", edge.id));
        assert!(
            path.len() >= 4,
            "labeled edge {} lost bend waypoints: path={path:?}",
            edge.id,
        );
    }
}

#[test]
fn tier_a_bent_paths_round_trip_byte_stable_through_mmds() {
    let src = "graph RL\n    A -->|x| B\n    A -->|yes<br>no| B\n";
    let json1 = render_json_with_config(
        src,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    // parse → re-emit via the same serde path as the runtime, then parse again.
    let output1 = parse_input(&json1).expect("first parse");
    let json2 = serde_json::to_string_pretty(&output1).expect("re-emit");
    let output2 = parse_input(&json2).expect("second parse");

    // Semantic equality via JSON value comparison (ignores cosmetic whitespace).
    let v1: Value = serde_json::to_value(&output1).unwrap();
    let v2: Value = serde_json::to_value(&output2).unwrap();
    assert_eq!(v1, v2, "MMDS Output is not round-trip stable");

    // And the bend waypoints survive the round-trip per-edge.
    for edge in output2.edges.iter() {
        if edge.label.as_deref().is_some_and(|s| !s.is_empty()) {
            let path = edge
                .path
                .as_ref()
                .unwrap_or_else(|| panic!("labeled edge {} lost path", edge.id));
            assert!(
                path.len() >= 4,
                "labeled edge {} lost bend after round-trip: {path:?}",
                edge.id,
            );
        }
    }
}

#[test]
fn orthogonal_route_mmds_routed_output_is_deterministic_for_fixture_subset() {
    for fixture in [
        "simple.mmd",
        "decision.mmd",
        "fan_out.mmd",
        "subgraph_direction_cross_boundary.mmd",
        "multi_subgraph_direction_override.mmd",
    ] {
        let input = flowchart_fixture(fixture);
        let first = render_routed_mmds_with_engine(&input, "flux-layered");
        let second = render_routed_mmds_with_engine(&input, "flux-layered");
        assert_eq!(
            second, first,
            "orthogonal MMDS routed output is nondeterministic for fixture {fixture}"
        );
    }
}

// -----------------------------------------------------------------------
// Contract: MMDS coordinates are in SVG pixel space, not text-grid space
// -----------------------------------------------------------------------

#[test]
fn mmds_node_sizes_are_in_svg_pixel_dimensions() {
    // A rectangle node labeled "A" in text-grid space is ~5×3 characters.
    // In SVG pixel space it should be roughly 40-80px wide and 40-60px tall.
    // This test catches the bug where MMDS emits text-grid char dimensions
    // instead of pixel dimensions.
    let json = render_json("graph TD\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();

    let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
    assert!(
        node_a.size.width > 20.0,
        "node width {} should be in pixel space (>20px), not text-grid chars",
        node_a.size.width
    );
    assert!(
        node_a.size.height > 20.0,
        "node height {} should be in pixel space (>20px), not text-grid chars",
        node_a.size.height
    );
}

#[test]
fn mmds_routed_subgraph_bounds_are_reasonable() {
    // Subgraph bounds should tightly wrap their children.
    // A subgraph containing two nodes in TD layout should have a height
    // proportional to the content, not spanning the full diagram.
    let json = render_json_with_level(
        "graph TD\nsubgraph sg1[Group]\nA-->B\nend\nC-->A",
        GeometryLevel::Routed,
    );
    let output: Output = serde_json::from_str(&json).unwrap();

    let sg = &output.subgraphs[0];
    let bounds = sg
        .bounds
        .as_ref()
        .expect("routed subgraph should have bounds");
    let diagram_height = output.metadata.bounds.height;

    // Subgraph bounds height should be less than 80% of the total diagram height
    // (it contains 2 of the 3 nodes, so it shouldn't span the whole thing).
    assert!(
        bounds.height < diagram_height * 0.8,
        "subgraph height {} should be well under diagram height {} (not spanning full diagram)",
        bounds.height,
        diagram_height
    );
}

// -----------------------------------------------------------------------
// Contract: layout-level edges have NO geometry
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_edges_exclude_path() {
    let json = render_json("graph TD\nA-->B");
    assert!(
        !json.contains("\"path\""),
        "layout JSON must not contain path"
    );
}

#[test]
fn mmds_layout_edges_exclude_is_backward() {
    let json = render_json("graph TD\nA-->B\nB-->A");
    assert!(
        !json.contains("\"is_backward\""),
        "layout JSON must not contain is_backward"
    );
}

#[test]
fn mmds_layout_edges_exclude_label_position() {
    let json = render_json("graph TD\nA--label-->B");
    assert!(
        !json.contains("\"label_position\""),
        "layout JSON must not contain label_position"
    );
}

#[test]
fn mmds_layout_edges_have_topology() {
    let json = render_json("graph TD\nA-.label.->B");
    let output: Output = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert_eq!(edge.id, "e0");
    assert_eq!(edge.source, "A");
    assert_eq!(edge.target, "B");
    assert_eq!(edge.stroke, "dotted");
    assert_eq!(edge.label, Some("label".to_string()));
    assert_eq!(edge.arrow_start, "none");
    assert_eq!(edge.arrow_end, "normal");
}

#[test]
fn mmds_edge_serializes_optional_subgraph_endpoint_intent_for_subgraph_as_node_edges() {
    let input = flowchart_fixture("subgraph_as_node_edge.mmd");
    let json = render_json(&input);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edges = value["edges"].as_array().unwrap();

    let into_subgraph = edges
        .iter()
        .find(|edge| edge["source"] == "Client" && edge["target"] == "API")
        .expect("client -> api edge should exist");
    assert_eq!(into_subgraph["to_subgraph"], "sg1");

    let from_subgraph = edges
        .iter()
        .find(|edge| edge["source"] == "DB" && edge["target"] == "Logs")
        .expect("db -> logs edge should exist");
    assert_eq!(from_subgraph["from_subgraph"], "sg1");
}

#[test]
fn mmds_edge_serializes_optional_subgraph_endpoint_intent_for_subgraph_to_subgraph_edges() {
    let input = flowchart_fixture("subgraph_to_subgraph_edge.mmd");
    let json = render_json(&input);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edges = value["edges"].as_array().unwrap();

    let edge = edges
        .iter()
        .find(|edge| edge["source"] == "State" && edge["target"] == "API")
        .expect("state -> api edge should exist");
    assert_eq!(edge["from_subgraph"], "frontend");
    assert_eq!(edge["to_subgraph"], "backend");
}

#[test]
fn mmds_layout_metadata_includes_bounds() {
    let json = render_json("graph TD\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert!(output.metadata.bounds.width > 0.0);
    assert!(output.metadata.bounds.height > 0.0);
}

// -----------------------------------------------------------------------
// Contract: layout-level subgraphs
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_subgraphs() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let output: Output = serde_json::from_str(&json).unwrap();

    assert_eq!(output.subgraphs.len(), 1);
    assert_eq!(output.subgraphs[0].id, "sg1");
    assert_eq!(output.subgraphs[0].title, "Group");
    assert!(output.subgraphs[0].direction.is_none());
    assert!(output.subgraphs[0].bounds.is_none());
}

#[test]
fn mmds_layout_subgraph_direction_override() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\ndirection LR\nA-->B\nend");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.subgraphs[0].direction.as_deref(), Some("LR"));
}

// -----------------------------------------------------------------------
// Contract: routed-level output
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_has_geometry_level_routed() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, "routed");
}

#[test]
fn mmds_routed_includes_edge_paths() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert!(edge.path.is_some());
    assert!(edge.path.as_ref().unwrap().len() >= 2);
    assert!(edge.is_backward.is_some());
}

#[test]
fn mmds_routed_includes_metadata_bounds() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let bounds = &output.metadata.bounds;
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn mmds_routed_subgraph_bounds() {
    let json = render_json_with_level(
        "graph TD\nsubgraph sg1[Group]\nA-->B\nend",
        GeometryLevel::Routed,
    );
    let output: Output = serde_json::from_str(&json).unwrap();

    let sg = &output.subgraphs[0];
    assert!(sg.bounds.is_some());
    assert!(sg.bounds.as_ref().unwrap().width > 0.0);
}

#[test]
fn mmds_routed_label_position_for_labeled_edge() {
    let json = render_json_with_level("graph TD\nA--label-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert!(edge.label_position.is_some());
}

// -----------------------------------------------------------------------
// Contract: defaults + omission behavior
// -----------------------------------------------------------------------

#[test]
fn mmds_includes_defaults_block() {
    let json = render_json("graph TD\nA-->B");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["defaults"]["node"]["shape"], "rectangle");
    assert_eq!(value["defaults"]["edge"]["stroke"], "solid");
    assert_eq!(value["defaults"]["edge"]["arrow_start"], "none");
    assert_eq!(value["defaults"]["edge"]["arrow_end"], "normal");
    assert_eq!(value["defaults"]["edge"]["minlen"], 1);
}

#[test]
fn mmds_omits_default_edge_fields() {
    let json = render_json("graph TD\nA-->B");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edge = &value["edges"][0];
    assert!(edge.get("stroke").is_none());
    assert!(edge.get("arrow_start").is_none());
    assert!(edge.get("arrow_end").is_none());
    assert!(edge.get("minlen").is_none());
}

#[test]
fn mmds_keeps_non_default_edge_fields() {
    let json = render_json("graph TD\nA -.-> B\nC --x D\nE ----> F");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edges = value["edges"].as_array().unwrap();
    assert_eq!(edges[0]["stroke"], "dotted");
    assert_eq!(edges[1]["arrow_end"], "cross");
    assert!(edges[2]["minlen"].as_i64().unwrap() > 1);
}

#[test]
fn mmds_omits_default_node_shape() {
    let json = render_json("graph TD\nA[Rect]\nB(Round)");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = value["nodes"].as_array().unwrap();
    assert!(nodes[0].get("shape").is_none());
    assert_eq!(nodes[1]["shape"], "round");
}

#[test]
fn mmds_omits_empty_subgraphs() {
    let json = render_json("graph TD\nA-->B");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("subgraphs").is_none());
}

#[test]
fn mmds_keeps_subgraphs_when_present() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("subgraphs").is_some());
}

#[test]
fn mmds_routed_still_includes_paths() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edge = &value["edges"][0];
    assert!(edge.get("path").is_some());
}

#[test]
fn mmds_deserializes_with_defaults() {
    let json = render_json("graph TD\nA-->B");
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.nodes[0].shape, "rectangle");
    assert_eq!(output.edges[0].stroke, "solid");
    assert_eq!(output.edges[0].arrow_start, "none");
    assert_eq!(output.edges[0].arrow_end, "normal");
    assert_eq!(output.edges[0].minlen, 1);
    assert!(output.edges[0].from_subgraph.is_none());
    assert!(output.edges[0].to_subgraph.is_none());
    assert!(output.subgraphs.is_empty());
}

// -----------------------------------------------------------------------
// Contract: direction variants
// -----------------------------------------------------------------------

#[test]
fn mmds_direction_variants() {
    for (dir_str, expected) in [("TD", "TD"), ("LR", "LR"), ("BT", "BT"), ("RL", "RL")] {
        let input = format!("graph {dir_str}\nA-->B");
        let json = render_json(&input);
        let output: Output = serde_json::from_str(&json).unwrap();
        assert_eq!(output.metadata.direction, expected);
    }
}

// -----------------------------------------------------------------------
// Contract: class diagram MMDS output
// -----------------------------------------------------------------------

#[test]
fn mmds_class_diagram_produces_json() {
    let config = RenderConfig::default();
    let output = render_json_with_config("classDiagram\nA --> B", &config);
    let parsed: Output = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.geometry_level, "layout");
    assert_eq!(parsed.metadata.diagram_type, "class");
    assert!(!output.contains("\"path\""));
}

#[test]
fn mmds_class_diagram_routed_level() {
    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..RenderConfig::default()
    };
    let output = render_json_with_config("classDiagram\nA --> B", &config);
    let parsed: Output = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed.geometry_level, "routed");
    assert!(output.contains("\"path\""));
}

// -----------------------------------------------------------------------
// Schema and documentation artifacts
// -----------------------------------------------------------------------

#[test]
fn mmds_schema_exists_and_has_required_fields() {
    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    assert!(schema.contains("\"$schema\""));
    assert!(schema.contains("\"properties\""));
    assert!(schema.contains("\"geometry_level\""));
    assert!(schema.contains("\"layout\""));
    assert!(schema.contains("\"routed\""));
    assert!(schema.contains("\"from_subgraph\""));
    assert!(schema.contains("\"to_subgraph\""));
}

#[test]
fn mmds_schema_includes_port_definition() {
    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    let v: Value = serde_json::from_str(&schema).unwrap();
    let defs = v["$defs"].as_object().expect("schema should have $defs");
    let port = defs.get("Port").expect("schema should define Port");
    let required = port["required"].as_array().unwrap();
    let required_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required_strs.contains(&"face"));
    assert!(required_strs.contains(&"fraction"));
    assert!(required_strs.contains(&"position"));
    assert!(required_strs.contains(&"group_size"));
}

#[test]
fn mmds_routed_output_with_ports_validates_against_schema() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let payload: Value = serde_json::from_str(&json).unwrap();
    assert_schema_valid(payload);
}

#[test]
fn schema_accepts_profiles_and_namespaced_extensions() {
    let payload = mmds_profile_fixture("profiles-svg-v1.json");
    assert_schema_valid(payload);
}

#[test]
fn shared_mmds_profile_vocabulary_is_exported_from_contract_module() {
    assert_eq!(
        SUPPORTED_PROFILES,
        &[
            "mmds-core-v1",
            "mmdflux-svg-v1",
            "mmdflux-text-v1",
            "mmdflux-node-style-v1",
        ]
    );
}

#[test]
fn schema_rejects_invalid_extension_namespace_shape() {
    let payload = mmds_fixture("invalid/extensions-not-object.json");
    assert_schema_invalid(payload);
}

#[test]
fn mmds_profiles_and_extensions_roundtrip_through_serde() {
    let payload = mmds_profile_fixture_text("profiles-svg-v1.json");
    let parsed: Output = serde_json::from_str(&payload).unwrap();
    let json = serde_json::to_string(&parsed).unwrap();

    assert!(json.contains("profiles"));
    assert!(json.contains("org.mmdflux.render.svg.v1"));
}

#[test]
fn mmds_spec_doc_exists() {
    assert!(Path::new("docs/mmds.md").exists());
}

#[test]
fn rust_api_examples_exist() {
    assert!(Path::new("examples/high_level_render.rs").exists());
    assert!(Path::new("examples/registry_adapter.rs").exists());
    assert!(Path::new("examples/mmds_replay.rs").exists());
    assert!(!Path::new("examples/mmds").exists());
}

#[test]
fn readme_mentions_mmds() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    assert!(readme.contains("MMDS"));
}

#[test]
fn readme_describes_high_level_and_low_level_rust_api_tiers() {
    let readme = std::fs::read_to_string("README.md").unwrap();

    for required in [
        "render_diagram",
        "detect_diagram",
        "validate_diagram",
        "builtins::default_registry()",
        "`registry`",
        "`payload`",
        "`mmds`",
        "internal implementation modules",
    ] {
        assert!(
            readme.contains(required),
            "README should describe the narrowed Rust API tiers: {required}"
        );
    }
}

#[test]
fn canonical_profile_examples_validate_against_schema() {
    for profile in ["profiles-svg-v1.json", "profiles-text-v1.json"] {
        let raw = mmds_profile_fixture_text(profile);
        let payload: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("invalid profile fixture {profile}: {err}"));
        assert_schema_valid(payload);
    }
}

#[test]
fn docs_reference_initial_profile_set() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert!(docs.contains("mmds-core-v1"));
    assert!(docs.contains("mmdflux-svg-v1"));
    assert!(docs.contains("mmdflux-text-v1"));
}

#[test]
fn docs_and_schema_reference_node_style_extension_contract() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert!(docs.contains("mmdflux-node-style-v1"));
    assert!(docs.contains("org.mmdflux.node-style.v1"));

    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    assert!(schema.contains("org.mmdflux.node-style.v1"));
}

#[test]
fn docs_cover_live_style_scope_and_wasm_color_config() {
    let mmds_docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert!(!mmds_docs.contains("Style/class/link directives are out of scope"));
    assert!(mmds_docs.contains("Mermaid regeneration from MMDS does not yet emit style"));

    let wasm_docs = std::fs::read_to_string("docs/development/wasm.md").unwrap();
    assert!(wasm_docs.contains("color"));
    assert!(wasm_docs.contains("off"));
    assert!(wasm_docs.contains("auto"));
    assert!(wasm_docs.contains("always"));

    let readme = std::fs::read_to_string("README.md").unwrap();
    assert!(readme.contains("NO_COLOR=1 mmdflux --format text"));
    assert!(readme.contains("--color always"));
}

#[test]
fn mmds_docs_point_to_fixture_backed_examples_and_rust_replay_example() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();

    for required in [
        "examples/mmds_replay.rs",
        "tests/fixtures/mmds/generation/basic-flow.json",
        "tests/fixtures/mmds/positioned/routed-basic.json",
    ] {
        assert!(
            docs.contains(required),
            "MMDS docs should reference the current example/fixture path: {required}"
        );
    }

    assert!(
        !docs.contains("examples/mmds/"),
        "MMDS docs should not reference the deleted examples/mmds directory"
    );
}

// -----------------------------------------------------------------------
// Contract: port metadata (routed level only)
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_includes_port_metadata() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    assert!(
        edge.source_port.is_some(),
        "routed edge should have source_port"
    );
    assert!(
        edge.target_port.is_some(),
        "routed edge should have target_port"
    );
}

#[test]
fn mmds_routed_port_faces_correct_td() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    let sp = edge.source_port.as_ref().unwrap();
    let tp = edge.target_port.as_ref().unwrap();
    assert_eq!(sp.face, "bottom", "TD source should exit bottom");
    assert_eq!(tp.face, "top", "TD target should enter top");
}

#[test]
fn mmds_routed_port_fractions_fan_in() {
    let json = render_json_with_level("graph TD\nA-->C\nB-->C", GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    let e0 = output.edges.iter().find(|e| e.source == "A").unwrap();
    let e1 = output.edges.iter().find(|e| e.source == "B").unwrap();
    let f0 = e0.target_port.as_ref().unwrap().fraction;
    let f1 = e1.target_port.as_ref().unwrap().fraction;
    assert!(
        (f0 - f1).abs() > 1e-6,
        "fan-in edges should have distinct target fractions: {f0} vs {f1}"
    );
}

#[test]
fn mmds_layout_excludes_port_metadata() {
    let json = render_json("graph TD\nA-->B");
    assert!(
        !json.contains("source_port"),
        "layout JSON must not contain source_port"
    );
    assert!(
        !json.contains("target_port"),
        "layout JSON must not contain target_port"
    );
}

// --- Task 4.5: MMDS engine metadata ---

#[test]
fn mmds_routed_output_includes_engine_metadata() {
    let input = "graph TD\nA-->B";
    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let output = render_json_with_config(input, &config);
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(json["metadata"]["engine"], "flux-layered");
}

// ---------------------------------------------------------------------------
// Sequence MMDS contract tests
// ---------------------------------------------------------------------------

#[test]
fn sequence_diagram_mmds_output_is_valid_json() {
    let input = "sequenceDiagram\n    Alice->>Bob: hello";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["metadata"]["diagram_type"], "sequence");
    assert_eq!(json["version"], 1);
}

#[test]
fn sequence_mmds_has_participants_with_positions() {
    let input = "sequenceDiagram\n    Alice->>Bob: hello";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let participants = json["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 2);

    let alice = &participants[0];
    assert_eq!(alice["label"], "Alice");
    assert_eq!(alice["kind"], "participant");
    assert!(alice["position"]["x"].is_number());
    assert!(alice["position"]["y"].is_number());
    assert!(alice["size"]["width"].is_number());
    assert!(alice["lifeline_x"].is_number());
}

#[test]
fn sequence_mmds_has_messages_with_coordinates() {
    let input = "sequenceDiagram\n    Alice->>Bob: hello\n    Bob-->>Alice: world";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);

    let msg0 = &messages[0];
    assert_eq!(msg0["from"], 0);
    assert_eq!(msg0["to"], 1);
    assert_eq!(msg0["text"], "hello");
    assert_eq!(msg0["line_style"], "solid");
    assert_eq!(msg0["arrow_head"], "filled");
    assert!(msg0["y"].is_number());

    let msg1 = &messages[1];
    assert_eq!(msg1["line_style"], "dashed");
    assert_eq!(msg1["arrow_head"], "filled");
}

#[test]
fn sequence_mmds_uses_none_for_plain_messages() {
    let input = "sequenceDiagram\n    Alice->Bob: hello\n    Bob-->Alice: world";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let messages = json["messages"].as_array().unwrap();

    assert_eq!(messages[0]["line_style"], "solid");
    assert_eq!(messages[0]["arrow_head"], "none");
    assert_eq!(messages[1]["line_style"], "dashed");
    assert_eq!(messages[1]["arrow_head"], "none");
}

#[test]
fn sequence_mmds_has_notes() {
    let input = "sequenceDiagram\n    Alice->>Bob: hi\n    Note right of Bob: thinking";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let notes = json["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);

    let note = &notes[0];
    assert_eq!(note["placement"], "right_of");
    assert_eq!(note["text"], "thinking");
    assert!(note["position"]["x"].is_number());
    assert!(note["size"]["width"].is_number());
}

#[test]
fn sequence_mmds_has_activations() {
    let input = "sequenceDiagram\n    Alice->>+Bob: hello\n    Bob-->>-Alice: bye";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let activations = json["activations"].as_array().unwrap();
    assert!(!activations.is_empty());

    let act = &activations[0];
    assert!(act["participant"].is_number());
    assert!(act["y_start"].is_number());
    assert!(act["y_end"].is_number());
}

#[test]
fn sequence_mmds_has_blocks() {
    let input = "sequenceDiagram\n    Alice->>Bob: hi\n    loop Every minute\n        Bob->>Alice: ping\n    end";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let blocks = json["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);

    let block = &blocks[0];
    assert_eq!(block["kind"], "loop");
    assert_eq!(block["label"], "Every minute");
    assert!(block["rect"]["x"].is_number());
    assert!(block["rect"]["width"].is_number());
}

#[test]
fn sequence_mmds_envelope_has_empty_nodes_edges() {
    let input = "sequenceDiagram\n    Alice->>Bob: hi";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["nodes"].as_array().unwrap().len(), 0);
    assert_eq!(json["edges"].as_array().unwrap().len(), 0);
}

#[test]
fn sequence_mmds_self_message() {
    let input = "sequenceDiagram\n    Alice->>Alice: think";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["from"], messages[0]["to"]);
    assert_eq!(messages[0]["text"], "think");
}

// ---------------------------------------------------------------------------
// Contract fixture validation
// ---------------------------------------------------------------------------

#[test]
fn state_mmds_contract_fixture_is_valid() {
    let fixture = mmds_fixture("contracts/state-simple.json");
    assert_eq!(fixture["metadata"]["diagram_type"], "state");
    assert!(fixture["nodes"].as_array().unwrap().len() >= 2);
    assert!(fixture["edges"].as_array().unwrap().len() >= 2);
}

#[test]
fn sequence_mmds_contract_fixture_is_valid() {
    let fixture = mmds_fixture("contracts/sequence-simple.json");
    assert_eq!(fixture["metadata"]["diagram_type"], "sequence");
    assert!(fixture["participants"].as_array().unwrap().len() >= 2);
    assert!(fixture["messages"].as_array().unwrap().len() >= 2);
}

// ---------------------------------------------------------------------------
// State MMDS contract tests
// ---------------------------------------------------------------------------

#[test]
fn state_diagram_mmds_output_is_valid_json() {
    let input = "stateDiagram-v2\n    [*] --> Active\n    Active --> [*]";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["metadata"]["diagram_type"], "state");
    assert_eq!(json["version"], 1);
}

#[test]
fn state_mmds_has_nodes_and_edges() {
    let input = "stateDiagram-v2\n    [*] --> Active\n    Active --> Idle\n    Idle --> [*]";
    let output = render_json(input);
    let json: Value = serde_json::from_str(&output).unwrap();

    let nodes = json["nodes"].as_array().unwrap();
    assert!(
        nodes.len() >= 3,
        "expected at least 3 nodes, got {}",
        nodes.len()
    );

    let edges = json["edges"].as_array().unwrap();
    assert!(
        edges.len() >= 3,
        "expected at least 3 edges, got {}",
        edges.len()
    );
}

#[test]
fn state_self_transition_routed_mmds_includes_self_edge_path() {
    let input = "stateDiagram-v2\n    [*] --> Processing\n    Processing --> Processing : retry\n    Processing --> Done\n    Done --> [*]";
    let output = render_json_with_level(input, GeometryLevel::Routed);
    let json: Value = serde_json::from_str(&output).unwrap();

    let self_edge = json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["source"] == edge["target"])
        .expect("expected routed MMDS to include a self-transition edge");

    let path = self_edge["path"]
        .as_array()
        .expect("expected routed self-transition to serialize a path");
    assert!(
        path.len() >= 4,
        "expected routed self-transition path to contain loop geometry, got {path:?}"
    );
    assert_eq!(self_edge["label"], "retry");
}

#[test]
fn mmds_layout_output_omits_edge_paths_regardless_of_engine() {
    let input = "graph TD\nA-->B";
    let config = RenderConfig {
        geometry_level: GeometryLevel::Layout,
        ..Default::default()
    };
    let output = render_json_with_config(input, &config);
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();

    // Layout level should not have edge paths
    assert!(
        json["edges"][0]["path"].is_null()
            || !json["edges"][0].as_object().unwrap().contains_key("path")
    );
}

// -----------------------------------------------------------------------
// Plan 0145 Task 1.12: MMDS Edge gains label_side + label_rect
// -----------------------------------------------------------------------

fn plan_0145_edge_contract_stub() -> MmdsEdge {
    MmdsEdge {
        id: "e1".into(),
        source: "A".into(),
        target: "B".into(),
        from_subgraph: None,
        to_subgraph: None,
        label: Some("x".into()),
        stroke: "solid".into(),
        arrow_start: "none".into(),
        arrow_end: "normal".into(),
        minlen: 1,
        path: None,
        label_position: None,
        is_backward: None,
        source_port: None,
        target_port: None,
        label_side: None,
        label_rect: None,
    }
}

#[test]
fn mmds_edge_supports_label_side_and_label_rect_fields() {
    let edge = MmdsEdge {
        label_side: Some("above".into()),
        label_rect: Some(MmdsRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 10.0,
        }),
        ..plan_0145_edge_contract_stub()
    };

    let json = serde_json::to_value(&edge).unwrap();
    assert_eq!(json["label_side"], "above");
    assert_eq!(json["label_rect"]["x"], 10.0);
    assert_eq!(json["label_rect"]["y"], 20.0);
    assert_eq!(json["label_rect"]["width"], 30.0);
    assert_eq!(json["label_rect"]["height"], 10.0);

    let roundtrip: MmdsEdge = serde_json::from_value(json).unwrap();
    assert_eq!(roundtrip.label_side.as_deref(), Some("above"));
    let rect = roundtrip.label_rect.expect("label_rect should round-trip");
    assert_eq!(rect.x, 10.0);
    assert_eq!(rect.y, 20.0);
    assert_eq!(rect.width, 30.0);
    assert_eq!(rect.height, 10.0);
}

#[test]
fn mmds_edge_omits_label_side_and_label_rect_when_none() {
    let edge = plan_0145_edge_contract_stub();
    let json = serde_json::to_string(&edge).unwrap();
    assert!(
        !json.contains("label_side"),
        "expected label_side to be skipped when None, got: {json}"
    );
    assert!(
        !json.contains("label_rect"),
        "expected label_rect to be skipped when None, got: {json}"
    );
}

#[test]
fn mmds_schema_validates_label_side_and_label_rect() {
    let payload = serde_json::json!({
        "version": 1,
        "defaults": {
            "node": { "shape": "rectangle" },
            "edge": {
                "stroke": "solid",
                "arrow_start": "none",
                "arrow_end": "normal",
                "minlen": 1
            }
        },
        "geometry_level": "routed",
        "metadata": {
            "diagram_type": "flowchart",
            "direction": "TD",
            "bounds": { "width": 120.0, "height": 200.0 }
        },
        "nodes": [
            {
                "id": "A",
                "label": "A",
                "position": { "x": 0.0, "y": 0.0 },
                "size": { "width": 10.0, "height": 10.0 }
            },
            {
                "id": "B",
                "label": "B",
                "position": { "x": 0.0, "y": 100.0 },
                "size": { "width": 10.0, "height": 10.0 }
            }
        ],
        "edges": [{
            "id": "e1",
            "source": "A",
            "target": "B",
            "label_side": "below",
            "label_rect": { "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 }
        }]
    });
    assert_schema_valid(payload);
}

// -----------------------------------------------------------------------
// Plan 0145 Task 1.13: MMDS Edge output populates label_side + label_rect
// -----------------------------------------------------------------------

#[test]
fn mmds_output_populates_label_side_at_layout_level() {
    // Reciprocal edges trigger label-side selection in the engine.
    let input = "graph TD\n    A -->|forward| B\n    B -->|reverse| A";
    let json = render_json_with_level(input, GeometryLevel::Layout);
    let output: Output = serde_json::from_str(&json).unwrap();

    let has_side = output.edges.iter().any(|e| e.label_side.is_some());
    assert!(
        has_side,
        "at least one edge must carry label_side at layout level; edges: {:?}",
        output
            .edges
            .iter()
            .map(|e| (&e.id, &e.label_side))
            .collect::<Vec<_>>()
    );

    // Each label_side value must be a recognized string.
    for edge in &output.edges {
        if let Some(side) = &edge.label_side {
            assert!(
                ["above", "below", "center"].contains(&side.as_str()),
                "unexpected label_side value: {side}"
            );
        }
    }
}

#[test]
fn mmds_output_populates_label_rect_at_routed_level_only() {
    let input = "graph TD\n    A -->|forward| B";

    // Layout level: label_rect must NOT appear.
    let layout_json = render_json_with_level(input, GeometryLevel::Layout);
    let layout_output: Output = serde_json::from_str(&layout_json).unwrap();
    assert!(
        layout_output.edges[0].label_rect.is_none(),
        "label_rect must not appear at layout level"
    );

    // Routed level: label_rect MUST appear for a labeled edge.
    let routed_json = render_json_with_level(input, GeometryLevel::Routed);
    let routed_output: Output = serde_json::from_str(&routed_json).unwrap();
    let rect = routed_output.edges[0]
        .label_rect
        .as_ref()
        .expect("label_rect must appear at routed level for labeled edges");
    assert!(rect.width > 0.0, "label_rect width must be positive");
    assert!(rect.height > 0.0, "label_rect height must be positive");
}

#[test]
fn mmds_output_label_rect_absent_for_unlabeled_edges() {
    let input = "graph TD\n    A --> B";
    let routed_json = render_json_with_level(input, GeometryLevel::Routed);
    let routed_output: Output = serde_json::from_str(&routed_json).unwrap();
    assert!(
        routed_output.edges[0].label_rect.is_none(),
        "unlabeled edges must not have label_rect"
    );
}

#[test]
fn mmds_output_label_side_present_at_routed_level_too() {
    // label_side should appear at BOTH levels, not just layout.
    let input = "graph TD\n    A -->|forward| B\n    B -->|reverse| A";
    let json = render_json_with_level(input, GeometryLevel::Routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let has_side = output.edges.iter().any(|e| e.label_side.is_some());
    assert!(
        has_side,
        "at least one edge must carry label_side at routed level; edges: {:?}",
        output
            .edges
            .iter()
            .map(|e| (&e.id, &e.label_side))
            .collect::<Vec<_>>()
    );
}

#[test]
fn mmds_schema_rejects_label_side_with_invalid_enum() {
    let payload = serde_json::json!({
        "version": 1,
        "defaults": {
            "node": { "shape": "rectangle" },
            "edge": {
                "stroke": "solid",
                "arrow_start": "none",
                "arrow_end": "normal",
                "minlen": 1
            }
        },
        "geometry_level": "layout",
        "metadata": {
            "diagram_type": "flowchart",
            "direction": "TD",
            "bounds": { "width": 120.0, "height": 200.0 }
        },
        "nodes": [],
        "edges": [{
            "id": "e1",
            "source": "A",
            "target": "B",
            "label_side": "sideways"
        }]
    });
    assert_schema_invalid(payload);
}

// -----------------------------------------------------------------------
// Contract: routed → layout down-conversion strips routed-only fields
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_to_layout_down_conversion_strips_routed_only_edge_fields() {
    let routed_json = render_json_with_level("graph TD\nA -->|forward| B", GeometryLevel::Routed);
    let layout_output = render_mmds_input(
        &routed_json,
        OutputFormat::Mmds,
        RenderConfig {
            geometry_level: GeometryLevel::Layout,
            ..RenderConfig::default()
        },
    );
    let parsed: Output = serde_json::from_str(&layout_output).unwrap();
    assert_eq!(parsed.geometry_level, "layout");
    for edge in &parsed.edges {
        assert!(edge.path.is_none(), "path must be stripped at layout level");
        assert!(
            edge.label_position.is_none(),
            "label_position must be stripped at layout level"
        );
        assert!(
            edge.is_backward.is_none(),
            "is_backward must be stripped at layout level"
        );
        assert!(
            edge.source_port.is_none(),
            "source_port must be stripped at layout level"
        );
        assert!(
            edge.target_port.is_none(),
            "target_port must be stripped at layout level"
        );
        assert!(
            edge.label_rect.is_none(),
            "label_rect must be stripped at layout level"
        );
    }
}
