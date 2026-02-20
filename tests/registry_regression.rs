//! Parity checks between the direct render API and registry instance API.

use mmdflux::diagram::{EngineAlgorithmId, OutputFormat, RenderConfig};
use mmdflux::registry::default_registry;
use mmdflux::render::{RenderOptions, render};
use mmdflux::{build_diagram, parse_flowchart};

/// Helper to compare direct vs registry rendering paths.
fn compare_outputs(input: &str, ascii: bool) {
    // Direct API path
    let flowchart = parse_flowchart(input).expect("Direct path parse failed");
    let diagram = build_diagram(&flowchart);
    let output_format = if ascii {
        OutputFormat::Ascii
    } else {
        OutputFormat::Text
    };
    let old_options = RenderOptions {
        output_format,
        ..Default::default()
    };
    let old_output = render(&diagram, &old_options);

    // Registry API path
    let registry = default_registry();
    let diagram_id = registry.detect(input).expect("Registry path detect failed");
    assert_eq!(diagram_id, "flowchart");

    let mut instance = registry
        .create(diagram_id)
        .expect("Registry path create failed");
    instance.parse(input).expect("Registry path parse failed");

    let new_output = instance
        .render(output_format, &RenderConfig::default())
        .expect("Registry path render failed");

    assert_eq!(
        old_output, new_output,
        "Output mismatch for input:\n{}\n\nOld:\n{}\n\nNew:\n{}",
        input, old_output, new_output
    );
}

#[test]
fn regression_simple_graph() {
    compare_outputs("graph TD\nA-->B", false);
    compare_outputs("graph TD\nA-->B", true);
}

#[test]
fn regression_graph_lr() {
    compare_outputs("graph LR\nA-->B-->C", false);
}

#[test]
fn regression_flowchart_keyword() {
    compare_outputs("flowchart TD\nA[Start]-->B[End]", false);
}

#[test]
fn regression_with_labels() {
    compare_outputs("graph TD\nA-->|label|B", false);
}

#[test]
fn regression_multiple_nodes() {
    compare_outputs("graph TD\nA-->B\nB-->C\nC-->D", false);
}

#[test]
fn regression_fan_out() {
    compare_outputs("graph TD\nA-->B\nA-->C\nA-->D", false);
}

#[test]
fn regression_fan_in() {
    compare_outputs("graph TD\nA-->D\nB-->D\nC-->D", false);
}

#[test]
fn regression_shapes() {
    compare_outputs("graph TD\nA[rect]\nB(round)\nC{diamond}", false);
}

#[test]
fn regression_edge_styles() {
    compare_outputs("graph TD\nA-->B\nA-.->C\nA==>D", false);
}

#[test]
fn regression_subgraph() {
    compare_outputs("graph TD\nsubgraph sg[Title]\nA-->B\nend\nC-->A", false);
}

#[test]
fn regression_backward_edge() {
    compare_outputs("graph TD\nA-->B\nB-->A", false);
}

#[test]
fn regression_self_edge() {
    compare_outputs("graph TD\nA-->A", false);
}

// Engine selection via registry path
#[test]
fn regression_engine_selection_via_registry() {
    let registry = default_registry();
    let input = "graph TD\nA-->B";

    let mut instance = registry.create("flowchart").unwrap();
    instance.parse(input).unwrap();

    // Default (None) and explicit "layered" should produce identical output
    let default_out = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    let layered_out = instance
        .render(
            OutputFormat::Text,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(default_out, layered_out);

    // Unknown engine should error
    let err = instance.render(
        OutputFormat::Text,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("elk-layered").unwrap()),
            ..Default::default()
        },
    );
    assert!(err.is_err());
}

// =============================================================================
// Layered stability: high-risk fixtures that exercise complex layout paths
// =============================================================================

#[test]
fn layered_stability_double_skip() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/double_skip.mmd")
        .expect("double_skip.mmd fixture");
    compare_outputs(&input, false);
}

#[test]
fn layered_stability_skip_edge_collision() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/skip_edge_collision.mmd")
        .expect("skip_edge_collision.mmd fixture");
    compare_outputs(&input, false);
}

#[test]
fn layered_stability_simple_cycle() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple_cycle.mmd")
        .expect("simple_cycle.mmd");
    compare_outputs(&input, false);
}

#[test]
fn layered_stability_multiple_cycles() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/multiple_cycles.mmd")
        .expect("multiple_cycles.mmd");
    compare_outputs(&input, false);
}

#[test]
fn layered_stability_nested_subgraph() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/nested_subgraph.mmd")
        .expect("nested_subgraph.mmd fixture");
    compare_outputs(&input, false);
}

// =============================================================================
// Engine selection stability
// =============================================================================

#[test]
fn layered_stability_engine_selection_consistent() {
    // Verify that explicit layered selection produces same output as default
    // for all high-risk fixtures
    let fixtures = [
        "tests/fixtures/flowchart/double_skip.mmd",
        "tests/fixtures/flowchart/skip_edge_collision.mmd",
        "tests/fixtures/flowchart/simple_cycle.mmd",
    ];

    for path in &fixtures {
        let input = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));

        let registry = default_registry();
        let mut instance = registry.create("flowchart").unwrap();
        instance.parse(&input).unwrap();

        let default_out = instance
            .render(OutputFormat::Text, &RenderConfig::default())
            .unwrap();
        let layered_out = instance
            .render(
                OutputFormat::Text,
                &RenderConfig {
                    layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(
            default_out, layered_out,
            "Engine selection changed output for {path}"
        );
    }
}

// =============================================================================
// Cross-family isolation: class diagrams don't regress flowcharts
// =============================================================================

#[test]
fn cross_family_flowchart_unchanged_after_class_support() {
    // Verify all flowchart fixtures still produce identical output
    // with class support registered in the same registry
    let registry = default_registry();
    assert!(
        registry.get("class").is_some(),
        "class should be registered"
    );
    assert!(
        registry.get("flowchart").is_some(),
        "flowchart should be registered"
    );

    // Flowchart detection still works
    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("flowchart LR\nA-->B"), Some("flowchart"));

    // Class detection doesn't interfere
    assert_eq!(registry.detect("classDiagram\nclass A"), Some("class"));

    // Both render independently
    let mut fc_instance = registry.create("flowchart").unwrap();
    fc_instance.parse("graph TD\nA-->B").unwrap();
    let fc_out = fc_instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();

    let mut cl_instance = registry.create("class").unwrap();
    cl_instance.parse("classDiagram\nA --> B").unwrap();
    let cl_out = cl_instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();

    // Both produce non-empty output with the expected nodes
    assert!(fc_out.contains('A'));
    assert!(cl_out.contains('A'));
}

#[test]
fn cross_family_class_does_not_steal_flowchart_detection() {
    let registry = default_registry();

    // Inputs with "class" in them that are NOT classDiagram
    assert_eq!(
        registry.detect("graph TD\nclassA --> classB"),
        Some("flowchart")
    );
    assert_eq!(
        registry.detect("graph TD\nA[class User]-->B"),
        Some("flowchart")
    );
}

#[test]
fn class_engine_selection_default_matches_explicit_layered() {
    let registry = default_registry();
    let mut instance = registry.create("class").unwrap();
    instance
        .parse("classDiagram\nclass A\nclass B\nA --> B")
        .unwrap();

    let default_out = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    let layered_out = instance
        .render(
            OutputFormat::Text,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..Default::default()
            },
        )
        .unwrap();

    // Class doesn't use engine selection path yet, but output should still be stable
    assert_eq!(default_out, layered_out);
}

// =============================================================================
// Cross-family isolation: sequence diagrams don't regress flowcharts/class
// =============================================================================

#[test]
fn cross_family_flowchart_unchanged_after_sequence_support() {
    let registry = default_registry();
    assert!(
        registry.get("sequence").is_some(),
        "sequence should be registered"
    );
    assert!(
        registry.get("flowchart").is_some(),
        "flowchart should be registered"
    );

    // Flowchart detection still works
    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));

    // Sequence detection doesn't interfere
    assert_eq!(
        registry.detect("sequenceDiagram\nA->>B: hi"),
        Some("sequence")
    );
}

#[test]
fn cross_family_sequence_does_not_steal_flowchart_detection() {
    let registry = default_registry();
    // "sequence" in node names should not trigger sequence detector
    assert_eq!(
        registry.detect("graph TD\nsequence-->end_seq"),
        Some("flowchart")
    );
}

#[test]
fn cross_family_sequence_does_not_steal_class_detection() {
    let registry = default_registry();
    assert_eq!(
        registry.detect("classDiagram\nclass Sequence"),
        Some("class")
    );
}

// Test all existing fixtures
#[test]
fn regression_all_fixtures() {
    use std::fs;

    let fixtures_dir = std::path::Path::new("tests/fixtures/flowchart");
    for entry in fs::read_dir(fixtures_dir).expect("fixtures dir") {
        let entry = entry.expect("fixture entry");
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "mmd") {
            let input = fs::read_to_string(&path).expect("read fixture");
            compare_outputs(&input, false);
        }
    }
}

#[test]
fn mmds_dispatch_path_reaches_mmds_instance() {
    let input = std::fs::read_to_string("tests/fixtures/mmds/minimal-layout.json")
        .expect("minimal-layout fixture should exist");
    let registry = default_registry();

    let diagram_id = registry
        .detect(&input)
        .expect("registry should detect MMDS");
    assert_eq!(diagram_id, "mmds");

    let mut instance = registry
        .create(diagram_id)
        .expect("registry should create MMDS instance");
    instance.parse(&input).expect("MMDS parse should succeed");

    let rendered = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .expect("layout MMDS payload should render through registry dispatch");
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("End"));
}
