//! Class diagram instance implementation.
//!
//! Parses class diagram syntax, compiles to `graph::Diagram` (graph-family IR),
//! then builds an owned graph-family payload for runtime dispatch.

use super::compiler;
use crate::errors::RenderError;
use crate::graph::Graph;
use crate::mermaid::class::parse_class_diagram;
use crate::registry::{DiagramInstance, ParsedDiagram};

/// Class diagram instance.
///
/// Compiles class diagram syntax to `graph::Graph`, then builds a
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
}

struct ParsedClass {
    diagram: Graph,
}

impl ParsedDiagram for ParsedClass {
    fn into_payload(self: Box<Self>) -> Result<crate::payload::Diagram, RenderError> {
        Ok(crate::payload::Diagram::Class(self.diagram))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_instance_parses_valid_input() {
        let parsed = Box::new(ClassInstance::new())
            .parse("classDiagram\nclass User")
            .expect("class input should parse");

        let payload = parsed
            .into_payload()
            .expect("parsed class input should build a payload");
        let crate::payload::Diagram::Class(graph) = payload else {
            panic!("class should yield a Class payload");
        };
        assert!(graph.nodes.contains_key("User"));
    }

    #[test]
    fn class_instance_rejects_invalid_input() {
        let result = Box::new(ClassInstance::new()).parse("not a class diagram");
        assert!(result.is_err());
    }

    // Format support is now tested at the registry level.
}
