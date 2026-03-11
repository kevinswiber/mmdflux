//! SVG format emitter for graph-family diagrams.
//!
//! Renders a `GraphSolveResult` to SVG.

use crate::engines::graph::{EdgeRouting, GraphSolveResult};
use crate::graph::Diagram;
use crate::render::graph::SvgRenderOptions;
use crate::render::graph::svg::render_svg_from_geometry;

/// Render a graph-family diagram to SVG from a solve result.
///
/// Uses default SVG render options. The solve result provides geometry;
/// edge routing defaults to orthogonal.
pub(crate) fn render_svg(diagram: &Diagram, result: &GraphSolveResult) -> String {
    let options = SvgRenderOptions::default();
    render_svg_with_options(diagram, result, &options)
}

/// Render a graph-family diagram to SVG with explicit options.
pub(crate) fn render_svg_with_options(
    diagram: &Diagram,
    result: &GraphSolveResult,
    options: &SvgRenderOptions,
) -> String {
    let edge_routing = match options.routing_style {
        crate::RoutingStyle::Direct => EdgeRouting::DirectRoute,
        crate::RoutingStyle::Polyline => EdgeRouting::PolylineRoute,
        crate::RoutingStyle::Orthogonal => EdgeRouting::OrthogonalRoute,
    };
    render_svg_from_geometry(diagram, options, &result.geometry, edge_routing)
}
