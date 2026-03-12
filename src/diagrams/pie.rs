//! Pie diagram shim.
//!
//! Pie diagrams are rendered as simple text representations.
//! Future enhancement: render as horizontal bar charts.

use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::frontends::mermaid::pie::Pie;
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
    crate::frontends::mermaid::detect_diagram_type(input)
        == Some(crate::frontends::mermaid::DiagramType::Pie)
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
pub struct PieInstance;

impl PieInstance {
    /// Create a new pie diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PieInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for PieInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(ParsedPie {
            pie: crate::frontends::mermaid::parse_pie(input)?,
            source: input.to_string(),
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedPie {
    pie: Pie,
    source: String,
}

impl ParsedDiagram for ParsedPie {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Pie(PreparedPie {
            pie: self.pie,
            source: self.source,
        }))
    }
}
