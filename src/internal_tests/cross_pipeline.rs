use std::fs;
use std::path::Path;

use crate::builtins::default_registry;
use crate::engines::graph::algorithms::layered::MeasurementMode;
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphGeometryContract, GraphSolveRequest, solve_graph_family,
};
use crate::graph::GeometryLevel;
use crate::prepared::PreparedDiagram;
use crate::render::graph::{TextRenderOptions, render_text_from_geometry};
use crate::{OutputFormat, RenderConfig};

#[test]
fn top_level_render_matches_flowchart_instance_for_subgraph_direction_mixed() {
    let input = flowchart_fixture("subgraph_direction_mixed.mmd");
    let top_level = render_text_via_owner_pipeline("subgraph_direction_mixed.mmd");

    let instance_output =
        crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
            .expect("instance render should succeed");

    assert_eq!(
        top_level, instance_output,
        "top-level render() should match the flowchart instance text pipeline for subgraph fixtures"
    );
}

fn flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn render_text_via_owner_pipeline(name: &str) -> String {
    let input = flowchart_fixture(name);
    let prepared = default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
        .parse(&input)
        .expect("fixture should parse")
        .prepare(&RenderConfig::default())
        .expect("fixture should prepare");
    let PreparedDiagram::Graph(graph) = prepared else {
        panic!("flowchart fixture should prepare a graph payload");
    };

    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        GeometryLevel::Layout,
        None,
    );
    let result = solve_graph_family(
        &graph.diagram,
        EngineAlgorithmId::FLUX_LAYERED,
        &EngineConfig::Layered(Default::default()),
        &request,
    )
    .expect("owner pipeline should solve");

    render_text_from_geometry(
        &graph.diagram,
        &result.geometry,
        result.routed.as_ref(),
        &TextRenderOptions::default(),
    )
}
