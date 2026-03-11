//! MMDS JSON format emitter for graph-family diagrams.
//!
//! Renders a `GraphSolveResult` to MMDS JSON.

use crate::engines::graph::{
    EdgeRouting, GeometryLevel, GraphSolveResult, PathSimplification, RenderError,
};
use crate::graph::Diagram;
use crate::mmds::to_mmds_json_typed;
use crate::render::graph::routing::route_graph_geometry;

/// Render a graph-family diagram to MMDS JSON from a solve result.
///
/// Uses routed geometry level and default path simplification.
/// If the solve result lacks routed geometry, orthogonal routing is applied.
pub fn render_mmds(diagram_type: &str, diagram: &Diagram, result: &GraphSolveResult) -> String {
    render_mmds_full(
        diagram_type,
        diagram,
        result,
        GeometryLevel::Routed,
        PathSimplification::default(),
    )
    .unwrap_or_else(|e| panic!("MMDS rendering failed: {}", e.message))
}

/// Render a graph-family diagram to MMDS JSON with full control.
///
/// `diagram_type` is the MMDS `diagram_type` metadata field (e.g.
/// `"flowchart"`, `"class"`).  If the geometry level is `Routed` and
/// the solve result lacks routed geometry, orthogonal routing is
/// applied automatically.
pub fn render_mmds_full(
    diagram_type: &str,
    diagram: &Diagram,
    result: &GraphSolveResult,
    level: GeometryLevel,
    path_simplification: PathSimplification,
) -> Result<String, RenderError> {
    let routed_owned;
    let routed_ref = match result.routed.as_ref() {
        Some(r) => Some(r),
        None if matches!(level, GeometryLevel::Routed) => {
            routed_owned =
                route_graph_geometry(diagram, &result.geometry, EdgeRouting::OrthogonalRoute);
            Some(&routed_owned)
        }
        None => None,
    };
    to_mmds_json_typed(
        diagram_type,
        diagram,
        &result.geometry,
        routed_ref,
        level,
        path_simplification,
        Some(result.engine_id),
    )
}
