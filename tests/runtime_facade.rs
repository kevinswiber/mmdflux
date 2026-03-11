//! Runtime facade and registry resolve tests.
//!
//! Verifies that the registry returns family-aware handles via `resolve()`,
//! and that graph-family rendering is dispatched through a shared runtime
//! facade rather than duplicated in each diagram instance.

use mmdflux::registry::default_registry;
use mmdflux::{DiagramFamily, OutputFormat, RenderConfig};

// ---------------------------------------------------------------------------
// resolve() returns a handle with diagram metadata
// ---------------------------------------------------------------------------

#[test]
fn registry_dispatches_to_compiler_then_family_pipeline() {
    let registry = default_registry();
    let handle = registry.resolve("graph TD\nA-->B").unwrap();

    assert_eq!(handle.diagram_id(), "flowchart");
    assert_eq!(handle.family(), DiagramFamily::Graph);
}

#[test]
fn sequence_diagrams_compile_to_timeline_family_without_graph_renderer_dependencies() {
    let registry = default_registry();
    let handle = registry.resolve("sequenceDiagram\nA->>B: hi").unwrap();

    assert_eq!(handle.family(), DiagramFamily::Timeline);
}

#[test]
fn resolve_returns_none_for_unrecognized_input() {
    let registry = default_registry();
    assert!(registry.resolve("not a diagram at all!!!").is_none());
}

#[test]
fn resolve_class_diagram_to_graph_family() {
    let registry = default_registry();
    let handle = registry.resolve("classDiagram\nclass User").unwrap();

    assert_eq!(handle.diagram_id(), "class");
    assert_eq!(handle.family(), DiagramFamily::Graph);
}

#[test]
fn resolve_pie_to_chart_family() {
    let registry = default_registry();
    let handle = registry.resolve("pie\n\"A\": 50").unwrap();

    assert_eq!(handle.diagram_id(), "pie");
    assert_eq!(handle.family(), DiagramFamily::Chart);
}

// ---------------------------------------------------------------------------
// Graph-family shared facade: flowchart and class use the same render path
// ---------------------------------------------------------------------------

#[test]
fn graph_family_facade_renders_flowchart_text() {
    let registry = default_registry();
    let mut instance = registry.create("flowchart").unwrap();
    instance.parse("graph TD\nA[Start]-->B[End]").unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("Start"));
    assert!(output.contains("End"));
}

#[test]
fn graph_family_facade_renders_class_text() {
    let registry = default_registry();
    let mut instance = registry.create("class").unwrap();
    instance.parse("classDiagram\nclass Animal").unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("Animal"));
}

#[test]
fn graph_family_facade_renders_flowchart_svg() {
    let registry = default_registry();
    let mut instance = registry.create("flowchart").unwrap();
    instance.parse("graph TD\nA-->B").unwrap();
    let output = instance
        .render(OutputFormat::Svg, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("<svg"));
}

#[test]
fn graph_family_facade_renders_class_svg() {
    let registry = default_registry();
    let mut instance = registry.create("class").unwrap();
    instance.parse("classDiagram\nclass Animal").unwrap();
    let output = instance
        .render(OutputFormat::Svg, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("<svg"));
}

#[test]
fn graph_family_facade_renders_flowchart_mmds() {
    let registry = default_registry();
    let mut instance = registry.create("flowchart").unwrap();
    instance.parse("graph TD\nA-->B").unwrap();
    let output = instance
        .render(OutputFormat::Mmds, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("\"diagram_type\": \"flowchart\""));
}

#[test]
fn graph_family_facade_renders_class_mmds() {
    let registry = default_registry();
    let mut instance = registry.create("class").unwrap();
    instance.parse("classDiagram\nclass Animal").unwrap();
    let output = instance
        .render(OutputFormat::Mmds, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("\"diagram_type\": \"class\""));
}

// ---------------------------------------------------------------------------
// Timeline family remains independent
// ---------------------------------------------------------------------------

#[test]
fn timeline_family_renders_without_graph_engine() {
    let registry = default_registry();
    let mut instance = registry.create("sequence").unwrap();
    instance
        .parse("sequenceDiagram\nAlice->>Bob: Hello")
        .unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(output.contains("Alice"));
    assert!(output.contains("Bob"));
}
