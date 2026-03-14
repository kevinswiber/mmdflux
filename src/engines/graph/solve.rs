//! Shared graph-family solve entry point.
//!
//! All graph-family diagram types (flowchart, class, …) dispatch layout
//! through this function.  It resolves the concrete engine from the
//! registry and delegates the solve.

use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphEngineRegistry, GraphSolveRequest, GraphSolveResult,
};
use crate::errors::RenderError;
use crate::graph::Graph;

/// Solve layout through the graph engine registry.
///
/// Shared by flowchart, class, and any future graph-family diagram.
///
pub(crate) fn solve_graph_family(
    diagram: &Graph,
    engine_id: EngineAlgorithmId,
    config: &EngineConfig,
    request: &GraphSolveRequest,
) -> Result<GraphSolveResult, RenderError> {
    let registry = GraphEngineRegistry::default();
    let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
        message: format!("no engine registered for: {engine_id}"),
    })?;
    engine.solve(diagram, config, request)
}
