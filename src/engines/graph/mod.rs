//! Graph-family engine registry, adapters, and shared algorithms.
//!
//! All graph-family diagram types share the same engine registry. The
//! namespace is split explicitly by concern:
//! - `flux`, `mermaid`, and `elk` are engine adapters
//! - `algorithms::layered` is the shared layered-layout kernel
//!
//! Low-level callers should use the fully qualified module paths instead
//! of relying on root-module re-exports for config, format, or error types.

pub mod algorithms;
#[doc(hidden)]
pub mod contracts;
#[cfg(feature = "engine-elk")]
pub mod elk;
pub mod flux;
pub mod mermaid;
#[doc(hidden)]
pub mod registry;
mod solve;
#[cfg(test)]
mod tests;

pub(crate) use contracts::{EngineConfig, GraphEngine, GraphSolveRequest, GraphSolveResult};
pub(crate) use registry::GraphEngineRegistry;
pub(crate) use solve::solve_graph_family;
