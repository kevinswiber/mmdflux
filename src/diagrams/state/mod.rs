//! State diagram implementation.
//!
//! State diagrams are node-edge graphs rendered using the graph-family layout engines.
//! Nodes represent states; edges represent transitions between states.

pub mod compiler;
mod instance;

pub use instance::StateInstance;

/// Detect if input is a state diagram.
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::State)
}
