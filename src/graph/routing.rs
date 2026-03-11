//! Graph-family routing stage.
//!
//! Canonical re-export of the routing function. The implementation lives in
//! `diagrams::flowchart::routing` during the architecture transition; this
//! module establishes `graph::routing` as the stable import path.

pub use crate::diagrams::flowchart::routing::route_graph_geometry;
