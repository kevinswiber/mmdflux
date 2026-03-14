//! Runtime rendering for graph-family payloads.

use crate::config::RenderConfig;
use crate::engines::graph::algorithms::layered::MeasurementMode;
use crate::engines::graph::contracts::GraphGeometryContract;
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, EngineId, GraphSolveRequest, GraphSolveResult,
    solve_graph_family,
};
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::measure::{
    DEFAULT_PROPORTIONAL_FONT_SIZE, DEFAULT_PROPORTIONAL_NODE_PADDING_X,
    DEFAULT_PROPORTIONAL_NODE_PADDING_Y, ProportionalTextMetrics,
};
use crate::graph::{GeometryLevel, Graph};
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};
use crate::simplification::PathSimplification;

pub(in crate::runtime) fn render_graph_family(
    diagram_id: &str,
    diagram: &Graph,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let engine_id = config
        .layout_engine
        .unwrap_or(EngineAlgorithmId::FLUX_LAYERED);
    engine_id.check_available()?;
    engine_id.check_routing_style(
        config
            .routing_style
            .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
    )?;
    let request = graph_solve_request_for(format, config);
    let engine_config = EngineConfig::Layered(config.layout.clone().into());
    let engine_id = resolve_graph_engine_for_request(engine_id, &request);
    let result = solve_graph_family(diagram, engine_id, &engine_config, &request)?;

    match format {
        OutputFormat::Mmds => render_mmds_from_solve_result(
            diagram_id,
            diagram,
            &result,
            config.geometry_level,
            config.path_simplification,
        ),
        OutputFormat::Svg => {
            let options: SvgRenderOptions = config.into();
            Ok(render_svg_from_solve_result(diagram, &result, &options))
        }
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options: TextRenderOptions = config.into();
            options.output_format = format;
            Ok(render_text_from_geometry(
                diagram,
                &result.geometry,
                result.routed.as_ref(),
                &options,
            ))
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

fn graph_solve_request_for(format: OutputFormat, config: &RenderConfig) -> GraphSolveRequest {
    let routing_style = config
        .routing_style
        .or_else(|| config.edge_preset.map(|preset| preset.expand().0));
    GraphSolveRequest::new(
        measurement_mode_for_format(format, config),
        geometry_contract_for_format(format),
        config.geometry_level,
        routing_style,
    )
}

fn measurement_mode_for_format(format: OutputFormat, config: &RenderConfig) -> MeasurementMode {
    match format {
        OutputFormat::Svg | OutputFormat::Mmds => {
            MeasurementMode::Proportional(proportional_text_metrics_for_config(config))
        }
        _ => MeasurementMode::Grid,
    }
}

fn geometry_contract_for_format(format: OutputFormat) -> GraphGeometryContract {
    match format {
        OutputFormat::Svg => GraphGeometryContract::Visual,
        _ => GraphGeometryContract::Canonical,
    }
}

fn proportional_text_metrics_for_config(config: &RenderConfig) -> ProportionalTextMetrics {
    let node_padding_x = config
        .svg_node_padding_x
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_X);
    let node_padding_y = config
        .svg_node_padding_y
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_Y);
    ProportionalTextMetrics::new(
        DEFAULT_PROPORTIONAL_FONT_SIZE,
        node_padding_x,
        node_padding_y,
    )
}

fn resolve_graph_engine_for_request(
    engine_id: EngineAlgorithmId,
    request: &GraphSolveRequest,
) -> EngineAlgorithmId {
    if engine_id.engine() == EngineId::Mermaid
        && matches!(request.measurement_mode, MeasurementMode::Grid)
    {
        EngineAlgorithmId::new(EngineId::Flux, engine_id.algorithm())
    } else {
        engine_id
    }
}

fn render_svg_from_solve_result(
    diagram: &Graph,
    result: &GraphSolveResult,
    options: &SvgRenderOptions,
) -> String {
    match result.routed.as_ref() {
        Some(routed) => render_svg_from_routed_geometry(diagram, routed, options),
        None => render_svg_from_geometry(diagram, &result.geometry, options),
    }
}

fn render_mmds_from_solve_result(
    diagram_type: &str,
    diagram: &Graph,
    result: &GraphSolveResult,
    level: GeometryLevel,
    path_simplification: PathSimplification,
) -> Result<String, RenderError> {
    crate::mmds::to_mmds_json_typed_with_routing(
        diagram_type,
        diagram,
        &result.geometry,
        result.routed.as_ref(),
        level,
        path_simplification,
        Some(result.engine_id),
    )
}

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::default_registry;
    use crate::graph::Graph;
    use crate::payload::Diagram as Payload;

    fn graph_fixture(input: &str) -> Graph {
        let payload = default_registry()
            .create("flowchart")
            .expect("flowchart should be registered")
            .parse(input)
            .expect("fixture should parse")
            .into_payload(&RenderConfig::default())
            .expect("fixture should build a payload");
        let Payload::Flowchart(graph) = payload else {
            panic!("flowchart should yield a flowchart payload");
        };
        graph
    }

    fn graph_solve_result_fixture() -> (Graph, GraphSolveResult) {
        let diagram = graph_fixture("graph TD\n    A[Start] --> B[End]\n");
        let request = GraphSolveRequest::new(
            MeasurementMode::Grid,
            GraphGeometryContract::Canonical,
            GeometryLevel::Layout,
            None,
        );
        let result = solve_graph_family(
            &diagram,
            EngineAlgorithmId::FLUX_LAYERED,
            &EngineConfig::Layered(Default::default()),
            &request,
        )
        .expect("graph solve should succeed");
        (diagram, result)
    }

    #[test]
    fn text_renderer_consumes_graph_solve_result() {
        let (diagram, result) = graph_solve_result_fixture();
        let text = render_text_from_geometry(
            &diagram,
            &result.geometry,
            result.routed.as_ref(),
            &TextRenderOptions::default(),
        );
        assert!(text.contains("Start"));
    }

    #[test]
    fn svg_renderer_consumes_graph_solve_result() {
        let (diagram, result) = graph_solve_result_fixture();
        let svg = render_svg_from_solve_result(&diagram, &result, &SvgRenderOptions::default());
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn mmds_renderer_consumes_graph_solve_result() {
        let (diagram, result) = graph_solve_result_fixture();
        let json = render_mmds_from_solve_result(
            "flowchart",
            &diagram,
            &result,
            GeometryLevel::Routed,
            PathSimplification::default(),
        )
        .expect("MMDS render should succeed");
        assert!(json.contains("\"nodes\""));
    }
}
