//! Shared runtime facade for CLI, WASM, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to, plus serde-friendly config input types for JSON consumers.

pub mod config;
pub mod config_input;

pub(crate) mod graph_family;
pub(crate) mod mmds;
mod payload;
mod timeline_family;

use config::RenderConfig;

use crate::builtins::default_registry;
use crate::errors::{ParseDiagnostic, RenderError};
use crate::format::OutputFormat;
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::mermaid::{ParseError, extract_theme_hint};
use crate::render::svg::theme::{
    ResolvedSvgTheme, SvgThemeRenderMode, SvgThemeSpec, resolve_svg_theme,
};
use crate::runtime::config::SvgThemeConfig;

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
        let svg_theme = if matches!(format, OutputFormat::Svg) {
            resolve_configured_svg_theme(config)?
        } else {
            None
        };
        return mmds::render_input(
            input,
            format,
            config.geometry_level,
            &config.text_render_options(format),
            &config.svg_render_options(),
            svg_theme.as_ref(),
        );
    }

    let effective_config = effective_render_config(input, format, config);

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

    let instance = registry.create(diagram_id).ok_or_else(|| RenderError {
        message: format!("no implementation for diagram type: {diagram_id}"),
    })?;

    let parsed = instance.parse(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;

    let payload = parsed.into_payload()?;
    payload::render_payload(payload, format, &effective_config)
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

fn effective_render_config(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> RenderConfig {
    let mut effective = config.clone();

    if !matches!(format, OutputFormat::Svg) || effective.svg_theme.is_some() {
        return effective;
    }

    if !matches!(detect_input_frontend(input), Some(InputFrontend::Mermaid)) {
        return effective;
    }

    if let Some(theme_name) = extract_theme_hint(input) {
        effective.svg_theme = Some(SvgThemeConfig {
            name: Some(theme_name),
            ..SvgThemeConfig::default()
        });
    }

    effective
}

pub(in crate::runtime) fn resolve_configured_svg_theme(
    config: &RenderConfig,
) -> Result<Option<ResolvedSvgTheme>, RenderError> {
    config
        .svg_theme
        .as_ref()
        .map(|theme| resolve_svg_theme(&svg_theme_spec_from_config(theme)))
        .transpose()
        .map_err(|error| RenderError {
            message: error.message,
        })
}

pub(in crate::runtime) fn svg_theme_spec_from_config(config: &SvgThemeConfig) -> SvgThemeSpec {
    SvgThemeSpec {
        name: config.name.clone(),
        mode: match config.mode {
            crate::runtime::config::SvgThemeMode::Static => SvgThemeRenderMode::Static,
            crate::runtime::config::SvgThemeMode::Dynamic => SvgThemeRenderMode::Dynamic,
        },
        bg: config.bg.clone(),
        fg: config.fg.clone(),
        line: config.line.clone(),
        accent: config.accent.clone(),
        muted: config.muted.clone(),
        surface: config.surface.clone(),
        border: config.border.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderConfig, effective_render_config, validate_diagram};
    use crate::format::OutputFormat;
    use crate::runtime::config::SvgThemeConfig;

    #[test]
    fn validate_diagram_skips_warning_for_frontmatter_theme_hint() {
        let input = "---\nconfig:\n  theme: dark\n---\ngraph TD\nA-->B\n";
        let value: serde_json::Value = serde_json::from_str(&validate_diagram(input)).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value.get("diagnostics").is_none());
    }

    #[test]
    fn validate_diagram_skips_warning_for_init_theme_hint() {
        let input = "%%{init: {\"theme\": \"dark\"}}%%\ngraph TD\nA-->B\n";
        let value: serde_json::Value = serde_json::from_str(&validate_diagram(input)).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value.get("diagnostics").is_none());
    }

    #[test]
    fn validate_diagram_keeps_warning_for_non_theme_init_keys() {
        let input = "%%{init: {\"theme\": \"dark\", \"flowchart\": {\"curve\": \"basis\"}}}%%\ngraph TD\nA-->B\n";
        let value: serde_json::Value = serde_json::from_str(&validate_diagram(input)).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"]
            .as_array()
            .expect("diagnostics should be present");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("Strict parsing would reject"))
        }));
    }

    #[test]
    fn runtime_uses_source_theme_hint_when_svg_theme_is_unset() {
        let input = "%%{init: {\"theme\": \"forest\"}}%%\nstateDiagram-v2\n[*] --> Idle\n";
        let effective = effective_render_config(input, OutputFormat::Svg, &RenderConfig::default());
        assert_eq!(
            effective
                .svg_theme
                .as_ref()
                .and_then(|theme| theme.name.as_deref()),
            Some("forest")
        );
    }

    #[test]
    fn explicit_svg_theme_overrides_source_hint() {
        let input = "%%{init: {\"theme\": \"forest\"}}%%\ngraph TD\nA-->B\n";
        let config = RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".to_string()),
                ..SvgThemeConfig::default()
            }),
            ..RenderConfig::default()
        };

        let effective = effective_render_config(input, OutputFormat::Svg, &config);
        assert_eq!(
            effective
                .svg_theme
                .as_ref()
                .and_then(|theme| theme.name.as_deref()),
            Some("dark")
        );
    }
}
