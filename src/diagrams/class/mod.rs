//! Class diagram implementation.
//!
//! Class diagrams are node-edge graphs rendered using the graph-family layout engines.
//! Nodes represent classes with optional member lists; edges represent relationships
//! (association, inheritance, composition, aggregation, dependency).

pub mod compiler;
mod instance;

pub use instance::ClassInstance;

/// Detect if input is a class diagram.
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Class)
}
