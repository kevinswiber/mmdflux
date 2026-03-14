//! Runtime-owned graph dispatch boundary tests.
//!
//! Verifies that graph-family diagrams stay reachable through the supported
//! public registry/payload/runtime workflow.

use std::path::Path;

use mmdflux::OutputFormat;
use mmdflux::builtins::default_registry;
use mmdflux::payload::Diagram;
use mmdflux::registry::DiagramFamily;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect, parse, and build a graph-family payload through the public registry.
fn build_graph_fixture(path: &str) -> (Diagram, DiagramFamily) {
    let input = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let registry = default_registry();
    let id = registry
        .detect(&input)
        .unwrap_or_else(|| panic!("failed to detect diagram type for {path}"));
    let family = registry.get(id).unwrap().family;
    let payload = registry
        .create(id)
        .unwrap_or_else(|| panic!("missing registry implementation for {id}"))
        .parse(&input)
        .unwrap_or_else(|e| panic!("failed to parse {path}: {e}"))
        .into_payload(&Default::default())
        .unwrap_or_else(|e| panic!("failed to build payload for {path}: {e}"));
    (payload, family)
}

fn repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Contract: both diagram types belong to the Graph family
// ---------------------------------------------------------------------------

#[test]
fn flowchart_and_class_build_the_same_graph_family_contract() {
    let (fc_payload, fc_family) = build_graph_fixture("tests/fixtures/flowchart/simple.mmd");
    let (class_payload, class_family) = build_graph_fixture("tests/fixtures/class/simple.mmd");

    assert_eq!(fc_family, DiagramFamily::Graph);
    assert_eq!(class_family, DiagramFamily::Graph);

    assert!(matches!(fc_payload, Diagram::Flowchart(_)));
    assert!(matches!(class_payload, Diagram::Class(_)));
}

// ---------------------------------------------------------------------------
// Contract: runtime-owned graph dispatch runs end-to-end through shared path
// ---------------------------------------------------------------------------

#[test]
fn graph_runtime_dispatch_end_to_end() {
    // Both flowchart and class should produce valid output through
    // the graph-family runtime path (engine → geometry → routing → format).
    for path in &[
        "tests/fixtures/flowchart/simple.mmd",
        "tests/fixtures/class/simple.mmd",
    ] {
        let input = std::fs::read_to_string(path).unwrap();

        // Text output
        let text =
            mmdflux::render_diagram(&input, OutputFormat::Text, &Default::default()).unwrap();
        assert!(!text.is_empty(), "{path}: text output should not be empty");

        // SVG output
        let svg = mmdflux::render_diagram(&input, OutputFormat::Svg, &Default::default()).unwrap();
        assert!(
            svg.contains("<svg"),
            "{path}: SVG output should contain <svg"
        );

        // MMDS output
        let mmds =
            mmdflux::render_diagram(&input, OutputFormat::Mmds, &Default::default()).unwrap();
        assert!(
            mmds.contains("\"nodes\""),
            "{path}: MMDS output should contain nodes"
        );
    }
}

#[test]
fn architecture_docs_no_longer_reference_graph_family_pipeline() {
    let docs = repo_file("docs/architecture/dependency-rules.md");
    assert!(!docs.contains("graph_family_pipeline"));
}

#[test]
fn crate_root_no_longer_declares_graph_family_pipeline_shim() {
    let lib_rs = repo_file("src/lib.rs");
    assert!(!lib_rs.contains("mod graph_family_pipeline;"));
}
