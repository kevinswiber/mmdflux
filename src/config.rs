//! Rendering configuration for layout engine selection and output tuning.

use crate::engines::graph::EngineAlgorithmId;
pub use crate::engines::graph::{LabelDummyStrategy, LayoutConfig, LayoutDirection, Ranker};
use crate::format::{Curve, EdgePreset, RoutingStyle};
use crate::graph::GeometryLevel;
use crate::render::text::TextColorMode;
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
