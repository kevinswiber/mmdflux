//! Flowchart-specific rendering and routing modules.

pub(crate) mod backward_policy;
pub(crate) mod layout_building;
pub(crate) mod layout_subgraph_ops;
pub(crate) mod orthogonal_router;
pub mod route_policy;
pub mod svg;
pub(crate) mod svg_metrics;
pub(crate) mod svg_router;
pub mod text_adapter;
pub mod text_edge;
pub mod text_layout;
pub mod text_router;
pub(crate) mod text_routing_core;
pub mod text_shape;
pub mod text_subgraph;
pub mod text_types;
