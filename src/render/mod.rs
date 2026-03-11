//! Output production namespaces.
//!
//! The top-level `render` module owns all output production for the crate:
//! - [`crate::render::graph`] for shared graph-family rendering backends
//! - [`crate::render::diagram`] for family-local renderers
//! - shared primitives (`Canvas`, `CharSet`, `intersect`) reused across both

pub mod diagram;
pub mod graph;
pub mod primitives;

pub use primitives::canvas::Canvas;
pub use primitives::chars::CharSet;
pub use primitives::intersect;
