use mmdflux::builtins::default_registry;

#[test]
fn flowchart_definition_is_registered() {
    let registry = default_registry();
    let def = registry
        .get("flowchart")
        .expect("flowchart should be registered");
    assert_eq!(def.id, "flowchart");
}

#[test]
fn flowchart_detection_handles_comments_and_case() {
    let registry = default_registry();

    for input in [
        "graph TD\nA-->B",
        "flowchart LR\nA-->B",
        "%% comment\ngraph TD\nA-->B",
        "%% line 1\n%% line 2\nflowchart LR\nA-->B",
        "GRAPH TD\nA-->B",
        "Graph LR\nA-->B",
        "FLOWCHART TD\nA-->B",
        "Flowchart LR\nA-->B",
    ] {
        assert_eq!(registry.detect(input), Some("flowchart"));
    }

    assert_ne!(registry.detect("pie\n\"A\": 50"), Some("flowchart"));
}

#[test]
fn class_definition_is_registered() {
    let registry = default_registry();
    let def = registry.get("class").expect("class should be registered");
    assert_eq!(def.id, "class");
}

#[test]
fn class_detection_handles_comments_and_case() {
    let registry = default_registry();

    for input in [
        "classDiagram\nclass User",
        "CLASSDIAGRAM\nclass User",
        "ClassDiagram\nclass User",
        "%% comment\nclassDiagram\nclass User",
    ] {
        assert_eq!(registry.detect(input), Some("class"));
    }

    assert_ne!(registry.detect("graph TD\nA-->B"), Some("class"));
    assert_ne!(registry.detect("pie\n\"A\": 50"), Some("class"));
    assert_ne!(
        registry.detect("graph TD\nclassA --> classB"),
        Some("class")
    );
}

#[test]
fn sequence_definition_is_registered() {
    let registry = default_registry();
    let def = registry
        .get("sequence")
        .expect("sequence should be registered");
    assert_eq!(def.id, "sequence");
}

#[test]
fn sequence_detection_handles_comments_and_case() {
    let registry = default_registry();

    for input in [
        "sequenceDiagram\nparticipant A",
        "SEQUENCEDIAGRAM\nparticipant A",
        "SequenceDiagram\nparticipant A",
        "%% comment\nsequenceDiagram\nparticipant A",
    ] {
        assert_eq!(registry.detect(input), Some("sequence"));
    }

    assert_ne!(registry.detect("graph TD\nA-->B"), Some("sequence"));
    assert_ne!(
        registry.detect("classDiagram\nclass User"),
        Some("sequence")
    );
}
