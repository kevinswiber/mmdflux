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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_instance_parses_valid_input() {
        let parsed = Box::new(ClassInstance::new())
            .parse("classDiagram\nclass User")
            .expect("class input should parse");

        let prepared = parsed
            .prepare(&RenderConfig::default())
            .expect("parsed class input should prepare");
        let PreparedDiagram::Graph(graph) = prepared else {
            panic!("class prepare should yield graph payload");
        };
        assert_eq!(graph.diagram_type, "class");
        assert!(graph.diagram.nodes.contains_key("User"));
    }

    #[test]
    fn class_instance_rejects_invalid_input() {
        let result = Box::new(ClassInstance::new()).parse("not a class diagram");
        assert!(result.is_err());
    }

    #[test]
    fn class_instance_supports_supported_formats() {
        let instance = ClassInstance::new();
        assert!(instance.supports_format(OutputFormat::Text));
        assert!(instance.supports_format(OutputFormat::Ascii));
        assert!(instance.supports_format(OutputFormat::Svg));
        assert!(instance.supports_format(OutputFormat::Mmds));
        assert!(!instance.supports_format(OutputFormat::Mermaid));
    }
}
