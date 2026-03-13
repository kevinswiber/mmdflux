//! Output production namespaces.
//!
//! The top-level `render` module owns all output production for the crate:
//! - [`crate::render::graph`] for shared graph-family rendering backends
//! - [`crate::render::diagram`] for family-local renderers
//! - [`crate::render::text`] for shared text-output canvas and character sets

pub mod diagram;
pub mod graph;
pub mod text;
