//! Flowchart diagram implementation.
//!
//! Flowcharts are node-edge graphs rendered using hierarchical (Sugiyama) layout.
//! This is the original and most feature-complete diagram type in mmdflux.

pub mod engine;
pub mod geometry;
mod instance;
pub(crate) mod render;
pub mod routing;

pub use instance::FlowchartInstance;

use crate::diagram::{DiagramFamily, OutputFormat};
use crate::registry::{DiagramDefinition, DiagramDetector};

/// Detect if input is a flowchart diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::parser::detect_diagram_type(input) == Some(crate::parser::DiagramType::Flowchart)
}

/// Flowchart diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "flowchart",
        family: DiagramFamily::Graph,
        detector: detect as DiagramDetector,
        factory: || Box::new(FlowchartInstance::default()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii, OutputFormat::Svg],
    }
}
