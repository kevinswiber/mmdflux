//! Serde-friendly config input for JSON-based consumers (Wasm, API, etc.).
//!
//! [`RuntimeConfigInput`] mirrors [`RenderConfig`] but uses `Option<String>`
//! for enum fields so that it can be deserialized from camelCase JSON.
//! Call [`RuntimeConfigInput::into_render_config`] to validate and convert.

use serde::Deserialize;

use crate::engines::graph::{EngineAlgorithmId, Ranker, SubgraphTitleMargin};
use crate::errors::RenderError;
use crate::format::{
    ColorWhen, Curve, EdgePreset, OutputFormat, RoutingStyle, normalize_enum_token,
};
use crate::graph::GeometryLevel;
use crate::graph::measure::{
    DEFAULT_GRAPH_FONT_FAMILY, DEFAULT_PROPORTIONAL_FONT_SIZE, font_family_compare_key,
    validate_text_metrics_profile_id,
};
use crate::runtime::config::{GraphTextStyleConfig, RenderConfig, SvgThemeConfig, SvgThemeMode};
use crate::simplification::PathSimplification;

/// Serde-friendly render config accepted from JSON callers.
///
/// All enum-valued fields are `Option<String>` so that consumers can pass
/// normalized or user-typed values. Conversion to the typed [`RenderConfig`]
/// happens in [`into_render_config`](Self::into_render_config).
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeConfigInput {
    pub layout_engine: Option<String>,
    pub cluster_ranksep: Option<f64>,
    pub padding: Option<usize>,
    pub svg_scale: Option<f64>,
    pub svg_theme: Option<SvgThemeConfigInput>,
    pub edge_preset: Option<String>,
    pub routing_style: Option<String>,
    pub curve: Option<String>,
    pub edge_radius: Option<f64>,
    pub svg_diagram_padding: Option<f64>,
    pub svg_node_padding_x: Option<f64>,
    pub svg_node_padding_y: Option<f64>,
    pub font_metrics_profile: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub theme_variables: Option<ThemeVariablesInput>,
    pub show_ids: Option<bool>,
    pub color: Option<String>,
    pub geometry_level: Option<String>,
    pub path_simplification: Option<String>,
    pub layout: Option<LayoutConfigInput>,
}

/// Serde-friendly layout config nested inside [`RuntimeConfigInput`].
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutConfigInput {
    pub node_sep: Option<f64>,
    pub edge_sep: Option<f64>,
    pub rank_sep: Option<f64>,
    pub margin: Option<f64>,
    pub ranker: Option<String>,
    /// Subgraph title margin in pixels, mirroring Mermaid's
    /// `flowchart.subGraphTitleMargin: { top, bottom }`. Both fields
    /// default to `0` when the object is provided. Omit entirely (or set
    /// to `null`) to keep mmdflux's measurement-driven default.
    ///
    /// Accepts Mermaid's `subGraphTitleMargin` (capital `G`) alongside the
    /// camelCase `subgraphTitleMargin` form produced by `rename_all`.
    #[serde(alias = "subGraphTitleMargin")]
    pub subgraph_title_margin: Option<SubgraphTitleMarginInput>,
}

/// Serde-friendly subgraph title margin, mirroring Mermaid's
/// `flowchart.subGraphTitleMargin: { top, bottom }`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SubgraphTitleMarginInput {
    /// Pixels above the subgraph title (Mermaid `subGraphTitleMargin.top`).
    pub top: Option<f64>,
    /// Pixels below the subgraph title (Mermaid `subGraphTitleMargin.bottom`).
    pub bottom: Option<f64>,
}

impl SubgraphTitleMarginInput {
    fn into_config(self) -> Result<SubgraphTitleMargin, RenderError> {
        let top = self.top.unwrap_or(0.0);
        let bottom = self.bottom.unwrap_or(0.0);
        for (label, value) in [("top", top), ("bottom", bottom)] {
            if !value.is_finite() {
                return Err(RenderError {
                    message: format!("subGraphTitleMargin.{label} must be a finite number"),
                });
            }
            if value < 0.0 {
                return Err(RenderError {
                    message: format!(
                        "subGraphTitleMargin.{label} must be non-negative, got {value}"
                    ),
                });
            }
        }
        Ok(SubgraphTitleMargin { top, bottom })
    }
}

/// Serde-friendly SVG theme config nested inside [`RuntimeConfigInput`].
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SvgThemeConfigInput {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub bg: Option<String>,
    pub fg: Option<String>,
    pub line: Option<String>,
    pub accent: Option<String>,
    pub muted: Option<String>,
    pub surface: Option<String>,
    pub border: Option<String>,
}

/// Narrow Mermaid-compatible themeVariables subset for graph font style.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeVariablesInput {
    pub font_family: Option<String>,
    pub font_size: Option<ThemeFontSizeInput>,
}

/// Mermaid-compatible font-size alias accepted under themeVariables.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ThemeFontSizeInput {
    Number(f64),
    String(String),
}

impl RuntimeConfigInput {
    /// Validate and convert into a typed [`RenderConfig`].
    pub fn into_render_config(self) -> Result<RenderConfig, RenderError> {
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

        if let Some(svg_theme) = self.svg_theme {
            config.svg_theme = Some(svg_theme.into_svg_theme_config()?);
        }
        if let Some(font_metrics_profile) = self.font_metrics_profile {
            validate_text_metrics_profile_id(&font_metrics_profile).map_err(|error| {
                RenderError {
                    message: error.to_string(),
                }
            })?;
            config.font_metrics_profile = Some(font_metrics_profile);
        }
        config.graph_text_style =
            graph_text_style_from_input(self.font_family, self.font_size, self.theme_variables)?;
        if let Some(layout_engine) = self.layout_engine {
            config.layout_engine = Some(EngineAlgorithmId::parse(&layout_engine)?);
        }
        if let Some(show_ids) = self.show_ids {
            config.show_ids = show_ids;
        }
        if let Some(color) = self.color {
            config.text_color_mode = color.parse::<ColorWhen>()?.resolve(false);
        }
        if let Some(edge_preset) = self.edge_preset {
            config.edge_preset = Some(EdgePreset::parse(&edge_preset)?);
        }
        if let Some(routing_style) = self.routing_style {
            config.routing_style = Some(routing_style.parse::<RoutingStyle>()?);
        }
        if let Some(curve) = self.curve {
            config.curve = Some(curve.parse::<Curve>()?);
        }
        if let Some(geometry_level) = self.geometry_level {
            config.geometry_level = geometry_level.parse::<GeometryLevel>()?;
        }
        if let Some(path_simplification) = self.path_simplification {
            config.path_simplification = path_simplification.parse::<PathSimplification>()?;
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
            if let Some(subgraph_title_margin) = layout.subgraph_title_margin {
                config.layout.subgraph_title_margin = Some(subgraph_title_margin.into_config()?);
            }
        }

        Ok(config)
    }
}

fn graph_text_style_from_input(
    font_family: Option<String>,
    font_size: Option<f64>,
    theme_variables: Option<ThemeVariablesInput>,
) -> Result<Option<GraphTextStyleConfig>, RenderError> {
    let theme_family = theme_variables
        .as_ref()
        .and_then(|theme| theme.font_family.as_ref());
    let theme_size = theme_variables
        .as_ref()
        .and_then(|theme| theme.font_size.as_ref());

    if font_family.is_none()
        && font_size.is_none()
        && theme_family.is_none()
        && theme_size.is_none()
    {
        return Ok(None);
    }

    let canonical_family = font_family
        .as_ref()
        .map(|value| normalize_font_family("fontFamily", value))
        .transpose()?;
    let theme_family = theme_family
        .map(|value| normalize_font_family("themeVariables.fontFamily", value))
        .transpose()?;
    if let (Some(canonical), Some(theme)) = (&canonical_family, &theme_family)
        && canonical.compare_key != theme.compare_key
    {
        return Err(RenderError {
            message: "conflicting fontFamily and themeVariables.fontFamily".to_string(),
        });
    }

    let canonical_size = font_size
        .map(|value| validate_font_size_px("fontSize", value))
        .transpose()?;
    let theme_size = theme_variables
        .and_then(|theme| theme.font_size)
        .map(theme_font_size_to_px)
        .transpose()?;
    if let (Some(canonical), Some(theme)) = (canonical_size, theme_size)
        && (canonical - theme).abs() > f64::EPSILON
    {
        return Err(RenderError {
            message: "conflicting fontSize and themeVariables.fontSize".to_string(),
        });
    }

    let font_family = match canonical_family.or(theme_family) {
        Some(value) => value.display,
        None => DEFAULT_GRAPH_FONT_FAMILY.to_string(),
    };
    let font_size_px = match canonical_size.or(theme_size) {
        Some(value) => value,
        None => DEFAULT_PROPORTIONAL_FONT_SIZE,
    };

    Ok(Some(GraphTextStyleConfig {
        font_family,
        font_size_px,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedFontFamily {
    display: String,
    compare_key: Vec<String>,
}

fn normalize_font_family(field: &str, value: &str) -> Result<NormalizedFontFamily, RenderError> {
    let trimmed = value.trim();
    let compare_key = font_family_compare_key_for_field(field, trimmed)?;
    Ok(NormalizedFontFamily {
        display: trimmed.to_string(),
        compare_key,
    })
}

fn validate_font_size_px(field: &str, value: f64) -> Result<f64, RenderError> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(RenderError {
            message: format!("{field} must be a finite positive number"),
        })
    }
}

fn theme_font_size_to_px(input: ThemeFontSizeInput) -> Result<f64, RenderError> {
    match input {
        ThemeFontSizeInput::Number(value) => {
            validate_font_size_px("themeVariables.fontSize", value)
        }
        ThemeFontSizeInput::String(value) => parse_theme_font_size_string(&value),
    }
}

fn parse_theme_font_size_string(value: &str) -> Result<f64, RenderError> {
    let field = "themeVariables.fontSize";
    let trimmed = value.trim();
    let numeric = if trimmed
        .get(trimmed.len().saturating_sub(2)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case("px"))
    {
        trimmed[..trimmed.len() - 2].trim_end()
    } else {
        trimmed
    };

    if !is_plain_decimal_number(numeric) {
        return Err(RenderError {
            message: format!("{field} must be a positive number or px value"),
        });
    }

    let value = numeric.parse::<f64>().map_err(|_| RenderError {
        message: format!("{field} must be a positive number or px value"),
    })?;
    validate_font_size_px(field, value)
}

fn is_plain_decimal_number(value: &str) -> bool {
    let Some((head, tail)) = value.split_once('.') else {
        return !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit());
    };

    !head.is_empty()
        && !tail.is_empty()
        && head.chars().all(|ch| ch.is_ascii_digit())
        && tail.chars().all(|ch| ch.is_ascii_digit())
        && !tail.contains('.')
}

fn font_family_compare_key_for_field(field: &str, value: &str) -> Result<Vec<String>, RenderError> {
    font_family_compare_key(value).map_err(|message| RenderError {
        message: format!("{field} {message}"),
    })
}

impl SvgThemeConfigInput {
    fn into_svg_theme_config(self) -> Result<SvgThemeConfig, RenderError> {
        let mode = match self.mode {
            Some(mode) => parse_svg_theme_mode(&mode)?,
            None => SvgThemeMode::default(),
        };

        Ok(SvgThemeConfig {
            name: self.name,
            mode,
            bg: self.bg,
            fg: self.fg,
            line: self.line,
            accent: self.accent,
            muted: self.muted,
            surface: self.surface,
            border: self.border,
        })
    }
}

fn parse_ranker(value: &str) -> Result<Ranker, RenderError> {
    match normalize_enum_token(value).as_str() {
        "network-simplex" | "networksimplex" => Ok(Ranker::NetworkSimplex),
        "longest-path" | "longestpath" => Ok(Ranker::LongestPath),
        _ => Err(RenderError {
            message: format!("unknown ranker: {value}"),
        }),
    }
}

fn parse_svg_theme_mode(value: &str) -> Result<SvgThemeMode, RenderError> {
    match normalize_enum_token(value).as_str() {
        "static" => Ok(SvgThemeMode::Static),
        "dynamic" => Ok(SvgThemeMode::Dynamic),
        _ => Err(RenderError {
            message: format!("unknown svg theme mode: {value} (expected static or dynamic)"),
        }),
    }
}

/// The default SVG layout engine (flux-layered).
pub fn default_svg_engine() -> EngineAlgorithmId {
    EngineAlgorithmId::FLUX_LAYERED
}

/// Apply SVG surface defaults for flux-layered engine.
///
/// When the output format is SVG and no edge styling is configured, this
/// applies `SmoothStep` as the default edge preset for the flux-layered engine.
///
/// The `force_engine` parameter controls whether to force the default SVG
/// engine when none is set:
/// - `true`: Wasm behavior — always set flux-layered for SVG.
/// - `false`: CLI behavior — leave engine unset (auto-detect later).
pub fn apply_svg_surface_defaults(
    format: OutputFormat,
    config: &mut RenderConfig,
    force_engine: bool,
) {
    if !matches!(format, OutputFormat::Svg) {
        return;
    }

    if force_engine && config.layout_engine.is_none() {
        config.layout_engine = Some(default_svg_engine());
    }

    if config.edge_preset.is_some() || config.routing_style.is_some() || config.curve.is_some() {
        return;
    }

    if config.layout_engine.unwrap_or(default_svg_engine()) == default_svg_engine() {
        config.edge_preset = Some(EdgePreset::SmoothStep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_family_compare_key_normalizes_quotes_case_and_spacing() {
        let first = font_family_compare_key_for_field(
            "fontFamily",
            r#" "Trebuchet   MS" , Verdana, ARIAL , sans-serif "#,
        )
        .unwrap();
        let second = font_family_compare_key_for_field(
            "fontFamily",
            r#"trebuchet ms,verdana,arial,sans-serif"#,
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first,
            vec!["trebuchet ms", "verdana", "arial", "sans-serif"]
        );
    }

    #[test]
    fn font_family_compare_key_rejects_empty_tokens() {
        let err = font_family_compare_key_for_field("fontFamily", "Inter, , Arial").unwrap_err();

        assert!(err.message.contains("fontFamily"), "{err}");
    }

    #[test]
    fn validate_font_size_rejects_non_finite_values() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = validate_font_size_px("fontSize", value).unwrap_err();
            assert!(err.message.contains("fontSize"), "{err}");
        }
    }
}
