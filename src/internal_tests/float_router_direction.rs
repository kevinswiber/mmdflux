//! Float-router tests that require cross-boundary imports (mermaid + diagrams).
//! Moved from engines::graph::algorithms::layered::float_router to respect
//! module boundary rules.

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::algorithms::layered::float_layout::build_float_layout_with_flags;
use crate::graph::Direction;
use crate::graph::direction_policy::build_node_directions;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::EdgeRouting;
use crate::mermaid::parse_flowchart;

#[test]
fn test_build_node_directions_basic() {
    let input = "graph TD\nsubgraph sg1\ndirection LR\nA --> B\nend\nC --> D\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let dirs = build_node_directions(&diagram);

    assert_eq!(dirs.get("A").copied(), Some(Direction::LeftRight));
    assert_eq!(dirs.get("B").copied(), Some(Direction::LeftRight));
    assert_eq!(dirs.get("C").copied(), Some(Direction::TopDown));
    assert_eq!(dirs.get("D").copied(), Some(Direction::TopDown));
}

#[test]
fn test_build_node_directions_nested_deepest_wins() {
    let input =
        "graph TD\nsubgraph outer\ndirection LR\nsubgraph inner\ndirection BT\nA --> B\nend\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let dirs = build_node_directions(&diagram);

    // Deepest override wins
    assert_eq!(dirs.get("A").copied(), Some(Direction::BottomTop));
    assert_eq!(dirs.get("B").copied(), Some(Direction::BottomTop));
}

#[test]
fn test_reroute_spreads_shared_face_attachment_points() {
    // Two cross-boundary edges entering the same node A from its top face.
    // Check that the engine-produced paths end at different x positions.
    let input = "graph TD\nsubgraph s1\ndirection LR\nA --> B\nend\nC --> A\nD --> A\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&flowchart);
    let metrics = ProportionalTextMetrics::new(16.0, 15.0, 15.0);
    let geometry = build_float_layout_with_flags(
        &diagram,
        &Default::default(),
        &metrics,
        EdgeRouting::EngineProvided,
        false,
        None,
    );

    let endpoints: Vec<_> = geometry
        .edges
        .iter()
        .filter_map(|edge| {
            let diagram_edge = diagram.edges.get(edge.index)?;
            if diagram_edge.to != "A" {
                return None;
            }
            edge.layout_path_hint
                .as_ref()
                .and_then(|path| path.last())
                .copied()
        })
        .collect();

    assert_eq!(
        endpoints.len(),
        2,
        "expected exactly two rerouted endpoints into A"
    );
    assert!(
        (endpoints[0].x - endpoints[1].x).abs() > 0.01,
        "shared-face endpoints should be spread apart: {endpoints:?}"
    );
}
