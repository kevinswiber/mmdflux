//! Flowchart diagram instance implementation.
//!
//! Compiles Mermaid flowchart syntax to `graph::Diagram` (graph-family IR),
//! then prepares an owned graph-family payload for runtime dispatch.

use super::compile_to_graph;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::parse_flowchart;
use crate::graph::Diagram;
use crate::prepared::{PreparedDiagram, PreparedGraph};
use crate::registry::DiagramInstance;

/// Flowchart diagram instance.
///
/// Compiles flowchart syntax to `graph::Diagram`, then prepares a
/// graph-family payload for runtime dispatch.
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

    fn prepare(self: Box<Self>, config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        let diagram = self.diagram.ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        // Diagram-specific pre-processing: annotate node IDs if requested.
        let diagram = if config.show_ids {
            annotate_node_ids(diagram)
        } else {
            diagram
        };

        Ok(PreparedDiagram::Graph(PreparedGraph {
            diagram_type: "flowchart",
            diagram,
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}

/// Annotate node labels as "ID: Label", skipping bare nodes where label == id.
fn annotate_node_ids(mut diagram: Diagram) -> Diagram {
    for node in diagram.nodes.values_mut() {
        if node.label != node.id {
            node.label = format!("{}: {}", node.id, node.label);
        }
    }
    diagram
}
