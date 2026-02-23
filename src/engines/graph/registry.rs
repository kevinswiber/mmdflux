//! Graph-family engine registry.
//!
//! Maps engine IDs to concrete engine adapters for the graph family.
//! All graph-family engines operate on `Diagram` → `GraphGeometry`.
//!
//! Two lookup paths coexist during transition (removed in Phase 5):
//! - Legacy: `get(LayoutEngineId)` → `&dyn GraphLayoutEngine`
//! - New: `get_solver(EngineAlgorithmId)` → `&dyn GraphEngine`

use std::collections::HashMap;

use crate::diagram::{EngineAlgorithmId, GraphEngine};
use crate::diagrams::flowchart::engine::{FluxLayeredEngine, MermaidLayeredEngine};

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
        use crate::diagram::{AlgorithmId, EngineId};

        let mut registry = Self {
            solvers: HashMap::new(),
        };

        // Combined-ID solver registrations.
        registry.register_solver(
            EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered),
            Box::new(FluxLayeredEngine::text()),
        );
        registry.register_solver(
            EngineAlgorithmId::new(EngineId::Mermaid, AlgorithmId::Layered),
            Box::new(MermaidLayeredEngine::new()),
        );

        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::{AlgorithmId, EngineId};

    #[test]
    fn default_registry_has_flux_layered_solver() {
        let registry = GraphEngineRegistry::default();
        let id = EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered);
        let solver = registry.get_solver(id);
        assert!(solver.is_some());
        assert_eq!(solver.unwrap().id().to_string(), "flux-layered");
    }

    #[test]
    fn default_registry_has_mermaid_layered_solver() {
        let registry = GraphEngineRegistry::default();
        let id = EngineAlgorithmId::new(EngineId::Mermaid, AlgorithmId::Layered);
        let solver = registry.get_solver(id);
        assert!(solver.is_some());
        assert_eq!(solver.unwrap().id().to_string(), "mermaid-layered");
    }
}
