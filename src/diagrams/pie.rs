//! Pie diagram shim.
//!
//! Pie diagrams are rendered as simple text representations.
//! Future enhancement: render as horizontal bar charts.

use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::mermaid::pie::Pie;
use crate::prepared::{PreparedDiagram, PreparedPie};
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance, ParsedDiagram};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Detect if input is a pie diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Pie)
}

/// Pie diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "pie",
        family: DiagramFamily::Chart,
        detector: detect as DiagramDetector,
        factory: || Box::new(PieInstance::new()),
        supported_formats: SUPPORTED_FORMATS,
    }
}

/// Pie diagram instance.
#[derive(Default)]
pub struct PieInstance;

impl PieInstance {
    /// Create a new pie diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for PieInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(ParsedPie {
            pie: crate::mermaid::parse_pie(input)?,
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedPie {
    pie: Pie,
}

impl ParsedDiagram for ParsedPie {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Pie(PreparedPie { pie: self.pie }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pie_definition_matches_registry_contract() {
        let definition = definition();
        assert_eq!(definition.id, "pie");
        assert_eq!(definition.family, DiagramFamily::Chart);
    }

    #[test]
    fn pie_detect_handles_supported_keywords() {
        assert!(detect("pie\n\"A\": 50"));
        assert!(detect("%% comment\nPie title Demo\n\"A\": 50"));
        assert!(!detect("piechart\n\"A\": 50"));
    }

    #[test]
    fn pie_instance_supports_text_and_ascii_only() {
        let instance = PieInstance::new();
        assert!(instance.supports_format(OutputFormat::Text));
        assert!(instance.supports_format(OutputFormat::Ascii));
        assert!(!instance.supports_format(OutputFormat::Svg));
    }
}
