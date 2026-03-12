//! Sequence diagram instance implementation.

use super::compiler;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::sequence::parse_sequence;
use crate::prepared::{PreparedDiagram, PreparedTimeline};
use crate::registry::DiagramInstance;
use crate::timeline::sequence::model::SequenceModel;

/// Sequence diagram instance.
///
/// Parses sequence diagram syntax, compiles to `SequenceModel`, then
/// renders through the timeline-family pipeline (layout + text renderer).
pub struct SequenceInstance {
    model: Option<SequenceModel>,
}

impl SequenceInstance {
    /// Create a new sequence diagram instance.
    pub fn new() -> Self {
        Self { model: None }
    }
}

impl Default for SequenceInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for SequenceInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let statements = parse_sequence(input)?;
        self.model = Some(compiler::compile(&statements)?);
        Ok(())
    }

    fn prepare(&self, config: &RenderConfig) -> Result<PreparedDiagram<'_>, RenderError> {
        let model = self.model.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        if config.layout_engine.is_some() {
            return Err(RenderError {
                message: "layout engine selection is not supported for sequence diagrams"
                    .to_string(),
            });
        }

        Ok(PreparedDiagram::Timeline(PreparedTimeline { model }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}
