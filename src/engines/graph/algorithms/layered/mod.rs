//! Shared layered-layout kernel and graph-family measurement runner.

pub(crate) mod acyclic;
pub(crate) mod adapter;
pub(crate) mod bk;
pub(crate) mod border;
pub mod debug;
pub(crate) mod float_layout;
pub(crate) mod float_router;
pub(crate) mod graph;
pub(crate) mod layout_building;
#[cfg(test)]
mod layout_building_tests;
pub(crate) mod layout_subgraph_ops;
mod measurement;
pub(crate) mod nesting;
pub(crate) mod network_simplex;
pub mod normalize;
pub(crate) mod order;
pub(crate) mod parent_dummy_chains;
pub mod pipeline;
pub(crate) mod position;
pub(crate) mod rank;
mod rank_core;
#[cfg(test)]
mod regression_tests;
pub mod support;
pub mod types;

pub(crate) use adapter::from_layered_layout;
pub(crate) use float_layout::build_float_layout_with_flags;
pub use graph::DiGraph;
pub(crate) use measurement::layout_config_from_layered;
pub use measurement::{MeasurementMode, run_layered_layout};
pub use pipeline::{layout, layout_with_labels};
pub use types::{
    Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Ranker,
    Rect, SelfEdge, SelfEdgeLayout,
};
