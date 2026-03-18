use mmdflux::builtins::default_registry;
use mmdflux::payload::Diagram;
use mmdflux::registry::DiagramInstance;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, RenderError};

fn render_sequence(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    mmdflux::render_diagram(input, format, config)
}

fn sequence_instance() -> Box<dyn DiagramInstance> {
    default_registry()
        .create("sequence")
        .expect("sequence should be registered")
}

#[test]
fn sequence_instance_into_payload_returns_sequence_payload() {
    let payload = sequence_instance()
        .parse("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
        .unwrap()
        .into_payload()
        .unwrap();
    let Diagram::Sequence(sequence) = payload else {
        panic!("sequence should yield a sequence payload");
    };
    assert_eq!(sequence.participants.len(), 2);
    assert_eq!(sequence.events.len(), 1);
}

#[test]
fn sequence_instance_unknown_engine_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(err.message.contains("unknown engine"));
}

#[test]
fn sequence_instance_prepare_rejects_layout_engine_selection() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..RenderConfig::default()
    };
    let result = mmdflux::render_diagram(
        "sequenceDiagram\nA->>B: hello",
        mmdflux::OutputFormat::Text,
        &config,
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
