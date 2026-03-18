//! Sequence diagram instance implementation.

use super::compiler;
use crate::errors::RenderError;
use crate::mermaid::sequence::parse_sequence;
use crate::registry::{DiagramInstance, ParsedDiagram};
use crate::timeline::Sequence;

/// Sequence diagram instance.
///
/// Parses sequence diagram syntax, compiles to `Sequence`, then
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
}

struct ParsedSequence {
    model: Sequence,
}

impl ParsedDiagram for ParsedSequence {
    fn into_payload(self: Box<Self>) -> Result<crate::payload::Diagram, RenderError> {
        Ok(crate::payload::Diagram::Sequence(self.model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_instance_builds_sequence_payload() {
        let payload = Box::new(SequenceInstance::new())
            .parse("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
            .expect("sequence input should parse")
            .into_payload()
            .expect("sequence input should build a payload");
        let crate::payload::Diagram::Sequence(sequence) = payload else {
            panic!("sequence should yield a sequence payload");
        };
        assert_eq!(sequence.participants.len(), 2);
        assert_eq!(sequence.events.len(), 1);
    }

    // Engine selection rejection and format support are now tested at the
    // runtime/registry level (see tests/sequence_instance.rs).
}
