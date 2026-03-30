//! State diagram instance implementation.
//!
//! Parses state diagram syntax, compiles to `graph::Graph` (graph-family IR),
//! then builds an owned graph-family payload for runtime dispatch.

use super::compiler;
use crate::errors::RenderError;
use crate::graph::Graph;
use crate::mermaid::state::parse_state_diagram;
use crate::registry::{DiagramInstance, ParsedDiagram};

/// State diagram instance.
///
/// Compiles state diagram syntax to `graph::Graph`, then builds a
/// graph-family payload for runtime dispatch.
#[derive(Default)]
pub struct StateInstance;

impl StateInstance {
    /// Create a new state diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for StateInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        let model = parse_state_diagram(input)?;
        Ok(Box::new(ParsedState {
            diagram: compiler::compile(&model),
        }))
    }
}

struct ParsedState {
    diagram: Graph,
}

impl ParsedDiagram for ParsedState {
    fn into_payload(self: Box<Self>) -> Result<crate::payload::Diagram, RenderError> {
        Ok(crate::payload::Diagram::State(self.diagram))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_instance_parses_valid_input() {
        let parsed = Box::new(StateInstance::new())
            .parse("stateDiagram-v2\n    [*] --> Idle")
            .expect("state input should parse");

        let payload = parsed
            .into_payload()
            .expect("parsed state input should build a payload");
        let crate::payload::Diagram::State(graph) = payload else {
            panic!("state should yield a State payload");
        };
        assert!(graph.nodes.contains_key("Idle"));
    }

    #[test]
    fn state_instance_rejects_invalid_input() {
        let result = Box::new(StateInstance::new()).parse("not a state diagram");
        assert!(result.is_err());
    }
}
