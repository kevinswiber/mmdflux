use mmdflux::builtins::default_registry;
use mmdflux::format::EdgePreset;
use mmdflux::graph::GeometryLevel;
use mmdflux::payload::Diagram;
use mmdflux::registry::DiagramInstance;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, RenderError};

fn render_flowchart(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    mmdflux::render_diagram(input, format, config)
}

fn flowchart_instance() -> Box<dyn DiagramInstance> {
    default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
}

fn edge_path_data(svg: &str) -> Vec<String> {
    svg.lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("<path d=\"")
                && (line.contains("marker-end=") || line.contains("marker-start="))
        })
        .filter_map(|line| {
            let start = line.find("d=\"")?;
            let after = &line[start + 3..];
            let end = after.find('"')?;
            Some(after[..end].to_string())
        })
        .collect()
}

fn parse_svg_path_points(path_data: &str) -> Vec<(f64, f64)> {
    path_data
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            let (x, y) = token.split_once(',')?;
            let x = x.parse::<f64>().ok()?;
            let y = y.parse::<f64>().ok()?;
            Some((x, y))
        })
        .collect()
}

fn min_segment_len(points: &[(f64, f64)]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    points
        .windows(2)
        .map(|segment| {
            let dx = segment[1].0 - segment[0].0;
            let dy = segment[1].1 - segment[0].1;
            (dx * dx + dy * dy).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn flowchart_instance_parse_simple() {
    let result = flowchart_instance().parse("graph TD\nA-->B");
    assert!(result.is_ok());
}

#[test]
fn flowchart_instance_parse_error_on_invalid() {
    let result = flowchart_instance().parse("not a valid diagram }{{}");
    assert!(result.is_err());
}

#[test]
fn flowchart_instance_render_text() {
    let output = render_flowchart(
        "graph TD\nA-->B",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn flowchart_instance_render_ascii() {
    let output = render_flowchart(
        "graph TD\nA-->B",
        OutputFormat::Ascii,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(!output.contains('│'));
    assert!(!output.contains('─'));
}

#[test]
fn flowchart_supports_text_ascii_and_svg() {
    let registry = default_registry();
    assert!(registry.supports_format("flowchart", OutputFormat::Text));
    assert!(registry.supports_format("flowchart", OutputFormat::Ascii));
    assert!(registry.supports_format("flowchart", OutputFormat::Svg));
    assert!(!registry.supports_format("flowchart", OutputFormat::Mermaid));
}

#[test]
fn flowchart_instance_render_svg() {
    let output = render_flowchart(
        "graph TD\nA-->B",
        OutputFormat::Svg,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.starts_with("<svg"));
    assert!(output.contains("<text"));
}

#[test]
fn flowchart_instance_render_json() {
    let output = flowchart_instance()
        .parse("graph TD\nA[Start] --> B[End]")
        .unwrap()
        .into_payload()
        .unwrap();
    let Diagram::Flowchart(graph) = output else {
        panic!("flowchart should yield a flowchart payload");
    };
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
}

#[test]
fn flowchart_instance_render_json_uses_defaults_omission() {
    let output = render_flowchart(
        "graph TD\nA-->B",
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(parsed["defaults"]["node"]["shape"], "rectangle");
    assert_eq!(parsed["defaults"]["edge"]["stroke"], "solid");
    assert_eq!(parsed["defaults"]["edge"]["minlen"], 1);
    assert_eq!(parsed["edges"][0]["id"], "e0");
    assert!(parsed["edges"][0]["stroke"].is_null());
    assert!(parsed["edges"][0]["arrow_start"].is_null());
    assert!(parsed["edges"][0]["arrow_end"].is_null());
    assert!(parsed["edges"][0]["minlen"].is_null());
}

#[test]
fn test_show_ids_annotates_labels() {
    let config = RenderConfig {
        show_ids: true,
        ..Default::default()
    };
    let output = mmdflux::render_diagram(
        "graph TD\nA[Start] --> B[End]\n",
        OutputFormat::Text,
        &config,
    )
    .unwrap();
    assert!(output.contains("A: Start"));
    assert!(output.contains("B: End"));
}

#[test]
fn test_show_ids_bare_nodes_unchanged() {
    let config = RenderConfig {
        show_ids: true,
        ..Default::default()
    };
    let output =
        mmdflux::render_diagram("graph TD\nA --> B\n", OutputFormat::Text, &config).unwrap();
    // Bare nodes (label == id) should not get "A: A" annotation
    assert!(!output.contains("A: A"));
    assert!(!output.contains("B: B"));
}

#[test]
fn test_show_ids_false_no_annotation() {
    let output = mmdflux::render_diagram(
        "graph TD\nA[Start] --> B[End]\n",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(!output.contains("A:"));
}

#[test]
fn test_json_with_show_ids() {
    let config = RenderConfig {
        show_ids: true,
        ..Default::default()
    };
    let output = render_flowchart(
        "graph TD\nA[Start] --> B[End]\n",
        OutputFormat::Mmds,
        &config,
    )
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

    let nodes = parsed["nodes"].as_array().unwrap();
    let node_a = nodes.iter().find(|n| n["id"] == "A").unwrap();
    assert_eq!(node_a["label"], "A: Start");
}

#[test]
fn test_json_without_show_ids() {
    let output = render_flowchart(
        "graph TD\nA[Start] --> B[End]\n",
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

    let nodes = parsed["nodes"].as_array().unwrap();
    let node_a = nodes.iter().find(|n| n["id"] == "A").unwrap();
    assert_eq!(node_a["label"], "Start");
}

// --- Solve-path integration tests (Task 4.1) ---

#[test]
fn flowchart_render_text_through_solve_path() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    let output = render_flowchart("graph TD\nA-->B", OutputFormat::Text, &config).unwrap();
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn flowchart_render_svg_through_solve_path() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    let output = render_flowchart("graph TD\nA-->B", OutputFormat::Svg, &config).unwrap();
    assert!(output.contains("<svg"));
}

#[test]
fn flowchart_render_mmds_through_solve_path() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let output = render_flowchart("graph TD\nA-->B", OutputFormat::Mmds, &config).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(json["edges"][0]["path"].is_array());
}

// --- Text geometry-driven integration tests (Task 4.2) ---

#[test]
fn text_render_from_solve_produces_output() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple.mmd").unwrap();
    let output = render_flowchart(&input, OutputFormat::Text, &RenderConfig::default()).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn text_snapshots_stable_after_geometry_driven_refactor() {
    for fixture in &["simple.mmd", "chain.mmd", "decision.mmd", "fan_in.mmd"] {
        let path = format!("tests/fixtures/flowchart/{fixture}");
        let input = std::fs::read_to_string(&path).unwrap();
        let output =
            render_flowchart(&input, OutputFormat::Text, &RenderConfig::default()).unwrap();
        let snapshot_path = format!(
            "tests/snapshots/flowchart/{}",
            fixture.replace(".mmd", ".txt")
        );
        if std::path::Path::new(&snapshot_path).exists() {
            let expected = std::fs::read_to_string(&snapshot_path).unwrap();
            assert_eq!(output, expected, "snapshot mismatch for {fixture}");
        }
    }
}

// --- Engine selection tests (Task 2.2) ---

#[test]
fn engine_selection_none_uses_default_layered() {
    let config = RenderConfig::default(); // layout_engine: None
    let output = render_flowchart("graph TD\nA-->B", OutputFormat::Text, &config).unwrap();
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn engine_selection_explicit_layered_matches_default() {
    let default_output = render_flowchart(
        "graph TD\nA-->B",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();

    let layered_config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    let layered_output =
        render_flowchart("graph TD\nA-->B", OutputFormat::Text, &layered_config).unwrap();

    assert_eq!(default_output, layered_output);
}

#[cfg(not(feature = "engine-elk"))]
#[test]
fn engine_selection_unavailable_engine_errors() {
    let err = EngineAlgorithmId::parse("elk-layered").unwrap_err();
    assert!(
        err.message.contains("not available"),
        "error should indicate unavailability: {}",
        err.message
    );
}

#[test]
fn engine_selection_unknown_engine_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(
        err.message.contains("unknown engine"),
        "error should mention unknown engine: {}",
        err.message
    );
}

#[test]
fn flowchart_instance_svg_polyline_ampersand_avoids_micro_corner_jogs_for_layered_engines() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/ampersand.mmd")
        .expect("fixture should load");

    for engine in ["flux-layered", "mermaid-layered"] {
        let config = RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse(engine).expect("engine id should parse")),
            edge_preset: Some(EdgePreset::Polyline),
            ..Default::default()
        };

        let svg = render_flowchart(&input, OutputFormat::Svg, &config)
            .expect("svg render should succeed");
        let paths: Vec<Vec<(f64, f64)>> = edge_path_data(&svg)
            .iter()
            .map(|path| parse_svg_path_points(path))
            .collect();
        assert!(
            paths.len() >= 4,
            "ampersand should render four routed edges, got {} for {engine}",
            paths.len()
        );

        for (edge_pos, path) in paths.iter().take(2).enumerate() {
            let min_len = min_segment_len(path);
            assert!(
                min_len >= 8.0,
                "ampersand fan-in edge {edge_pos} should not contain tiny corner jog segments for {engine}: min_segment={min_len:.2}, path={path:?}"
            );
        }
    }
}
