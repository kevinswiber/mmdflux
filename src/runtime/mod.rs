//! Shared runtime facade for CLI, WASM, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to, plus serde-friendly config input types for JSON consumers.

pub mod config_input;

mod graph_family;
mod prepared;

use crate::config::RenderConfig;
use crate::diagnostics::ParseDiagnostic;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::{
    DiagramType, ParseError, ParseOptions, detect_diagram_type, parse_flowchart_with_options,
};
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::lint::{collect_subgraph_warnings, collect_unsupported_warnings};
use crate::registry_builtins::default_registry;

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

    let prepared = instance.prepare(config)?;
    prepared::render_prepared(prepared, format, config)
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
