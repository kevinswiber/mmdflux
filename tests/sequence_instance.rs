use mmdflux::diagrams::sequence::SequenceInstance;
use mmdflux::prepared::PreparedDiagram;
use mmdflux::registry::DiagramInstance;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, RenderError};

fn render_sequence(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    mmdflux::render_diagram(input, format, config)
}

#[test]
fn sequence_instance_prepare_returns_timeline_payload() {
    let mut instance = SequenceInstance::new();
    instance
        .parse("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
        .unwrap();
    let prepared = instance.prepare(&RenderConfig::default()).unwrap();
    let PreparedDiagram::Timeline(timeline) = prepared else {
        panic!("sequence prepare should yield timeline payload");
    };
    assert_eq!(timeline.model.participants.len(), 2);
    assert_eq!(timeline.model.events.len(), 1);
}

#[test]
fn sequence_instance_unknown_engine_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(err.message.contains("unknown engine"));
}

#[test]
fn sequence_instance_prepare_rejects_layout_engine_selection() {
    let mut instance = SequenceInstance::new();
    instance.parse("sequenceDiagram\nA->>B: hello").unwrap();
    let result = instance.prepare(&RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..RenderConfig::default()
    });
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message
            .contains("layout engine selection is not supported for sequence diagrams"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn runtime_dispatch_renders_sequence_text() {
    let out = render_sequence(
        "sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(out.contains("hello"));
}
