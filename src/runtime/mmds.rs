//! MMDS replay rendering: hydrate to graph-family IR and re-render.
//!
//! The mmds module owns the interchange format (parse, hydrate, serialize).
//! This module owns the render dispatch for MMDS input.

use std::fmt::Display;

use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::GeometryLevel;
use crate::mmds::{
    Output, from_output, generate_mermaid, hydrate_graph_geometry_from_output_with_diagram,
    hydrate_routed_geometry_from_output, parse_input, resolve_logical_diagram_id,
};
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};

/// Render MMDS input through the MMDS replay path.
pub fn render_input(
    input: &str,
    format: OutputFormat,
    geometry_level: GeometryLevel,
    text_options: &TextRenderOptions,
    svg_options: &SvgRenderOptions,
) -> Result<String, RenderError> {
    let payload =
        parse_input(input).map_err(|error| prefixed_display_error("parse error", error))?;
    render_output(&payload, format, geometry_level, text_options, svg_options)
}

/// Render a parsed MMDS payload through the MMDS replay path.
pub fn render_output(
    payload: &Output,
    format: OutputFormat,
    geometry_level: GeometryLevel,
    text_options: &TextRenderOptions,
    svg_options: &SvgRenderOptions,
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
        let output = if has_routed_geometry && geometry_level == GeometryLevel::Layout {
            strip_routed_fields(payload)
        } else {
            payload.clone()
        };
        return serde_json::to_string_pretty(&output)
            .map_err(|error| prefixed_display_error("MMDS serialization error", error));
    }

    if matches!(format, OutputFormat::Mermaid) {
        return generate_mermaid(payload).map_err(display_error);
    }

    let diagram = from_output(payload).map_err(display_error)?;

    let geometry = hydrate_graph_geometry_from_output_with_diagram(payload, &diagram)
        .map_err(display_error)?;
    let routed = has_routed_geometry
        .then(|| hydrate_routed_geometry_from_output(payload))
        .transpose()
        .map_err(display_error)?;

    match format {
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options = text_options.clone();
            options.output_format = format;
            Ok(render_text_from_geometry(
                &diagram,
                &geometry,
                routed.as_ref(),
                &options,
            ))
        }
        OutputFormat::Svg => Ok(match routed.as_ref() {
            Some(routed) => render_svg_from_routed_geometry(&diagram, routed, svg_options),
            None => render_svg_from_geometry(&diagram, &geometry, svg_options),
        }),
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

fn strip_routed_fields(payload: &Output) -> Output {
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
