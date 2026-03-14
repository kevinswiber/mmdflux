use mmdflux::OutputFormat;
use mmdflux::registry::{DiagramDefinition, DiagramFamily, DiagramRegistry};

#[test]
fn empty_registry_detects_nothing() {
    let registry = DiagramRegistry::new();
    assert!(registry.detect("graph TD\nA-->B").is_none());
}

#[test]
fn registry_detects_registered_diagram() {
    let mut registry = DiagramRegistry::new();

    registry.register(DiagramDefinition {
        id: "test",
        family: DiagramFamily::Graph,
        detector: |input| input.starts_with("test"),
        factory: || panic!("factory not called in this test"),
        supported_formats: &[OutputFormat::Text],
    });

    assert_eq!(registry.detect("test diagram"), Some("test"));
    assert!(registry.detect("other input").is_none());
}

#[test]
fn registry_detection_order_is_registration_order() {
    let mut registry = DiagramRegistry::new();

    // Register two diagrams where both could match "graph"
    registry.register(DiagramDefinition {
        id: "first",
        family: DiagramFamily::Graph,
        detector: |input| input.contains("graph"),
        factory: || panic!(),
        supported_formats: &[OutputFormat::Text],
    });
    registry.register(DiagramDefinition {
        id: "second",
        family: DiagramFamily::Graph,
        detector: |input| input.starts_with("graph"),
        factory: || panic!(),
        supported_formats: &[OutputFormat::Text],
    });

    // First registered wins
    assert_eq!(registry.detect("graph TD"), Some("first"));
}

#[test]
fn registry_get_returns_definition() {
    let mut registry = DiagramRegistry::new();

    registry.register(DiagramDefinition {
        id: "flowchart",
        family: DiagramFamily::Graph,
        detector: |_| true,
        factory: || panic!(),
        supported_formats: &[OutputFormat::Text, OutputFormat::Svg],
    });

    let def = registry.get("flowchart").unwrap();
    assert_eq!(def.id, "flowchart");
    assert_eq!(def.family, DiagramFamily::Graph);
    assert!(def.supported_formats.contains(&OutputFormat::Svg));

    assert!(registry.get("unknown").is_none());
}
