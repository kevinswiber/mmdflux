//! Pie compliance tests on the supported public workflow.
//!
//! Detailed parser invariants live owner-local in `src/mermaid/pie.rs`.

use mmdflux::builtins::default_registry;
use mmdflux::prepared::PreparedDiagram;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

#[test]
fn pie_detects_via_builtin_registry() {
    let registry = default_registry();
    assert_eq!(registry.detect("pie\n\"A\": 50"), Some("pie"));
    assert_eq!(
        registry.detect("%% comment\nPie title Demo\n\"A\": 50"),
        Some("pie")
    );
}

#[test]
fn pie_parse_prepare_via_registry() {
    let prepared = default_registry()
        .create("pie")
        .expect("pie should be registered")
        .parse("pie showData title Demo\n\"Dogs\": 50\n\"Cats\": 25\n")
        .expect("pie input should parse")
        .prepare(&RenderConfig::default())
        .expect("pie input should prepare");

    let PreparedDiagram::Pie(prepared) = prepared else {
        panic!("pie input should prepare a pie payload");
    };

    assert!(prepared.pie.show_data);
    assert_eq!(prepared.pie.title.as_deref(), Some("Demo"));
    assert_eq!(prepared.pie.sections.len(), 2);
    assert_eq!(prepared.pie.sections[0].label, "Dogs");
    assert_eq!(prepared.pie.sections[1].label, "Cats");
}

#[test]
fn pie_renders_text_and_ascii_via_public_facade() {
    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = render_diagram(
            "pie showData title Demo\n\"Dogs\": 50\n\"Cats\": 25\n",
            format,
            &RenderConfig::default(),
        )
        .expect("pie should render");
        assert!(output.contains("[Pie Chart]"));
        assert!(output.contains("pie showData title Demo"));
        assert!(output.contains("\"Dogs\": 50"));
        assert!(output.contains("\"Cats\": 25"));
    }
}

#[test]
fn pie_invalid_input_is_rejected_via_public_workflow() {
    let result = default_registry()
        .create("pie")
        .expect("pie should be registered")
        .parse("not pie\n");
    assert!(result.is_err());
}
