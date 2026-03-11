//! Shared graph-family solve entry point.
//!
//! All graph-family diagram types (flowchart, class, …) dispatch layout
//! through this function.  It resolves the concrete engine from the
//! registry and delegates the solve.

use crate::diagram::{
    AlgorithmId, EngineAlgorithmId, EngineConfig, EngineId, GraphSolveRequest, GraphSolveResult,
    OutputFormat, RenderConfig, RenderError,
};
use crate::engines::graph::GraphEngineRegistry;
use crate::graph::Diagram;

/// Solve layout through the graph engine registry.
///
/// Shared by flowchart, class, and any future graph-family diagram.
///
/// For text/ascii output, engines that only support SVG/MMDS (e.g.
/// mermaid-layered) are transparently replaced by the flux variant of
/// the same algorithm family.
pub(crate) fn solve_graph_family(
    diagram: &Diagram,
    engine_id: EngineAlgorithmId,
    config: &RenderConfig,
    format: OutputFormat,
) -> Result<GraphSolveResult, RenderError> {
    // Mermaid-layered only supports SVG/MMDS; fall back to flux for text.
    let engine_id = if matches!(format, OutputFormat::Text | OutputFormat::Ascii)
        && engine_id.engine() == EngineId::Mermaid
    {
        EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered)
    } else {
        engine_id
    };

    let request = GraphSolveRequest::from_config(config, format);
    let registry = GraphEngineRegistry::default();
    let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
        message: format!("no engine registered for: {engine_id}"),
    })?;
    engine.solve(
        diagram,
        &EngineConfig::Layered(config.layout.clone()),
        &request,
    )
}
