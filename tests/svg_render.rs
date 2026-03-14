use std::fs;
use std::path::Path;

use mmdflux::format::{CornerStyle, Curve, RoutingStyle};
use mmdflux::simplification::PathSimplification;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, render_diagram};

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

fn render_svg(input: &str, config: &RenderConfig) -> String {
    render_diagram(input, OutputFormat::Svg, config).expect("SVG render should succeed")
}

#[test]
fn basic_flowchart_svg_has_root_text_and_arrow_marker() {
    let input = "graph TD\nA[Start] --> B[End]\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("End"));
    assert!(svg.contains("marker-end="));
    assert!(svg.contains("<path d=\""));
}

#[test]
fn svg_runtime_honors_supported_style_options() {
    let input = load_flowchart_fixture("complex.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            layout_engine: Some(
                EngineAlgorithmId::parse("flux-layered")
                    .expect("flux-layered engine id should parse"),
            ),
            routing_style: Some(RoutingStyle::Orthogonal),
            curve: Some(Curve::Linear(CornerStyle::Rounded)),
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<text"));
    assert!(svg.contains("<path d=\""));
}

#[test]
fn positioned_mmds_payload_renders_svg_through_runtime() {
    let payload = load_mmds_fixture("positioned/routed-fan-in-ports.json");
    let svg = render_svg(
        &payload,
        &RenderConfig {
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("marker-end="));
    assert!(svg.contains("<path d=\""));
}
