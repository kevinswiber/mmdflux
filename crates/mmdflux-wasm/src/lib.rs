use mmdflux::ParseDiagnostic;
use mmdflux::diagram::{
    AlgorithmId, CornerStyle, EdgePreset, EngineAlgorithmId, EngineId, GeometryLevel,
    InterpolationStyle, OutputFormat, PathSimplification, RenderConfig, RenderError, RoutingStyle,
};
use mmdflux::layered::Ranker;
use mmdflux::lint::{collect_subgraph_warnings, collect_unsupported_warnings};
use mmdflux::parser::{
    DiagramType, ParseError, ParseOptions, detect_diagram_type, parse_flowchart_with_options,
};
use mmdflux::registry::default_registry;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct WasmRenderConfig {
    layout_engine: Option<String>,
    cluster_ranksep: Option<f64>,
    padding: Option<usize>,
    svg_scale: Option<f64>,
    /// Edge style preset (straight, step, smoothstep, or bezier).
    edge_preset: Option<String>,
    routing_style: Option<String>,
    interpolation_style: Option<String>,
    corner_style: Option<String>,
    edge_radius: Option<f64>,
    svg_diagram_padding: Option<f64>,
    svg_node_padding_x: Option<f64>,
    svg_node_padding_y: Option<f64>,
    show_ids: Option<bool>,
    geometry_level: Option<String>,
    path_simplification: Option<String>,
    layout: Option<WasmLayoutConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct WasmLayoutConfig {
    node_sep: Option<f64>,
    edge_sep: Option<f64>,
    rank_sep: Option<f64>,
    margin: Option<f64>,
    ranker: Option<String>,
}

#[wasm_bindgen]
pub fn render(input: &str, format: &str, config_json: &str) -> Result<String, JsError> {
    let format = parse_output_format(format)?;
    let config = parse_render_config(format, config_json)?;
    let registry = default_registry();

    let diagram_id = registry
        .detect(input)
        .ok_or_else(|| js_error("unknown diagram type"))?;
    let mut instance = registry
        .create(diagram_id)
        .ok_or_else(|| js_error(format!("no implementation for diagram type: {diagram_id}")))?;

    instance
        .parse(input)
        .map_err(|error| js_error(format!("parse error: {error}")))?;

    if !instance.supports_format(format) {
        return Err(js_error(format!(
            "{diagram_id} diagrams do not support {format} output"
        )));
    }

    instance
        .render(format, &config)
        .map_err(|error| js_error(format!("render error: {error}")))
}

#[wasm_bindgen]
pub fn detect(input: &str) -> Option<String> {
    default_registry().detect(input).map(str::to_string)
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
            // would fail, surface the strict error as a warning. This catches
            // cases like a subgraph without `end` where the permissive
            // preprocessor silently strips or reinterprets the input.
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

fn parse_render_config(format: OutputFormat, config_json: &str) -> Result<RenderConfig, JsError> {
    if config_json.trim().is_empty() {
        let mut config = RenderConfig::default();
        apply_wasm_format_defaults(format, &mut config);
        return Ok(config);
    }

    let wasm_config: WasmRenderConfig = serde_json::from_str(config_json)
        .map_err(|error| js_error(format!("invalid config_json: {error}")))?;
    let mut config = wasm_config.into_render_config()?;
    apply_wasm_format_defaults(format, &mut config);
    Ok(config)
}

impl WasmRenderConfig {
    fn into_render_config(self) -> Result<RenderConfig, JsError> {
        let mut config = RenderConfig {
            cluster_ranksep: self.cluster_ranksep,
            padding: self.padding,
            svg_scale: self.svg_scale,
            edge_radius: self.edge_radius,
            svg_diagram_padding: self.svg_diagram_padding,
            svg_node_padding_x: self.svg_node_padding_x,
            svg_node_padding_y: self.svg_node_padding_y,
            ..RenderConfig::default()
        };

        if let Some(layout_engine) = self.layout_engine {
            config.layout_engine = Some(
                EngineAlgorithmId::parse(&layout_engine).map_err(|err| js_error(err.message))?,
            );
        }
        if let Some(show_ids) = self.show_ids {
            config.show_ids = show_ids;
        }
        if let Some(edge_preset) = self.edge_preset {
            config.edge_preset = Some(parse_edge_preset(&edge_preset)?);
        }
        if let Some(routing_style) = self.routing_style {
            config.routing_style = Some(parse_via_render_error::<RoutingStyle>(&routing_style)?);
        }
        if let Some(interpolation_style) = self.interpolation_style {
            config.interpolation_style = Some(parse_via_render_error::<InterpolationStyle>(
                &interpolation_style,
            )?);
        }
        if let Some(corner_style) = self.corner_style {
            config.corner_style = Some(parse_via_render_error::<CornerStyle>(&corner_style)?);
        }
        if let Some(geometry_level) = self.geometry_level {
            config.geometry_level = parse_geometry_level(&geometry_level)?;
        }
        if let Some(path_simplification) = self.path_simplification {
            config.path_simplification = parse_path_simplification(&path_simplification)?;
        }
        if let Some(layout) = self.layout {
            if let Some(node_sep) = layout.node_sep {
                config.layout.node_sep = node_sep;
            }
            if let Some(edge_sep) = layout.edge_sep {
                config.layout.edge_sep = edge_sep;
            }
            if let Some(rank_sep) = layout.rank_sep {
                config.layout.rank_sep = rank_sep;
            }
            if let Some(margin) = layout.margin {
                config.layout.margin = margin;
            }
            if let Some(ranker) = layout.ranker {
                config.layout.ranker = parse_ranker(&ranker)?;
            }
        }

        Ok(config)
    }
}

fn parse_output_format(value: &str) -> Result<OutputFormat, JsError> {
    parse_via_render_error(value)
}

fn parse_edge_preset(value: &str) -> Result<EdgePreset, JsError> {
    EdgePreset::parse(value).map_err(|err| js_error(err.message))
}

fn parse_geometry_level(value: &str) -> Result<GeometryLevel, JsError> {
    parse_via_render_error(value)
}

fn parse_path_simplification(value: &str) -> Result<PathSimplification, JsError> {
    parse_via_render_error(value)
}

fn parse_ranker(value: &str) -> Result<Ranker, JsError> {
    match normalized(value).as_str() {
        "network-simplex" | "networksimplex" => Ok(Ranker::NetworkSimplex),
        "longest-path" | "longestpath" => Ok(Ranker::LongestPath),
        _ => Err(js_error(format!("unknown ranker: {value}"))),
    }
}

fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn parse_via_render_error<T>(value: &str) -> Result<T, JsError>
where
    T: std::str::FromStr<Err = RenderError>,
{
    value
        .parse::<T>()
        .map_err(|err| js_error(err.message.to_string()))
}

fn js_error(message: impl Into<String>) -> JsError {
    JsError::new(&message.into())
}

fn apply_wasm_format_defaults(format: OutputFormat, config: &mut RenderConfig) {
    // For SVG output, default to flux-layered engine (provides orthogonal routing).
    // This preserves the previous behavior where SVG defaulted to orthogonal routing.
    if matches!(format, OutputFormat::Svg) && config.layout_engine.is_none() {
        config.layout_engine = Some(EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_export_signatures_are_stable() {
        let _render: fn(&str, &str, &str) -> Result<String, JsError> = render;
        let _detect: fn(&str) -> Option<String> = detect;
        let _version: fn() -> String = version;
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
    fn parse_render_config_does_not_force_default_with_layout_engine_override() {
        let config =
            parse_render_config(OutputFormat::Svg, r#"{"layoutEngine":"mermaid-layered"}"#)
                .expect("layout engine config should parse");
        // When an explicit engine is set, no additional default is forced
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
    fn validate_returns_valid_true_for_pie_chart() {
        let result = validate("pie\n\"Apples\": 50\n\"Bananas\": 50");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
    }

    #[test]
    fn validate_returns_warning_for_style_statement() {
        let result = validate("graph TD\nA --> B\nstyle A fill:#f9f");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["severity"], "warning");
        assert!(diagnostics[0]["line"].is_number());
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("style")
        );
    }

    #[test]
    fn validate_returns_warning_for_classdef_statement() {
        let result = validate("graph TD\nA --> B\nclassDef highlight fill:#ff0");
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["valid"], true);
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["severity"], "warning");
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("classDef")
        );
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
        // Directive is stripped in permissive mode but rejected in strict mode
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
        // No diagnostics at all for clean input
        assert!(value["diagnostics"].is_null());
    }

    #[test]
    fn validate_strict_warning_has_position_info() {
        // Directive is stripped in permissive mode but rejected in strict mode.
        // The strict parse error should carry line/column position info.
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
