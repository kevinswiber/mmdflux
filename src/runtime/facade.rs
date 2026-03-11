//! Shared runtime facade for CLI, WASM, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to. It eliminates duplicated registry, engine, and render
//! dispatch logic.
//!
//! - [`detect_diagram`] — identify diagram type from input text.
//! - [`render_diagram`] — detect, parse, and render in one call.
//! - [`validate_diagram`] — parse and return structured diagnostics as JSON.
//! - [`render_graph`] — (internal) graph-family engine+render pipeline.

use crate::diagnostics::ParseDiagnostic;
use crate::engines::graph::{
    AlgorithmId, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError,
    solve_graph_family,
};
use crate::frontends::mermaid::{
    DiagramType, ParseError, ParseOptions, detect_diagram_type, parse_flowchart_with_options,
};
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::graph::Diagram;
use crate::lint::{collect_subgraph_warnings, collect_unsupported_warnings};
use crate::registry::default_registry;
use crate::render::graph::{SvgRenderOptions, TextRenderOptions};

/// Render a graph-family diagram through the shared pipeline.
///
/// Handles engine resolution, layout solve, and format dispatch for all
/// graph-family diagram types. Diagram-specific pre-processing (e.g.,
/// `show_ids` annotation) should be applied before calling this function.
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
        OutputFormat::Mmds => crate::render::graph::backends::mmds::render_mmds_full(
            diagram_id,
            diagram,
            &result,
            config.geometry_level,
            config.path_simplification,
        ),
        OutputFormat::Svg => {
            let options: SvgRenderOptions = config.into();
            Ok(
                crate::render::graph::backends::svg::render_svg_with_options(
                    diagram, &result, &options,
                ),
            )
        }
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options: TextRenderOptions = config.into();
            options.output_format = format;
            Ok(
                crate::render::graph::backends::text::render_text_with_options(
                    diagram, &result, &options,
                ),
            )
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

/// Detect the diagram type from input text.
///
/// Returns the diagram type identifier (e.g. `"flowchart"`, `"class"`,
/// `"sequence"`) or `None` if the input is not recognized.
pub fn detect_diagram(input: &str) -> Option<&'static str> {
    match detect_input_frontend(input)? {
        InputFrontend::Mermaid => default_registry().detect(input),
        InputFrontend::Mmds => crate::frontends::mmds::detect_diagram_type(input).ok(),
    }
}

/// Detect, parse, and render a diagram in one call.
///
/// This is the primary entrypoint for both CLI and WASM adapters.
/// Adapter-specific policy (format defaults, color resolution) should be
/// applied to `config` before calling this function.
pub fn render_diagram(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    if matches!(detect_input_frontend(input), Some(InputFrontend::Mmds)) {
        return crate::frontends::mmds::render_input(input, format, config);
    }

    let registry = default_registry();

    let diagram_id = registry.detect(input).ok_or_else(|| RenderError {
        message: "unknown diagram type".to_string(),
    })?;

    let mut instance = registry.create(diagram_id).ok_or_else(|| RenderError {
        message: format!("no implementation for diagram type: {diagram_id}"),
    })?;

    instance.parse(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;

    if !instance.supports_format(format) {
        return Err(RenderError {
            message: format!("{diagram_id} diagrams do not support {format} output"),
        });
    }

    instance.render(format, config)
}

/// Validate Mermaid input and return structured diagnostics as JSON.
///
/// Returns a JSON string with shape:
/// - `{"valid": true}` on success with no warnings
/// - `{"valid": true, "diagnostics": [...]}` on success with warnings
/// - `{"valid": false, "diagnostics": [...]}` on error
pub fn validate_diagram(input: &str) -> String {
    if matches!(detect_input_frontend(input), Some(InputFrontend::Mmds)) {
        return match crate::frontends::mmds::validate_input(input) {
            Ok(()) => serde_json::json!({ "valid": true }).to_string(),
            Err(error) => serde_json::json!({
                "valid": false,
                "diagnostics": [{
                    "message": error.message
                }]
            })
            .to_string(),
        };
    }

    let registry = default_registry();

    let diagram_id = match registry.detect(input) {
        Some(id) => id,
        None => {
            return serde_json::json!({
                "valid": false,
                "diagnostics": [{"message": "unknown diagram type"}]
            })
            .to_string();
        }
    };

    let mut instance = match registry.create(diagram_id) {
        Some(inst) => inst,
        None => {
            return serde_json::json!({
                "valid": false,
                "diagnostics": [{
                    "message": format!("no implementation for diagram type: {diagram_id}")
                }]
            })
            .to_string();
        }
    };

    match instance.parse(input) {
        Ok(()) => {
            let mut warnings: Vec<ParseDiagnostic> = collect_unsupported_warnings(input)
                .into_iter()
                .chain(collect_subgraph_warnings(input))
                .map(|w| ParseDiagnostic::warning(w.line, w.column, w.message))
                .collect();

            // For flowcharts: if permissive parse succeeded but strict parse
            // would fail, surface the strict error as a warning.
            if detect_diagram_type(input) == Some(DiagramType::Flowchart) {
                let strict = ParseOptions { strict: true };
                if let Err(strict_err) = parse_flowchart_with_options(input, &strict) {
                    let mut diag = ParseDiagnostic::from(&strict_err);
                    diag.severity = "warning".to_string();
                    diag.message =
                        format!("Strict parsing would reject this input: {}", diag.message);
                    warnings.push(diag);
                }
            }

            if warnings.is_empty() {
                serde_json::json!({ "valid": true }).to_string()
            } else {
                serde_json::json!({
                    "valid": true,
                    "diagnostics": warnings
                })
                .to_string()
            }
        }
        Err(error) => {
            let diagnostic = match error.downcast_ref::<ParseError>() {
                Some(parse_error) => ParseDiagnostic::from(parse_error),
                None => ParseDiagnostic {
                    severity: "error".to_string(),
                    line: None,
                    column: None,
                    end_line: None,
                    end_column: None,
                    message: error.to_string(),
                },
            };

            serde_json::json!({
                "valid": false,
                "diagnostics": [diagnostic]
            })
            .to_string()
        }
    }
}
