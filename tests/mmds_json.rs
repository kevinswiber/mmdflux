//! MMDS JSON contract tests.
//!
//! Verifies that `--format mmds` output (with `json` alias) matches the MMDS specification:
//! - Default output is `geometry_level: "layout"` with no edge geometry.
//! - Routed output is explicit opt-in with edge paths and bounds.

mod common;

use std::path::Path;

use mmdflux::graph::attachment::PortFace;
use mmdflux::graph::geometry::EdgeLabelSide;
use mmdflux::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
};
use mmdflux::graph::{Arrow, Direction, GeometryLevel, Shape, Stroke};
use mmdflux::mmds::{
    Document, Edge as MmdsEdge, MmdsToken, Port as MmdsPort, Position as MmdsPosition,
    Rect as MmdsRect, SUPPORTED_PROFILES, evaluate_profiles, hydrate_routed_geometry_from_document,
    parse_input,
};
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    EngineAlgorithmId, OutputFormat, RenderConfig, TextColorMode, materialize_diagram,
    render_diagram,
};
use serde_json::{Value, json};

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

fn render_routed_mmds_with_profile(input: &str, profile_id: &str) -> String {
    render_json_with_config(
        input,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            font_metrics_profile: Some(profile_id.to_string()),
            ..RenderConfig::default()
        },
    )
}

fn render_json_with_config(input: &str, config: &RenderConfig) -> String {
    mmdflux::render_diagram(input, OutputFormat::Mmds, config).unwrap()
}

fn render_svg_with_profile(input: &str, profile_id: &str) -> String {
    render_diagram(
        input,
        OutputFormat::Svg,
        &RenderConfig {
            font_metrics_profile: Some(profile_id.to_string()),
            ..RenderConfig::default()
        },
    )
    .unwrap()
}

fn render_svg_from_mmds_with_profile(input: &str, profile_id: &str) -> String {
    render_mmds_input(
        input,
        OutputFormat::Svg,
        RenderConfig {
            font_metrics_profile: Some(profile_id.to_string()),
            ..RenderConfig::default()
        },
    )
}

fn render_mmds_input(input: &str, format: OutputFormat, config: RenderConfig) -> String {
    render_mmds_input_result(input, format, config).unwrap()
}

fn render_mmds_input_result(
    input: &str,
    format: OutputFormat,
    config: RenderConfig,
) -> Result<String, mmdflux::RenderError> {
    mmdflux::render_diagram(input, format, &config)
}

fn compatibility_text_metrics_extension() -> Value {
    json!({
        "metricsProfile": {
            "id": COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
            "source": "heuristic",
            "version": 1
        },
        "defaultTextStyle": {
            "font-family": "\"trebuchet ms\", verdana, arial, sans-serif",
            "font-size": 16.0,
            "font-style": "normal",
            "font-weight": "400",
            "line-height": 24.0
        },
        "layoutText": {
            "node-padding-x": 15.0,
            "node-padding-y": 15.0,
            "label-padding-x": 4.0,
            "label-padding-y": 2.0,
            "edge-label-max-width": 200.0
        }
    })
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
    let parsed_from_str: Document = payload
        .parse()
        .expect("Document FromStr should use MMDS parse semantics");
    let parsed_try_from =
        Document::try_from(payload.as_str()).expect("Document TryFrom should parse MMDS JSON");
    let negotiation =
        evaluate_profiles(&payload).expect("shared contract fixture profile evaluation");

    assert_eq!(parsed.metadata.diagram_type, "flowchart");
    assert_eq!(parsed_from_str.metadata.diagram_type, "flowchart");
    assert_eq!(parsed_try_from.metadata.diagram_type, "flowchart");
    assert_eq!(
        negotiation.supported,
        vec![
            "mmds-core-v1".to_string(),
            "mmdflux-text-v1".to_string(),
            "mmdflux-text-metrics-v1".to_string(),
        ]
    );
    assert_eq!(negotiation.unknown, Vec::<String>::new());
}

#[test]
fn materialize_diagram_matches_mmds_json_render_for_graph_family() {
    let source = "graph TD\n    A[Start] --> B[End]";
    let config = RenderConfig::default();
    let document = materialize_diagram(source, &config).expect("source should materialize");
    let json = render_json_with_config(source, &config);
    let rendered_document: Document =
        serde_json::from_str(&json).expect("MMDS JSON render should parse as a document");

    assert_eq!(document.metadata.diagram_type, "flowchart");
    assert_eq!(document.nodes.len(), rendered_document.nodes.len());
    assert_eq!(document.edges.len(), rendered_document.edges.len());
    assert_eq!(document.edges[0].id, rendered_document.edges[0].id);
}

// -----------------------------------------------------------------------
// Contract: MMDS envelope
// -----------------------------------------------------------------------

#[test]
fn mmds_default_has_version_1() {
    let json = render_json("graph TD\nA-->B");
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.version, 1);
}

#[test]
fn mmds_default_geometry_level_is_layout() {
    let json = render_json("graph TD\nA-->B");
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, GeometryLevel::Layout);
}

#[test]
fn mmds_has_metadata_with_direction() {
    let json = render_json("graph LR\nA-->B");
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.metadata.diagram_type, "flowchart");
    assert_eq!(output.metadata.direction, Direction::LeftRight);
}

#[test]
fn mmds_has_nodes_and_edges() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: Document = serde_json::from_str(&json).unwrap();
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
fn font_metrics_explicit_compatibility_profile_matches_default_mmds_geometry() {
    let input = flowchart_fixture("labeled_edges.mmd");
    let default_mmds: Value = serde_json::from_str(&render_json(&input)).unwrap();
    let explicit_mmds: Value = serde_json::from_str(&render_json_with_config(
        &input,
        &RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    ))
    .unwrap();

    assert_eq!(explicit_mmds["metadata"], default_mmds["metadata"]);
    assert_eq!(explicit_mmds["nodes"], default_mmds["nodes"]);
    assert_eq!(explicit_mmds["edges"], default_mmds["edges"]);
    assert_eq!(explicit_mmds["subgraphs"], default_mmds["subgraphs"]);
}

#[test]
fn text_metrics_mmds_output_emits_profile_and_extension_contract() {
    let json = render_json("graph TD\nA[Alpha] -->|a labeled edge| B[Beta]");
    let value: Value = serde_json::from_str(&json).unwrap();

    let profiles = value["profiles"]
        .as_array()
        .expect("profiles should be present");
    assert!(
        profiles
            .iter()
            .any(|profile| profile == "mmdflux-text-metrics-v1"),
        "MMDS profiles should include text metrics profile: {profiles:?}"
    );

    let extension = &value["extensions"]["org.mmdflux.text-metrics.v1"];
    assert_eq!(
        extension["metricsProfile"]["id"],
        "mmdflux-heuristic-proportional-v1"
    );
    assert_eq!(extension["metricsProfile"]["source"], "heuristic");
    assert_eq!(extension["metricsProfile"]["version"], 1);
    assert_eq!(
        extension["defaultTextStyle"]["font-family"],
        "\"trebuchet ms\", verdana, arial, sans-serif"
    );
    assert_eq!(extension["defaultTextStyle"]["font-size"], 16.0);
    assert_eq!(extension["defaultTextStyle"]["font-style"], "normal");
    assert_eq!(extension["defaultTextStyle"]["font-weight"], "400");
    assert_eq!(extension["defaultTextStyle"]["line-height"], 24.0);
    assert_eq!(extension["layoutText"]["node-padding-x"], 15.0);
    assert_eq!(extension["layoutText"]["node-padding-y"], 15.0);
    assert_eq!(extension["layoutText"]["label-padding-x"], 4.0);
    assert_eq!(extension["layoutText"]["label-padding-y"], 2.0);
    assert_eq!(extension["layoutText"]["edge-label-max-width"], 200.0);
    assert_schema_valid(value);
}

#[test]
fn text_metrics_old_mmds_without_extension_still_replays() {
    let svg = render_mmds_input(
        STYLED_MMDS_LAYOUT,
        OutputFormat::Svg,
        RenderConfig::default(),
    );

    assert!(svg.starts_with("<svg"), "{svg}");
    assert!(svg.contains("Alpha"), "{svg}");
}

#[test]
fn text_metrics_old_mmds_replay_matches_same_document_with_extension() {
    let legacy_svg = render_mmds_input(
        STYLED_MMDS_LAYOUT,
        OutputFormat::Svg,
        RenderConfig::default(),
    );
    let mut value: Value = serde_json::from_str(STYLED_MMDS_LAYOUT).unwrap();
    value["profiles"]
        .as_array_mut()
        .unwrap()
        .push(Value::String("mmdflux-text-metrics-v1".to_string()));
    value["extensions"]["org.mmdflux.text-metrics.v1"] = compatibility_text_metrics_extension();
    let input = serde_json::to_string(&value).unwrap();

    let extension_svg = render_mmds_input(&input, OutputFormat::Svg, RenderConfig::default());

    assert_eq!(extension_svg, legacy_svg);
}

#[test]
fn text_metrics_direct_svg_matches_routed_mmds_replay_svg() {
    let input = "graph TD\nA[Alpha] -->|a labeled edge| B[Beta]";
    let direct_svg = render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).unwrap();
    let mmds = render_json_with_level(input, GeometryLevel::Routed);

    let replay_svg = render_mmds_input(&mmds, OutputFormat::Svg, RenderConfig::default());

    assert_eq!(replay_svg, direct_svg);
}

#[test]
fn mmds_output_records_mmdflux_sans_profile() {
    let json = render_json_with_config(
        "graph TD\nA[mmmm] --> B[iiii]",
        &RenderConfig {
            font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    );
    let value: Value = serde_json::from_str(&json).unwrap();
    let extension = &value["extensions"]["org.mmdflux.text-metrics.v1"];

    assert_eq!(
        extension["metricsProfile"]["id"],
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID
    );
    assert_eq!(extension["metricsProfile"]["source"], "recorded");
    assert_schema_valid(value);
}

#[test]
fn mmdflux_sans_direct_svg_matches_routed_mmds_replay_svg() {
    let input = "graph TD\nA[mmmm] -->|mmmm| B[iiii]";
    let direct_svg = render_svg_with_profile(input, RECORDED_SANS_TEXT_METRICS_PROFILE_ID);
    let routed_mmds = render_routed_mmds_with_profile(input, RECORDED_SANS_TEXT_METRICS_PROFILE_ID);

    let replay_svg =
        render_svg_from_mmds_with_profile(&routed_mmds, RECORDED_SANS_TEXT_METRICS_PROFILE_ID);

    assert_eq!(replay_svg, direct_svg);
}

#[test]
fn mmdflux_sans_replay_rejects_mismatched_caller_profile() {
    let routed_mmds = render_routed_mmds_with_profile(
        "graph TD\nA[mmmm] -->|mmmm| B[iiii]",
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
    );

    let err = render_mmds_input_result(
        &routed_mmds,
        OutputFormat::Svg,
        RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    )
    .expect_err("MMDS replay should reject caller/profile mismatch");

    assert!(
        err.message.contains(&format!(
            "font metrics profile '{}' does not match MMDS replay profile '{}'",
            COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID
        )),
        "{err}"
    );
}

#[test]
fn text_metrics_unsupported_persisted_profile_fails_replay() {
    let json = render_json("graph TD\nA --> B");
    let mut value: Value = serde_json::from_str(&json).unwrap();
    value["extensions"]["org.mmdflux.text-metrics.v1"]["metricsProfile"]["id"] =
        Value::String("unknown-profile-v1".to_string());
    let input = serde_json::to_string(&value).unwrap();

    let err = render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect_err("unsupported persisted metrics profile should fail");

    assert!(
        err.message
            .contains("unsupported text metrics profile 'unknown-profile-v1'"),
        "{err}"
    );
}

#[test]
fn text_metrics_extension_requires_schema_required_objects_on_replay() {
    for field in ["defaultTextStyle", "layoutText"] {
        let json = render_json("graph TD\nA --> B");
        let mut value: Value = serde_json::from_str(&json).unwrap();
        value["extensions"]["org.mmdflux.text-metrics.v1"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        let input = serde_json::to_string(&value).unwrap();

        let err = render_mmds_input_result(&input, OutputFormat::Svg, RenderConfig::default())
            .expect_err("recognized text metrics extension should reject missing required object");

        assert!(
            err.message
                .contains(&format!("invalid text metrics extension: missing {field}")),
            "{err}"
        );
    }
}

#[test]
fn text_metrics_routed_to_layout_mmds_preserves_profile_and_extension() {
    let routed_json = render_json_with_level("graph TD\nA -->|label| B", GeometryLevel::Routed);
    let layout_json = render_mmds_input(
        &routed_json,
        OutputFormat::Mmds,
        RenderConfig {
            geometry_level: GeometryLevel::Layout,
            ..RenderConfig::default()
        },
    );
    let value: Value = serde_json::from_str(&layout_json).unwrap();

    assert_eq!(value["geometry_level"], "layout");
    assert!(
        value["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile == "mmdflux-text-metrics-v1")
    );
    assert_eq!(
        value["extensions"]["org.mmdflux.text-metrics.v1"]["metricsProfile"]["id"],
        "mmdflux-heuristic-proportional-v1"
    );
    assert!(value["edges"].as_array().unwrap().iter().all(|edge| {
        edge.get("path").is_none()
            && edge.get("label_position").is_none()
            && edge.get("label_rect").is_none()
    }));
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

#[test]
fn mmds_hydration_replays_hyphenated_node_style_keys_into_svg() {
    let mut payload: Value =
        serde_json::from_str(STYLED_MMDS_LAYOUT).expect("styled MMDS should parse");
    let style = payload["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]
        .as_object_mut()
        .expect("node style extension should contain A style object");
    style.insert(
        "font-style".to_string(),
        Value::String("italic".to_string()),
    );
    style.insert("font-weight".to_string(), Value::String("700".to_string()));
    style.insert("stroke-width".to_string(), Value::String("3".to_string()));
    style.insert(
        "stroke-dasharray".to_string(),
        Value::String("4 2".to_string()),
    );

    let input = serde_json::to_string(&payload).expect("MMDS payload should serialize");
    let svg = render_mmds_input(&input, OutputFormat::Svg, RenderConfig::default());

    assert!(
        svg.contains("font-style=\"italic\""),
        "styled MMDS SVG font-style missing: {svg}"
    );
    assert!(
        svg.contains("font-weight=\"700\""),
        "styled MMDS SVG font-weight missing: {svg}"
    );
    assert!(
        svg.contains("stroke-width=\"3\""),
        "styled MMDS SVG stroke-width missing: {svg}"
    );
    assert!(
        svg.contains("stroke-dasharray=\"4 2\""),
        "styled MMDS SVG stroke-dasharray missing: {svg}"
    );
}

// -----------------------------------------------------------------------
// Contract: layout-level node geometry
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_nodes_have_positions_and_sizes() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: Document = serde_json::from_str(&json).unwrap();

    let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
    assert_eq!(node_a.label, "Start");
    assert_eq!(node_a.shape, Shape::Rectangle);
    assert!(node_a.size.width > 0.0);
    assert!(node_a.size.height > 0.0);
}

#[test]
fn mmds_layout_nodes_sorted_by_id() {
    let json = render_json("graph TD\nC-->B\nB-->A");
    let output: Document = serde_json::from_str(&json).unwrap();
    let ids: Vec<&str> = output.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn mmds_layout_node_shapes() {
    let json = render_json("graph TD\nA[Rect]\nB(Round)\nC{Diamond}\nD([Stadium])");
    let output: Document = serde_json::from_str(&json).unwrap();

    let shapes: std::collections::HashMap<String, Shape> = output
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.shape))
        .collect();
    assert_eq!(shapes["A"], Shape::Rectangle);
    assert_eq!(shapes["B"], Shape::Round);
    assert_eq!(shapes["C"], Shape::Diamond);
    assert_eq!(shapes["D"], Shape::Stadium);
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

    let full: Document = serde_json::from_str(&full).unwrap();
    let compact: Document = serde_json::from_str(&compact).unwrap();
    let simplified: Document = serde_json::from_str(&simplified).unwrap();

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
fn compound_backward_disconnected_routed_mmds_engine_contracts() {
    let input = flowchart_fixture("compound_backward_disconnected.mmd");
    let edge_is_backward = |engine: &str, edge_id: &str| {
        let json = render_routed_mmds_with_engine(&input, engine);
        let output: Document = serde_json::from_str(&json).unwrap();
        output
            .edges
            .iter()
            .find(|edge| edge.id == edge_id)
            .unwrap_or_else(|| panic!("{engine} missing edge {edge_id}"))
            .is_backward
    };

    assert_eq!(edge_is_backward("flux-layered", "e1"), Some(false));
    assert_eq!(edge_is_backward("flux-layered", "e2"), Some(true));
    assert_eq!(edge_is_backward("mermaid-layered", "e1"), Some(false));
    assert_eq!(edge_is_backward("mermaid-layered", "e2"), Some(false));
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
        let output: Document = serde_json::from_str(json).unwrap();
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
        let output: Document = serde_json::from_str(json).unwrap();
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
    let output: Document = serde_json::from_str(&json).expect("routed MMDS parses");

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
    assert_eq!(v1, v2, "MMDS Document is not round-trip stable");

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
    let output: Document = serde_json::from_str(&json).unwrap();

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
    let output: Document = serde_json::from_str(&json).unwrap();

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
    let output: Document = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert_eq!(edge.id, "e0");
    assert_eq!(edge.source, "A");
    assert_eq!(edge.target, "B");
    assert_eq!(edge.stroke, Stroke::Dotted);
    assert_eq!(edge.label, Some("label".to_string()));
    assert_eq!(edge.arrow_start, Arrow::None);
    assert_eq!(edge.arrow_end, Arrow::Normal);
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
    let output: Document = serde_json::from_str(&json).unwrap();
    assert!(output.metadata.bounds.width > 0.0);
    assert!(output.metadata.bounds.height > 0.0);
}

// -----------------------------------------------------------------------
// Contract: layout-level subgraphs
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_subgraphs() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let output: Document = serde_json::from_str(&json).unwrap();

    assert_eq!(output.subgraphs.len(), 1);
    assert_eq!(output.subgraphs[0].id, "sg1");
    assert_eq!(output.subgraphs[0].title, "Group");
    assert!(output.subgraphs[0].direction.is_none());
    assert!(output.subgraphs[0].bounds.is_none());
}

#[test]
fn mmds_layout_subgraph_direction_override() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\ndirection LR\nA-->B\nend");
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.subgraphs[0].direction, Some(Direction::LeftRight));
}

// -----------------------------------------------------------------------
// Contract: routed-level output
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_has_geometry_level_routed() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, GeometryLevel::Routed);
}

#[test]
fn mmds_routed_includes_edge_paths() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Document = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert!(edge.path.is_some());
    assert!(edge.path.as_ref().unwrap().len() >= 2);
    assert!(edge.is_backward.is_some());
}

#[test]
fn mmds_routed_includes_metadata_bounds() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: Document = serde_json::from_str(&json).unwrap();

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
    let output: Document = serde_json::from_str(&json).unwrap();

    let sg = &output.subgraphs[0];
    assert!(sg.bounds.is_some());
    assert!(sg.bounds.as_ref().unwrap().width > 0.0);
}

#[test]
fn mmds_routed_label_position_for_labeled_edge() {
    let json = render_json_with_level("graph TD\nA--label-->B", GeometryLevel::Routed);
    let output: Document = serde_json::from_str(&json).unwrap();

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
    let output: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(output.nodes[0].shape, Shape::Rectangle);
    assert_eq!(output.edges[0].stroke, Stroke::Solid);
    assert_eq!(output.edges[0].arrow_start, Arrow::None);
    assert_eq!(output.edges[0].arrow_end, Arrow::Normal);
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
    for (dir_str, expected) in [
        ("TD", Direction::TopDown),
        ("LR", Direction::LeftRight),
        ("BT", Direction::BottomTop),
        ("RL", Direction::RightLeft),
    ] {
        let input = format!("graph {dir_str}\nA-->B");
        let json = render_json(&input);
        let output: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(output.metadata.direction, expected);
    }
}

#[test]
fn mmds_direction_tb_deserializes_to_canonical_top_down() {
    let input = STYLED_MMDS_LAYOUT.replace(r#""direction": "TD""#, r#""direction": "TB""#);
    let output = parse_input(&input).unwrap();
    assert_eq!(output.metadata.direction, Direction::TopDown);

    let value = serde_json::to_value(&output).unwrap();
    assert_eq!(value["metadata"]["direction"], "TD");
}

// -----------------------------------------------------------------------
// Contract: class diagram MMDS output
// -----------------------------------------------------------------------

#[test]
fn mmds_class_diagram_produces_json() {
    let config = RenderConfig::default();
    let output = render_json_with_config("classDiagram\nA --> B", &config);
    let parsed: Document = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.geometry_level, GeometryLevel::Layout);
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
    let parsed: Document = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed.geometry_level, GeometryLevel::Routed);
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
    let required_strings: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required_strings.contains(&"face"));
    assert!(required_strings.contains(&"fraction"));
    assert!(required_strings.contains(&"position"));
    assert!(required_strings.contains(&"group_size"));
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
            "mmdflux-text-metrics-v1",
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
    let parsed: Document = serde_json::from_str(&payload).unwrap();
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
    assert!(Path::new("examples/materialized_view.rs").exists());
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
        "`views`",
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
fn docs_and_schema_reference_text_metrics_extension_contract() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert!(docs.contains("mmdflux-text-metrics-v1"));
    assert!(docs.contains("org.mmdflux.text-metrics.v1"));
    assert!(docs.contains("mmdflux-heuristic-proportional-v1"));
    assert!(docs.contains("mmdflux-sans-v1"));
    assert!(docs.contains("source font is provenance"));
    assert!(docs.contains("Browser `measureText` remains out of scope"));
    assert!(docs.contains("Sequence-family full text-metrics parity remains deferred"));
    assert!(docs.contains("line-height"));

    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    assert!(schema.contains("org.mmdflux.text-metrics.v1"));
    assert!(schema.contains("metricsProfile"));
    assert!(schema.contains("mmdflux-sans-v1"));
    assert!(schema.contains("heuristic"));
    assert!(schema.contains("recorded"));
    assert!(schema.contains("dynamic"));
    assert!(schema.contains("edge-label-max-width"));
}

#[test]
fn docs_and_schema_reference_view_extension_contract() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert!(docs.contains("mmdflux::views"));
    assert!(docs.contains("ViewSpec"));
    assert!(docs.contains("project"));
    assert!(docs.contains("ViewEvent"));
    assert!(docs.contains("org.mmdflux.view.v1"));
    assert!(docs.contains("[\"e0\", \"e2\", \"e4\"]"));
    assert!(docs.contains("examples/materialized_view.rs"));

    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    assert!(schema.contains("org.mmdflux.view.v1"));
    assert!(schema.contains("shared_coordinates"));
    assert!(schema.contains("boundary_policy"));
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
    assert!(wasm_docs.contains("fontMetricsProfile"));
    assert!(wasm_docs.contains("mmdflux-heuristic-proportional-v1"));
    assert!(wasm_docs.contains("mmdflux-sans-v1"));
    assert!(!wasm_docs.contains("currently accepts only"));

    let readme = std::fs::read_to_string("README.md").unwrap();
    assert!(readme.contains("NO_COLOR=1 mmdflux --format text"));
    assert!(readme.contains("--color always"));
}

#[test]
fn mmds_docs_point_to_fixture_backed_examples_and_rust_replay_example() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();

    for required in [
        "examples/mmds_replay.rs",
        "examples/materialized_view.rs",
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
    let output: Document = serde_json::from_str(&json).unwrap();
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
    let output: Document = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    let sp = edge.source_port.as_ref().unwrap();
    let tp = edge.target_port.as_ref().unwrap();
    assert_eq!(sp.face, PortFace::Bottom, "TD source should exit bottom");
    assert_eq!(tp.face, PortFace::Top, "TD target should enter top");
}

#[test]
fn mmds_routed_port_fractions_fan_in() {
    let json = render_json_with_level("graph TD\nA-->C\nB-->C", GeometryLevel::Routed);
    let output: Document = serde_json::from_str(&json).unwrap();
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
fn mmds_routed_port_roundtrip_preserves_direction_override_boundary_ports_and_path() {
    let input = flowchart_fixture("subgraph_direction_lr.mmd");
    let direct_json = render_json_with_level(&input, GeometryLevel::Routed);
    let direct: Document = serde_json::from_str(&direct_json).unwrap();
    let roundtrip_json = render_mmds_input(
        &direct_json,
        OutputFormat::Mmds,
        RenderConfig {
            geometry_level: GeometryLevel::Routed,
            ..RenderConfig::default()
        },
    );
    let roundtrip: Document = serde_json::from_str(&roundtrip_json).unwrap();

    let direct_edge = find_mmds_edge(&direct, "C", "End");
    let roundtrip_edge = find_mmds_edge(&roundtrip, "C", "End");

    assert_mmds_ports_equal(
        direct_edge.source_port.as_ref(),
        roundtrip_edge.source_port.as_ref(),
        "C -> End source_port",
    );
    assert_mmds_ports_equal(
        direct_edge.target_port.as_ref(),
        roundtrip_edge.target_port.as_ref(),
        "C -> End target_port",
    );
    assert_eq!(
        direct_edge.path, roundtrip_edge.path,
        "C -> End routed path should round-trip unchanged"
    );
}

#[test]
fn mmds_docs_and_schema_define_ports_as_logical_anchors() {
    let docs = std::fs::read_to_string("docs/mmds.md").unwrap();
    assert_contract_terms_near(&docs, "edge.path", &["visible", "geometry"]);
    assert_contract_terms_near(&docs, "source_port", &["logical", "anchor"]);
    assert_contract_terms_near(&docs, "target_port", &["logical", "anchor"]);
    assert_contract_terms_near(&docs, "position", &["logical", "not guaranteed"]);

    let schema = std::fs::read_to_string("docs/mmds.schema.json").unwrap();
    assert_contract_terms_near(&schema, "\"path\"", &["visible", "path"]);
    assert_contract_terms_near(&schema, "\"source_port\"", &["logical", "anchor"]);
    assert_contract_terms_near(&schema, "\"target_port\"", &["logical", "anchor"]);
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

fn assert_contract_terms_near(document: &str, needle: &str, terms: &[&str]) {
    let document = document.to_lowercase();
    let needle = needle.to_lowercase();
    let terms: Vec<String> = terms.iter().map(|term| term.to_lowercase()).collect();

    for (index, _) in document.match_indices(&needle) {
        let end = (index + 1_000).min(document.len());
        let window = &document[index..end];
        if terms.iter().all(|term| window.contains(term)) {
            return;
        }
    }

    panic!("expected {needle:?} to appear near contract terms {terms:?}");
}

fn find_mmds_edge<'a>(output: &'a Document, source: &str, target: &str) -> &'a MmdsEdge {
    output
        .edges
        .iter()
        .find(|edge| edge.source == source && edge.target == target)
        .unwrap_or_else(|| panic!("missing MMDS edge {source} -> {target}"))
}

fn assert_mmds_ports_equal(left: Option<&MmdsPort>, right: Option<&MmdsPort>, label: &str) {
    let left = left.unwrap_or_else(|| panic!("{label} missing from direct output"));
    let right = right.unwrap_or_else(|| panic!("{label} missing from roundtrip output"));

    assert_eq!(left.face, right.face, "{label} face should round-trip");
    assert_eq!(
        left.fraction, right.fraction,
        "{label} fraction should round-trip"
    );
    assert_eq!(
        left.group_size, right.group_size,
        "{label} group_size should round-trip"
    );
    assert_eq!(
        left.position.x, right.position.x,
        "{label} position.x should round-trip"
    );
    assert_eq!(
        left.position.y, right.position.y,
        "{label} position.y should round-trip"
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
        stroke: Stroke::Solid,
        arrow_start: Arrow::None,
        arrow_end: Arrow::Normal,
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
        label_side: Some(EdgeLabelSide::Above),
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
    assert_eq!(roundtrip.label_side, Some(EdgeLabelSide::Above));
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
    let output: Document = serde_json::from_str(&json).unwrap();

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

    // Each label_side value must serialize to a recognized string.
    for edge in &output.edges {
        if let Some(side) = &edge.label_side {
            assert!(
                ["above", "below", "center"].contains(&side.as_mmds_str()),
                "unexpected label_side value: {}",
                side.as_mmds_str()
            );
        }
    }
}

#[test]
fn mmds_output_populates_label_rect_at_routed_level_only() {
    let input = "graph TD\n    A -->|forward| B";

    // Layout level: label_rect must NOT appear.
    let layout_json = render_json_with_level(input, GeometryLevel::Layout);
    let layout_output: Document = serde_json::from_str(&layout_json).unwrap();
    assert!(
        layout_output.edges[0].label_rect.is_none(),
        "label_rect must not appear at layout level"
    );

    // Routed level: label_rect MUST appear for a labeled edge.
    let routed_json = render_json_with_level(input, GeometryLevel::Routed);
    let routed_output: Document = serde_json::from_str(&routed_json).unwrap();
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
    let routed_output: Document = serde_json::from_str(&routed_json).unwrap();
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
    let output: Document = serde_json::from_str(&json).unwrap();

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
    let parsed: Document = serde_json::from_str(&layout_output).unwrap();
    assert_eq!(parsed.geometry_level, GeometryLevel::Layout);
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

// ---------------------------------------------------------------------------
// Plan 0151 Task 2.1: routed MMDS replay must preserve authoritative label
// geometry, not snap back to a stale `label_position`.
//
// A routed MMDS payload carries both `label_rect` and `label_position`. The
// authoritative truth is the rectangle — it was shaped by the producer
// pipeline, including Plan 0151 Task 1.2's side-aware alignment. If an
// adapter or a future producer change leaves the two fields disagreeing,
// MMDS replay must prefer the rect.
//
// Today `hydrate_routed_geometry_from_document` rebuilds `label_geometry`
// from `label_position` inside `populate_label_geometry`, ignoring the
// serialized `label_rect`. Replay silently loses authoritative routed
// label placement whenever the producer anchor and raw position disagree.
//
// Red: authoritative `label_rect` center vs. poisoned `label_position` 40 px
// off. After hydration, `label_geometry.center` must track the rect, not
// the position. Passes after Task 2.2 makes hydrate treat `label_rect` as
// the authoritative source.
// ---------------------------------------------------------------------------

mod plan_0151_routed_mmds_replay_contract {
    use super::*;

    fn poisoned_synthetic_routed_document() -> Document {
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("flowchart")
                .join("git_workflow.mmd"),
        )
        .unwrap();
        let routed_json = render_json_with_level(&input, GeometryLevel::Routed);
        let mut output: Document = serde_json::from_str(&routed_json).unwrap();

        let e3 = output
            .edges
            .iter_mut()
            .find(|e| e.id == "e3")
            .expect("git_workflow routed output should expose edge e3 (git pull)");
        let auth_rect = e3
            .label_rect
            .clone()
            .expect("producer fix should have serialized label_rect");

        // Poison `label_position` by shifting it 40 px off the authoritative
        // rect center. `label_rect` stays untouched — it is the claim the
        // replay contract must honor.
        let rect_center_x = auth_rect.x + auth_rect.width / 2.0;
        let rect_center_y = auth_rect.y + auth_rect.height / 2.0;
        e3.label_position = Some(MmdsPosition {
            x: rect_center_x,
            y: rect_center_y - 40.0,
        });

        output
    }

    #[test]
    fn hydrate_routed_geometry_preserves_authoritative_label_rect_when_label_position_disagrees() {
        let output = poisoned_synthetic_routed_document();
        let e3_rect = output
            .edges
            .iter()
            .find(|e| e.id == "e3")
            .and_then(|e| e.label_rect.clone())
            .expect("poisoned fixture must still carry label_rect");
        let expected_center_x = e3_rect.x + e3_rect.width / 2.0;
        let expected_center_y = e3_rect.y + e3_rect.height / 2.0;

        let routed = hydrate_routed_geometry_from_document(&output)
            .expect("hydration should succeed for a valid routed payload");
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "Remote" && e.to == "Working")
            .expect("hydrated routed edges should include Remote -> Working");
        let geom = edge
            .label_geometry
            .as_ref()
            .expect("hydrated edge must carry label_geometry");

        let dx = geom.center.x - expected_center_x;
        let dy = geom.center.y - expected_center_y;
        let drift = (dx * dx + dy * dy).sqrt();
        assert!(
            drift <= 0.5,
            "replayed label_geometry.center {:?} must track the authoritative rect center ({expected_center_x}, {expected_center_y}); drift {drift:.2} px. The poisoned label_position was 40 px away — if hydration follows it, that failure mode is what this test pins.",
            geom.center
        );
    }

    // -----------------------------------------------------------------------
    // Task 2.3: replay SVG canaries prove the hydration contract reaches the
    // rendered label, not just the internal `EdgeLabelGeometry`.
    // -----------------------------------------------------------------------

    fn extract_svg_text_anchor(svg: &str, label: &str) -> Option<(f64, f64)> {
        for line in svg.lines() {
            let line = line.trim();
            if !line.starts_with("<text") {
                continue;
            }
            if !line.contains(&format!(">{label}<")) {
                continue;
            }
            let x = attr(line, " x=\"")?.parse::<f64>().ok()?;
            let y = attr(line, " y=\"")?.parse::<f64>().ok()?;
            return Some((x, y));
        }
        None
    }

    fn attr<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
        let start = line.find(prefix)? + prefix.len();
        let rest = &line[start..];
        let end = rest.find('"')?;
        Some(&rest[..end])
    }

    #[test]
    fn synthetic_shifted_label_survives_routed_mmds_svg_replay() {
        let output = poisoned_synthetic_routed_document();
        let rect = output
            .edges
            .iter()
            .find(|e| e.id == "e3")
            .and_then(|e| e.label_rect.clone())
            .expect("poisoned fixture carries label_rect");
        let authoritative_center = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

        let payload_json =
            serde_json::to_string(&output).expect("synthetic output should serialize back to JSON");
        let svg = render_mmds_input(&payload_json, OutputFormat::Svg, RenderConfig::default());
        let anchor = extract_svg_text_anchor(&svg, "git pull")
            .expect("replay SVG should emit a <text> element for git pull");

        let dx = anchor.0 - authoritative_center.0;
        let dy = anchor.1 - authoritative_center.1;
        let drift = (dx * dx + dy * dy).sqrt();
        assert!(
            drift <= 0.5,
            "replay SVG anchor {anchor:?} must track the authoritative rect center {authoritative_center:?}; drift {drift:.2} px. If SVG revalidation snaps this back toward the corridor midpoint, the hydration-synthesized `compartment_size = 2` trust signal is not reaching the emitter."
        );
    }

    #[test]
    fn git_workflow_routed_mmds_replay_svg_tracks_authoritative_rect() {
        // The producer already emits an authoritative `label_rect` on this
        // fixture. Replay must render the "git pull" label at that rect's
        // center, without being snapped back to the rendered-path midpoint
        // by SVG revalidation (the hydration-synthesized
        // `compartment_size = 2` trust signal is the mechanism).
        //
        // This is intentionally *not* a direct-vs-replay parity check:
        // direct SVG currently runs side-offset labels through revalidation against the
        // rendered path, so direct and replay legitimately disagree by the
        // side-offset magnitude (~14 px). Converging that gap is out of
        // scope for this plan — see the Non-Goals section of the plan.
        let input = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("flowchart")
                .join("git_workflow.mmd"),
        )
        .unwrap();
        let routed_mmds = render_json_with_level(&input, GeometryLevel::Routed);
        let output: Document = serde_json::from_str(&routed_mmds).unwrap();
        let rect = output
            .edges
            .iter()
            .find(|e| e.id == "e3")
            .and_then(|e| e.label_rect.clone())
            .expect("git_workflow e3 should carry routed label_rect after producer fix");
        let authoritative = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

        let replay_svg =
            render_mmds_input(&routed_mmds, OutputFormat::Svg, RenderConfig::default());
        let anchor = extract_svg_text_anchor(&replay_svg, "git pull")
            .expect("replay SVG should carry git pull");

        let dx = anchor.0 - authoritative.0;
        let dy = anchor.1 - authoritative.1;
        let drift = (dx * dx + dy * dy).sqrt();
        assert!(
            drift <= 0.5,
            "replay SVG anchor {anchor:?} must track the authoritative label_rect center {authoritative:?}; drift {drift:.2} px"
        );
    }
}
