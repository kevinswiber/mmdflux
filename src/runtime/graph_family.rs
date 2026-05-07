//! Runtime rendering for graph-family payloads.

use crate::engines::graph::contracts::{GraphGeometryContract, MeasurementMode};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, EngineId, GraphSolveRequest, GraphSolveResult,
    SubgraphDirectionPolicy, solve_graph_family,
};
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::label_wrap::prepare_wrapped_labels;
use crate::graph::measure::{
    DEFAULT_PROPORTIONAL_NODE_PADDING_X, DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
    ProportionalTextMetrics, ResolvedTextMetrics, TextMetricsProfileConfig,
    TextMetricsProfileDescriptor, resolve_text_metrics_profile,
};
use crate::graph::{GeometryLevel, Graph};
use crate::mmds::Document;
use crate::render::graph::{
    SvgRenderOptions, render_svg_from_geometry_with_theme_routing_and_metrics,
    render_svg_from_routed_geometry_with_theme_and_metrics, render_text_from_geometry,
};
use crate::runtime::config::RenderConfig;
use crate::runtime::resolve_configured_svg_theme;
use crate::simplification::PathSimplification;

pub(crate) fn render_graph_family(
    diagram_id: &str,
    diagram: &mut Graph,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let render_result = solve_graph_family_for_render(diagram_id, diagram, format, config)?;

    match format {
        OutputFormat::Mmds => render_mmds_from_solve_result(
            diagram_id,
            diagram,
            &render_result.solve,
            &render_result.text_metrics.descriptor,
            config.geometry_level,
            config.path_simplification,
        ),
        OutputFormat::Svg => {
            let options = config.svg_render_options();
            Ok(render_svg_from_solve_result(
                diagram,
                &render_result.solve,
                &options,
                config,
                &render_result.text_metrics.metrics,
            )?)
        }
        OutputFormat::Text | OutputFormat::Ascii => {
            let options = config.text_render_options(format);
            Ok(render_text_from_geometry(
                diagram,
                &render_result.solve.geometry,
                render_result.solve.routed.as_ref(),
                &options,
            ))
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

pub(crate) fn materialize_graph_family(
    diagram_id: &str,
    diagram: &mut Graph,
    config: &RenderConfig,
) -> Result<Document, RenderError> {
    let render_result =
        solve_graph_family_for_render(diagram_id, diagram, OutputFormat::Mmds, config)?;
    mmds_document_from_solve_result(
        diagram_id,
        diagram,
        &render_result.solve,
        &render_result.text_metrics.descriptor,
        config.geometry_level,
        config.path_simplification,
    )
}

struct GraphFamilyRenderResult {
    solve: GraphSolveResult,
    text_metrics: ResolvedTextMetrics,
}

fn solve_graph_family_for_render(
    diagram_id: &str,
    diagram: &mut Graph,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<GraphFamilyRenderResult, RenderError> {
    let engine_id = config
        .layout_engine
        .unwrap_or(EngineAlgorithmId::FLUX_LAYERED);
    engine_id.check_available()?;
    engine_id.check_routing_style(
        config
            .routing_style
            .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
    )?;

    // Pre-engine wrap pass. Populates
    // `diagram::Edge.wrapped_label_lines` once per render so the kernel
    // sizing scan, label geometry, SVG text, and MMDS replay all agree on
    // the wrap decision. Uses Proportional metrics regardless of render
    // mode — Grid consumers read the same line splits (see design.md §6.1).
    // Threshold comes from `RenderConfig.layout.edge_label_max_width`; the
    // user-facing LayoutConfig default is `Some(200.0)` so wrap is on by
    // default. Explicit `None` disables wrap (dagre-parity fallback).
    let text_metrics = resolve_text_metrics_for_config(config)?;
    prepare_wrapped_labels(
        &mut diagram.edges,
        &text_metrics.metrics,
        config.layout.edge_label_max_width,
    );

    let request = graph_solve_request_for(format, config, diagram_id, &text_metrics.metrics);
    let engine_config = EngineConfig::Layered(config.layout.clone().into());
    let engine_id = resolve_graph_engine_for_request(engine_id, &request);
    let solve = solve_graph_family(diagram, engine_id, &engine_config, &request)?;

    Ok(GraphFamilyRenderResult {
        solve,
        text_metrics,
    })
}

fn subgraph_direction_policy_for(diagram_id: &str) -> SubgraphDirectionPolicy {
    match diagram_id {
        "flowchart" => SubgraphDirectionPolicy::AlternateAxes,
        _ => SubgraphDirectionPolicy::Preserve,
    }
}

fn graph_solve_request_for(
    format: OutputFormat,
    config: &RenderConfig,
    diagram_id: &str,
    text_metrics: &ProportionalTextMetrics,
) -> GraphSolveRequest {
    let routing_style = config
        .routing_style
        .or_else(|| config.edge_preset.map(|preset| preset.expand().0));
    GraphSolveRequest::new(
        measurement_mode_for_format(format, text_metrics),
        geometry_contract_for_format(format),
        config.geometry_level,
        routing_style,
        subgraph_direction_policy_for(diagram_id),
    )
}

fn measurement_mode_for_format(
    format: OutputFormat,
    text_metrics: &ProportionalTextMetrics,
) -> MeasurementMode {
    match format {
        OutputFormat::Svg | OutputFormat::Mmds => {
            MeasurementMode::Proportional(text_metrics.clone())
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

fn resolve_text_metrics_for_config(
    config: &RenderConfig,
) -> Result<ResolvedTextMetrics, RenderError> {
    let node_padding_x = config
        .svg_node_padding_x
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_X);
    let node_padding_y = config
        .svg_node_padding_y
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_Y);
    resolve_text_metrics_profile(TextMetricsProfileConfig {
        profile_id: config.font_metrics_profile.as_deref(),
        node_padding_x,
        node_padding_y,
        edge_label_max_width: config.layout.edge_label_max_width,
    })
    .map_err(|error| RenderError {
        message: error.to_string(),
    })
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
    config: &RenderConfig,
    text_metrics: &ProportionalTextMetrics,
) -> Result<String, RenderError> {
    let theme = resolve_configured_svg_theme(config)?;

    Ok(match result.routed.as_ref() {
        Some(routed) => render_svg_from_routed_geometry_with_theme_and_metrics(
            diagram,
            routed,
            options,
            theme.as_ref(),
            text_metrics,
        ),
        None => render_svg_from_geometry_with_theme_routing_and_metrics(
            diagram,
            &result.geometry,
            options,
            crate::render::graph::edge_routing_from_style(options.routing_style),
            theme.as_ref(),
            text_metrics,
        ),
    })
}

fn render_mmds_from_solve_result(
    diagram_type: &str,
    diagram: &Graph,
    result: &GraphSolveResult,
    text_metrics_descriptor: &TextMetricsProfileDescriptor,
    level: GeometryLevel,
    path_simplification: PathSimplification,
) -> Result<String, RenderError> {
    let document = mmds_document_from_solve_result(
        diagram_type,
        diagram,
        result,
        text_metrics_descriptor,
        level,
        path_simplification,
    )?;
    serde_json::to_string_pretty(&document).map_err(|error| RenderError {
        message: format!("MMDS serialization error: {error}"),
    })
}

fn mmds_document_from_solve_result(
    diagram_type: &str,
    diagram: &Graph,
    result: &GraphSolveResult,
    text_metrics_descriptor: &TextMetricsProfileDescriptor,
    level: GeometryLevel,
    path_simplification: PathSimplification,
) -> Result<Document, RenderError> {
    let engine_id = result.engine_id.to_string();
    crate::mmds::document::to_document_typed_with_routing_and_text_metrics(
        diagram_type,
        diagram,
        &result.geometry,
        result.routed.as_ref(),
        level,
        path_simplification,
        Some(engine_id.as_str()),
        Some(text_metrics_descriptor),
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
            Default::default(),
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
        let text_metrics = resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .expect("default text metrics should resolve");
        let svg = render_svg_from_solve_result(
            &diagram,
            &result,
            &SvgRenderOptions::default(),
            &RenderConfig::default(),
            &text_metrics.metrics,
        )
        .expect("SVG render should succeed");
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn mmds_renderer_consumes_graph_solve_result() {
        let (diagram, result) = graph_solve_result_fixture();
        let text_metrics = resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .expect("default text metrics should resolve");
        let json = render_mmds_from_solve_result(
            "flowchart",
            &diagram,
            &result,
            &text_metrics.descriptor,
            GeometryLevel::Routed,
            PathSimplification::default(),
        )
        .expect("MMDS render should succeed");
        assert!(json.contains("\"nodes\""));
    }

    // -- regression tests (formerly runtime/graph_family/regression_tests.rs) --

    #[test]
    fn runtime_owner_local_smoke_renders_graph_family_text() {
        let mut diagram = smoke_diagram();
        let rendered = super::render_graph_family(
            "flowchart",
            &mut diagram,
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
