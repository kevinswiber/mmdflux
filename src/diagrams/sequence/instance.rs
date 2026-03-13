//! Sequence diagram instance implementation.

use super::compiler;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::mermaid::sequence::parse_sequence;
use crate::prepared::{PreparedDiagram, PreparedTimeline};
use crate::registry::{DiagramInstance, ParsedDiagram};
use crate::timeline::sequence::model::SequenceModel;

/// Sequence diagram instance.
///
/// Parses sequence diagram syntax, compiles to `SequenceModel`, then
/// renders through the timeline-family pipeline (layout + text renderer).
#[derive(Default)]
pub struct SequenceInstance;

impl SequenceInstance {
    /// Create a new sequence diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for SequenceInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        let statements = parse_sequence(input)?;
        Ok(Box::new(ParsedSequence {
            model: compiler::compile(&statements)?,
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        super::SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedSequence {
    model: SequenceModel,
}

impl ParsedDiagram for ParsedSequence {
    fn prepare(self: Box<Self>, config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        if config.layout_engine.is_some() {
            return Err(RenderError {
                message: "layout engine selection is not supported for sequence diagrams"
                    .to_string(),
            });
        }

        Ok(PreparedDiagram::Timeline(PreparedTimeline {
            model: self.model,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::OutputFormat;

    #[test]
    fn sequence_instance_prepares_timeline_payload() {
        let prepared = Box::new(SequenceInstance::new())
            .parse("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
            .expect("sequence input should parse")
            .prepare(&RenderConfig::default())
            .expect("sequence input should prepare");
        let PreparedDiagram::Timeline(timeline) = prepared else {
            panic!("sequence prepare should yield timeline payload");
        };
        assert_eq!(timeline.model.participants.len(), 2);
        assert_eq!(timeline.model.events.len(), 1);
    }

    #[test]
    fn sequence_instance_rejects_layout_engine_selection() {
        let result = Box::new(SequenceInstance::new())
            .parse("sequenceDiagram\nA->>B: hello")
            .expect("sequence input should parse")
            .prepare(&RenderConfig {
                layout_engine: Some(crate::EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..RenderConfig::default()
            });
        assert!(result.is_err());
    }

    #[test]
    fn sequence_instance_supports_text_only_formats() {
        let instance = SequenceInstance::new();
        assert!(instance.supports_format(OutputFormat::Text));
        assert!(instance.supports_format(OutputFormat::Ascii));
        assert!(!instance.supports_format(OutputFormat::Svg));
        assert!(!instance.supports_format(OutputFormat::Mmds));
    }
}
