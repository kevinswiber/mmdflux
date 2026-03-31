//! ER diagram instance implementation.
//!
//! Parses ER diagram syntax, compiles to `graph::Graph` (graph-family IR),
//! then builds an owned graph-family payload for runtime dispatch.

use super::compiler;
use crate::errors::RenderError;
use crate::graph::Graph;
use crate::mermaid::er::parse_er_diagram;
use crate::registry::{DiagramInstance, ParsedDiagram};

/// ER diagram instance.
///
/// Compiles ER diagram syntax to `graph::Graph`, then builds a
/// graph-family payload for runtime dispatch.
#[derive(Default)]
pub struct ErInstance;

impl ErInstance {
    /// Create a new ER diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for ErInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        let model = parse_er_diagram(input)?;
        Ok(Box::new(ParsedEr {
            diagram: compiler::compile(&model),
        }))
    }
}

struct ParsedEr {
    diagram: Graph,
}

impl ParsedDiagram for ParsedEr {
    fn into_payload(self: Box<Self>) -> Result<crate::payload::Diagram, RenderError> {
        Ok(crate::payload::Diagram::ER(self.diagram))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn er_instance_parses_valid_input() {
        let parsed = Box::new(ErInstance::new())
            .parse("erDiagram\n    CUSTOMER ||--o{ ORDER : places")
            .expect("ER input should parse");

        let payload = parsed
            .into_payload()
            .expect("parsed ER input should build a payload");
        let crate::payload::Diagram::ER(graph) = payload else {
            panic!("ER should yield an ER payload");
        };
        assert!(graph.nodes.contains_key("CUSTOMER"));
        assert!(graph.nodes.contains_key("ORDER"));
    }

    #[test]
    fn er_instance_rejects_invalid_input() {
        let result = Box::new(ErInstance::new()).parse("not an ER diagram");
        assert!(result.is_err());
    }
}
