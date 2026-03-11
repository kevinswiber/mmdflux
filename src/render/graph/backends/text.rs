//! Text format emitter for graph-family diagrams.
//!
//! Renders a `GraphSolveResult` to Unicode or ASCII text via the
//! character-grid pipeline.

use crate::engines::graph::{EdgeRouting, GraphSolveResult};
use crate::graph::Diagram;
use crate::render::graph::routing::route_graph_geometry;
use crate::render::graph::text_adapter::geometry_to_text_layout_with_routed;
use crate::render::graph::{TextRenderOptions, layout_config_for_diagram, render_text_from_layout};

/// Render a graph-family diagram to text from a solve result.
///
/// Uses default render options. The solve result provides geometry and
/// optional routed paths; if routed paths are absent they are produced
/// using orthogonal routing.
pub(crate) fn render_text(diagram: &Diagram, result: &GraphSolveResult) -> String {
    let options = TextRenderOptions::default();
    render_text_with_options(diagram, result, &options)
}

/// Render a graph-family diagram to text with explicit options.
pub(crate) fn render_text_with_options(
    diagram: &Diagram,
    result: &GraphSolveResult,
    options: &TextRenderOptions,
) -> String {
    let edge_routing = match options.routing_style {
        crate::RoutingStyle::Direct => EdgeRouting::DirectRoute,
        crate::RoutingStyle::Polyline => EdgeRouting::PolylineRoute,
        crate::RoutingStyle::Orthogonal => EdgeRouting::OrthogonalRoute,
    };
    let routed = match &result.routed {
        Some(r) => r.clone(),
        None => route_graph_geometry(diagram, &result.geometry, edge_routing),
    };
    let config = layout_config_for_diagram(diagram, options);
    let layout =
        geometry_to_text_layout_with_routed(diagram, &result.geometry, Some(&routed), &config);
    render_text_from_layout(diagram, &layout, options)
}
