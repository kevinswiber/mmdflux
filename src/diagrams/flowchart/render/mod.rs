//! Flowchart-specific rendering and routing modules.

pub mod edge;
pub(crate) mod layout_building;
pub(crate) mod layout_subgraph_ops;
pub(crate) mod orthogonal_router;
pub mod route_policy;
pub mod router;
pub(crate) mod routing_core;
pub mod shape;
pub mod subgraph;
pub mod svg;
pub(crate) mod svg_metrics;
pub(crate) mod svg_router;
pub mod text_adapter;
pub mod text_layout;
pub mod text_types;
