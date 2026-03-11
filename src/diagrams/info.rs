//! Info diagram shim.
//!
//! Info diagrams display mmdflux version and build information.

use crate::engines::graph::{DiagramFamily, OutputFormat, RenderConfig, RenderError};
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance};

/// Detect if input is an info diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::parser::detect_diagram_type(input) == Some(crate::parser::DiagramType::Info)
}

/// Info diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "info",
        family: DiagramFamily::Chart,
        detector: detect as DiagramDetector,
        factory: || Box::new(InfoInstance::default()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii],
    }
}

/// Info diagram instance.
pub struct InfoInstance {
    parsed: bool,
}

impl InfoInstance {
    /// Create a new info diagram instance.
    pub fn new() -> Self {
        Self { parsed: false }
    }
}

impl Default for InfoInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for InfoInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !detect(input) {
            return Err("Not an info diagram".into());
        }
        self.parsed = true;
        Ok(())
    }

    fn render(&self, _format: OutputFormat, _config: &RenderConfig) -> Result<String, RenderError> {
        if !self.parsed {
            return Err("Not parsed".into());
        }

        Ok(format!(
            "mmdflux v{}\n\
             Mermaid flowchart to text/SVG renderer",
            env!("CARGO_PKG_VERSION")
        ))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(format, OutputFormat::Text | OutputFormat::Ascii)
    }
}
