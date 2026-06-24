//! Shared runtime facade for CLI, Wasm, and library consumers.
//!
//! This module provides the single orchestration contract that all adapters
//! delegate to, plus serde-friendly config input types for JSON consumers.

pub mod config;
pub mod config_input;
// Keep this gate in lockstep with the crate-root re-export so the experimental
// dynamic metrics bridge stays opt-in and doc-hidden as a single surface.
#[cfg(feature = "unstable-text-metrics-provider")]
#[doc(hidden)]
pub mod dynamic_text_metrics;

pub(crate) mod graph_family;
pub mod layout;
pub(crate) mod mmds;
mod payload;
mod timeline_family;

use config::RenderConfig;

use crate::builtins::default_registry;
use crate::errors::{ParseDiagnostic, RenderError};
use crate::format::OutputFormat;
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::graph::measure::validate_text_metrics_profile_id;
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

/// Detect, parse, and render a diagram from text input in one call.
///
/// The input can be Mermaid source or MMDS JSON. The frontend is auto-detected
/// before dispatch, so adapters can pass user-provided diagram text without
/// pre-selecting the wire format.
///
/// Use [`render_document`] when you already hold a parsed [`crate::mmds::Document`],
/// for example after [`materialize_diagram`] or [`crate::views::project`].
/// Use [`render_diagram`] when you have text input.
pub fn render_diagram(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    validate_render_config(config)?;

    let frontend = detect_input_frontend(input);
    let render_span = tracing::debug_span!(
        "render_diagram",
        format = %format,
        input_bytes = input.len(),
        frontend = tracing::field::Empty,
        diagram_type = tracing::field::Empty,
        geometry_level = %config.geometry_level,
    );
    let _render_span_guard = render_span.enter();

    if matches!(frontend, Some(InputFrontend::Mmds)) {
        render_span.record("frontend", "mmds");
        render_span.record("diagram_type", "mmds");
        tracing::debug!(event = "render", "render_diagram");
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
            config.font_metrics_profile.as_deref(),
        );
    }

    let effective_config = effective_render_config(input, format, config);

    let registry = default_registry();

    render_span.record("frontend", "mermaid");
    let diagram_id = registry.detect(input).ok_or_else(|| RenderError {
        message: "unknown diagram type".to_string(),
    })?;
    render_span.record("diagram_type", diagram_id);
    tracing::debug!(event = "render", "render_diagram");

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

/// Detect, parse, solve, and materialize graph-family text input as MMDS.
///
/// The input can be Mermaid source or MMDS JSON. This is the typed counterpart
/// to `render_diagram(input, OutputFormat::Mmds, config)`: it returns a
/// graph-family [`crate::mmds::Document`] directly instead of serializing to
/// JSON first. For MMDS JSON input, this is equivalent to parsing the
/// [`crate::mmds::Document`] and `config` is unused.
pub fn materialize_diagram(
    input: &str,
    config: &RenderConfig,
) -> Result<crate::mmds::Document, RenderError> {
    validate_render_config(config)?;

    if matches!(detect_input_frontend(input), Some(InputFrontend::Mmds)) {
        return crate::mmds::parse_input(input).map_err(|error| RenderError {
            message: format!("parse error: {error}"),
        });
    }

    let effective_config = effective_render_config(input, OutputFormat::Mmds, config);
    let registry = default_registry();
    let diagram_id = registry.detect(input).ok_or_else(|| RenderError {
        message: "unknown diagram type".to_string(),
    })?;

    if !registry.supports_format(diagram_id, OutputFormat::Mmds) {
        return Err(RenderError {
            message: format!("{diagram_id} diagrams do not support MMDS output"),
        });
    }

    let instance = registry.create(diagram_id).ok_or_else(|| RenderError {
        message: format!("no implementation for diagram type: {diagram_id}"),
    })?;

    let parsed = instance.parse(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;

    let payload = parsed.into_payload()?;
    payload::materialize_payload(payload, &effective_config)
}

/// Render a parsed graph-family MMDS document.
///
/// Use this when you already have a [`crate::mmds::Document`], for example
/// after [`materialize_diagram`] or [`crate::views::project`]. Use
/// [`render_diagram`] when you have text input, either Mermaid source or MMDS
/// JSON. Rendering a document directly avoids serializing it to JSON just to
/// feed it back through the text-input facade.
pub fn render_document(
    document: &crate::mmds::Document,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    validate_render_config(config)?;

    let svg_theme = if matches!(format, OutputFormat::Svg) {
        resolve_configured_svg_theme(config)?
    } else {
        None
    };

    mmds::render_document(
        document,
        format,
        config.geometry_level,
        &config.text_render_options(format),
        &config.svg_render_options(),
        svg_theme.as_ref(),
        config.font_metrics_profile.as_deref(),
    )
}

fn validate_render_config(config: &RenderConfig) -> Result<(), RenderError> {
    let Some(profile_id) = config.font_metrics_profile.as_deref() else {
        return Ok(());
    };

    validate_text_metrics_profile_id(profile_id).map_err(|error| RenderError {
        message: error.to_string(),
    })
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
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::Context;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::registry::LookupSpan;

    use super::{RenderConfig, effective_render_config, render_diagram, validate_diagram};
    use crate::format::OutputFormat;
    use crate::runtime::config::SvgThemeConfig;

    #[test]
    fn render_diagram_emits_render_trace_context() {
        let input = "graph TD\nA-->B";
        let collected = collect_trace_events_with_scoped_subscriber(|| {
            let _ = render_diagram(input, OutputFormat::Text, &RenderConfig::default())
                .expect("render");
        });

        assert!(collected.contains_target("mmdflux::runtime"));
        assert!(collected.contains_field("format", "text"));
        assert!(collected.contains_field("input_bytes", &input.len().to_string()));
        assert!(collected.contains_field("frontend", "mermaid"));
        assert!(collected.contains_field("diagram_type", "flowchart"));
        assert!(collected.contains_field("geometry_level", "layout"));
    }

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

    fn collect_trace_events_with_scoped_subscriber(f: impl FnOnce()) -> CollectedTraceEvents {
        let records = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(TraceCollector {
            records: Arc::clone(&records),
        });

        tracing::subscriber::with_default(subscriber, f);

        CollectedTraceEvents {
            records: records.lock().expect("records lock").clone(),
        }
    }

    #[derive(Debug, Clone)]
    struct CollectedTraceEvents {
        records: Vec<TraceRecord>,
    }

    impl CollectedTraceEvents {
        fn contains_target(&self, target: &str) -> bool {
            self.records.iter().any(|record| record.target == target)
        }

        fn contains_field(&self, name: &str, value: &str) -> bool {
            self.records.iter().any(|record| {
                record
                    .fields
                    .iter()
                    .any(|(field_name, field_value)| field_name == name && field_value == value)
            })
        }
    }

    #[derive(Debug, Clone)]
    struct TraceRecord {
        target: String,
        fields: Vec<(String, String)>,
    }

    #[derive(Debug, Clone)]
    struct TraceCollector {
        records: Arc<Mutex<Vec<TraceRecord>>>,
    }

    impl<S> tracing_subscriber::Layer<S> for TraceCollector
    where
        S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            _ctx: Context<'_, S>,
        ) {
            let mut visitor = FieldVisitor::default();
            attrs.record(&mut visitor);
            self.records
                .lock()
                .expect("records lock")
                .push(TraceRecord {
                    target: attrs.metadata().target().to_string(),
                    fields: visitor.fields,
                });
        }

        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            self.records
                .lock()
                .expect("records lock")
                .push(TraceRecord {
                    target: event.metadata().target().to_string(),
                    fields: visitor.fields,
                });
        }

        fn on_record(
            &self,
            id: &tracing::span::Id,
            values: &tracing::span::Record<'_>,
            ctx: Context<'_, S>,
        ) {
            let Some(span) = ctx.span(id) else {
                return;
            };

            let mut visitor = FieldVisitor::default();
            values.record(&mut visitor);
            self.records
                .lock()
                .expect("records lock")
                .push(TraceRecord {
                    target: span.metadata().target().to_string(),
                    fields: visitor.fields,
                });
        }
    }

    #[derive(Debug, Default)]
    struct FieldVisitor {
        fields: Vec<(String, String)>,
    }

    impl FieldVisitor {
        fn push(&mut self, field: &Field, value: String) {
            self.fields.push((field.name().to_string(), value));
        }
    }

    impl Visit for FieldVisitor {
        fn record_bool(&mut self, field: &Field, value: bool) {
            self.push(field, value.to_string());
        }

        fn record_i64(&mut self, field: &Field, value: i64) {
            self.push(field, value.to_string());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.push(field, value.to_string());
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.push(field, value.to_string());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.push(field, format!("{value:?}"));
        }
    }
}
