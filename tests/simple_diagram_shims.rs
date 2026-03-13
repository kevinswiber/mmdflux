use mmdflux::builtins::default_registry;
use mmdflux::{DiagramFamily, OutputFormat, RenderConfig, render_diagram};

fn render_simple(input: &str) -> String {
    render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap()
}

#[test]
fn pie_definition_exists() {
    let registry = default_registry();
    let def = registry.get("pie").expect("pie should be registered");
    assert_eq!(def.id, "pie");
    assert_eq!(def.family, DiagramFamily::Chart);
}

#[test]
fn pie_detector_works() {
    let registry = default_registry();
    assert_eq!(registry.detect("pie\n\"A\": 50"), Some("pie"));
    assert_eq!(
        registry.detect("pie title My Chart\n\"A\": 50"),
        Some("pie")
    );
    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
}

#[test]
fn pie_detector_skips_comments() {
    let registry = default_registry();
    assert_eq!(registry.detect("%% comment\npie\n\"A\": 50"), Some("pie"));
}

#[test]
fn pie_detector_case_insensitive() {
    let registry = default_registry();
    assert_eq!(registry.detect("PIE\n\"A\": 50"), Some("pie"));
    assert_eq!(registry.detect("Pie\n\"A\": 50"), Some("pie"));
}

#[test]
fn pie_detector_first_word_only() {
    let registry = default_registry();
    assert_eq!(registry.detect("piechart\n\"A\": 50"), None);
}

#[test]
fn pie_instance_renders() {
    let output = render_simple("pie\n\"A\": 50\n\"B\": 50");
    assert!(!output.is_empty());
}

#[test]
fn info_definition_exists() {
    let registry = default_registry();
    let def = registry.get("info").expect("info should be registered");
    assert_eq!(def.id, "info");
    assert_eq!(def.family, DiagramFamily::Chart);
}

#[test]
fn info_detector_works() {
    let registry = default_registry();
    assert_eq!(registry.detect("info"), Some("info"));
    assert_eq!(registry.detect("pie"), Some("pie"));
}

#[test]
fn info_detector_skips_comments() {
    let registry = default_registry();
    assert_eq!(registry.detect("%% comment\ninfo"), Some("info"));
}

#[test]
fn info_detector_case_insensitive() {
    let registry = default_registry();
    assert_eq!(registry.detect("INFO"), Some("info"));
    assert_eq!(registry.detect("Info"), Some("info"));
}

#[test]
fn info_detector_first_word_only() {
    let registry = default_registry();
    assert_eq!(registry.detect("infographic"), None);
}

#[test]
fn info_instance_renders() {
    let output = render_simple("info");
    assert!(output.contains("mmdflux"));
}

#[test]
fn packet_definition_exists() {
    let registry = default_registry();
    let def = registry.get("packet").expect("packet should be registered");
    assert_eq!(def.id, "packet");
    assert_eq!(def.family, DiagramFamily::Table);
}

#[test]
fn packet_detector_works() {
    let registry = default_registry();
    assert_eq!(registry.detect("packet-beta"), Some("packet"));
    assert_eq!(registry.detect("packet"), Some("packet"));
    assert_eq!(registry.detect("graph TD"), Some("flowchart"));
}

#[test]
fn packet_detector_skips_comments() {
    let registry = default_registry();
    assert_eq!(registry.detect("%% comment\npacket-beta"), Some("packet"));
    assert_eq!(registry.detect("%% comment\npacket"), Some("packet"));
}

#[test]
fn packet_detector_case_insensitive() {
    let registry = default_registry();
    assert_eq!(registry.detect("PACKET-BETA"), Some("packet"));
    assert_eq!(registry.detect("Packet-Beta"), Some("packet"));
    assert_eq!(registry.detect("PACKET"), Some("packet"));
}

#[test]
fn packet_instance_renders() {
    let output = render_simple("packet-beta\n0-15: \"Header\"");
    assert!(!output.is_empty());
}

#[test]
fn simple_diagrams_dont_support_svg() {
    let registry = default_registry();
    for diagram_id in ["pie", "info", "packet"] {
        let instance = registry
            .create(diagram_id)
            .unwrap_or_else(|| panic!("{diagram_id} should be registered"));
        assert!(
            !instance.supports_format(OutputFormat::Svg),
            "{diagram_id} should not advertise SVG support"
        );
    }
}
