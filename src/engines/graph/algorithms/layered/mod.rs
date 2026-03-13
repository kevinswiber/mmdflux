//! Shared layered layout split between a pure internal kernel and a
//! graph-family adapter bridge.

pub(crate) mod adapter;
pub(crate) mod float_layout;
pub(crate) mod float_router;
pub(crate) mod kernel;
pub(crate) mod layout_building;
#[cfg(test)]
mod layout_building_tests;
pub(crate) mod layout_subgraph_ops;
mod measurement;

pub(crate) use adapter::from_layered_layout;
pub(crate) use float_layout::build_float_layout_with_flags;
#[allow(unused_imports)]
pub use kernel::graph::DiGraph;
#[cfg(test)]
pub(crate) use kernel::pipeline::layout;
#[allow(unused_imports)]
pub use kernel::pipeline::layout_with_labels;
#[allow(unused_imports)]
pub use kernel::types::{
    Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Ranker,
    Rect, SelfEdge, SelfEdgeLayout,
};
#[allow(unused_imports)]
pub(crate) use kernel::{
    acyclic, bk, border, nesting, network_simplex, order, parent_dummy_chains, position, rank,
    rank_core,
};
#[allow(unused_imports)]
pub use kernel::{debug, graph, normalize, pipeline, support, types};
pub(crate) use measurement::layout_config_from_layered;
pub use measurement::{MeasurementMode, run_layered_layout};
