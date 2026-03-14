//! Flowchart compliance tests focused on the supported public adapter workflow.
//!
//! Detailed flowchart parser invariants live owner-local in `src/mermaid/flowchart.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::builtins::default_registry;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn flowchart_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
}

fn list_flowchart_fixtures() -> Vec<String> {
    let dir = flowchart_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read flowchart fixtures dir: {e}"))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|e| e == "mmd") {
                Some(path.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn load_fixture(name: &str) -> String {
    let path = flowchart_fixture_dir().join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read flowchart fixture {name}: {e}"))
}

fn render_flowchart(input: &str, format: OutputFormat) -> String {
    render_diagram(input, format, &RenderConfig::default())
        .unwrap_or_else(|e| panic!("Failed to render flowchart as {format}: {e}"))
}

#[test]
fn flowchart_fixtures_detect_via_builtin_registry() {
    let registry = default_registry();

    for fixture in list_flowchart_fixtures() {
        let input = load_fixture(&fixture);
        assert_eq!(
            registry.detect(&input),
            Some("flowchart"),
            "fixture should detect as flowchart: {fixture}"
        );
    }
}

#[test]
fn flowchart_fixtures_parse_prepare_via_registry() {
    let registry = default_registry();

    for fixture in list_flowchart_fixtures() {
        let input = load_fixture(&fixture);
        let instance = registry
            .create("flowchart")
            .expect("flowchart should be registered");
        let payload = instance
            .parse(&input)
            .unwrap_or_else(|e| panic!("Failed to parse flowchart fixture {fixture}: {e}"))
            .into_payload(&RenderConfig::default())
            .unwrap_or_else(|e| panic!("Failed to build flowchart payload for {fixture}: {e}"));
        assert!(
            matches!(payload, mmdflux::payload::Diagram::Flowchart(_)),
            "flowchart fixtures should build flowchart payloads: {fixture}"
        );
    }
}

#[test]
fn flowchart_fixtures_render_text() {
    for fixture in list_flowchart_fixtures() {
        let output = render_flowchart(&load_fixture(&fixture), OutputFormat::Text);
        assert!(
            !output.is_empty(),
            "flowchart text output should be non-empty for {fixture}"
        );
    }
}

#[test]
fn flowchart_fixtures_render_svg() {
    for fixture in list_flowchart_fixtures() {
        let output = render_flowchart(&load_fixture(&fixture), OutputFormat::Svg);
        assert!(
            output.starts_with("<svg"),
            "flowchart svg output should start with <svg for {fixture}"
        );
    }
}

#[test]
fn flowchart_selected_fixtures_render_mmds() {
    for fixture in [
        "simple.mmd",
        "labeled_edges.mmd",
        "nested_subgraph.mmd",
        "git_workflow.mmd",
    ] {
        let output = render_flowchart(&load_fixture(fixture), OutputFormat::Mmds);
        assert!(
            output.contains("\"diagram_type\": \"flowchart\""),
            "flowchart MMDS output should include diagram_type metadata for {fixture}"
        );
    }
}

#[test]
fn flowchart_ascii_output_avoids_unicode_box_drawing() {
    let output = render_flowchart("graph TD\nA[Start] --> B[End]\n", OutputFormat::Ascii);
    assert!(!output.contains('│'));
    assert!(!output.contains('─'));
}

#[test]
fn flowchart_comment_and_subgraph_inputs_render_through_public_api() {
    let input = "%% comment\ngraph TD\nsubgraph Group\nA --> B\nend\n";
    let output = render_flowchart(input, OutputFormat::Text);

    assert!(output.contains("Group"));
    assert!(output.contains("A"));
    assert!(output.contains("B"));
}

#[test]
fn flowchart_directional_and_labeled_edges_render_through_public_api() {
    let output = render_flowchart("graph LR\nA -->|yes| B\nA -->|no| C\n", OutputFormat::Text);

    assert!(output.contains("A"));
    assert!(output.contains("yes"));
    assert!(output.contains("no"));
}
