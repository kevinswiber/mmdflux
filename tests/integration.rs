use std::fs;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::mmds::generate_mermaid_from_mmds_str;
use mmdflux::payload::Diagram;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read flowchart fixture {}: {e}", path.display()))
}

fn load_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read MMDS fixture {}: {e}", path.display()))
}

#[test]
fn flowchart_registry_into_payload_returns_graph_payload() {
    let input = load_flowchart_fixture("chain.mmd");
    let payload = default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
        .parse(&input)
        .expect("flowchart fixture should parse")
        .into_payload(&RenderConfig::default())
        .expect("flowchart fixture should build a payload");

    let Diagram::Flowchart(graph) = payload else {
        panic!("flowchart fixture should build a flowchart payload");
    };

    assert!(graph.nodes.contains_key("A"));
    assert!(graph.nodes.contains_key("B"));
    assert!(!graph.edges.is_empty());
}

#[test]
fn runtime_renders_flowchart_fixture_as_text_and_svg() {
    let input = load_flowchart_fixture("decision.mmd");

    let text = render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("text render should succeed");
    let svg = render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("svg render should succeed");

    assert!(text.contains("Yes"));
    assert!(text.contains("No"));
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Yes"));
    assert!(svg.contains("No"));
    assert!(svg.contains("marker-end="));
}

#[test]
fn runtime_renders_positioned_mmds_fixture_as_svg() {
    let payload = load_mmds_fixture("positioned/routed-basic.json");
    let svg = render_diagram(&payload, OutputFormat::Svg, &RenderConfig::default())
        .expect("positioned MMDS fixture should render as SVG");

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<path d=\""));
    assert!(svg.contains("marker-end="));
}

#[test]
fn generated_mermaid_from_mmds_rerenders_through_runtime() {
    let payload = load_mmds_fixture("generation/basic-flow.json");
    let mermaid = generate_mermaid_from_mmds_str(&payload)
        .expect("basic flow MMDS fixture should generate Mermaid");
    let text = render_diagram(&mermaid, OutputFormat::Text, &RenderConfig::default())
        .expect("generated Mermaid should render through the runtime facade");

    assert!(text.contains("Start"));
    assert!(text.contains("End"));
}
