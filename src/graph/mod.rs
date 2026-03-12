//! Shared graph-family core.
//!
//! `graph` owns the graph-family intermediate representation, geometry, and
//! routing primitives that both engines and render consume. It also owns the
//! shared measurement and grid-projection contracts used to move between
//! float-space solves and text-grid replay without coupling those concerns to
//! a higher-level render or engine namespace.

mod diagram;
pub mod direction_policy;
mod edge;
pub mod geometry;
pub mod grid_projection;
pub mod measure;
mod node;
pub(crate) mod routing;

pub use diagram::*;
pub use edge::*;
pub use node::*;
pub use routing::{EdgeRouting, route_graph_geometry};
