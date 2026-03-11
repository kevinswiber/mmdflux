// Verify core expected items are accessible from crate root.
use mmdflux::{
    Diagram,
    DiagramType,
    Direction,
    Edge,
    Node,
    ParseError,
    Shape,
    build_diagram,
    detect_diagram_type,
    // Core diagram abstractions
    diagram::{DiagramFamily, OutputFormat, RenderConfig},
    // Diagrams
    diagrams::flowchart,
    parse_flowchart,
    // Registry
    registry::{DiagramInstance, default_registry},
    // Direct rendering API
    render::{RenderOptions, render},
};

#[test]
fn all_exports_accessible() {
    // This test passes if it compiles
    let _ = OutputFormat::default();
    let _ = DiagramFamily::Graph;
    let _: Box<dyn DiagramInstance> = Box::new(flowchart::FlowchartInstance::new());
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
