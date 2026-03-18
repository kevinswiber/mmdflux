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

use crate::format::{Curve, RoutingStyle};
use crate::graph::Graph;
use crate::graph::direction_policy::build_override_node_map;
use crate::graph::geometry::{FPoint, FRect, GraphGeometry};
use crate::graph::measure::{DEFAULT_PROPORTIONAL_FONT_SIZE, ProportionalTextMetrics};
use crate::graph::routing::EdgeRouting;
use crate::simplification::PathSimplification;

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

// RenderConfig conversion tests live in runtime/config.rs.
