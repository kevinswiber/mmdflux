//! SVG rendering for graph-family diagrams.

mod bounds;
mod edges;
mod labels;
mod nodes;
mod self_edges;
mod text;
mod writer;

use std::collections::{HashMap, HashSet};

use bounds::compute_svg_bounds;
use edges::{prepare_rendered_edge_paths, render_edges};
use labels::render_edge_labels;
use nodes::{render_nodes, render_subgraphs};
use self_edges::compute_self_edge_paths;
use writer::{SvgWriter, render_defs};

use crate::format::{Curve, EdgePreset, RoutingStyle};
use crate::graph::Graph;
use crate::graph::direction_policy::build_override_node_map;
use crate::graph::geometry::{FPoint, FRect, GraphGeometry};
use crate::graph::measure::{DEFAULT_PROPORTIONAL_FONT_SIZE, ProportionalTextMetrics};
use crate::graph::routing::EdgeRouting;
use crate::simplification::PathSimplification;
use crate::{EngineId, RenderConfig};

const DEFAULT_FONT_FAMILY: &str = "\"trebuchet ms\", verdana, arial, sans-serif";

type Point = FPoint;
type Rect = FRect;

const STROKE_COLOR: &str = "#333";
const SUBGRAPH_STROKE: &str = "#888";
const NODE_FILL: &str = "white";
const TEXT_COLOR: &str = "#333";
const MIN_BASIS_VISIBLE_STEM_PX: f64 = 8.0;

/// Public SVG render options for render-only geometry emission.
#[derive(Debug, Clone)]
pub struct SvgRenderOptions {
    pub scale: f64,
    pub font_family: String,
    pub font_size: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub routing_style: RoutingStyle,
    pub curve: Curve,
    pub edge_radius: f64,
    pub diagram_padding: f64,
    pub path_simplification: PathSimplification,
}

impl Default for SvgRenderOptions {
    fn default() -> Self {
        let font_size = DEFAULT_PROPORTIONAL_FONT_SIZE;
        Self {
            scale: 1.0,
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size,
            node_padding_x: 15.0,
            node_padding_y: 15.0,
            routing_style: RoutingStyle::Orthogonal,
            curve: Curve::Basis,
            edge_radius: 5.0,
            diagram_padding: 8.0,
            path_simplification: PathSimplification::default(),
        }
    }
}

impl From<&RenderConfig> for SvgRenderOptions {
    fn from(config: &RenderConfig) -> Self {
        let mut svg = Self::default();
        if let Some(scale) = config.svg_scale {
            svg.scale = scale;
        }
        if let Some(padding_x) = config.svg_node_padding_x {
            svg.node_padding_x = padding_x;
        }
        if let Some(padding_y) = config.svg_node_padding_y {
            svg.node_padding_y = padding_y;
        }
        if let Some(radius) = config.edge_radius {
            svg.edge_radius = radius;
        }
        if let Some(padding) = config.svg_diagram_padding {
            svg.diagram_padding = padding;
        }

        let engine_id = config.layout_engine.map(|id| id.engine());
        let (def_routing, def_curve) = engine_style_defaults(engine_id);
        let (preset_routing, preset_curve) = config
            .edge_preset
            .map(EdgePreset::expand)
            .unwrap_or((def_routing, def_curve));

        svg.routing_style = config.routing_style.unwrap_or(preset_routing);
        svg.curve = config.curve.unwrap_or(preset_curve);
        svg.path_simplification = config.path_simplification;
        svg
    }
}

/// Engine defaults for SVG style (routing + curve).
///
/// When no preset or explicit style is specified, these engine-specific defaults
/// preserve the pre-Phase-7 rendering behaviour.
fn engine_style_defaults(engine: Option<EngineId>) -> (RoutingStyle, Curve) {
    match engine {
        Some(EngineId::Mermaid) => (RoutingStyle::Polyline, Curve::Basis),
        _ => (RoutingStyle::Orthogonal, Curve::Basis),
    }
}

/// Render SVG directly from precomputed graph geometry.
///
/// This is used by callers that already have `GraphGeometry`, including the
/// runtime facade and low-level replay paths.
pub(crate) fn render_svg_from_geometry(
    diagram: &Graph,
    options: &SvgRenderOptions,
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> String {
    // Merge mode-derived rerouted edges with any engine-provided rerouted edges
    // (e.g., direction-override subgraph edges set by build_float_layout).
    let mut rerouted_edges = rerouted_edge_indexes_for_mode(geom, edge_routing);
    if !matches!(edge_routing, EdgeRouting::DirectRoute) {
        rerouted_edges.extend(geom.rerouted_edges.iter().copied());
    }
    let override_nodes = build_override_node_map(diagram);
    render_svg_with_geometry_context(
        diagram,
        options,
        geom,
        &rerouted_edges,
        &override_nodes,
        edge_routing,
    )
}

fn rerouted_edge_indexes_for_mode(
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> HashSet<usize> {
    match edge_routing {
        // Pass-through paths are already positioned by the layout engine
        // and should not receive extra shape clipping.
        EdgeRouting::EngineProvided => geom.edges.iter().map(|e| e.index).collect(),
        // Orthgonal routes already encode endpoint intent and should not
        // be shape-adjusted again in SVG (all path styles).
        EdgeRouting::OrthogonalRoute => geom.edges.iter().map(|e| e.index).collect(),
        // Direct and polyline routes need normal endpoint adjustment.
        EdgeRouting::DirectRoute | EdgeRouting::PolylineRoute => HashSet::new(),
    }
}

fn render_svg_with_geometry_context(
    diagram: &Graph,
    options: &SvgRenderOptions,
    geom: &GraphGeometry,
    rerouted_edges: &HashSet<usize>,
    override_nodes: &HashMap<String, String>,
    edge_routing: EdgeRouting,
) -> String {
    let scale = options.scale;
    let metrics = ProportionalTextMetrics::new(
        options.font_size,
        options.node_padding_x,
        options.node_padding_y,
    );

    let self_edge_paths = compute_self_edge_paths(diagram, geom, &metrics);
    let prepared_edges = prepare_rendered_edge_paths(
        diagram,
        geom,
        override_nodes,
        &self_edge_paths,
        rerouted_edges,
        edge_routing,
        options.curve,
        options.edge_radius,
        options.path_simplification,
    );
    let bounds = compute_svg_bounds(
        diagram,
        geom,
        &metrics,
        &self_edge_paths,
        &prepared_edges.paths,
    );
    let padding = options.diagram_padding;
    let (min_x, min_y, max_x, max_y) = bounds.finalize(geom.bounds.width, geom.bounds.height);
    let width = (max_x - min_x + padding * 2.0) * scale;
    let height = (max_y - min_y + padding * 2.0) * scale;
    let offset_x = (-min_x + padding) * scale;
    let offset_y = (-min_y + padding) * scale;

    let mut writer = SvgWriter::new();
    writer.start_svg(
        width,
        height,
        &options.font_family,
        options.font_size * scale,
    );

    render_defs(&mut writer, scale);
    writer.start_group_transform(offset_x, offset_y);
    render_subgraphs(&mut writer, diagram, geom, &metrics, scale);
    // Render nodes before edges so arrowhead markers draw on top of node fills,
    // preventing the white node background from hiding arrowheads.
    render_nodes(&mut writer, diagram, geom, &metrics, scale);
    render_edges(
        &mut writer,
        diagram,
        &prepared_edges,
        options.curve,
        options.edge_radius,
        scale,
    );
    render_edge_labels(
        &mut writer,
        diagram,
        geom,
        &self_edge_paths,
        &prepared_edges.paths,
        override_nodes,
        &metrics,
        scale,
    );
    writer.end_group();

    writer.end_svg();
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::SvgRenderOptions;
    use crate::format::{CornerStyle, Curve, EdgePreset, RoutingStyle};
    use crate::simplification::PathSimplification;
    use crate::{EngineAlgorithmId, RenderConfig};

    #[test]
    fn default_config_uses_orthogonal_routing() {
        let options = SvgRenderOptions::from(&RenderConfig::default());
        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Basis);
    }

    #[test]
    fn step_preset_expands_to_orthogonal_linear_sharp() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Step),
            ..Default::default()
        };
        let options = SvgRenderOptions::from(&config);

        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Linear(CornerStyle::Sharp));
    }

    #[test]
    fn basis_preset_expands_to_polyline_basis() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Basis),
            ..Default::default()
        };
        let options = SvgRenderOptions::from(&config);

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
        let options = SvgRenderOptions::from(&config);

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
        let options = SvgRenderOptions::from(&config);

        assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
        assert_eq!(options.curve, Curve::Basis);
    }

    #[test]
    fn path_simplification_is_preserved() {
        let config = RenderConfig {
            edge_preset: Some(EdgePreset::Polyline),
            path_simplification: PathSimplification::Lossless,
            ..Default::default()
        };
        let options = SvgRenderOptions::from(&config);

        assert_eq!(options.path_simplification, PathSimplification::Lossless);
    }

    #[test]
    fn mermaid_engine_uses_polyline_by_default() {
        let config = RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..Default::default()
        };
        let options = SvgRenderOptions::from(&config);

        assert_eq!(options.routing_style, RoutingStyle::Polyline);
        assert_eq!(options.curve, Curve::Basis);
    }
}
