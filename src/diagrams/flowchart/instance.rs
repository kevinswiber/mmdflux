//! Flowchart diagram instance implementation.
//!
//! Compiles Mermaid flowchart syntax to `graph::Diagram` (graph-family IR),
//! then prepares an owned graph-family payload for runtime dispatch.

use super::compile_to_graph;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::Diagram;
use crate::mermaid::parse_flowchart;
use crate::prepared::{PreparedDiagram, PreparedGraph};
use crate::registry::{DiagramInstance, ParsedDiagram};

/// Flowchart diagram instance.
///
/// Compiles flowchart syntax to `graph::Diagram`, then prepares a
/// graph-family payload for runtime dispatch.
#[derive(Default)]
pub struct FlowchartInstance;

impl FlowchartInstance {
    /// Create a new flowchart instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for FlowchartInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        let flowchart = parse_flowchart(input)?;
        Ok(Box::new(ParsedFlowchart {
            diagram: compile_to_graph(&flowchart),
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedFlowchart {
    diagram: Diagram,
}

impl ParsedDiagram for ParsedFlowchart {
    fn prepare(self: Box<Self>, config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        // Diagram-specific pre-processing: annotate node IDs if requested.
        let diagram = if config.show_ids {
            annotate_node_ids(self.diagram)
        } else {
            self.diagram
        };

        Ok(PreparedDiagram::Graph(PreparedGraph {
            diagram_type: "flowchart",
            diagram,
        }))
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
