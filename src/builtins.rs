//! Built-in diagram registry assembly.
//!
//! This module wires the crate's built-in diagram definitions into a concrete
//! [`crate::registry::DiagramRegistry`]. Reusable registry contracts live in
//! [`crate::registry`].

use crate::diagrams::{class, flowchart, sequence};
use crate::format::OutputFormat;
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramFamily, DiagramRegistry};

const GRAPH_FAMILY_FORMATS: &[OutputFormat] = &[
    OutputFormat::Text,
    OutputFormat::Ascii,
    OutputFormat::Svg,
    OutputFormat::Mmds,
];

const TIMELINE_FAMILY_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Create the default registry with all built-in diagram types.
///
/// Registration order determines detection priority. Flowchart is registered
/// first as the most common diagram type.
pub fn default_registry() -> DiagramRegistry {
    let mut registry = DiagramRegistry::new();

    // Flowchart - most common, register first.
    registry.register(DiagramDefinition {
        id: "flowchart",
        family: DiagramFamily::Graph,
        detector: flowchart::detect as DiagramDetector,
        factory: || Box::new(flowchart::FlowchartInstance::new()),
        supported_formats: GRAPH_FAMILY_FORMATS,
    });

    // Graph-family diagrams.
    registry.register(DiagramDefinition {
        id: "class",
        family: DiagramFamily::Graph,
        detector: class::detect as DiagramDetector,
        factory: || Box::new(class::ClassInstance::new()),
        supported_formats: GRAPH_FAMILY_FORMATS,
    });

    // Timeline-family diagrams.
    registry.register(DiagramDefinition {
        id: "sequence",
        family: DiagramFamily::Timeline,
        detector: sequence::detect as DiagramDetector,
        factory: || Box::new(sequence::SequenceInstance::new()),
        supported_formats: TIMELINE_FAMILY_FORMATS,
    });

    registry
}
