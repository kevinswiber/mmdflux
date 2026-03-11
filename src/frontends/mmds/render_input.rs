use super::detect::resolve_logical_diagram_id;
use super::hydrate::{from_mmds_output, hydrate_graph_geometry_from_output_with_diagram};
use crate::engines::graph::{EdgeRouting, GeometryLevel, OutputFormat, RenderConfig, RenderError};
use crate::mmds::{MmdsOutput, generate_mermaid_from_mmds, parse_mmds_input};
use crate::render::graph::{RenderOptions, render};

/// Render MMDS input through the frontend path.
pub fn render_input(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let payload = parse_mmds_input(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;
    render_output(&payload, format, config)
}

/// Render a parsed MMDS payload through the frontend path.
pub fn render_output(
    payload: &MmdsOutput,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let diagram_id = resolve_logical_diagram_id(payload)?;

    if !matches!(payload.geometry_level.as_str(), "layout" | "routed") {
        return Err(RenderError {
            message: format!(
                "MMDS validation error: invalid geometry_level '{}'",
                payload.geometry_level
            ),
        });
    }

    if matches!(format, OutputFormat::Mmds) {
        let output = if payload.geometry_level == "routed"
            && config.geometry_level == GeometryLevel::Layout
        {
            strip_routed_fields(payload)
        } else {
            payload.clone()
        };
        return serde_json::to_string_pretty(&output).map_err(|error| RenderError {
            message: format!("MMDS serialization error: {error}"),
        });
    }

    if matches!(format, OutputFormat::Mermaid) {
        return generate_mermaid_from_mmds(payload).map_err(|error| RenderError {
            message: error.to_string(),
        });
    }

    let diagram = from_mmds_output(payload).map_err(|error| RenderError {
        message: error.to_string(),
    })?;

    let mut options: RenderOptions = config.into();
    options.output_format = format;

    if payload.geometry_level == "routed" && matches!(format, OutputFormat::Svg) {
        let geometry = hydrate_graph_geometry_from_output_with_diagram(payload, &diagram).map_err(
            |error| RenderError {
                message: error.to_string(),
            },
        )?;
        return Ok(crate::render::graph::render_svg_from_geometry(
            &diagram,
            &options,
            &geometry,
            EdgeRouting::EngineProvided,
        ));
    }

    match format {
        OutputFormat::Text | OutputFormat::Ascii | OutputFormat::Svg => {
            Ok(render(&diagram, &options))
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
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
