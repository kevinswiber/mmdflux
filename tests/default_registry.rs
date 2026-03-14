use mmdflux::builtins::default_registry;

#[test]
fn default_registry_detects_flowchart() {
    let registry = default_registry();

    // Various flowchart syntax forms
    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("graph LR\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("flowchart TD\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("flowchart\nA-->B"), Some("flowchart"));
}

#[test]
fn default_registry_detects_class() {
    let registry = default_registry();
    assert_eq!(registry.detect("classDiagram\nclass User"), Some("class"));
}

#[test]
fn default_registry_includes_class_definition() {
    let registry = default_registry();
    assert!(registry.get("class").is_some());
}

#[test]
fn default_registry_class_is_graph_family() {
    let registry = default_registry();
    let def = registry.get("class").unwrap();
    assert_eq!(def.family, mmdflux::registry::DiagramFamily::Graph);
}

#[test]
fn default_registry_detects_sequence() {
    let registry = default_registry();
    assert_eq!(
        registry.detect("sequenceDiagram\nparticipant A"),
        Some("sequence")
    );
}

#[test]
fn default_registry_includes_sequence_definition() {
    let registry = default_registry();
    assert!(registry.get("sequence").is_some());
}

#[test]
fn default_registry_sequence_is_timeline_family() {
    let registry = default_registry();
    let def = registry.get("sequence").unwrap();
    assert_eq!(def.family, mmdflux::registry::DiagramFamily::Timeline);
}

#[test]
fn default_registry_does_not_register_mmds_as_a_logical_diagram() {
    let registry = default_registry();
    assert!(registry.get("mmds").is_none());
    assert!(!registry.diagram_ids().any(|id| id == "mmds"));
}

#[test]
fn default_registry_ignores_mmds_json_input() {
    let registry = default_registry();
    let input = r#"{
  "version": 1,
  "geometry_level": "layout",
  "metadata": {
    "diagram_type": "flowchart",
    "direction": "TD",
    "bounds": {"width": 100.0, "height": 50.0}
  },
  "defaults": {
    "node": {"shape": "rectangle"},
    "edge": {"stroke": "solid", "arrow_start": "none", "arrow_end": "normal", "minlen": 1}
  },
  "nodes": [],
  "edges": []
}"#;
    assert_eq!(registry.detect(input), None);
}

#[test]
fn default_registry_flowchart_first() {
    let registry = default_registry();
    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
}
