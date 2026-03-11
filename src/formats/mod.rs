//! Format-owned emitters for graph-family diagrams.
//!
//! Each submodule renders a `GraphSolveResult` into a specific output format.
//! The emitters consume graph-family contracts (`Diagram` + `GraphSolveResult`)
//! rather than parser-specific or renderer-specific intermediate state.

pub mod mmds;
pub mod svg;
pub mod text;
