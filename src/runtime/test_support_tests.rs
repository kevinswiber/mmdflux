use crate::OutputFormat;
use crate::engines::graph::algorithms::layered::MeasurementMode;
use crate::engines::graph::algorithms::layered::layout_building::layered_config_for_layout;
use crate::engines::graph::contracts::{
    EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest,
};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::Diagram;
use crate::graph::grid::{GridLayout, GridLayoutConfig, geometry_to_grid_layout_with_routed};
use crate::graph_family_pipeline::render_graph;

pub(crate) fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> GridLayout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        crate::GeometryLevel::Layout,
        None,
    );
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("runtime test layout solve failed");

    geometry_to_grid_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}

pub(crate) fn render_text_diagram(diagram: &Diagram) -> String {
    render_graph(
        "flowchart",
        diagram,
        OutputFormat::Text,
        &crate::RenderConfig::default(),
    )
    .expect("runtime test text render failed")
}
