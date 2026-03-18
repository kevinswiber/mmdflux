//! Rendering configuration for layout engine selection and output tuning.

use crate::engines::graph::{EngineAlgorithmId, EngineId};
pub use crate::engines::graph::{LabelDummyStrategy, LayoutConfig, LayoutDirection, Ranker};
use crate::format::{Curve, EdgePreset, OutputFormat, RoutingStyle, TextColorMode};
use crate::graph::GeometryLevel;
use crate::render::graph::{SvgRenderOptions, TextRenderOptions};
use crate::simplification::PathSimplification;

/// Configuration for rendering.
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Layout configuration.
    pub layout: LayoutConfig,
    /// Layout engine+algorithm selection.
    pub layout_engine: Option<EngineAlgorithmId>,
    /// Cluster (subgraph) rank separation override.
    pub cluster_ranksep: Option<f64>,
    /// Padding around content.
    pub padding: Option<usize>,
    /// Resolved text color mode for text/ascii output.
    pub text_color_mode: TextColorMode,
    /// SVG-specific: scale factor.
    pub svg_scale: Option<f64>,
    /// SVG edge style preset. Expands to routing + curve defaults.
    pub edge_preset: Option<EdgePreset>,
    /// SVG routing style override.
    pub routing_style: Option<RoutingStyle>,
    /// SVG curve override.
    pub curve: Option<Curve>,
    /// SVG-specific: corner arc radius (px).
    pub edge_radius: Option<f64>,
    /// SVG-specific: diagram padding (px).
    pub svg_diagram_padding: Option<f64>,
    /// SVG-specific: node padding on x-axis (px).
    pub svg_node_padding_x: Option<f64>,
    /// SVG-specific: node padding on y-axis (px).
    pub svg_node_padding_y: Option<f64>,
    /// Show node IDs alongside labels.
    pub show_ids: bool,
    /// MMDS geometry level for JSON output.
    pub geometry_level: GeometryLevel,
    /// Path simplification level for edge waypoints.
    pub path_simplification: PathSimplification,
}

impl RenderConfig {
    /// Build text render options from this config.
    pub fn text_render_options(&self, format: OutputFormat) -> TextRenderOptions {
        TextRenderOptions {
            output_format: format,
            text_color_mode: self.text_color_mode,
            routing_style: self
                .routing_style
                .or_else(|| self.edge_preset.map(|preset| preset.expand().0))
                .unwrap_or(RoutingStyle::Orthogonal),
            cluster_ranksep: self.cluster_ranksep,
            padding: self.padding,
            path_simplification: self.path_simplification,
        }
    }

    /// Build SVG render options from this config.
    ///
    /// Resolves engine-specific defaults for routing style and curve.
    pub fn svg_render_options(&self) -> SvgRenderOptions {
        let mut svg = SvgRenderOptions::default();
        if let Some(scale) = self.svg_scale {
            svg.scale = scale;
        }
        if let Some(padding_x) = self.svg_node_padding_x {
            svg.node_padding_x = padding_x;
        }
        if let Some(padding_y) = self.svg_node_padding_y {
            svg.node_padding_y = padding_y;
        }
        if let Some(radius) = self.edge_radius {
            svg.edge_radius = radius;
        }
        if let Some(padding) = self.svg_diagram_padding {
            svg.diagram_padding = padding;
        }

        let engine_id = self.layout_engine.map(|id| id.engine());
        let (def_routing, def_curve) = match engine_id {
            Some(EngineId::Mermaid) => (RoutingStyle::Polyline, Curve::Basis),
            _ => (RoutingStyle::Orthogonal, Curve::Basis),
        };
        let (preset_routing, preset_curve) = self
            .edge_preset
            .map(EdgePreset::expand)
            .unwrap_or((def_routing, def_curve));

        svg.routing_style = self.routing_style.unwrap_or(preset_routing);
        svg.curve = self.curve.unwrap_or(preset_curve);
        svg.path_simplification = self.path_simplification;
        svg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{CornerStyle, Curve, EdgePreset, OutputFormat, RoutingStyle};
    use crate::simplification::PathSimplification;

    // -- text_render_options tests --

    #[test]
    fn text_render_options_default_config_targets_text_output() {
        let options = RenderConfig::default().text_render_options(OutputFormat::Text);

        assert_eq!(options.output_format, OutputFormat::Text);
        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
    }

    #[test]
    fn text_render_options_preserve_padding_and_path_simplification() {
        let config = RenderConfig {
            padding: Some(4),
            routing_style: Some(RoutingStyle::Direct),
            path_simplification: PathSimplification::Lossless,
            ..Default::default()
        };
        let options = config.text_render_options(OutputFormat::Text);

        assert_eq!(options.padding, Some(4));
        assert_eq!(options.routing_style, RoutingStyle::Direct);
        assert_eq!(options.path_simplification, PathSimplification::Lossless);
    }

    // -- svg_render_options tests --

    #[test]
    fn default_config_uses_orthogonal_routing() {
        let options = RenderConfig::default().svg_render_options();
        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Basis);
    }

    #[test]
    fn step_preset_expands_to_orthogonal_linear_sharp() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Step),
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Linear(CornerStyle::Sharp));
    }

    #[test]
    fn basis_preset_expands_to_polyline_basis() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Basis),
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.routing_style, RoutingStyle::Polyline);
        assert_eq!(options.curve, Curve::Basis);
    }

    #[test]
    fn explicit_routing_style_overrides_preset_routing() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Step),
            routing_style: Some(RoutingStyle::Polyline),
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.routing_style, RoutingStyle::Polyline);
        assert_eq!(options.curve, Curve::Linear(CornerStyle::Sharp));
    }

    #[test]
    fn explicit_curve_overrides_preset_curve() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Step),
            curve: Some(Curve::Basis),
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Basis);
    }

    #[test]
    fn svg_path_simplification_is_preserved() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Polyline),
            path_simplification: PathSimplification::Lossless,
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.path_simplification, PathSimplification::Lossless);
    }

    #[test]
    fn mermaid_engine_uses_polyline_by_default() {
        let config = RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..Default::default()
        };
        let options = config.svg_render_options();

        assert_eq!(options.routing_style, RoutingStyle::Polyline);
        assert_eq!(options.curve, Curve::Basis);
    }
}
