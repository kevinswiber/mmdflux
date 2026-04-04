//! SVG rendering for graph-family diagrams.

mod bounds;
mod edges;
mod labels;
mod nodes;
mod self_edges;
mod text;
mod writer;

use std::collections::{BTreeSet, HashMap, HashSet};

use bounds::compute_svg_bounds;
use edges::{marker_id_for_arrow, prepare_rendered_edge_paths, render_edges};
use labels::render_edge_labels;
use nodes::{render_nodes, render_subgraphs};
use self_edges::compute_self_edge_paths;
use writer::render_defs;

use crate::format::{Curve, RoutingStyle};
use crate::graph::direction_policy::build_override_node_map;
use crate::graph::geometry::{FPoint, FRect, GraphGeometry};
use crate::graph::measure::{DEFAULT_PROPORTIONAL_FONT_SIZE, ProportionalTextMetrics};
use crate::graph::routing::EdgeRouting;
use crate::graph::{Graph, Stroke};
use crate::render::svg::SvgWriter;
use crate::render::svg::theme::{ResolvedSvgTheme, SvgRootStyle};
use crate::simplification::PathSimplification;

const DEFAULT_FONT_FAMILY: &str = "\"trebuchet ms\", verdana, arial, sans-serif";

type Point = FPoint;
type Rect = FRect;

const UNTHEMED_STROKE_COLOR: &str = "#333";
const UNTHEMED_SUBGRAPH_STROKE: &str = "#888";
const UNTHEMED_NODE_FILL: &str = "white";
const UNTHEMED_TEXT_COLOR: &str = "#333";
const MIN_BASIS_VISIBLE_STEM_PX: f64 = 8.0;

#[derive(Debug, Clone)]
pub(super) struct GraphSvgPalette {
    pub(super) root_style: SvgRootStyle,
    pub(super) node_fill: String,
    pub(super) node_stroke: String,
    pub(super) node_text: String,
    pub(super) edge_stroke: String,
    pub(super) edge_label_text: String,
    pub(super) subgraph_stroke: String,
    pub(super) subgraph_title_text: String,
    pub(super) edge_label_background: String,
    pub(super) marker_color: String,
    pub(super) dynamic_css: bool,
}

impl GraphSvgPalette {
    fn from_theme(theme: Option<&ResolvedSvgTheme>) -> Self {
        match theme {
            Some(theme) => Self {
                root_style: theme
                    .dynamic
                    .as_ref()
                    .map(|dynamic| dynamic.root_style.clone())
                    .unwrap_or_else(|| SvgRootStyle {
                        background_color: Some(theme.roles.background.clone()),
                        ..SvgRootStyle::default()
                    }),
                node_fill: theme.roles.node_fill.clone(),
                node_stroke: theme.roles.node_stroke.clone(),
                node_text: theme.roles.text.clone(),
                edge_stroke: theme.roles.line.clone(),
                edge_label_text: theme.roles.text.clone(),
                edge_label_background: theme.roles.background.clone(),
                subgraph_stroke: theme.roles.inner_stroke.clone(),
                subgraph_title_text: theme.roles.group_header.clone(),
                marker_color: theme.roles.arrow.clone(),
                dynamic_css: theme.dynamic.is_some(),
            },
            None => Self {
                root_style: SvgRootStyle::default(),
                node_fill: UNTHEMED_NODE_FILL.to_string(),
                node_stroke: UNTHEMED_STROKE_COLOR.to_string(),
                node_text: UNTHEMED_TEXT_COLOR.to_string(),
                edge_stroke: UNTHEMED_STROKE_COLOR.to_string(),
                edge_label_text: UNTHEMED_TEXT_COLOR.to_string(),
                edge_label_background: UNTHEMED_NODE_FILL.to_string(),
                subgraph_stroke: UNTHEMED_SUBGRAPH_STROKE.to_string(),
                subgraph_title_text: UNTHEMED_TEXT_COLOR.to_string(),
                marker_color: UNTHEMED_STROKE_COLOR.to_string(),
                dynamic_css: false,
            },
        }
    }
}

pub(super) fn dynamic_css_attrs(
    enabled: bool,
    role: &'static str,
    declarations: &[&'static str],
) -> String {
    if !enabled || declarations.is_empty() {
        return String::new();
    }

    let mut style = String::new();
    for declaration in declarations {
        style.push_str(declaration);
    }

    format!(" data-svg-role=\"{role}\" style=\"{style}\"")
}

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

pub(crate) fn render_svg_from_geometry_with_theme(
    diagram: &Graph,
    options: &SvgRenderOptions,
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
    theme: Option<&ResolvedSvgTheme>,
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
        theme,
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
    theme: Option<&ResolvedSvgTheme>,
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
    let palette = GraphSvgPalette::from_theme(theme);
    let used_marker_ids = collect_used_marker_ids(diagram);

    let mut writer = SvgWriter::new();
    writer.start_svg_with_root_style(
        width,
        height,
        &options.font_family,
        options.font_size * scale,
        &palette.root_style,
    );

    render_defs(&mut writer, scale, &palette, &used_marker_ids);
    writer.start_group_transform(offset_x, offset_y);
    render_subgraphs(&mut writer, diagram, geom, &metrics, scale, &palette);
    // Render nodes before edges so arrowhead markers draw on top of node fills,
    // preventing the white node background from hiding arrowheads.
    render_nodes(&mut writer, diagram, geom, &metrics, scale, &palette);
    render_edges(
        &mut writer,
        diagram,
        &prepared_edges,
        options.curve,
        options.edge_radius,
        scale,
        &palette,
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
        &palette,
    );
    writer.end_group();

    writer.end_svg();
    writer.finish()
}

fn collect_used_marker_ids(diagram: &Graph) -> BTreeSet<&'static str> {
    let mut used_marker_ids = BTreeSet::new();
    for edge in diagram
        .edges
        .iter()
        .filter(|edge| edge.stroke != Stroke::Invisible)
    {
        if let Some(marker_id) = marker_id_for_arrow(edge.arrow_start) {
            used_marker_ids.insert(marker_id);
        }
        if let Some(marker_id) = marker_id_for_arrow(edge.arrow_end) {
            used_marker_ids.insert(marker_id);
        }
    }
    used_marker_ids
}

// RenderConfig conversion tests live in runtime/config.rs.
