//! Shared graph-family solve/render pipeline.
//!
//! Graph-family diagram instances compile to [`crate::graph::Diagram`] and
//! delegate to this module for engine resolution, layout solve, and final
//! format dispatch. Runtime adapters call the same path.

use crate::config::{
    AlgorithmId, EngineAlgorithmId, EngineId, GeometryLevel, PathSimplification, RenderConfig,
};
use crate::engines::graph::{GraphSolveResult, solve_graph_family};
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::Diagram;
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};

/// Render a graph-family diagram through the shared pipeline.
///
/// Handles engine resolution, layout solve, and format dispatch for all
/// graph-family diagram types. Diagram-specific pre-processing (for example
/// flowchart node-id annotation) should be applied before calling this
/// function.
pub(crate) fn render_graph(
    diagram_id: &str,
    diagram: &Diagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    // Resolve engine (default: flux-layered).
    let engine_id = config
        .layout_engine
        .unwrap_or_else(|| EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered));
    engine_id.check_available()?;
    engine_id.check_routing_style(config)?;

    // Solve layout through the engine registry.
    let result = solve_graph_family(diagram, engine_id, config, format)?;

    // Dispatch to format-owned emitters.
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

fn render_svg_from_solve_result(
    diagram: &Diagram,
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
    diagram: &Diagram,
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
