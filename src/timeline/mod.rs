//! Timeline-family runtime namespaces.
//!
//! Timeline-family diagrams do not use the shared graph-family engine stack.
//! Shared runtime model/layout code for those families lives here so parsers
//! and renderers can depend on a neutral namespace.

pub(crate) mod sequence;

pub use sequence::model::Sequence;
