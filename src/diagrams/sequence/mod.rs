//! Sequence diagram implementation.
//!
//! Sequence diagrams are timeline-family: participants arranged horizontally
//! with messages flowing vertically between lifelines.

pub mod compiler;
mod instance;
pub mod layout;
pub mod model;

pub use instance::SequenceInstance;

use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::registry::{DiagramDefinition, DiagramDetector};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Detect if input is a sequence diagram.
pub fn detect(input: &str) -> bool {
    crate::frontends::mermaid::detect_diagram_type(input)
        == Some(crate::frontends::mermaid::DiagramType::Sequence)
}

/// Sequence diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "sequence",
        family: DiagramFamily::Timeline,
        detector: detect as DiagramDetector,
        factory: || Box::new(SequenceInstance::default()),
        supported_formats: SUPPORTED_FORMATS,
    }
}
