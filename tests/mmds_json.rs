//! MMDS JSON contract tests.
//!
//! Verifies that `--format mmds` output (with `json` alias) matches the MMDS specification:
//! - Default output is `geometry_level: "layout"` with no edge geometry.
//! - Routed output is explicit opt-in with edge paths and bounds.

use std::path::Path;

use mmdflux::diagram::{
    EngineAlgorithmId, GeometryLevel, OutputFormat, PathSimplification, RenderConfig,
};
use mmdflux::diagrams::flowchart::FlowchartInstance;
use mmdflux::mmds::MmdsOutput;
use mmdflux::registry::DiagramInstance;
use serde_json::Value;

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
    let mut instance = FlowchartInstance::new();
    instance.parse(input).unwrap();
    instance
        .render(OutputFormat::Mmds, &RenderConfig::default())
        .unwrap()
}

fn render_json_with_level(input: &str, level: GeometryLevel) -> String {
    let mut instance = FlowchartInstance::new();
    instance.parse(input).unwrap();
    let config = RenderConfig {
        geometry_level: level,
        ..RenderConfig::default()
    };
    instance.render(OutputFormat::Mmds, &config).unwrap()
}

fn render_routed_mmds_with_engine(input: &str, engine: &str) -> String {
    let mut instance = FlowchartInstance::new();
    instance.parse(input).unwrap();
    instance
        .render(
            OutputFormat::Mmds,
            &RenderConfig {
                geometry_level: GeometryLevel::Routed,
                layout_engine: EngineAlgorithmId::parse(engine).ok(),
                ..RenderConfig::default()
            },
        )
        .unwrap()
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
// -----------------------------------------------------------------------
// Contract: MMDS envelope
// -----------------------------------------------------------------------

#[test]
fn mmds_default_has_version_1() {
    let json = render_json("graph TD\nA-->B");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.version, 1);
}

#[test]
fn mmds_default_geometry_level_is_layout() {
    let json = render_json("graph TD\nA-->B");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, "layout");
}

#[test]
fn mmds_has_metadata_with_direction() {
    let json = render_json("graph LR\nA-->B");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.metadata.diagram_type, "flowchart");
    assert_eq!(output.metadata.direction, "LR");
}

#[test]
fn mmds_has_nodes_and_edges() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.nodes.len(), 2);
    assert_eq!(output.edges.len(), 1);
}

// -----------------------------------------------------------------------
// Contract: layout-level node geometry
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_nodes_have_positions_and_sizes() {
    let json = render_json("graph TD\nA[Start]-->B[End]");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

    let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
    assert_eq!(node_a.label, "Start");
    assert_eq!(node_a.shape, "rectangle");
    assert!(node_a.size.width > 0.0);
    assert!(node_a.size.height > 0.0);
}

#[test]
fn mmds_layout_nodes_sorted_by_id() {
    let json = render_json("graph TD\nC-->B\nB-->A");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    let ids: Vec<&str> = output.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn mmds_layout_node_shapes() {
    let json = render_json("graph TD\nA[Rect]\nB(Round)\nC{Diamond}\nD([Stadium])");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
        let mut instance = FlowchartInstance::new();
        instance.parse(&input).unwrap();
        instance
            .render(
                OutputFormat::Mmds,
                &RenderConfig {
                    geometry_level: GeometryLevel::Routed,
                    path_simplification,
                    layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                    ..RenderConfig::default()
                },
            )
            .unwrap()
    };

    let full = render_for(PathSimplification::None);
    let compact = render_for(PathSimplification::Lossless);
    let simplified = render_for(PathSimplification::Lossy);

    let full: MmdsOutput = serde_json::from_str(&full).unwrap();
    let compact: MmdsOutput = serde_json::from_str(&compact).unwrap();
    let simplified: MmdsOutput = serde_json::from_str(&simplified).unwrap();

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
        let mut instance = FlowchartInstance::new();
        instance.parse(&input).unwrap();
        let mut config = RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        };
        if let Some(path_simplification) = path_simplification {
            config.path_simplification = path_simplification;
        }
        instance.render(OutputFormat::Mmds, &config).unwrap()
    };
    let edge_len = |json: &str| {
        let output: MmdsOutput = serde_json::from_str(json).unwrap();
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
        let mut instance = FlowchartInstance::new();
        instance.parse(&input).unwrap();
        instance
            .render(
                OutputFormat::Mmds,
                &RenderConfig {
                    geometry_level: GeometryLevel::Routed,
                    path_simplification,
                    layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                    ..RenderConfig::default()
                },
            )
            .unwrap()
    };
    let edge_len = |json: &str| {
        let output: MmdsOutput = serde_json::from_str(json).unwrap();
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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert!(output.metadata.bounds.width > 0.0);
    assert!(output.metadata.bounds.height > 0.0);
}

// -----------------------------------------------------------------------
// Contract: layout-level subgraphs
// -----------------------------------------------------------------------

#[test]
fn mmds_layout_subgraphs() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

    assert_eq!(output.subgraphs.len(), 1);
    assert_eq!(output.subgraphs[0].id, "sg1");
    assert_eq!(output.subgraphs[0].title, "Group");
    assert!(output.subgraphs[0].direction.is_none());
    assert!(output.subgraphs[0].bounds.is_none());
}

#[test]
fn mmds_layout_subgraph_direction_override() {
    let json = render_json("graph TD\nsubgraph sg1[Group]\ndirection LR\nA-->B\nend");
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.subgraphs[0].direction.as_deref(), Some("LR"));
}

// -----------------------------------------------------------------------
// Contract: routed-level output
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_has_geometry_level_routed() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(output.geometry_level, "routed");
}

#[test]
fn mmds_routed_includes_edge_paths() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert!(edge.path.is_some());
    assert!(edge.path.as_ref().unwrap().len() >= 2);
    assert!(edge.is_backward.is_some());
}

#[test]
fn mmds_routed_includes_metadata_bounds() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

    let sg = &output.subgraphs[0];
    assert!(sg.bounds.is_some());
    assert!(sg.bounds.as_ref().unwrap().width > 0.0);
}

#[test]
fn mmds_routed_label_position_for_labeled_edge() {
    let json = render_json_with_level("graph TD\nA--label-->B", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();

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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
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
        let output: MmdsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.metadata.direction, expected);
    }
}

// -----------------------------------------------------------------------
// Contract: class diagram MMDS output
// -----------------------------------------------------------------------

#[test]
fn mmds_class_diagram_produces_json() {
    use mmdflux::diagrams::class::ClassInstance;

    let mut instance = ClassInstance::new();
    instance.parse("classDiagram\nA --> B").unwrap();

    let config = RenderConfig::default();
    let output = instance.render(OutputFormat::Mmds, &config).unwrap();
    let parsed: MmdsOutput = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.geometry_level, "layout");
    assert_eq!(parsed.metadata.diagram_type, "class");
    assert!(!output.contains("\"path\""));
}

#[test]
fn mmds_class_diagram_routed_level() {
    use mmdflux::diagrams::class::ClassInstance;

    let mut instance = ClassInstance::new();
    instance.parse("classDiagram\nA --> B").unwrap();

    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..RenderConfig::default()
    };
    let output = instance.render(OutputFormat::Mmds, &config).unwrap();
    let parsed: MmdsOutput = serde_json::from_str(&output).unwrap();

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
    let payload = mmds_fixture("profiles/profiles-svg-v1.json");
    assert_schema_valid(payload);
}

#[test]
fn schema_rejects_invalid_extension_namespace_shape() {
    let payload = mmds_fixture("invalid/extensions-not-object.json");
    assert_schema_invalid(payload);
}

#[test]
fn mmds_profiles_and_extensions_roundtrip_through_serde() {
    let payload = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("mmds")
            .join("profiles")
            .join("profiles-svg-v1.json"),
    )
    .unwrap();
    let parsed: MmdsOutput = serde_json::from_str(&payload).unwrap();
    let json = serde_json::to_string(&parsed).unwrap();

    assert!(json.contains("profiles"));
    assert!(json.contains("org.mmdflux.render.svg.v1"));
}

#[test]
fn mmds_spec_doc_exists() {
    assert!(Path::new("docs/mmds.md").exists());
}

#[test]
fn mmds_examples_exist() {
    assert!(Path::new("examples/mmds/react_flow.js").exists());
    assert!(Path::new("examples/mmds/cytoscape.js").exists());
    assert!(Path::new("examples/mmds/d3.js").exists());
    assert!(Path::new("examples/mmds/svg_passthrough.js").exists());
    assert!(Path::new("examples/mmds/profile-mmdflux-svg-v1.json").exists());
    assert!(Path::new("examples/mmds/profile-mmdflux-text-v1.json").exists());
}

#[test]
fn readme_mentions_mmds() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    assert!(readme.contains("MMDS"));
}

#[test]
fn canonical_profile_examples_validate_against_schema() {
    for path in [
        "examples/mmds/profile-mmdflux-svg-v1.json",
        "examples/mmds/profile-mmdflux-text-v1.json",
    ] {
        let absolute = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let raw = std::fs::read_to_string(&absolute)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", absolute.display()));
        let payload: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("invalid JSON {}: {err}", absolute.display()));
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

// -----------------------------------------------------------------------
// Contract: port metadata (routed level only)
// -----------------------------------------------------------------------

#[test]
fn mmds_routed_includes_port_metadata() {
    let json = render_json_with_level("graph TD\nA-->B", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
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
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    let sp = edge.source_port.as_ref().unwrap();
    let tp = edge.target_port.as_ref().unwrap();
    assert_eq!(sp.face, "bottom", "TD source should exit bottom");
    assert_eq!(tp.face, "top", "TD target should enter top");
}

#[test]
fn mmds_routed_port_fractions_fan_in() {
    let json = render_json_with_level("graph TD\nA-->C\nB-->C", GeometryLevel::Routed);
    let output: MmdsOutput = serde_json::from_str(&json).unwrap();
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
    let mut instance = FlowchartInstance::new();
    instance.parse(input).unwrap();

    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let output = instance.render(OutputFormat::Mmds, &config).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(json["metadata"]["engine"], "flux-layered");
}

#[test]
fn mmds_layout_output_omits_edge_paths_regardless_of_engine() {
    let input = "graph TD\nA-->B";
    let mut instance = FlowchartInstance::new();
    instance.parse(input).unwrap();

    let config = RenderConfig {
        geometry_level: GeometryLevel::Layout,
        ..Default::default()
    };
    let output = instance.render(OutputFormat::Mmds, &config).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();

    // Layout level should not have edge paths
    assert!(
        json["edges"][0]["path"].is_null()
            || !json["edges"][0].as_object().unwrap().contains_key("path")
    );
}
