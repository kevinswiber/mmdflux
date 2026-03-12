//! Class diagram instance implementation.
//!
//! Parses class diagram syntax, compiles to `graph::Diagram` (graph-family IR),
//! then prepares an owned graph-family payload for runtime dispatch.

use super::compiler;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::class::parse_class_diagram;
use crate::graph::Diagram;
use crate::prepared::{PreparedDiagram, PreparedGraph};
use crate::registry::DiagramInstance;

/// Class diagram instance.
///
/// Compiles class diagram syntax to `graph::Diagram`, then prepares a
/// graph-family payload for runtime dispatch.
pub struct ClassInstance {
    /// Compiled graph-family IR.
    diagram: Option<Diagram>,
}

impl ClassInstance {
    /// Create a new class diagram instance.
    pub fn new() -> Self {
        Self { diagram: None }
    }
}

impl Default for ClassInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for ClassInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let model = parse_class_diagram(input)?;
        self.diagram = Some(compiler::compile(&model));
        Ok(())
    }

    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        let diagram = self.diagram.ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        Ok(PreparedDiagram::Graph(PreparedGraph {
            diagram_type: "class",
            diagram,
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}
