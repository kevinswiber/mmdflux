//! Graph-family pipeline boundary tests.
//!
//! Verifies that flowchart and class diagrams share the same graph-family
//! pipeline contract, and that text/SVG/MMDS renderers consume graph-family
//! contracts (not parser-specific or renderer-specific state).

use mmdflux::graph::Diagram;
use mmdflux::registry::default_registry;
use mmdflux::testing::{EngineConfig, GraphEngineRegistry, GraphSolveRequest, GraphSolveResult};
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

/// Produce a GraphSolveResult fixture from a simple flowchart.
fn graph_solve_result_fixture() -> (Diagram, GraphSolveResult) {
    let input = "graph TD\n    A[Start] --> B[End]\n";
    let fc = mmdflux::frontends::mermaid::parse_flowchart(input).unwrap();
    let diagram = mmdflux::diagrams::flowchart::compile_to_graph(&fc);
    let registry = GraphEngineRegistry::default();
    let engine_id = EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered);
    let engine = registry.get_solver(engine_id).unwrap();
    let config = EngineConfig::Layered(Default::default());
    let request = GraphSolveRequest::from_config(&Default::default(), OutputFormat::Text);
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
    let text = mmdflux::testing::backends::text::render_text(&diagram, &result);
    assert!(
        text.contains("Start"),
        "text output should contain node labels"
    );
}

#[test]
fn svg_renderer_consumes_graph_solve_result() {
    let (diagram, result) = graph_solve_result_fixture();
    let svg = mmdflux::testing::backends::svg::render_svg(&diagram, &result);
    assert!(svg.contains("<svg"), "SVG output should start with <svg");
}

#[test]
fn mmds_renderer_consumes_graph_solve_result() {
    let (diagram, result) = graph_solve_result_fixture();
    let json = mmdflux::testing::backends::mmds::render_mmds("flowchart", &diagram, &result);
    assert!(
        json.contains("\"nodes\""),
        "MMDS output should contain nodes key"
    );
}

// ---------------------------------------------------------------------------
// Contract: graph-family pipeline runs end-to-end through shared path
// ---------------------------------------------------------------------------

#[test]
fn graph_family_pipeline_end_to_end() {
    // Both flowchart and class should produce valid output through
    // the graph-family pipeline (engine → geometry → routing → format).
    for path in &[
        "tests/fixtures/flowchart/simple.mmd",
        "tests/fixtures/class/simple.mmd",
    ] {
        let input = std::fs::read_to_string(path).unwrap();
        let registry = default_registry();
        let id = registry.detect(&input).unwrap();
        let mut instance = registry.create(id).unwrap();
        instance.parse(&input).unwrap();

        // Text output
        let text = instance
            .render(OutputFormat::Text, &Default::default())
            .unwrap();
        assert!(!text.is_empty(), "{path}: text output should not be empty");

        // SVG output
        let svg = instance
            .render(OutputFormat::Svg, &Default::default())
            .unwrap();
        assert!(
            svg.contains("<svg"),
            "{path}: SVG output should contain <svg"
        );

        // MMDS output
        let mmds = instance
            .render(OutputFormat::Mmds, &Default::default())
            .unwrap();
        assert!(
            mmds.contains("\"nodes\""),
            "{path}: MMDS output should contain nodes"
        );
    }
}
