//! Class diagram implementation.
//!
//! Class diagrams are node-edge graphs rendered using the graph-family layout engines.
//! Nodes represent classes with optional member lists; edges represent relationships
//! (association, inheritance, composition, aggregation, dependency).

pub mod compiler;
mod instance;
pub mod parser;

pub use instance::ClassInstance;

use crate::engines::graph::{DiagramFamily, OutputFormat};
use crate::registry::{DiagramDefinition, DiagramDetector};

/// Detect if input is a class diagram.
pub fn detect(input: &str) -> bool {
    crate::parser::detect_diagram_type(input) == Some(crate::parser::DiagramType::Class)
}

/// Class diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "class",
        family: DiagramFamily::Graph,
        detector: detect as DiagramDetector,
        factory: || Box::new(ClassInstance::default()),
        supported_formats: &[
            OutputFormat::Text,
            OutputFormat::Ascii,
            OutputFormat::Svg,
            OutputFormat::Mmds,
        ],
    }
}
