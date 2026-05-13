use std::cell::Cell;

use mmdflux::dynamic_text_metrics::{
    DynamicMetricsInput, DynamicTextMetricsError, render_graph_family_with_dynamic_text_metrics,
    resolve_browser_text_metrics_request,
};
use mmdflux::errors::RenderError;
use mmdflux::format::OutputFormat;
use mmdflux::{
    RenderConfig, RuntimeConfigInput, apply_svg_surface_defaults, detect_diagram, render_diagram,
    validate_diagram,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

thread_local! {
    static DYNAMIC_RENDER_DEPTH: Cell<u32> = const { Cell::new(0) };
    static DYNAMIC_RENDER_REENTERED: Cell<bool> = const { Cell::new(false) };
}

#[wasm_bindgen]
pub fn render(input: &str, format: &str, config_json: &str) -> Result<String, JsError> {
    let format = parse_output_format(format)?;
    let config = parse_render_config(format, config_json)?;

    render_diagram(input, format, &config).map_err(|err| js_error(err.message))
}

#[wasm_bindgen(js_name = browserTextMetricsRequest)]
pub fn browser_text_metrics_request(
    input: &str,
    format: &str,
    config_json: &str,
) -> Result<String, JsError> {
    let format = parse_output_format(format)?;
    let config = parse_resolver_config(config_json)?;
    let decision = resolve_browser_text_metrics_request(input, format, &config)
        .map_err(|err| js_error(err.message))?;

    serde_json::to_string(&decision).map_err(|error| {
        js_error(format!(
            "failed to serialize browser metrics decision: {error}"
        ))
    })
}

#[wasm_bindgen(js_name = renderWithBrowserTextMetrics)]
pub fn render_with_browser_text_metrics(
    input: &str,
    format: &str,
    config_json: &str,
    metrics_json: &str,
    measure_text: &js_sys::Function,
) -> Result<String, JsError> {
    let reentry_guard = DynamicRenderGuard::enter()?;
    let format = parse_output_format(format)?;
    let config = parse_dynamic_render_config(config_json)?;
    let metrics = parse_dynamic_metrics_input(metrics_json).map_err(|err| js_error(err.message))?;

    let rendered = render_graph_family_with_dynamic_text_metrics(
        input,
        format,
        &config,
        metrics,
        |text, css_font| measure_text_width(measure_text, text, css_font),
    )
    .map_err(|err| js_error(err.message));

    if reentry_guard.reentered() {
        return Err(js_error("renderWithBrowserTextMetrics re-entered"));
    }

    rendered
}

#[wasm_bindgen]
pub fn detect(input: &str) -> Option<String> {
    detect_diagram(input).map(str::to_string)
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Validate Mermaid input and return structured parse diagnostics as JSON.
///
/// Returns a JSON string with shape:
/// - `{"valid": true}` on success
/// - `{"valid": false, "diagnostics": [{"line": N, "column": N, ...}]}` on error
#[wasm_bindgen]
pub fn validate(input: &str) -> String {
    validate_diagram(input)
}

fn parse_render_config(format: OutputFormat, config_json: &str) -> Result<RenderConfig, JsError> {
    if config_json.trim().is_empty() {
        let mut config = RenderConfig::default();
        // Wasm forces flux-layered for SVG.
        apply_svg_surface_defaults(format, &mut config, true);
        return Ok(config);
    }

    let input: RuntimeConfigInput = serde_json::from_str(config_json)
        .map_err(|error| js_error(format!("invalid config_json: {error}")))?;
    let mut config = input
        .into_render_config()
        .map_err(|err| js_error(err.message))?;
    // Wasm forces flux-layered for SVG.
    apply_svg_surface_defaults(format, &mut config, true);
    Ok(config)
}

fn parse_resolver_config(config_json: &str) -> Result<RenderConfig, JsError> {
    if config_json.trim().is_empty() {
        return Ok(RenderConfig::default());
    }

    let input: RuntimeConfigInput = serde_json::from_str(config_json)
        .map_err(|error| js_error(format!("invalid config_json: {error}")))?;
    input
        .into_render_config()
        .map_err(|err| js_error(err.message))
}

fn parse_dynamic_render_config(config_json: &str) -> Result<RenderConfig, JsError> {
    if config_json.trim().is_empty() {
        return Ok(RenderConfig::default());
    }

    let input: RuntimeConfigInput = serde_json::from_str(config_json)
        .map_err(|error| js_error(format!("invalid config_json: {error}")))?;
    let config = input
        .into_render_config()
        .map_err(|err| js_error(err.message))?;
    if config.graph_text_style.is_some() {
        return Err(js_error(
            "renderWithBrowserTextMetrics uses metricsJson for font identity; do not pass fontFamily, fontSize, or themeVariables in configJson",
        ));
    }
    Ok(config)
}

fn parse_dynamic_metrics_input(metrics_json: &str) -> Result<DynamicMetricsInput, RenderError> {
    let input: DynamicMetricsInput =
        serde_json::from_str(metrics_json).map_err(|error| RenderError {
            message: format!("invalid metrics_json: {error}"),
        })?;
    input.validate()?;
    Ok(input)
}

fn measure_text_width(
    measure_text: &js_sys::Function,
    text: &str,
    css_font: &str,
) -> Result<f64, DynamicTextMetricsError> {
    let value = measure_text
        .call2(
            &JsValue::UNDEFINED,
            &JsValue::from_str(text),
            &JsValue::from_str(css_font),
        )
        .map_err(|error| {
            DynamicTextMetricsError::new(format!(
                "measureText callback threw: {}",
                js_value_message(&error)
            ))
        })?;

    if value.is_instance_of::<js_sys::Promise>() {
        return Err(DynamicTextMetricsError::new(
            "measureText callback must be synchronous",
        ));
    }

    let Some(width) = value.as_f64() else {
        return Err(DynamicTextMetricsError::new(
            "measureText callback must return a number",
        ));
    };
    if !width.is_finite() || width < 0.0 {
        return Err(DynamicTextMetricsError::new(
            "measureText callback must return a finite non-negative number",
        ));
    }
    Ok(width)
}

fn parse_output_format(value: &str) -> Result<OutputFormat, JsError> {
    value
        .parse::<OutputFormat>()
        .map_err(|err: RenderError| js_error(err.message))
}

fn js_error(message: impl Into<String>) -> JsError {
    JsError::new(&message.into())
}

fn js_value_message(value: &JsValue) -> String {
    if let Some(message) = value.as_string() {
        return message;
    }

    js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}

struct DynamicRenderGuard;

impl DynamicRenderGuard {
    fn enter() -> Result<Self, JsError> {
        let already_rendering = DYNAMIC_RENDER_DEPTH.with(|depth| {
            let current = depth.get();
            if current > 0 {
                true
            } else {
                depth.set(1);
                false
            }
        });

        if already_rendering {
            DYNAMIC_RENDER_REENTERED.with(|reentered| reentered.set(true));
            return Err(js_error("renderWithBrowserTextMetrics re-entered"));
        }

        DYNAMIC_RENDER_REENTERED.with(|reentered| reentered.set(false));
        Ok(Self)
    }

    fn reentered(&self) -> bool {
        DYNAMIC_RENDER_REENTERED.with(Cell::get)
    }
}

impl Drop for DynamicRenderGuard {
    fn drop(&mut self) {
        DYNAMIC_RENDER_DEPTH.with(|depth| depth.set(0));
        DYNAMIC_RENDER_REENTERED.with(|reentered| reentered.set(false));
    }
}

#[cfg(test)]
mod tests {
    use mmdflux::format::EdgePreset;
    use mmdflux::graph::GeometryLevel;
    use mmdflux::simplification::PathSimplification;
    use mmdflux::{AlgorithmId, EngineAlgorithmId, EngineId, TextColorMode};

    use super::*;

    #[test]
    fn wasm_export_signatures_are_stable() {
        let _render: fn(&str, &str, &str) -> Result<String, JsError> = render;
        let _render_dynamic: fn(
            &str,
            &str,
            &str,
            &str,
            &js_sys::Function,
        ) -> Result<String, JsError> = render_with_browser_text_metrics;
        let _detect: fn(&str) -> Option<String> = detect;
        let _version: fn() -> String = version;
    }

    const STRICT_DYNAMIC_METRICS_JSON: &str = r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}]}"#;

    #[test]
    fn parse_dynamic_metrics_input_accepts_strict_contract() {
        let input = parse_dynamic_metrics_input(STRICT_DYNAMIC_METRICS_JSON)
            .expect("dynamic metrics input should parse");

        assert_eq!(input.default_style, "s0");
        assert_eq!(input.text_styles.len(), 1);
        let style = &input.text_styles[0];
        assert_eq!(style.id, "s0");
        assert_eq!(style.css_font, "16px Inter");
        assert_eq!(style.font_family, "Inter");
        assert_eq!(style.font_size, 16.0);
        assert_eq!(style.font_style, "normal");
        assert_eq!(style.font_weight, "400");
        assert_eq!(style.line_height, 24.0);
    }

    #[test]
    fn parse_dynamic_metrics_input_rejects_invalid_json() {
        let err = parse_dynamic_metrics_input("{").expect_err("invalid metrics JSON should fail");
        assert!(err.message.contains("invalid metrics_json"), "{err}");
    }

    #[test]
    fn parse_dynamic_metrics_input_rejects_unknown_fields() {
        let err = parse_dynamic_metrics_input(
            r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}],"extra":true}"#,
        )
        .expect_err("unknown field should fail");
        assert!(err.message.contains("unknown field"), "{err}");
        assert!(err.message.contains("extra"), "{err}");
    }

    #[test]
    fn parse_dynamic_metrics_input_validates_required_fields() {
        for (field, json) in [
            (
                "textStyles.cssFont",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":""}]}"#,
            ),
            (
                "textStyles.fontFamily",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}]}"#,
            ),
            (
                "textStyles.fontSize",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":0,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}]}"#,
            ),
            (
                "textStyles.lineHeight",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":-1,"cssFont":"16px Inter"}]}"#,
            ),
        ] {
            let err = parse_dynamic_metrics_input(json).expect_err("invalid field should fail");
            assert!(err.message.contains(field), "{err}");
        }
    }

    #[test]
    fn render_text_output_contains_nodes() {
        let output = render("graph TD\nA-->B", "text", "{}").expect("render should succeed");
        assert!(output.contains("A"));
        assert!(output.contains("B"));
    }

    #[test]
    fn detect_returns_flowchart_for_graph_input() {
        assert_eq!(detect("graph TD\nA-->B"), Some("flowchart".to_string()));
    }

    #[test]
    fn parse_render_config_defaults_svg_to_flux_layered_engine() {
        let config = parse_render_config(OutputFormat::Svg, "{}")
            .expect("svg config parsing should succeed");
        assert_eq!(
            config.layout_engine,
            Some(EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered))
        );
    }

    #[test]
    fn parse_render_config_defaults_flux_svg_to_smooth_step() {
        let config = parse_render_config(OutputFormat::Svg, "{}")
            .expect("svg config parsing should succeed");
        assert_eq!(config.edge_preset, Some(EdgePreset::SmoothStep));
    }

    #[test]
    fn parse_render_config_keeps_non_svg_without_engine_default() {
        let config = parse_render_config(OutputFormat::Text, "{}")
            .expect("text config parsing should succeed");
        assert_eq!(config.layout_engine, None);
    }

    #[test]
    fn parse_render_config_respects_explicit_layout_engine() {
        let config =
            parse_render_config(OutputFormat::Svg, r#"{"layoutEngine":"mermaid-layered"}"#)
                .expect("explicit layout engine should parse");
        assert_eq!(
            config.layout_engine,
            Some(EngineAlgorithmId::new(
                EngineId::Mermaid,
                AlgorithmId::Layered
            ))
        );
    }

    #[test]
    fn parse_render_config_keeps_mermaid_layered_without_default_flux_preset() {
        let config =
            parse_render_config(OutputFormat::Svg, r#"{"layoutEngine":"mermaid-layered"}"#)
                .expect("explicit layout engine should parse");
        assert_eq!(config.edge_preset, None);
    }

    #[test]
    fn parse_render_config_does_not_force_default_with_layout_engine_override() {
        let config =
            parse_render_config(OutputFormat::Svg, r#"{"layoutEngine":"mermaid-layered"}"#)
                .expect("layout engine config should parse");
        assert_eq!(
            config.layout_engine,
            Some(EngineAlgorithmId::new(
                EngineId::Mermaid,
                AlgorithmId::Layered
            ))
        );
    }

    #[test]
    fn parse_render_config_applies_mmds_geometry_and_path_fields() {
        let config = parse_render_config(
            OutputFormat::Mmds,
            r#"{"geometryLevel":"routed","pathSimplification":"minimal"}"#,
        )
        .expect("mmds config parsing should succeed");

        assert_eq!(config.geometry_level, GeometryLevel::Routed);
        assert_eq!(config.path_simplification, PathSimplification::Minimal);
    }

    #[test]
    fn parse_render_config_applies_curve_field() {
        let config = parse_render_config(OutputFormat::Svg, r#"{"curve":"linear-rounded"}"#)
            .expect("curve config should parse");
        assert_eq!(
            config.curve.map(|curve| curve.to_string()),
            Some("linear-rounded".to_string())
        );
    }

    #[test]
    fn parse_render_config_accepts_text_color_policy() {
        let off = parse_render_config(OutputFormat::Text, r#"{"color":"off"}"#)
            .expect("off color config should parse");
        let auto = parse_render_config(OutputFormat::Text, r#"{"color":"auto"}"#)
            .expect("auto color config should parse");
        let always = parse_render_config(OutputFormat::Text, r#"{"color":"always"}"#)
            .expect("always color config should parse");

        assert_eq!(off.text_color_mode, TextColorMode::Plain);
        assert_eq!(auto.text_color_mode, TextColorMode::Plain);
        assert_eq!(always.text_color_mode, TextColorMode::Ansi);
    }

    #[test]
    fn parse_render_config_accepts_font_metrics_profile() {
        for profile in [
            mmdflux::graph::measure::COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
            mmdflux::graph::measure::RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
        ] {
            let config = parse_render_config(
                OutputFormat::Svg,
                &format!(r#"{{"fontMetricsProfile":"{profile}"}}"#),
            )
            .expect("font metrics profile config should parse");

            assert_eq!(config.font_metrics_profile.as_deref(), Some(profile));
        }
    }

    #[test]
    fn render_accepts_font_metrics_profile_config() {
        for profile in [
            mmdflux::graph::measure::COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
            mmdflux::graph::measure::RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
        ] {
            let output = render(
                "graph TD\nA[mmmm]-->B[iiii]",
                "svg",
                &format!(r#"{{"fontMetricsProfile":"{profile}"}}"#),
            )
            .expect("render should accept font metrics profile");

            assert!(output.contains("<svg"));
        }
    }

    #[test]
    fn parse_render_config_rejects_unsupported_font_metrics_profile() {
        let input: RuntimeConfigInput =
            serde_json::from_str(r#"{"fontMetricsProfile":"mermaid-sans-v1"}"#)
                .expect("font metrics profile JSON should parse");
        let err = input
            .into_render_config()
            .expect_err("unsupported font metrics profile should be rejected");

        assert!(
            err.message
                .contains("unsupported text metrics profile 'mermaid-sans-v1'"),
            "{err}"
        );
    }

    #[test]
    fn parse_render_config_rejects_legacy_interpolation_style_field() {
        let err = serde_json::from_str::<RuntimeConfigInput>(r#"{"interpolationStyle":"linear"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field"));
        assert!(err.contains("interpolationStyle"));
    }

    #[test]
    fn parse_render_config_rejects_legacy_corner_style_field() {
        let err = serde_json::from_str::<RuntimeConfigInput>(r#"{"cornerStyle":"rounded"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field"));
        assert!(err.contains("cornerStyle"));
    }

    #[test]
    fn parse_render_config_defaults_svg_to_flux_layered_when_empty() {
        let config =
            parse_render_config(OutputFormat::Svg, "{}").expect("empty config should parse");
        assert_eq!(
            config.layout_engine,
            Some(EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered))
        );
    }

    #[test]
    fn validate_export_signature_is_stable() {
        let _validate: fn(&str) -> String = validate;
    }

    #[test]
    fn validate_returns_valid_true_for_good_input() {
        let result = validate("graph TD\nA-->B");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
    }

    #[test]
    fn validate_returns_diagnostics_for_invalid_flowchart() {
        let result = validate("graph TD\n!!!");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], false);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert!(!diagnostics.is_empty());
        let diag = &diagnostics[0];
        assert!(diag["line"].is_number());
        assert!(diag["column"].is_number());
        assert!(diag["message"].is_string());
    }

    #[test]
    fn validate_returns_valid_false_for_unknown_diagram_type() {
        let result = validate("not a diagram at all");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], false);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0]["message"].is_string());
    }

    #[test]
    fn validate_returns_no_diagnostics_for_supported_style_statement() {
        let result = validate("graph TD\nA --> B\nstyle A fill:#f9f");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value.get("diagnostics").is_none());
    }

    #[test]
    fn validate_returns_warning_for_classdef_statement() {
        // classDef with an unsupported property produces a warning.
        let result = validate("graph TD\nA --> B\nclassDef highlight opacity:0.5");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["severity"], "warning");
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("opacity")
        );
    }

    #[test]
    fn validate_returns_no_diagnostics_for_supported_classdef() {
        // classDef with only supported properties produces no diagnostics.
        let result = validate("graph TD\nA --> B\nclassDef highlight fill:#ff0");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value.get("diagnostics").is_none());
    }

    #[test]
    fn validate_returns_no_diagnostics_for_clean_input() {
        let result = validate("graph TD\nA --> B");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value["diagnostics"].is_null());
    }

    #[test]
    fn validate_error_diagnostics_have_error_severity() {
        let result = validate("graph TD\n!!!");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], false);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics[0]["severity"], "error");
    }

    #[test]
    fn validate_warns_when_strict_would_reject_directive() {
        let result = validate("%%{init: {}}%%\ngraph TD\nA --> B");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert!(!diagnostics.is_empty());
        let strict_warning = diagnostics
            .iter()
            .find(|d| {
                d["severity"] == "warning"
                    && d["message"]
                        .as_str()
                        .unwrap_or("")
                        .contains("Strict parsing")
            })
            .expect("should have a strict-parsing warning");
        assert!(strict_warning["line"].is_number());
    }

    #[test]
    fn validate_no_strict_warning_for_clean_flowchart() {
        let result = validate("graph TD\nA --> B\nB --> C");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        assert!(value["diagnostics"].is_null());
    }

    #[test]
    fn validate_strict_warning_has_position_info() {
        let result = validate("%%{init: {}}%%\ngraph TD\nA --> B");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        let strict_warning = diagnostics.iter().find(|d| {
            d["message"]
                .as_str()
                .unwrap_or("")
                .contains("Strict parsing")
        });
        assert!(
            strict_warning.is_some(),
            "should have a strict-parsing warning"
        );
        let w = strict_warning.unwrap();
        assert!(w["line"].is_number(), "warning should have line number");
        assert!(w["column"].is_number(), "warning should have column number");
    }

    #[test]
    fn validate_warns_on_subgraph_missing_end() {
        let input = "graph TD\n    subgraph lr_group[Left to Right]\n        direction LR\n        A --> B\n    en";
        let result = validate(input);
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        let subgraph_warning = diagnostics
            .iter()
            .find(|d| {
                d["severity"] == "warning" && d["message"].as_str().unwrap_or("").contains("end")
            })
            .expect("should have a subgraph missing-end warning");
        assert_eq!(subgraph_warning["line"], 2);
    }
}
