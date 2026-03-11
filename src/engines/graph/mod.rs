//! Graph-family engine registry and adapters.
//!
//! All graph-family diagram types (flowchart, state, class, ER) share
//! the same engine registry. Engines are looked up by `LayoutEngineId`.

pub mod cose;
#[cfg(feature = "engine-elk")]
pub mod elk;
pub mod layered;
mod registry;
mod solve;

pub use registry::GraphEngineRegistry;
pub(crate) use solve::solve_graph_family;

pub use crate::diagram::{
    EdgeRouting, EngineConfig, GraphEngine, GraphSolveRequest, GraphSolveResult,
};
