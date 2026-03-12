//! Flowchart diagram instance implementation.
//!
//! Compiles Mermaid flowchart syntax to `graph::Diagram` (graph-family IR),
//! then delegates rendering to the shared graph-family facade.

use super::compile_to_graph;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::parse_flowchart;
use crate::graph::Diagram;
use crate::registry::DiagramInstance;

/// Flowchart diagram instance.
///
/// Compiles flowchart syntax to `graph::Diagram`, then renders through
/// the shared graph-family pipeline via [`crate::runtime::facade`].
pub struct FlowchartInstance {
    /// Compiled graph-family IR.
    diagram: Option<Diagram>,
}

impl FlowchartInstance {
    /// Create a new flowchart instance.
    pub fn new() -> Self {
        Self { diagram: None }
    }
}

impl Default for FlowchartInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for FlowchartInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let flowchart = parse_flowchart(input)?;
        self.diagram = Some(compile_to_graph(&flowchart));
        Ok(())
    }

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let diagram = self.diagram.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        // Diagram-specific pre-processing: annotate node IDs if requested.
        let annotated;
        let diagram = if config.show_ids {
            annotated = annotate_node_ids(diagram);
            &annotated
        } else {
            diagram
        };

        // Delegate to the shared graph-family pipeline.
        crate::runtime::facade::render_graph("flowchart", diagram, format, config)
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}

/// Create a copy of the diagram with node labels annotated as "ID: Label".
/// Skips nodes where label == id (bare nodes).
fn annotate_node_ids(diagram: &Diagram) -> Diagram {
    let mut annotated = diagram.clone();
    for node in annotated.nodes.values_mut() {
        if node.label != node.id {
            node.label = format!("{}: {}", node.id, node.label);
        }
    }
    annotated
}
