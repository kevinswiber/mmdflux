//! MMDS replay pipeline: hydrate to graph-family IR and re-render.

use std::fmt::Display;

use super::detect::resolve_logical_diagram_id;
use super::hydrate::{
    from_mmds_output, hydrate_graph_geometry_from_output_with_diagram,
    hydrate_routed_geometry_from_output,
};
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::GeometryLevel;
use crate::mmds::{MmdsOutput, generate_mermaid_from_mmds, parse_mmds_input};
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};

/// Render MMDS input through the MMDS replay path.
pub fn render_input(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let payload =
        parse_mmds_input(input).map_err(|error| prefixed_display_error("parse error", error))?;
    render_output(&payload, format, config)
}

/// Render a parsed MMDS payload through the MMDS replay path.
pub fn render_output(
    payload: &MmdsOutput,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let diagram_id = resolve_logical_diagram_id(payload)?;
    let has_routed_geometry = payload.geometry_level == "routed";

    if !matches!(payload.geometry_level.as_str(), "layout" | "routed") {
        return Err(RenderError {
            message: format!(
                "MMDS validation error: invalid geometry_level '{}'",
                payload.geometry_level
            ),
        });
    }

    if matches!(format, OutputFormat::Mmds) {
        let output = if has_routed_geometry && config.geometry_level == GeometryLevel::Layout {
            strip_routed_fields(payload)
        } else {
            payload.clone()
        };
        return serde_json::to_string_pretty(&output)
            .map_err(|error| prefixed_display_error("MMDS serialization error", error));
    }

    if matches!(format, OutputFormat::Mermaid) {
        return generate_mermaid_from_mmds(payload).map_err(display_error);
    }

    let diagram = from_mmds_output(payload).map_err(display_error)?;

    let geometry = hydrate_graph_geometry_from_output_with_diagram(payload, &diagram)
        .map_err(display_error)?;
    let routed = has_routed_geometry
        .then(|| hydrate_routed_geometry_from_output(payload))
        .transpose()
        .map_err(display_error)?;

    match format {
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options: TextRenderOptions = config.into();
            options.output_format = format;
            Ok(render_text_from_geometry(
                &diagram,
                &geometry,
                routed.as_ref(),
                &options,
            ))
        }
        OutputFormat::Svg => {
            let options: SvgRenderOptions = config.into();
            Ok(match routed.as_ref() {
                Some(routed) => render_svg_from_routed_geometry(&diagram, routed, &options),
                None => render_svg_from_geometry(&diagram, &geometry, &options),
            })
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

fn display_error(error: impl Display) -> RenderError {
    RenderError {
        message: error.to_string(),
    }
}

fn prefixed_display_error(prefix: &str, error: impl Display) -> RenderError {
    RenderError {
        message: format!("{prefix}: {error}"),
    }
}

fn strip_routed_fields(payload: &MmdsOutput) -> MmdsOutput {
    let mut output = payload.clone();
    output.geometry_level = "layout".to_string();
    for edge in &mut output.edges {
        edge.path = None;
        edge.label_position = None;
        edge.is_backward = None;
    }
    for subgraph in &mut output.subgraphs {
        subgraph.bounds = None;
    }
    output
}
