//! Runtime facade and registry resolve tests.
//!
//! Verifies that the registry returns family-aware handles via `resolve()`,
//! and that graph-family rendering is dispatched through the shared runtime
//! facade rather than duplicated in each diagram instance.

use mmdflux::builtins::default_registry;
use mmdflux::registry::DiagramFamily;
use mmdflux::{OutputFormat, RenderConfig};

// ---------------------------------------------------------------------------
// resolve() returns a handle with diagram metadata
// ---------------------------------------------------------------------------

#[test]
fn registry_dispatches_to_compiler_then_graph_family_runtime_dispatch() {
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

// ---------------------------------------------------------------------------
// Graph-family shared facade: flowchart and class use the same runtime path
// ---------------------------------------------------------------------------

#[test]
fn runtime_dispatch_renders_graph_family_from_payload() {
    let output = mmdflux::render_diagram(
        "graph TD\nA[Start]-->B[End]",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("Start"));
    assert!(output.contains("End"));
}

#[test]
fn graph_family_facade_renders_class_text() {
    let output = mmdflux::render_diagram(
        "classDiagram\nclass Animal",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("Animal"));
}

#[test]
fn graph_family_facade_renders_flowchart_svg() {
    let output = mmdflux::render_diagram(
        "graph TD\nA-->B",
        OutputFormat::Svg,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("<svg"));
}

#[test]
fn graph_family_facade_renders_class_svg() {
    let output = mmdflux::render_diagram(
        "classDiagram\nclass Animal",
        OutputFormat::Svg,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("<svg"));
}

#[test]
fn graph_family_facade_renders_flowchart_mmds() {
    let output = mmdflux::render_diagram(
        "graph TD\nA-->B",
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("\"diagram_type\": \"flowchart\""));
}

#[test]
fn graph_family_facade_renders_class_mmds() {
    let output = mmdflux::render_diagram(
        "classDiagram\nclass Animal",
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("\"diagram_type\": \"class\""));
}

// ---------------------------------------------------------------------------
// Timeline family remains independent
// ---------------------------------------------------------------------------

#[test]
fn runtime_dispatch_renders_sequence_without_diagrams_importing_render() {
    let output = mmdflux::render_diagram(
        "sequenceDiagram\nAlice->>Bob: Hello",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(output.contains("Alice"));
    assert!(output.contains("Bob"));
}
