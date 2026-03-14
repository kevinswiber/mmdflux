use std::fs;
use std::path::Path;

use crate::config::RenderConfig;
use crate::format::OutputFormat;
use crate::graph::{Direction, Edge, Graph, Node};

#[test]
fn runtime_owner_local_smoke_renders_graph_family_text() {
    let rendered = super::render_graph_family(
        "flowchart",
        &smoke_diagram(),
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .expect("runtime graph-family smoke render should succeed");

    assert!(rendered.contains("Start"));
}

#[test]
fn runtime_entrypoint_dispatches_mmds_input_through_frontend() {
    let input = mmds_fixture("minimal-layout.json");
    let diagram_id = crate::detect_diagram(&input).expect("runtime should resolve MMDS fixture");
    assert_eq!(diagram_id, "flowchart");

    let output = crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("layout MMDS payload should render via runtime frontend dispatch");
    assert!(output.contains("Start"));
    assert!(output.contains("End"));
}

fn mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn smoke_diagram() -> Graph {
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram
}
