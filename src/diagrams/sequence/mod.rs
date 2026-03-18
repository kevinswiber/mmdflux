//! Sequence diagram implementation.
//!
//! Sequence diagrams are timeline-family: participants arranged horizontally
//! with messages flowing vertically between lifelines.

pub mod compiler;
mod instance;

pub use instance::SequenceInstance;

/// Detect if input is a sequence diagram.
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Sequence)
}
