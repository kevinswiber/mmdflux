//! Graph-family engine registry.
//!
//! Maps engine IDs to concrete engine adapters for the graph family.
//! All graph-family engines operate on `Diagram` → `GraphGeometry`.
//! Low-level callers can use this registry together with
//! [`crate::engines::graph::contracts`] to select engines and drive solves
//! without going through the runtime facade.

use std::collections::HashMap;

use crate::engines::graph::GraphEngine;
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::engines::graph::mermaid::MermaidLayeredEngine;
use crate::engines::graph::selection::EngineAlgorithmId;

/// Concrete trait object type for graph solvers.
type BoxedGraphSolver = Box<dyn GraphEngine>;

/// Registry of graph-family layout engines.
///
/// Maps `EngineAlgorithmId` → `GraphEngine` solver.
pub struct GraphEngineRegistry {
    solvers: HashMap<EngineAlgorithmId, BoxedGraphSolver>,
}

impl GraphEngineRegistry {
    /// Look up a solver by combined `EngineAlgorithmId`.
    pub fn get_solver(&self, id: EngineAlgorithmId) -> Option<&dyn GraphEngine> {
        self.solvers.get(&id).map(|e| e.as_ref())
    }

    /// Register a solver by combined `EngineAlgorithmId`.
    pub fn register_solver(&mut self, id: EngineAlgorithmId, engine: BoxedGraphSolver) {
        self.solvers.insert(id, engine);
    }
}

impl Default for GraphEngineRegistry {
    fn default() -> Self {
        let mut registry = Self {
            solvers: HashMap::new(),
        };

        // Combined-ID solver registrations.
        registry.register_solver(
            EngineAlgorithmId::FLUX_LAYERED,
            Box::new(FluxLayeredEngine::text()),
        );
        registry.register_solver(
            EngineAlgorithmId::MERMAID_LAYERED,
            Box::new(MermaidLayeredEngine::new()),
        );

        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_flux_layered_solver() {
        let registry = GraphEngineRegistry::default();
        let id = EngineAlgorithmId::FLUX_LAYERED;
        let solver = registry.get_solver(id);
        assert!(solver.is_some());
        assert_eq!(solver.unwrap().id().to_string(), "flux-layered");
    }

    #[test]
    fn default_registry_has_mermaid_layered_solver() {
        let registry = GraphEngineRegistry::default();
        let id = EngineAlgorithmId::MERMAID_LAYERED;
        let solver = registry.get_solver(id);
        assert!(solver.is_some());
        assert_eq!(solver.unwrap().id().to_string(), "mermaid-layered");
    }
}
