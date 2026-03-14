use std::fs;
use std::path::Path;

use mmdflux::RenderConfig;
use mmdflux::builtins::default_registry;
use mmdflux::graph::{Direction, Graph, Shape};
use mmdflux::mmds::{from_mmds_str, generate_mermaid_from_mmds_str};
use mmdflux::payload::Diagram as Payload;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticDiagram {
    direction: Direction,
    nodes: Vec<SemanticNode>,
    edges: Vec<SemanticEdge>,
    subgraphs: Vec<SemanticSubgraph>,
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticNode {
    id: String,
    label: String,
    shape: Shape,
    parent: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticEdge {
    source: String,
    target: String,
    label: Option<String>,
    stroke: String,
    arrow_start: String,
    arrow_end: String,
    minlen: i32,
    from_subgraph: Option<String>,
    to_subgraph: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticSubgraph {
    id: String,
    title: String,
    // Membership semantics are represented via node.parent + subgraph parent links.
    // Subgraph.nodes list representation is intentionally ignored here.
    parent: Option<String>,
    direction: Option<Direction>,
}

#[test]
fn mmds_mermaid_roundtrip_is_semantically_equivalent() {
    let mmds = fixture("generation/complex-roundtrip.json");
    assert_semantic_roundtrip(&mmds);
}

#[test]
fn mmds_roundtrip_fixture_matrix_is_semantically_equivalent() {
    let fixtures = [
        "generation/basic-flow.json",
        "generation/minlen-style-matrix.json",
        "generation/shapes-and-strokes.json",
        "generation/subgraph-hierarchy.json",
        "generation/nested-membership-roundtrip.json",
        "generation/complex-roundtrip.json",
    ];

    for fixture_path in fixtures {
        let mmds = fixture(fixture_path);
        assert_semantic_roundtrip(&mmds);
    }
}

#[test]
fn nested_subgraph_membership_roundtrip_remains_semantically_equivalent() {
    let mmds = fixture("generation/nested-membership-roundtrip.json");
    assert_semantic_roundtrip(&mmds);
}

fn assert_semantic_roundtrip(mmds: &str) {
    let generated = generate_mermaid_from_mmds_str(mmds).expect("generator output");
    let from_mmds = from_mmds_str(mmds).expect("valid MMDS fixture");
    let payload = default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
        .parse(&generated)
        .expect("generated Mermaid must parse")
        .into_payload(&RenderConfig::default())
        .expect("generated Mermaid must build a payload");
    let Payload::Flowchart(graph_payload) = payload else {
        panic!("generated Mermaid should build a flowchart payload");
    };
    let rebuilt = graph_payload;

    let expected = semantic_diagram(&from_mmds);
    let actual = semantic_diagram(&rebuilt);
    assert_eq!(actual, expected, "generated Mermaid:\n{generated}");
}

fn semantic_diagram(diagram: &Graph) -> SemanticDiagram {
    let mut nodes: Vec<SemanticNode> = diagram
        .nodes
        .values()
        .map(|node| SemanticNode {
            id: node.id.clone(),
            label: node.label.clone(),
            shape: node.shape,
            parent: node.parent.clone(),
        })
        .collect();
    nodes.sort_by(|left, right| left.id.cmp(&right.id));

    let mut edges: Vec<SemanticEdge> = diagram
        .edges
        .iter()
        .map(|edge| SemanticEdge {
            source: edge.from.clone(),
            target: edge.to.clone(),
            label: edge.label.clone(),
            stroke: format!("{:?}", edge.stroke),
            arrow_start: format!("{:?}", edge.arrow_start),
            arrow_end: format!("{:?}", edge.arrow_end),
            minlen: edge.minlen,
            from_subgraph: edge.from_subgraph.clone(),
            to_subgraph: edge.to_subgraph.clone(),
        })
        .collect();
    edges.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then(left.target.cmp(&right.target))
            .then(left.label.cmp(&right.label))
            .then(left.stroke.cmp(&right.stroke))
            .then(left.arrow_start.cmp(&right.arrow_start))
            .then(left.arrow_end.cmp(&right.arrow_end))
            .then(left.minlen.cmp(&right.minlen))
            .then(left.from_subgraph.cmp(&right.from_subgraph))
            .then(left.to_subgraph.cmp(&right.to_subgraph))
    });

    let mut subgraphs: Vec<SemanticSubgraph> = diagram
        .subgraphs
        .values()
        .map(|subgraph| SemanticSubgraph {
            id: subgraph.id.clone(),
            title: subgraph.title.clone(),
            parent: subgraph.parent.clone(),
            direction: subgraph.dir,
        })
        .collect();
    subgraphs.sort_by(|left, right| left.id.cmp(&right.id));

    SemanticDiagram {
        direction: diagram.direction,
        nodes,
        edges,
        subgraphs,
    }
}
