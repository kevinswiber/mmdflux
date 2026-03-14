//! Pure layered-layout kernel.
//!
//! This subtree is the internal extraction boundary for graph-agnostic
//! Sugiyama/dagre-like phases. Graph-family adapters and measurement helpers
//! stay in the parent `layered/` module.

pub mod debug;
pub mod graph;
pub mod normalize;
pub mod pipeline;
pub mod support;
pub mod types;

pub(crate) mod acyclic;
pub(crate) mod bk;
pub(crate) mod border;
pub(crate) mod nesting;
pub(crate) mod network_simplex;
pub(crate) mod order;
pub(crate) mod parent_dummy_chains;
pub(crate) mod position;
pub(crate) mod rank;
pub(crate) mod rank_core;

#[cfg(test)]
mod dagre_parity_tests;
#[cfg(test)]
mod model_order_tests;
#[cfg(test)]
mod regression_tests;

pub use graph::DiGraph;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use pipeline::layout;
#[allow(unused_imports)]
pub use pipeline::layout_with_labels;
#[allow(unused_imports)]
pub use types::{
    Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Ranker,
    Rect, SelfEdge, SelfEdgeLayout,
};
