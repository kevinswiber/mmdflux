//! Sequence diagram instance implementation.

use super::model::SequenceModel;
use super::{compiler, layout};
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::mermaid::sequence::parse_sequence;
use crate::registry::DiagramInstance;
use crate::render::CharSet;
use crate::render::diagram::sequence::text;

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

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let model = self.model.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        if config.layout_engine.is_some() {
            return Err(RenderError {
                message: "layout engine selection is not supported for sequence diagrams"
                    .to_string(),
            });
        }

        if !self.supports_format(format) {
            return Err(RenderError {
                message: format!(
                    "sequence diagrams do not support {} output",
                    match format {
                        OutputFormat::Svg => "svg",
                        _ => "unknown",
                    }
                ),
            });
        }

        let seq_layout = layout::layout(model);
        let charset = match format {
            OutputFormat::Ascii => CharSet::ascii(),
            _ => CharSet::unicode(),
        };

        Ok(text::render(&seq_layout, &charset))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}
