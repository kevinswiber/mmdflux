//! ER diagram implementation.
//!
//! ER diagrams are node-edge graphs rendered using the graph-family layout engines.
//! Entities become compartmented nodes; relationships become edges with crow's foot
//! cardinality markers.

pub mod compiler;
mod instance;

pub use instance::ErInstance;

/// Detect if input is an ER diagram.
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::ER)
}
