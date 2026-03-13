use mmdflux::RenderConfig;

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::algorithms::layered::MeasurementMode;
use crate::engines::graph::contracts::{
    EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest,
};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::Diagram;
use crate::graph::measure::default_proportional_text_metrics;
use crate::mermaid::parse_flowchart;
use crate::render::graph::{
    SvgRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
};

#[test]
fn render_svg_basic_flowchart_has_svg_root() {
    let diagram = basic_flowchart_diagram();
    let svg = render_svg(&diagram, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<text"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("End"));
}

fn basic_flowchart_diagram() -> Diagram {
    let flowchart = parse_flowchart("graph TD\nA[Start] --> B[End]\n")
        .expect("basic flowchart smoke input should parse");
    compile_to_graph(&flowchart)
}

fn render_svg(diagram: &Diagram, config: &RenderConfig) -> String {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::new(
        MeasurementMode::Proportional(default_proportional_text_metrics()),
        GraphGeometryContract::Visual,
        config.geometry_level,
        config
            .routing_style
            .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
    );
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(config.layout.clone().into()),
            &request,
        )
        .expect("SVG regression test solve should succeed");

    let options: SvgRenderOptions = config.into();
    if let Some(routed) = result.routed.as_ref() {
        render_svg_from_routed_geometry(diagram, routed, &options)
    } else {
        render_svg_from_geometry(diagram, &result.geometry, &options)
    }
}
