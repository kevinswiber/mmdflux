//! Runtime rendering for graph-family payloads.

use crate::engines::graph::contracts::{GraphGeometryContract, MeasurementMode};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, EngineId, GraphSolveRequest, GraphSolveResult,
    SubgraphDirectionPolicy, solve_graph_family,
};
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::label_wrap::prepare_wrapped_labels_with_provider;
use crate::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, DEFAULT_PROPORTIONAL_NODE_PADDING_X,
    DEFAULT_PROPORTIONAL_NODE_PADDING_Y, GraphTextStyleKey, ResolvedTextMetrics,
    TextMeasurementCache, TextMetricsProfileConfig, TextMetricsProfileDescriptor,
    TextMetricsProvider, edge_text_style_key, node_text_style_key, resolve_text_metrics_profile,
    text_style_matches_descriptor,
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
            MmdsSolveContext {
                text_metrics_descriptor: &render_result.text_metrics.descriptor,
                text_metrics: &render_result.text_metrics.metrics,
                text_measurements: None,
                level: config.geometry_level,
                path_simplification: config.path_simplification,
            },
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
        MmdsSolveContext {
            text_metrics_descriptor: &render_result.text_metrics.descriptor,
            text_metrics: &render_result.text_metrics.metrics,
            text_measurements: None,
            level: config.geometry_level,
            path_simplification: config.path_simplification,
        },
    )
}

#[cfg(feature = "unstable-text-metrics-provider")]
pub(in crate::runtime) fn render_graph_family_svg_with_provider(
    diagram_id: &str,
    diagram: &mut Graph,
    config: &RenderConfig,
    options: &SvgRenderOptions,
    text_metrics: &dyn TextMetricsProvider,
) -> Result<String, RenderError> {
    let solve = solve_graph_family_with_provider(
        diagram_id,
        diagram,
        OutputFormat::Svg,
        config,
        text_metrics,
    )?;
    render_svg_from_solve_result(diagram, &solve, options, config, text_metrics)
}

#[cfg(feature = "unstable-text-metrics-provider")]
pub(in crate::runtime) fn render_graph_family_mmds_with_provider_and_measurements(
    diagram_id: &str,
    diagram: &mut Graph,
    config: &RenderConfig,
    text_metrics_descriptor: &TextMetricsProfileDescriptor,
    text_metrics: &dyn TextMetricsProvider,
    measurement_cache: impl FnOnce() -> Result<TextMeasurementCache, RenderError>,
) -> Result<String, RenderError> {
    let solve = solve_graph_family_with_provider(
        diagram_id,
        diagram,
        OutputFormat::Mmds,
        config,
        text_metrics,
    )?;
    let measurements = measurement_cache()?;
    render_mmds_from_solve_result_with_measurements(
        diagram_id,
        diagram,
        &solve,
        MmdsSolveContext {
            text_metrics_descriptor,
            text_metrics,
            text_measurements: Some(&measurements),
            level: config.geometry_level,
            path_simplification: config.path_simplification,
        },
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
    let text_metrics = resolve_text_metrics_for_config(format, config)?;
    validate_provider_free_graph_text_style(format, config, &text_metrics.descriptor)?;
    validate_provider_free_mermaid_font_styles(
        format,
        diagram,
        &text_metrics.descriptor,
        &text_metrics.metrics,
    )?;
    let solve = solve_graph_family_with_provider(
        diagram_id,
        diagram,
        format,
        config,
        &text_metrics.metrics,
    )?;

    Ok(GraphFamilyRenderResult {
        solve,
        text_metrics,
    })
}

fn validate_provider_free_graph_text_style(
    format: OutputFormat,
    config: &RenderConfig,
    descriptor: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    let Some(style) = &config.graph_text_style else {
        return Ok(());
    };

    if matches!(format, OutputFormat::Text | OutputFormat::Ascii) {
        return Err(RenderError {
            message: format!(
                "graph font style is not supported for {format} output; remove fontFamily/fontSize"
            ),
        });
    }

    if !matches!(format, OutputFormat::Svg | OutputFormat::Mmds) {
        return Ok(());
    }

    match text_style_matches_descriptor(
        &style.font_family,
        style.font_size_px,
        &descriptor.default_text_style,
    ) {
        Ok(true) => Ok(()),
        Ok(false) => Err(provider_free_graph_text_style_error(descriptor)),
        Err(message) => Err(RenderError {
            message: format!("fontFamily {message}"),
        }),
    }
}

fn provider_free_graph_text_style_error(descriptor: &TextMetricsProfileDescriptor) -> RenderError {
    RenderError {
        message: format!(
            "custom graph font style requires dynamic text metrics; provider-free static rendering only accepts fontFamily '{}' and fontSize {} for text metrics profile '{}'",
            descriptor.default_text_style.font_family,
            descriptor.default_text_style.font_size,
            descriptor.profile_id
        ),
    }
}

fn validate_provider_free_mermaid_font_styles(
    format: OutputFormat,
    diagram: &Graph,
    descriptor: &TextMetricsProfileDescriptor,
    provider: &dyn TextMetricsProvider,
) -> Result<(), RenderError> {
    if !matches!(
        format,
        OutputFormat::Svg | OutputFormat::Mmds | OutputFormat::Text | OutputFormat::Ascii
    ) {
        return Ok(());
    }

    for node in diagram.nodes.values() {
        let style = node_text_style_key(provider, node);
        if !text_style_key_matches_descriptor(&style, &descriptor.default_text_style)? {
            return Err(provider_free_graph_text_style_error(descriptor));
        }
    }

    for edge in &diagram.edges {
        if edge.label.is_none() && edge.head_label.is_none() && edge.tail_label.is_none() {
            continue;
        }
        let style = edge_text_style_key(provider, edge);
        if !text_style_key_matches_descriptor(&style, &descriptor.default_text_style)? {
            return Err(provider_free_graph_text_style_error(descriptor));
        }
    }

    Ok(())
}

fn text_style_key_matches_descriptor(
    style: &GraphTextStyleKey,
    descriptor: &crate::graph::measure::TextMetricsStyleDescriptor,
) -> Result<bool, RenderError> {
    let family_and_size_matches =
        text_style_matches_descriptor(&style.font_family, style.font_size_px(), descriptor)
            .map_err(|message| RenderError {
                message: format!("fontFamily {message}"),
            })?;
    Ok(family_and_size_matches
        && (style.line_height_px() - descriptor.line_height).abs() <= f64::EPSILON)
}

fn solve_graph_family_with_provider(
    diagram_id: &str,
    diagram: &mut Graph,
    format: OutputFormat,
    config: &RenderConfig,
    text_metrics: &dyn TextMetricsProvider,
) -> Result<GraphSolveResult, RenderError> {
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
    prepare_wrapped_labels_with_provider(
        &mut diagram.edges,
        text_metrics,
        config.layout.edge_label_max_width,
    );

    let request = graph_solve_request_for(format, config, diagram_id, text_metrics);
    let engine_config = EngineConfig::Layered(config.layout.clone().into());
    let engine_id = resolve_graph_engine_for_request(engine_id, &request);
    solve_graph_family(diagram, engine_id, &engine_config, &request)
}

fn subgraph_direction_policy_for(diagram_id: &str) -> SubgraphDirectionPolicy {
    match diagram_id {
        "flowchart" => SubgraphDirectionPolicy::AlternateAxes,
        _ => SubgraphDirectionPolicy::Preserve,
    }
}

fn graph_solve_request_for<'a>(
    format: OutputFormat,
    config: &RenderConfig,
    diagram_id: &str,
    text_metrics: &'a dyn TextMetricsProvider,
) -> GraphSolveRequest<'a> {
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
    text_metrics: &dyn TextMetricsProvider,
) -> MeasurementMode<'_> {
    match format {
        OutputFormat::Svg | OutputFormat::Mmds => MeasurementMode::Proportional(text_metrics),
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
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<ResolvedTextMetrics, RenderError> {
    let node_padding_x = config
        .svg_node_padding_x
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_X);
    let node_padding_y = config
        .svg_node_padding_y
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_Y);
    resolve_text_metrics_profile(TextMetricsProfileConfig {
        profile_id: text_metrics_profile_id_for_format(format, config),
        node_padding_x,
        node_padding_y,
        edge_label_max_width: config.layout.edge_label_max_width,
    })
    .map_err(|error| RenderError {
        message: error.to_string(),
    })
}

fn text_metrics_profile_id_for_format(format: OutputFormat, config: &RenderConfig) -> Option<&str> {
    match format {
        // Text/ASCII rendering remains grid-based and compatibility-stable, so
        // caller-supplied proportional profile IDs are ignored for wrap prep.
        OutputFormat::Text | OutputFormat::Ascii => Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID),
        _ => config.font_metrics_profile.as_deref(),
    }
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
    text_metrics: &dyn TextMetricsProvider,
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
    context: MmdsSolveContext<'_>,
) -> Result<String, RenderError> {
    render_mmds_from_solve_result_with_measurements(diagram_type, diagram, result, context)
}

fn render_mmds_from_solve_result_with_measurements(
    diagram_type: &str,
    diagram: &Graph,
    result: &GraphSolveResult,
    context: MmdsSolveContext<'_>,
) -> Result<String, RenderError> {
    let document = mmds_document_from_solve_result(diagram_type, diagram, result, context)?;
    serde_json::to_string_pretty(&document).map_err(|error| RenderError {
        message: format!("MMDS serialization error: {error}"),
    })
}

#[derive(Clone, Copy)]
struct MmdsSolveContext<'a> {
    text_metrics_descriptor: &'a TextMetricsProfileDescriptor,
    text_metrics: &'a dyn TextMetricsProvider,
    text_measurements: Option<&'a TextMeasurementCache>,
    level: GeometryLevel,
    path_simplification: PathSimplification,
}

fn mmds_document_from_solve_result(
    diagram_type: &str,
    diagram: &Graph,
    result: &GraphSolveResult,
    context: MmdsSolveContext<'_>,
) -> Result<Document, RenderError> {
    let engine_id = result.engine_id.to_string();
    crate::mmds::document::to_document_typed_with_routing_and_text_metrics(
        diagram_type,
        diagram,
        &result.geometry,
        result.routed.as_ref(),
        context.level,
        context.path_simplification,
        Some(engine_id.as_str()),
        Some(context.text_metrics_descriptor),
        Some(context.text_metrics),
        context.text_measurements,
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
            MmdsSolveContext {
                text_metrics_descriptor: &text_metrics.descriptor,
                text_metrics: &text_metrics.metrics,
                text_measurements: None,
                level: GeometryLevel::Routed,
                path_simplification: PathSimplification::default(),
            },
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
