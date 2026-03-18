use std::fs;
use std::path::Path;

use mmdflux::mmds::generate_mermaid_from_str;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n")
}

fn assert_contains_connector(mermaid: &str, connector: &str) {
    assert!(
        mermaid.contains(connector),
        "expected connector '{connector}' in generated Mermaid:\n{mermaid}"
    );
}

#[test]
fn generator_emits_canonical_mermaid_for_basic_graph() {
    let mmds = fixture("generation/basic-flow.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert_eq!(
        normalize_line_endings(&mermaid),
        "flowchart TD\nA[Start]\nB[End]\nA --> B\n"
    );
}

#[test]
fn generator_maps_shape_and_edge_style_baselines() {
    let mmds = fixture("generation/shapes-and-strokes.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert!(mermaid.contains("Decision{Gate}"));
    assert!(mermaid.contains("A -.-> B"));
}

#[test]
fn generator_emits_nested_subgraph_hierarchy_with_direction() {
    let mmds = fixture("generation/subgraph-hierarchy.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert!(mermaid.contains("subgraph sg1[Pipeline]"));
    assert!(mermaid.contains("subgraph sg2[Checks]"));
    assert!(mermaid.contains("direction LR"));
}

#[test]
fn generator_escapes_labels_and_normalizes_invalid_ids() {
    let mmds = fixture("generation/escaping-cases.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert!(mermaid.contains(r#"node_1["A | B"]"#));
    assert!(mermaid.contains("node_1_2[Second]"));
    assert!(mermaid.contains("node_1 --> node_1_2"));
}

#[test]
fn docs_example_for_escaped_label_matches_generator_output() {
    let mmds = fixture("generation/escaping-cases.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert!(mermaid.contains(r#"node_1["A | B"]"#));
}

#[test]
fn generator_output_is_stable_across_repeated_runs() {
    let mmds = fixture("generation/complex-roundtrip.json");
    let first = generate_mermaid_from_str(&mmds).unwrap();
    let second = generate_mermaid_from_str(&mmds).unwrap();

    assert_eq!(first, second);
}

#[test]
fn generator_emits_minlen_connector_variants_across_styles() {
    let mmds = fixture("generation/minlen-style-matrix.json");
    let mermaid = generate_mermaid_from_str(&mmds).unwrap();

    assert_contains_connector(&mermaid, "A ---> B");
    assert_contains_connector(&mermaid, "B -..-> C");
    assert_contains_connector(&mermaid, "C ===> D");
}

#[test]
fn generator_rejects_non_graph_diagram_payloads() {
    let mmds = fixture("generation/non-graph-payload.json");
    let err = generate_mermaid_from_str(&mmds).unwrap_err();

    assert_eq!(
        err.to_string(),
        "MMDS generation error: unsupported MMDS diagram_type 'sequence'; expected flowchart or class"
    );
}
