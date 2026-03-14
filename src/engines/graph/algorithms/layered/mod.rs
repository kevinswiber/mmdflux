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
#[cfg(test)]
pub use kernel::graph::DiGraph;
#[cfg(test)]
pub(crate) use kernel::pipeline::layout;
pub use kernel::types::{Direction, LabelDummyStrategy, LayoutConfig, Ranker};
#[cfg(test)]
pub use kernel::types::{EdgeLayout, LayoutResult, NodeId, Point, Rect, SelfEdgeLayout};
#[cfg(test)]
pub(crate) use kernel::{acyclic, border, nesting, order, rank};
#[cfg(test)]
pub use kernel::{graph, normalize, support, types};
pub(crate) use measurement::layout_config_from_layered;
pub use measurement::{MeasurementMode, run_layered_layout};
