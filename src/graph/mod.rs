//! Shared graph-family core.
//!
//! `graph` owns the graph-family intermediate representation, geometry, and
//! routing primitives that both engines and render consume. It also owns the
//! shared measurement and derived grid contracts used to move between
//! float-space solves and downstream grid replay without coupling those
//! concerns to a higher-level render or engine namespace.

pub mod attachment;
mod diagram;
pub mod direction_policy;
mod edge;
pub mod geometry;
pub mod grid;
pub mod measure;
mod node;
pub mod projection;
pub mod routing;
pub mod space;
pub mod style;

pub use diagram::{Direction, Graph, Subgraph};
pub use edge::{Arrow, Edge, Stroke};
pub use geometry::GeometryLevel;
pub use node::{Node, Shape};
pub use style::{ColorToken, NodeStyle};
