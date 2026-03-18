use std::fs;
use std::path::Path;

use mmdflux::graph::{Direction, GeometryLevel, Graph, Node};
use mmdflux::mmds::SUPPORTED_OUTPUT_FORMATS as MMDS_SUPPORTED_OUTPUT_FORMATS;
use mmdflux::payload::Diagram;
use mmdflux::registry::{
    DiagramDefinition, DiagramFamily, DiagramInstance, DiagramRegistry, ParsedDiagram,
};
use mmdflux::{OutputFormat, RenderError};

struct MockDiagram;

struct MockParsedDiagram {
    content: String,
}

impl MockDiagram {
    fn new() -> Self {
        Self
    }
}

impl DiagramInstance for MockDiagram {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(MockParsedDiagram {
            content: input.to_string(),
        }))
    }
}

impl ParsedDiagram for MockParsedDiagram {
    fn into_payload(self: Box<Self>) -> Result<Diagram, RenderError> {
        let mut graph = Graph::new(Direction::TopDown);
        graph.add_node(Node::new(&self.content));
        Ok(Diagram::Flowchart(graph))
    }
}

#[test]
fn diagram_instance_parse_and_into_payload() {
    let payload = Box::new(MockDiagram::new())
        .parse("test input")
        .unwrap()
        .into_payload()
        .unwrap();
    let Diagram::Flowchart(graph) = payload else {
        panic!("mock should yield a flowchart payload");
    };
    assert!(graph.nodes.contains_key("test input"));
}

#[test]
fn diagram_instance_into_payload_returns_expected_variant() {
    let payload = Box::new(MockDiagram::new())
        .parse("test input")
        .unwrap()
        .into_payload()
        .unwrap();
    assert!(matches!(payload, Diagram::Flowchart(_)));
}

#[test]
fn diagram_instance_trait_is_phase_split() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("registry.rs"),
    )
    .unwrap();
    let diagram_instance_block = source
        .split("pub trait ParsedDiagram")
        .next()
        .expect("DiagramInstance trait should precede ParsedDiagram");

    assert!(!source.contains("fn render(&self"));
    assert!(source.contains("fn parse(\n        self: Box<Self>,"));
    assert!(source.contains("pub trait ParsedDiagram"));
    assert!(!diagram_instance_block.contains("fn into_payload("));
}

#[test]
fn registry_supports_format_queries_definition() {
    let mut registry = DiagramRegistry::new();
    registry.register(DiagramDefinition {
        id: "mock",
        family: DiagramFamily::Graph,
        detector: |_| true,
        factory: || Box::new(MockDiagram::new()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii],
    });
    assert!(registry.supports_format("mock", OutputFormat::Text));
    assert!(registry.supports_format("mock", OutputFormat::Ascii));
    assert!(!registry.supports_format("mock", OutputFormat::Svg));
}

#[test]
fn registry_create_returns_instance() {
    let mut registry = DiagramRegistry::new();

    registry.register(DiagramDefinition {
        id: "mock",
        family: DiagramFamily::Graph,
        detector: |_| true,
        factory: || Box::new(MockDiagram::new()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii],
    });

    let instance = registry.create("mock").expect("should create instance");
    let payload = instance.parse("hello").unwrap().into_payload().unwrap();
    let Diagram::Flowchart(graph) = payload else {
        panic!("mock should yield a flowchart payload");
    };
    assert!(graph.nodes.contains_key("hello"));
}

#[test]
fn registry_create_unknown_returns_none() {
    let registry = DiagramRegistry::new();
    assert!(registry.create("unknown").is_none());
}

#[test]
fn mmds_frontend_is_not_registered_as_diagram_type() {
    let registry = mmdflux::builtins::default_registry();

    assert!(
        registry.get("mmds").is_none(),
        "MMDS should not appear as a registered logical diagram type"
    );
}

#[test]
fn mmds_format_supported_list_includes_replay_formats() {
    assert!(MMDS_SUPPORTED_OUTPUT_FORMATS.contains(&OutputFormat::Text));
    assert!(MMDS_SUPPORTED_OUTPUT_FORMATS.contains(&OutputFormat::Svg));
    assert!(MMDS_SUPPORTED_OUTPUT_FORMATS.contains(&OutputFormat::Mmds));
}

#[test]
fn geometry_level_default_is_layout() {
    assert_eq!(GeometryLevel::default(), GeometryLevel::Layout);
}
