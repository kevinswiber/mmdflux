//! Runtime rendering for graph-family payloads.

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
    SvgRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};
use crate::runtime::config::RenderConfig;
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
            let options = config.svg_render_options();
            Ok(render_svg_from_solve_result(diagram, &result, &options))
        }
        OutputFormat::Text | OutputFormat::Ascii => {
            let options = config.text_render_options(format);
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
    let engine_id = result.engine_id.to_string();
    crate::mmds::to_json_typed_with_routing(
        diagram_type,
        diagram,
        &result.geometry,
        result.routed.as_ref(),
        level,
        path_simplification,
        Some(engine_id.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::builtins::default_registry;
    use crate::graph::{Direction, Edge, Graph, Node};
    use crate::payload::Diagram as Payload;
    use crate::render::graph::TextRenderOptions;
    use crate::runtime::config::RenderConfig;

    fn graph_fixture(input: &str) -> Graph {
        let payload = default_registry()
            .create("flowchart")
            .expect("flowchart should be registered")
            .parse(input)
            .expect("fixture should parse")
            .into_payload()
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

    // -- regression tests (formerly runtime/graph_family/regression_tests.rs) --

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
        let diagram_id =
            crate::detect_diagram(&input).expect("runtime should resolve MMDS fixture");
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
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
    }

    fn smoke_diagram() -> Graph {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B"));
        diagram
    }
}
