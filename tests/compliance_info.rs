//! Info compliance tests on the supported public workflow.
//!
//! Detailed parser invariants live owner-local in `src/mermaid/info.rs`.

use mmdflux::builtins::default_registry;
use mmdflux::prepared::PreparedDiagram;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

#[test]
fn info_detects_via_builtin_registry() {
    let registry = default_registry();
    assert_eq!(registry.detect("info"), Some("info"));
    assert_eq!(registry.detect("%% comment\nINFO"), Some("info"));
}

#[test]
fn info_parse_prepare_via_registry() {
    let prepared = default_registry()
        .create("info")
        .expect("info should be registered")
        .parse("info\nshowInfo\ntitle My Info\n")
        .expect("info input should parse")
        .prepare(&RenderConfig::default())
        .expect("info input should prepare");

    assert!(matches!(prepared, PreparedDiagram::Info));
}

#[test]
fn info_renders_text_and_ascii_via_public_facade() {
    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = render_diagram(
            "info\nshowInfo\ntitle My Info\n",
            format,
            &RenderConfig::default(),
        )
        .expect("info should render");
        assert!(output.contains("mmdflux v"));
        assert!(output.contains("Mermaid flowchart to text/SVG renderer"));
    }
}

#[test]
fn info_invalid_input_is_rejected_via_public_workflow() {
    let result = default_registry()
        .create("info")
        .expect("info should be registered")
        .parse("not info\n");
    assert!(result.is_err());
}
