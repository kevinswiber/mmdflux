use crate::engines::graph::{EngineConfig, OutputFormat, RenderConfig, RenderError};
use crate::graph::Diagram;
use crate::graph::geometry::GraphGeometry;
use crate::render::graph::SvgOptions;
use crate::render::graph::layout_building::build_layered_layout_with_config;
use crate::render::graph::svg::svg_node_dimensions;
use crate::render::graph::svg_metrics::SvgTextMetrics;
use crate::render::graph::text_layout::{center_override_subgraphs, expand_parent_bounds};

/// Measurement mode controls whether layout uses text-grid character
/// dimensions or SVG pixel dimensions for node sizing.
#[derive(Debug, Clone)]
pub enum MeasurementMode {
    /// Text-grid character dimensions (for text/ascii rendering).
    Text,
    /// SVG pixel dimensions (for MMDS and SVG output).
    Svg(SvgTextMetrics),
}

impl MeasurementMode {
    /// Determine the measurement mode from the output format.
    pub fn for_format(format: OutputFormat, config: &RenderConfig) -> Self {
        match format {
            OutputFormat::Mmds | OutputFormat::Svg => {
                let defaults = SvgOptions::default();
                let font_size = defaults.font_size;
                let node_padding_x = config.svg_node_padding_x.unwrap_or(defaults.node_padding_x);
                let node_padding_y = config.svg_node_padding_y.unwrap_or(defaults.node_padding_y);
                let metrics = SvgTextMetrics::new(font_size, node_padding_x, node_padding_y);
                MeasurementMode::Svg(metrics)
            }
            _ => MeasurementMode::Text,
        }
    }
}

fn text_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Build a flowchart `TextLayoutConfig` from layered-engine settings.
///
/// This bridges the engine-facing layered config back to the render-facing
/// config used by shared graph-family layout construction.
pub(crate) fn layout_config_from_layered(
    layered_cfg: &super::LayoutConfig,
    diagram: &Diagram,
) -> crate::render::graph::text_layout::TextLayoutConfig {
    use crate::render::graph::text_layout::TextLayoutConfig;

    let defaults = TextLayoutConfig::default();
    let extra_padding = if diagram.has_subgraphs() {
        diagram
            .subgraphs
            .keys()
            .map(|id| diagram.subgraph_depth(id))
            .max()
            .unwrap_or(0)
            * 2
    } else {
        0
    };

    TextLayoutConfig {
        node_sep: layered_cfg.node_sep,
        edge_sep: layered_cfg.edge_sep,
        rank_sep: layered_cfg.rank_sep,
        margin: layered_cfg.margin,
        ranker: Some(layered_cfg.ranker),
        padding: defaults.padding + extra_padding,
        ..defaults
    }
}

/// Run layered layout with a given measurement mode.
///
/// Shared by the Flux and Mermaid-compatible engines. Both use the same
/// layered kernel; they diverge in adapter policy and routing behavior.
pub fn run_layered_layout(
    mode: &MeasurementMode,
    diagram: &Diagram,
    config: &EngineConfig,
) -> Result<GraphGeometry, RenderError> {
    use crate::graph::geometry;

    let EngineConfig::Layered(layered_cfg) = config;
    let text_config = layout_config_from_layered(layered_cfg, diagram);
    let mut lc =
        crate::render::graph::layout_building::layered_config_for_layout(diagram, &text_config);
    lc.greedy_switch = layered_cfg.greedy_switch;
    lc.model_order_tiebreak = layered_cfg.model_order_tiebreak;
    lc.variable_rank_spacing = layered_cfg.variable_rank_spacing;
    lc.always_compound_ordering = layered_cfg.always_compound_ordering;
    lc.track_reversed_chains = layered_cfg.track_reversed_chains;
    lc.per_edge_label_spacing = layered_cfg.per_edge_label_spacing;
    lc.label_side_selection = layered_cfg.label_side_selection;
    lc.label_dummy_strategy = layered_cfg.label_dummy_strategy;

    let direction = diagram.direction;
    let mut result = match mode {
        MeasurementMode::Text => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| {
                let (w, h) = crate::render::graph::text_shape::node_dimensions(node, direction);
                (w as f64, h as f64)
            },
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| text_edge_label_dimensions(label))
            },
        ),
        MeasurementMode::Svg(metrics) => build_layered_layout_with_config(
            diagram,
            &lc,
            |node| svg_node_dimensions(metrics, node, direction),
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| metrics.edge_label_dimensions(label))
            },
        ),
    };

    center_override_subgraphs(diagram, &mut result);
    expand_parent_bounds(diagram, &mut result, 0.0, 0.0);

    let mut geom = geometry::from_layered_layout(&result, diagram);
    let has_enhancements = layered_cfg.greedy_switch
        || layered_cfg.model_order_tiebreak
        || layered_cfg.variable_rank_spacing;
    geom.enhanced_backward_routing = has_enhancements;
    Ok(geom)
}
