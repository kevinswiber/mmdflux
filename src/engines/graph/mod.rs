//! Graph-family engine registry, adapters, and shared algorithms.
//!
//! All graph-family diagram types share the same engine registry. The
//! namespace is split explicitly by concern:
//! - `flux`, `mermaid`, and `elk` are engine adapters
//! - `algorithms::layered::kernel` is the pure graph-agnostic layered engine
//! - the outer `algorithms::layered` root owns the graph-family bridge layer:
//!   layout building / measurement adapters, float layout, and float routing
//! - `contracts` and `registry` form the explicit low-level engine API
//!
//! Low-level callers should use the fully qualified module paths instead
//! of relying on root-module re-exports for config, format, or error types.

pub mod algorithms;
/// Low-level graph-family solve contracts for direct engine callers.
pub mod contracts;
#[cfg(feature = "engine-elk")]
pub mod elk;
pub mod flux;
mod layout;
pub mod mermaid;
/// Low-level graph-family engine registry for direct engine callers.
pub mod registry;
mod selection;
mod solve;
#[cfg(test)]
mod tests;

pub use contracts::{
    EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest, GraphSolveResult,
};
pub use layout::{LabelDummyStrategy, LayoutConfig, LayoutDirection, Ranker};
pub use registry::GraphEngineRegistry;
pub use selection::{AlgorithmId, EngineAlgorithmCapabilities, EngineAlgorithmId, EngineId};
pub use solve::solve_graph_family;
