//! Shared runtime facade for CLI, WASM, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to, plus serde-friendly config input types for JSON consumers.

pub mod config;
pub mod config_input;

mod graph_family;
pub(crate) mod mmds;
mod payload;

use config::RenderConfig;

use crate::builtins::default_registry;
use crate::errors::{ParseDiagnostic, RenderError};
use crate::format::OutputFormat;
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::mermaid::ParseError;
use crate::registry::DiagramFamily;

/// Detect the diagram type from input text.
///
/// Returns the diagram type identifier (e.g. `"flowchart"`, `"class"`,
/// `"sequence"`) or `None` if the input is not recognized.
pub fn detect_diagram(input: &str) -> Option<&'static str> {
    match detect_input_frontend(input)? {
        InputFrontend::Mermaid => default_registry().detect(input),
        InputFrontend::Mmds => crate::mmds::detect_diagram_type(input).ok(),
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
        return mmds::render_input(
            input,
            format,
            config.geometry_level,
            &config.text_render_options(format),
            &config.svg_render_options(),
        );
    }

    let registry = default_registry();

    let diagram_id = registry.detect(input).ok_or_else(|| RenderError {
        message: "unknown diagram type".to_string(),
    })?;

    // Check format support and engine policy before creating an instance.
    if !registry.supports_format(diagram_id, format) {
        return Err(RenderError {
            message: format!("{diagram_id} diagrams do not support {format} output"),
        });
    }

    if config.layout_engine.is_some() {
        let family = registry
            .get(diagram_id)
            .map(|d| d.family)
            .unwrap_or(DiagramFamily::Graph);
        if family != DiagramFamily::Graph {
            return Err(RenderError {
                message: "layout engine selection is not supported for sequence diagrams"
                    .to_string(),
            });
        }
    }

    let instance = registry.create(diagram_id).ok_or_else(|| RenderError {
        message: format!("no implementation for diagram type: {diagram_id}"),
    })?;

    let parsed = instance.parse(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;

    let payload = parsed.into_payload()?;
    payload::render_payload(payload, format, config)
}

/// Validate Mermaid input and return structured diagnostics as JSON.
///
/// Returns a JSON string with shape:
/// - `{"valid": true}` on success with no warnings
/// - `{"valid": true, "diagnostics": [...]}` on success with warnings
/// - `{"valid": false, "diagnostics": [...]}` on error
pub fn validate_diagram(input: &str) -> String {
    if matches!(detect_input_frontend(input), Some(InputFrontend::Mmds)) {
        return match crate::mmds::validate_input(input) {
            Ok(()) => validation_success_json(Vec::new()),
            Err(error) => validation_message_error_json(error.message),
        };
    }

    let registry = default_registry();

    let diagram_id = match registry.detect(input) {
        Some(id) => id,
        None => return validation_message_error_json("unknown diagram type"),
    };

    let instance = match registry.create(diagram_id) {
        Some(inst) => inst,
        None => {
            return validation_message_error_json(format!(
                "no implementation for diagram type: {diagram_id}"
            ));
        }
    };

    let warnings = instance.validation_warnings(input);
    match instance.parse(input) {
        Ok(_) => validation_success_json(warnings),
        Err(error) => validation_failure_json(parse_failure_diagnostic(error.as_ref())),
    }
}

fn validation_success_json(diagnostics: Vec<ParseDiagnostic>) -> String {
    validation_json(true, diagnostics)
}

fn validation_failure_json(diagnostic: ParseDiagnostic) -> String {
    validation_json(false, vec![diagnostic])
}

fn validation_json(valid: bool, diagnostics: Vec<ParseDiagnostic>) -> String {
    if diagnostics.is_empty() {
        serde_json::json!({ "valid": valid }).to_string()
    } else {
        serde_json::json!({
            "valid": valid,
            "diagnostics": diagnostics
        })
        .to_string()
    }
}

fn validation_message_error_json(message: impl Into<String>) -> String {
    serde_json::json!({
        "valid": false,
        "diagnostics": [{
            "message": message.into()
        }]
    })
    .to_string()
}

fn parse_failure_diagnostic(error: &(dyn std::error::Error + 'static)) -> ParseDiagnostic {
    match error.downcast_ref::<ParseError>() {
        Some(parse_error) => ParseDiagnostic::from(parse_error),
        None => ParseDiagnostic {
            severity: "error".to_string(),
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            message: error.to_string(),
        },
    }
}
