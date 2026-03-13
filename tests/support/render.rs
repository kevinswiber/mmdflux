#![allow(dead_code)]

use mmdflux::diagrams::class::compiler::compile as compile_class_diagram;
use mmdflux::diagrams::flowchart::compile_to_graph;
use mmdflux::engines::graph::contracts::{EngineConfig, GraphEngine};
use mmdflux::engines::graph::flux::FluxLayeredEngine;
use mmdflux::mermaid::class::parse_class_diagram;
use mmdflux::mermaid::parse_flowchart;
use mmdflux::mmds::{from_mmds_str, is_mmds_input};
use mmdflux::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};
use mmdflux::{
    Diagram, OutputFormat, RenderConfig, RenderError, TextColorMode, detect_diagram, render_diagram,
};

use super::graph_family::{
    GraphGeometryContract, default_grid_request, default_proportional_request,
};

fn parse_graph_family_diagram(input: &str) -> Result<Option<Diagram>, RenderError> {
    if is_mmds_input(input) {
        return from_mmds_str(input).map(Some).map_err(|error| RenderError {
            message: error.to_string(),
        });
    }

    match detect_diagram(input) {
        Some("flowchart") => parse_flowchart(input)
            .map(|flowchart| Some(compile_to_graph(&flowchart)))
            .map_err(|error| RenderError {
                message: format!("parse error: {error}"),
            }),
        Some("class") => parse_class_diagram(input)
            .map(|diagram| Some(compile_class_diagram(&diagram)))
            .map_err(|error| RenderError {
                message: format!("parse error: {error}"),
            }),
        Some(_) => Ok(None),
        None => Err(RenderError {
            message: "unknown diagram type".to_string(),
        }),
    }
}

pub fn render_with_config(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    render_diagram(input, format, config)
}

fn solve_diagram(
    diagram: &Diagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<mmdflux::engines::graph::contracts::GraphSolveResult, RenderError> {
    let engine = FluxLayeredEngine::text();
    let request = match format {
        OutputFormat::Svg => default_proportional_request(
            GraphGeometryContract::Visual,
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
        OutputFormat::Mmds => default_proportional_request(
            GraphGeometryContract::Canonical,
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
        _ => default_grid_request(
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
    };
    engine.solve(
        diagram,
        &EngineConfig::Layered(config.layout.clone().into()),
        &request,
    )
}

pub fn render_diagram_with_config(
    diagram: &Diagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    let result = solve_diagram(diagram, format, config)?;

    match format {
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
        OutputFormat::Svg => {
            let options: SvgRenderOptions = config.into();
            Ok(if let Some(routed) = result.routed.as_ref() {
                render_svg_from_routed_geometry(diagram, routed, &options)
            } else {
                render_svg_from_geometry(diagram, &result.geometry, &options)
            })
        }
        other => Err(RenderError {
            message: format!("diagram support helper does not render {other} output"),
        }),
    }
}

pub fn render_diagram_with_text_options(
    diagram: &Diagram,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> Result<String, RenderError> {
    render_diagram_with_config(
        diagram,
        format,
        &RenderConfig {
            text_color_mode,
            ..RenderConfig::default()
        },
    )
}

pub fn render_text_with_config(input: &str, config: &RenderConfig) -> Result<String, RenderError> {
    render_with_config(input, OutputFormat::Text, config)
}

pub fn render_ascii_with_config(input: &str, config: &RenderConfig) -> Result<String, RenderError> {
    render_with_config(input, OutputFormat::Ascii, config)
}

pub fn render_svg_with_config(input: &str, config: &RenderConfig) -> Result<String, RenderError> {
    let Some(diagram) = parse_graph_family_diagram(input)? else {
        return render_with_config(input, OutputFormat::Svg, config);
    };
    render_diagram_with_config(&diagram, OutputFormat::Svg, config)
}

pub fn render_svg_diagram_with_config(
    diagram: &Diagram,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    render_diagram_with_config(diagram, OutputFormat::Svg, config)
}
