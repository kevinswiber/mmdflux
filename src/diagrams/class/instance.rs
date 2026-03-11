//! Class diagram instance implementation.
//!
//! Parses class diagram syntax, compiles to `graph::Diagram` (graph-family IR),
//! then delegates rendering to the shared graph-family facade.

use super::compiler;
use crate::engines::graph::{OutputFormat, RenderConfig, RenderError};
use crate::frontends::mermaid::class::parse_class_diagram;
use crate::graph::Diagram;
use crate::registry::DiagramInstance;

/// Class diagram instance.
///
/// Compiles class diagram syntax to `graph::Diagram`, then renders through
/// the shared graph-family pipeline via [`crate::runtime::facade`].
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

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let diagram = self.diagram.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        // Delegate to the shared graph-family pipeline.
        crate::runtime::facade::render_graph("class", diagram, format, config)
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}
