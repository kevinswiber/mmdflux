//! Runtime-owned graph dispatch boundary tests.
//!
//! Verifies that flowchart and class diagrams share the same graph-family
//! prepare/solve/render contract, and that text/SVG/MMDS renderers consume
//! graph-family contracts (not parser-specific or renderer-specific state).

use std::path::Path;

use mmdflux::config::{GeometryLevel, PathSimplification};
use mmdflux::engines::graph::algorithms::layered::MeasurementMode;
use mmdflux::engines::graph::contracts::{
    EngineConfig, GraphGeometryContract, GraphSolveRequest, GraphSolveResult,
};
use mmdflux::engines::graph::registry::GraphEngineRegistry;
use mmdflux::graph::{Diagram, EdgeRouting, route_graph_geometry};
use mmdflux::mmds::to_mmds_json_typed;
use mmdflux::registry_builtins::default_registry;
use mmdflux::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_text_from_geometry,
};
use mmdflux::{AlgorithmId, DiagramFamily, EngineAlgorithmId, EngineId, OutputFormat};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse and compile a fixture to its graph::Diagram via the registry.
fn compile_graph_fixture(path: &str) -> (Diagram, DiagramFamily) {
    let input = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let registry = default_registry();
    let id = registry
        .detect(&input)
        .unwrap_or_else(|| panic!("failed to detect diagram type for {path}"));
    let family = registry.get(id).unwrap().family;
    let diagram = match id {
        "flowchart" => {
            let fc = mmdflux::frontends::mermaid::parse_flowchart(&input).unwrap();
            mmdflux::diagrams::flowchart::compile_to_graph(&fc)
        }
        "class" => {
            let model = mmdflux::frontends::mermaid::class::parse_class_diagram(&input).unwrap();
            mmdflux::diagrams::class::compiler::compile(&model)
        }
        _ => panic!("unexpected diagram type: {id}"),
    };
    (diagram, family)
}

fn repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Produce a GraphSolveResult fixture from a simple flowchart.
fn graph_solve_result_fixture() -> (Diagram, GraphSolveResult) {
    let input = "graph TD\n    A[Start] --> B[End]\n";
    let fc = mmdflux::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = mmdflux::diagrams::flowchart::compile_to_graph(&fc);
    let registry = GraphEngineRegistry::default();
    let engine_id = EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered);
    let engine = registry.get_solver(engine_id).unwrap();
    let config = EngineConfig::Layered(Default::default());
    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        GeometryLevel::Layout,
        None,
    );
    let result = engine.solve(&diagram, &config, &request).unwrap();
    (diagram, result)
}

// ---------------------------------------------------------------------------
// Contract: both diagram types belong to the Graph family
// ---------------------------------------------------------------------------

#[test]
fn flowchart_and_class_compile_to_the_same_graph_family_contract() {
    let (_, fc_family) = compile_graph_fixture("tests/fixtures/flowchart/simple.mmd");
    let (_, class_family) = compile_graph_fixture("tests/fixtures/class/simple.mmd");

    assert_eq!(fc_family, DiagramFamily::Graph);
    assert_eq!(class_family, DiagramFamily::Graph);
}

// ---------------------------------------------------------------------------
// Contract: format emitters consume GraphSolveResult, not parser state
// ---------------------------------------------------------------------------

#[test]
fn text_renderer_consumes_graph_solve_result() {
    let (diagram, result) = graph_solve_result_fixture();
    let text = render_text_from_geometry(
        &diagram,
        &result.geometry,
        result.routed.as_ref(),
        &TextRenderOptions::default(),
    );
    assert!(
        text.contains("Start"),
        "text output should contain node labels"
    );
}

#[test]
fn svg_renderer_consumes_graph_solve_result() {
    let (diagram, result) = graph_solve_result_fixture();
    let svg = render_svg_from_geometry(&diagram, &result.geometry, &SvgRenderOptions::default());
    assert!(svg.contains("<svg"), "SVG output should start with <svg");
}

#[test]
fn mmds_renderer_consumes_graph_solve_result() {
    let (diagram, result) = graph_solve_result_fixture();
    let routed = result.routed.as_ref().cloned().unwrap_or_else(|| {
        route_graph_geometry(&diagram, &result.geometry, EdgeRouting::OrthogonalRoute)
    });
    let json = to_mmds_json_typed(
        "flowchart",
        &diagram,
        &result.geometry,
        Some(&routed),
        GeometryLevel::Routed,
        PathSimplification::default(),
        Some(result.engine_id),
    )
    .expect("MMDS render should succeed");
    assert!(
        json.contains("\"nodes\""),
        "MMDS output should contain nodes key"
    );
}

// ---------------------------------------------------------------------------
// Contract: runtime-owned graph dispatch runs end-to-end through shared path
// ---------------------------------------------------------------------------

#[test]
fn graph_runtime_dispatch_end_to_end() {
    // Both flowchart and class should produce valid output through
    // the graph-family runtime path (engine → geometry → routing → format).
    for path in &[
        "tests/fixtures/flowchart/simple.mmd",
        "tests/fixtures/class/simple.mmd",
    ] {
        let input = std::fs::read_to_string(path).unwrap();

        // Text output
        let text =
            mmdflux::render_diagram(&input, OutputFormat::Text, &Default::default()).unwrap();
        assert!(!text.is_empty(), "{path}: text output should not be empty");

        // SVG output
        let svg = mmdflux::render_diagram(&input, OutputFormat::Svg, &Default::default()).unwrap();
        assert!(
            svg.contains("<svg"),
            "{path}: SVG output should contain <svg"
        );

        // MMDS output
        let mmds =
            mmdflux::render_diagram(&input, OutputFormat::Mmds, &Default::default()).unwrap();
        assert!(
            mmds.contains("\"nodes\""),
            "{path}: MMDS output should contain nodes"
        );
    }
}

#[test]
fn architecture_docs_no_longer_reference_graph_family_pipeline() {
    let docs = repo_file("docs/architecture/dependency-rules.md");
    assert!(!docs.contains("graph_family_pipeline"));
}

#[test]
fn crate_root_no_longer_declares_graph_family_pipeline_shim() {
    let lib_rs = repo_file("src/lib.rs");
    assert!(!lib_rs.contains("mod graph_family_pipeline;"));
}
