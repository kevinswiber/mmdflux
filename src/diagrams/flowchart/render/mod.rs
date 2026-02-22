//! Flowchart-specific rendering and routing modules.

pub mod edge;
pub mod layout;
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
