//! Packet compliance tests on the supported public workflow.
//!
//! Detailed parser invariants live owner-local in `src/mermaid/packet.rs`.

use mmdflux::builtins::default_registry;
use mmdflux::prepared::PreparedDiagram;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

#[test]
fn packet_detects_via_builtin_registry() {
    let registry = default_registry();
    assert_eq!(
        registry.detect("packet-beta\n0-7: \"Header\"\n"),
        Some("packet")
    );
    assert_eq!(registry.detect("packet\n0-7: \"Header\"\n"), Some("packet"));
}

#[test]
fn packet_parse_prepare_via_registry() {
    let prepared = default_registry()
        .create("packet")
        .expect("packet should be registered")
        .parse("packet\ntitle Hello world\n0-7: \"Header\"\n+8: \"Payload\"\n")
        .expect("packet input should parse")
        .prepare(&RenderConfig::default())
        .expect("packet input should prepare");

    let PreparedDiagram::Packet(prepared) = prepared else {
        panic!("packet input should prepare a packet payload");
    };

    assert_eq!(prepared.packet.title.as_deref(), Some("Hello world"));
    assert_eq!(prepared.packet.blocks.len(), 2);
}

#[test]
fn packet_renders_text_and_ascii_via_public_facade() {
    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = render_diagram(
            "packet\ntitle Hello world\n0-7: \"Header\"\n+8: \"Payload\"\n",
            format,
            &RenderConfig::default(),
        )
        .expect("packet should render");
        assert!(output.contains("[Packet Diagram]"));
        assert!(output.contains("packet-beta"));
        assert!(output.contains("title Hello world"));
        assert!(output.contains("Header"));
        assert!(output.contains("Payload"));
    }
}

#[test]
fn packet_invalid_input_is_rejected_via_public_workflow() {
    let result = default_registry()
        .create("packet")
        .expect("packet should be registered")
        .parse("not packet\n");
    assert!(result.is_err());
}
