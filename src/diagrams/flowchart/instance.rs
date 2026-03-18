//! Flowchart diagram instance implementation.
//!
//! Compiles Mermaid flowchart syntax to `graph::Diagram` (graph-family IR),
//! then builds an owned graph-family payload for runtime dispatch.

use super::compile_to_graph;
use crate::errors::RenderError;
use crate::graph::Graph;
use crate::mermaid::parse_flowchart;
use crate::registry::{DiagramInstance, ParsedDiagram};

/// Flowchart diagram instance.
///
/// Compiles flowchart syntax to `graph::Graph`, then builds a
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

    fn validation_warnings(&self, input: &str) -> Vec<crate::errors::ParseDiagnostic> {
        super::validation::collect_all_warnings(input)
    }
}

struct ParsedFlowchart {
    diagram: Graph,
}

impl ParsedDiagram for ParsedFlowchart {
    fn into_payload(self: Box<Self>) -> Result<crate::payload::Diagram, RenderError> {
        Ok(crate::payload::Diagram::Flowchart(self.diagram))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_instance_parses_valid_input() {
        let payload = Box::new(FlowchartInstance::new())
            .parse("graph TD\nA[Start] --> B[End]")
            .expect("flowchart input should parse")
            .into_payload()
            .expect("parsed flowchart should build a payload");
        let crate::payload::Diagram::Flowchart(graph) = payload else {
            panic!("flowchart should yield a Flowchart payload");
        };
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn flowchart_instance_rejects_invalid_input() {
        let result = Box::new(FlowchartInstance::new()).parse("not a valid diagram }{{}");
        assert!(result.is_err());
    }

    // show_ids annotation and format support are now tested at the
    // runtime/registry level.
}
