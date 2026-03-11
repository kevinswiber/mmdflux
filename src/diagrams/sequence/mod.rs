//! Sequence diagram implementation.
//!
//! Sequence diagrams are timeline-family: participants arranged horizontally
//! with messages flowing vertically between lifelines.

pub mod compiler;
mod instance;
pub mod layout;
pub mod model;
pub mod parser;
pub mod render;

pub use instance::SequenceInstance;

use crate::engines::graph::{DiagramFamily, OutputFormat};
use crate::registry::{DiagramDefinition, DiagramDetector};

/// Detect if input is a sequence diagram.
pub fn detect(input: &str) -> bool {
    crate::parser::detect_diagram_type(input) == Some(crate::parser::DiagramType::Sequence)
}

/// Sequence diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "sequence",
        family: DiagramFamily::Timeline,
        detector: detect as DiagramDetector,
        factory: || Box::new(SequenceInstance::default()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii],
    }
}
