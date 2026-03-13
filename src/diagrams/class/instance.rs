//! Class diagram instance implementation.
//!
//! Parses class diagram syntax, compiles to `graph::Diagram` (graph-family IR),
//! then prepares an owned graph-family payload for runtime dispatch.

use super::compiler;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::Diagram;
use crate::mermaid::class::parse_class_diagram;
use crate::prepared::{PreparedDiagram, PreparedGraph};
use crate::registry::{DiagramInstance, ParsedDiagram};

/// Class diagram instance.
///
/// Compiles class diagram syntax to `graph::Diagram`, then prepares a
/// graph-family payload for runtime dispatch.
#[derive(Default)]
pub struct ClassInstance;

impl ClassInstance {
    /// Create a new class diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for ClassInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        let model = parse_class_diagram(input)?;
        Ok(Box::new(ParsedClass {
            diagram: compiler::compile(&model),
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedClass {
    diagram: Diagram,
}

impl ParsedDiagram for ParsedClass {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Graph(PreparedGraph {
            diagram_type: "class",
            diagram: self.diagram,
        }))
    }
}
