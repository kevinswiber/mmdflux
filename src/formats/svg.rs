//! SVG format emitter for graph-family diagrams.
//!
//! Renders a `GraphSolveResult` to SVG.

use crate::diagram::{EdgeRouting, GraphSolveResult};
use crate::diagrams::flowchart::render::svg::render_svg_from_geometry;
use crate::graph::Diagram;
use crate::render::RenderOptions;

/// Render a graph-family diagram to SVG from a solve result.
///
/// Uses default SVG render options. The solve result provides geometry;
/// edge routing defaults to orthogonal.
pub fn render_svg(diagram: &Diagram, result: &GraphSolveResult) -> String {
    let options = RenderOptions::default_svg();
    render_svg_with_options(diagram, result, &options)
}

/// Render a graph-family diagram to SVG with explicit options.
pub fn render_svg_with_options(
    diagram: &Diagram,
    result: &GraphSolveResult,
    options: &RenderOptions,
) -> String {
    let edge_routing = options.edge_routing.unwrap_or(EdgeRouting::OrthogonalRoute);
    render_svg_from_geometry(diagram, options, &result.geometry, edge_routing)
}
