use crate::engines::graph::algorithms::layered::layout_building::layered_config_for_layout;
use crate::engines::graph::contracts::{EngineConfig, GraphEngine, GraphSolveRequest};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::Diagram;
use crate::graph_family_pipeline::render_graph;
use crate::render::graph::text_replay::{
    GridLayoutConfig, Layout, geometry_to_text_layout_with_routed,
};
use crate::{OutputFormat, RenderConfig};

pub(crate) fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> Layout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::from_config(&RenderConfig::default(), OutputFormat::Text);
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("runtime test layout solve failed");

    geometry_to_text_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}

pub(crate) fn render_text_diagram(diagram: &Diagram) -> String {
    render_graph(
        "flowchart",
        diagram,
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .expect("runtime test text render failed")
}
