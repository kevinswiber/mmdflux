//! Class diagram implementation.
//!
//! Class diagrams are node-edge graphs rendered using the graph-family layout engines.
//! Nodes represent classes with optional member lists; edges represent relationships
//! (association, inheritance, composition, aggregation, dependency).

pub mod compiler;
mod instance;

pub use instance::ClassInstance;

use crate::format::OutputFormat;
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramFamily};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[
    OutputFormat::Text,
    OutputFormat::Ascii,
    OutputFormat::Svg,
    OutputFormat::Mmds,
];

/// Detect if input is a class diagram.
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Class)
}

/// Class diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "class",
        family: DiagramFamily::Graph,
        detector: detect as DiagramDetector,
        factory: || Box::new(ClassInstance::new()),
        supported_formats: SUPPORTED_FORMATS,
    }
}
