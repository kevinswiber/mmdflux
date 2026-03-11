use mmdflux::diagrams::sequence::SequenceInstance;
use mmdflux::registry::DiagramInstance;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig};

#[test]
fn sequence_instance_parse_and_render_text() {
    let mut instance = SequenceInstance::new();
    instance
        .parse("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
        .unwrap();
    let out = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(out.contains("hello"));
}

#[test]
fn sequence_instance_unknown_engine_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(err.message.contains("unknown engine"));
}

#[test]
fn sequence_instance_rejects_layout_engine_selection() {
    let mut instance = SequenceInstance::new();
    instance.parse("sequenceDiagram\nA->>B: hello").unwrap();
    let result = instance.render(
        OutputFormat::Text,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message
            .contains("layout engine selection is not supported for sequence diagrams"),
        "unexpected error: {}",
        err.message
    );
}
