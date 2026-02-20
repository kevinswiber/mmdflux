use mmdflux::diagram::{
    AlgorithmId, CornerStyle, EdgePreset, EngineAlgorithmId, EngineId, GeometryLevel,
    InterpolationStyle, OutputFormat, PathSimplification, RenderConfig, RenderError, RoutingStyle,
};
use mmdflux::layered::Ranker;
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
}
