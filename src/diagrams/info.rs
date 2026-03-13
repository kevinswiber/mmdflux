//! Info diagram shim.
//!
//! Info diagrams display mmdflux version and build information.

use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::prepared::PreparedDiagram;
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance, ParsedDiagram};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Detect if input is an info diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Info)
}

/// Info diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "info",
        family: DiagramFamily::Chart,
        detector: detect as DiagramDetector,
        factory: || Box::new(InfoInstance::new()),
        supported_formats: SUPPORTED_FORMATS,
    }
}

/// Info diagram instance.
#[derive(Default)]
pub struct InfoInstance;

impl InfoInstance {
    /// Create a new info diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for InfoInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        crate::mermaid::parse_info(input)?;
        Ok(Box::new(ParsedInfo))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedInfo;

impl ParsedDiagram for ParsedInfo {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_definition_matches_registry_contract() {
        let definition = definition();
        assert_eq!(definition.id, "info");
        assert_eq!(definition.family, DiagramFamily::Chart);
    }

    #[test]
    fn info_detect_handles_supported_keywords() {
        assert!(detect("info"));
        assert!(detect("%% comment\nINFO"));
        assert!(!detect("infographic"));
    }

    #[test]
    fn info_instance_supports_text_and_ascii_only() {
        let instance = InfoInstance::new();
        assert!(instance.supports_format(OutputFormat::Text));
        assert!(instance.supports_format(OutputFormat::Ascii));
        assert!(!instance.supports_format(OutputFormat::Svg));
    }
}
