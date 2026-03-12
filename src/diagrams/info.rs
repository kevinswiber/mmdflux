//! Info diagram shim.
//!
//! Info diagrams display mmdflux version and build information.

use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::prepared::PreparedDiagram;
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Detect if input is an info diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::frontends::mermaid::detect_diagram_type(input)
        == Some(crate::frontends::mermaid::DiagramType::Info)
}

/// Info diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "info",
        family: DiagramFamily::Chart,
        detector: detect as DiagramDetector,
        factory: || Box::new(InfoInstance::default()),
        supported_formats: SUPPORTED_FORMATS,
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
        crate::frontends::mermaid::parse_info(input)?;
        self.parsed = true;
        Ok(())
    }

    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        if !self.parsed {
            return Err("Not parsed".into());
        }

        Ok(PreparedDiagram::Info)
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}
