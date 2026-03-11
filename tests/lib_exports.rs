use std::collections::BTreeSet;
use std::path::Path;

// Verify core expected items are accessible from crate root.
use mmdflux::{
    Diagram,
    DiagramType,
    Direction,
    Edge,
    Node,
    OutputFormat,
    ParseError,
    RenderConfig,
    RenderError,
    RenderRequest,
    Shape,
    build_diagram,
    default_registry,
    detect_diagram_type,
    // Diagrams
    diagrams::flowchart,
    parse_flowchart,
    // Registry
    registry::DiagramInstance,
    // Direct rendering API
    render::{RenderOptions, render},
};

fn public_exports_for_test() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap();
    let mut exports = BTreeSet::new();

    let joined = content.replace('\n', " ");
    for segment in joined.split("pub use ").skip(1) {
        let Some(stmt) = segment.split(';').next() else {
            continue;
        };
        let stmt = stmt.trim();

        if let Some(brace_start) = stmt.find('{') {
            let brace_end = stmt.find('}').unwrap_or(stmt.len());
            let symbols = &stmt[brace_start + 1..brace_end];
            for sym in symbols.split(',') {
                let sym = sym.trim();
                if !sym.is_empty() {
                    exports.insert(sym.to_string());
                }
            }
        } else if let Some(colon_pos) = stmt.rfind("::") {
            exports.insert(stmt[colon_pos + 2..].trim().to_string());
        }
    }

    exports
}

#[test]
fn all_exports_accessible() {
    // This test passes if it compiles
    let _ = OutputFormat::default();
    let _ = RenderConfig::default();
    let _ = RenderError::from("surface");
    let _ = RenderRequest::new("graph TD\nA-->B", OutputFormat::Text);
    let _: Box<dyn DiagramInstance> = Box::new(flowchart::FlowchartInstance::new());
}

#[test]
fn crate_root_exports_only_the_curated_render_and_validation_surface() {
    let exports = public_exports_for_test();

    assert!(exports.contains("OutputFormat"));
    assert!(exports.contains("RenderConfig"));
    assert!(exports.contains("RenderError"));
    assert!(exports.contains("RenderRequest"));
}

#[test]
fn engine_level_solve_types_are_not_crate_root_api() {
    let exports = public_exports_for_test();

    assert!(!exports.contains("GraphEngine"));
    assert!(!exports.contains("GraphSolveRequest"));
    assert!(!exports.contains("GraphSolveResult"));
    assert!(!exports.contains("EngineConfig"));
    assert!(!exports.contains("EdgeRouting"));
}

#[test]
fn implementor_traits_and_engine_capabilities_are_not_crate_root_api() {
    let exports = public_exports_for_test();

    assert!(!exports.contains("DiagramModel"));
    assert!(!exports.contains("DiagramParser"));
    assert!(!exports.contains("DiagramRenderer"));
    assert!(!exports.contains("EngineAlgorithmCapabilities"));
    assert!(!exports.contains("RouteOwnership"));
    assert!(!exports.contains("LayoutConfig"));
}

#[test]
fn registry_api_works() {
    let registry = default_registry();
    let input = "graph TD\n    A-->B";

    let diagram_id = registry.detect(input).unwrap();
    assert_eq!(diagram_id, "flowchart");

    let mut instance = registry.create(diagram_id).unwrap();
    instance.parse(input).unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn direct_render_api_works() {
    let input = "graph TD\nA-->B";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);
    let output = render(&diagram, &RenderOptions::default());
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn core_types_accessible() {
    let _ = DiagramType::Flowchart;
    let _ = detect_diagram_type("graph TD\nA-->B");
    let _ = ParseError::UnexpectedEof;

    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_shape(Shape::Rectangle));
    diagram.add_node(Node::new("B"));
    diagram.add_edge(Edge::new("A", "B"));
}

#[test]
fn legacy_layered_module_is_only_a_compatibility_facade_during_breakup() {
    let mut graph = mmdflux::layered::DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_edge("A", "B");

    let _ = mmdflux::layered::Ranker::NetworkSimplex;
    let result = mmdflux::engines::graph::layered::run_layered_layout(
        &graph,
        &mmdflux::layered::LayoutConfig::default(),
        |_, dims| *dims,
    );

    assert!(
        result
            .nodes
            .contains_key(&mmdflux::layered::NodeId::from("A"))
    );
    assert!(
        result
            .nodes
            .contains_key(&mmdflux::layered::NodeId::from("B"))
    );
}
