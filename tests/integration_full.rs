//! Full integration tests for the multi-format architecture.
//!
//! These tests validate that registry detection, parsing, and rendering work
//! together across diagram types and output formats.

use std::fs;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::mmds::generate_mermaid_from_mmds_str;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn render_with_registry(input: &str, format: OutputFormat) -> String {
    render_diagram(input, format, &RenderConfig::default()).expect("should render")
}

fn render_flowchart_svg(input: &str) -> String {
    render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).expect("should render svg")
}

fn render_flowchart_svg_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("flowchart fixture should render through supported SVG path")
}

fn render_mmds_svg_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    let payload = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read MMDS fixture {}: {e}", path.display()));
    render_diagram(&payload, OutputFormat::Svg, &RenderConfig::default())
        .expect("MMDS fixture should render through supported SVG path")
}

fn assert_direct_and_mmds_svg_smoke(case: &str) {
    let (flowchart_fixture, mmds_fixture) = match case {
        "subgraph_as_node_edge" => (
            "subgraph_as_node_edge.mmd",
            "subgraph-endpoint-intent-present.json",
        ),
        "subgraph_to_subgraph_edge" => (
            "subgraph_to_subgraph_edge.mmd",
            "subgraph-endpoint-subgraph-to-subgraph-present.json",
        ),
        _ => panic!("unknown parity case: {case}"),
    };

    let direct_svg = render_flowchart_svg_fixture(flowchart_fixture);
    let replay_svg = render_mmds_svg_fixture(mmds_fixture);

    assert!(
        direct_svg.starts_with("<svg") && direct_svg.contains("</svg>"),
        "direct SVG render should succeed for case {case}"
    );
    assert!(
        replay_svg.starts_with("<svg") && replay_svg.contains("</svg>"),
        "MMDS replay SVG render should succeed for case {case}"
    );
}

#[test]
fn registry_detects_all_diagram_types() {
    let registry = default_registry();

    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("flowchart LR\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("classDiagram\nclass User"), Some("class"));
    assert_eq!(
        registry.detect("sequenceDiagram\nA->>B: hi"),
        Some("sequence")
    );
}

#[test]
fn all_diagram_types_render_text() {
    let cases = [
        ("graph TD\nA-->B", "A"),
        ("classDiagram\nclass User", "User"),
        ("sequenceDiagram\nAlice->>Bob: hi", "Alice"),
    ];

    for (input, expected) in cases {
        let output = render_with_registry(input, OutputFormat::Text);
        assert!(
            output.contains(expected),
            "output missing expected content for {}",
            input
        );
    }
}

#[test]
fn flowchart_renders_all_formats() {
    let input = "graph TD\nA[Start]-->B[End]";
    let text = render_with_registry(input, OutputFormat::Text);
    assert!(text.contains("Start"));
    assert!(text.contains("End"));
    assert!(text.contains('│'));

    let ascii = render_with_registry(input, OutputFormat::Ascii);
    assert!(ascii.contains("Start"));
    assert!(!ascii.contains('│'));

    let svg = render_with_registry(input, OutputFormat::Svg);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("</svg>"));
}

#[test]
fn svg_shapes_render_expected_elements() {
    let input = r#"graph TD
    A[Rectangle]-->B(Rounded)
    B-->C{Diamond}
    C-->D((Circle))"#;
    let svg = render_flowchart_svg(input);

    assert!(svg.contains("<rect"));
    assert!(svg.contains("rx="));
    assert!(svg.contains("ry="));
    assert!(svg.contains("<polygon"));
    assert!(svg.contains("<ellipse"));
}

#[test]
fn svg_edges_and_subgraphs_render() {
    let input = r#"graph TD
    subgraph sg[Group]
        A-->A
    end
    B-.->C"#;
    let svg = render_flowchart_svg(input);

    assert!(svg.contains("class=\"subgraph\""));
    assert!(svg.contains("Group"));
    assert!(svg.matches("<path").count() >= 2);
    assert!(svg.contains("stroke-dasharray"));
}

#[test]
fn registry_render_smoke() {
    let text = render_with_registry("graph TD\nA-->B", OutputFormat::Text);
    assert!(text.contains("A"));

    let svg = render_with_registry("graph TD\nA-->B", OutputFormat::Svg);
    assert!(svg.starts_with("<svg"));
}

#[test]
fn generated_mermaid_from_mmds_renders_through_registry() {
    let payload = include_str!("fixtures/mmds/generation/basic-flow.json");
    let mermaid = generate_mermaid_from_mmds_str(payload).expect("should generate Mermaid");
    let rendered = render_with_registry(&mermaid, OutputFormat::Text);
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("End"));
}

#[test]
fn subgraph_endpoint_fixture_set_renders_through_direct_and_mmds_paths() {
    for case in ["subgraph_as_node_edge", "subgraph_to_subgraph_edge"] {
        assert_direct_and_mmds_svg_smoke(case);
    }
}
