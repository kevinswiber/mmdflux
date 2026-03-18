//! Serde-friendly config input for JSON-based consumers (WASM, API, etc.).
//!
//! [`RuntimeConfigInput`] mirrors [`RenderConfig`] but uses `Option<String>`
//! for enum fields so that it can be deserialized from camelCase JSON.
//! Call [`RuntimeConfigInput::into_render_config`] to validate and convert.

use serde::Deserialize;

use crate::engines::graph::{EngineAlgorithmId, Ranker};
use crate::errors::RenderError;
use crate::format::{
    ColorWhen, Curve, EdgePreset, OutputFormat, RoutingStyle, normalize_enum_token,
};
use crate::graph::GeometryLevel;
use crate::runtime::config::RenderConfig;
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
    pub edge_preset: Option<String>,
    pub routing_style: Option<String>,
    pub curve: Option<String>,
    pub edge_radius: Option<f64>,
    pub svg_diagram_padding: Option<f64>,
    pub svg_node_padding_x: Option<f64>,
    pub svg_node_padding_y: Option<f64>,
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
        }

        Ok(config)
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
/// - `true`: WASM behavior — always set flux-layered for SVG.
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
