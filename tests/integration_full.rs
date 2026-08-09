//! Full integration tests for the multi-format architecture.
//!
//! These tests validate that registry detection, parsing, and rendering work
//! together across diagram types and output formats.

mod common;

use std::fs;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
};
use mmdflux::mmds::generate_mermaid_from_str;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn rust_log_requests_mmdflux_trace() -> bool {
    std::env::var("RUST_LOG").is_ok_and(|value| {
        value.split(',').any(|directive| {
            let directive = directive.trim();
            directive == "mmdflux=trace"
                || directive == "mmdflux::runtime=trace"
                || directive == "mmdflux::runtime=debug"
        })
    })
}

fn assert_render_trace_enabled_when_requested() {
    if rust_log_requests_mmdflux_trace() {
        assert!(
            tracing::enabled!(target: "mmdflux::runtime", tracing::Level::DEBUG),
            "RUST_LOG requests mmdflux render tracing, but no test subscriber enabled it"
        );
    }
}

fn render_with_registry(input: &str, format: OutputFormat) -> String {
    render_diagram(input, format, &RenderConfig::default()).expect("should render")
}

#[test]
fn font_metrics_explicit_compatibility_profile_matches_default_text_output() {
    let input = "graph TD\nA[Collect metrics] --> B[Render output]";
    let default = render_diagram(input, OutputFormat::Text, &RenderConfig::default())
        .expect("default text render should succeed");
    let explicit = render_diagram(
        input,
        OutputFormat::Text,
        &RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    )
    .expect("explicit compatibility profile text render should succeed");

    assert_eq!(explicit, default);
}

#[test]
fn font_metrics_profiles_do_not_change_terminal_output() {
    let input = "graph TD\nA[Collect metrics] -->|edge label| B[Render output]";
    let default_text = render_diagram(input, OutputFormat::Text, &RenderConfig::default())
        .expect("default text render should succeed");
    let default_ascii = render_diagram(input, OutputFormat::Ascii, &RenderConfig::default())
        .expect("default ascii render should succeed");

    for profile in [
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
    ] {
        let config = RenderConfig {
            font_metrics_profile: Some(profile.to_string()),
            ..RenderConfig::default()
        };
        let text = render_diagram(input, OutputFormat::Text, &config)
            .expect("profile-selected text render should succeed");
        let ascii = render_diagram(input, OutputFormat::Ascii, &config)
            .expect("profile-selected ascii render should succeed");

        assert_eq!(text, default_text, "Text output changed for {profile}");
        assert_eq!(ascii, default_ascii, "ASCII output changed for {profile}");
    }
}

/// A `<br/>` inside a diamond/hexagon label used to underflow the text
/// renderer's centering arithmetic (panic in debug, dropped label in release).
#[test]
fn multi_line_labels_render_inside_bracketed_shapes() {
    let cases = [
        (
            "{Is the cache warm?<br/>Check the index}",
            ["< Is the cache warm? >", "<  Check the index   >"],
        ),
        ("{{Retry?<br/>Give up}}", ["< Retry?  >", "< Give up >"]),
    ];

    for (shape, expected_rows) in cases {
        let input = format!("flowchart TD\n    A[Start] --> B{shape}\n");
        for format in [OutputFormat::Text, OutputFormat::Ascii] {
            let output = render_diagram(&input, format, &RenderConfig::default())
                .unwrap_or_else(|e| panic!("{shape} should render as {format:?}: {e}"));
            for row in expected_rows {
                assert!(
                    output.contains(row),
                    "{format:?} output for {shape} is missing {row:?}:\n{output}"
                );
            }
        }
    }
}

/// The borderless (`shape: text`) node shares the diamond's centering path and
/// underflowed the same way on a multi-line label.
#[test]
fn multi_line_label_renders_inside_borderless_node() {
    let input =
        "flowchart TD\n    A@{ shape: text, label: \"Hello there<br/>World again\" } --> B[x]\n";

    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let output = render_diagram(input, format, &RenderConfig::default())
            .unwrap_or_else(|e| panic!("borderless node should render as {format:?}: {e}"));
        assert!(output.contains("Hello there"), "{format:?}:\n{output}");
        assert!(output.contains("World again"), "{format:?}:\n{output}");
    }
}

fn render_flowchart_svg(input: &str) -> String {
    render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).expect("should render svg")
}

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

fn render_flowchart_text_fixture(name: &str) -> String {
    let input = load_flowchart_fixture(name);
    render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("flowchart fixture should render through supported text path")
}

fn render_flowchart_text_fixture_as_td(name: &str) -> String {
    let input = load_flowchart_fixture(name)
        .replacen("graph LR", "graph TD", 1)
        .replacen("flowchart LR", "flowchart TD", 1);
    render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("TD-converted flowchart fixture should render through supported text path")
}

fn render_flowchart_svg_fixture(name: &str) -> String {
    let input = load_flowchart_fixture(name);
    render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("flowchart fixture should render through supported SVG path")
}

fn render_mmds_svg_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    let payload = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read MMDS fixture {}: {e}", path.display()));
    render_diagram(&payload, OutputFormat::Svg, &RenderConfig::default())
        .expect("MMDS fixture should render through supported SVG path")
}

fn assert_direct_and_mmds_svg_smoke(case: &str) {
    let (flowchart_fixture, mmds_fixture) = match case {
        "subgraph_as_node_edge" => (
            "subgraph_as_node_edge.mmd",
            "subgraph-endpoint-intent-present.json",
        ),
        "subgraph_to_subgraph_edge" => (
            "subgraph_to_subgraph_edge.mmd",
            "subgraph-endpoint-subgraph-to-subgraph-present.json",
        ),
        _ => panic!("unknown parity case: {case}"),
    };

    let direct_svg = render_flowchart_svg_fixture(flowchart_fixture);
    let replay_svg = render_mmds_svg_fixture(mmds_fixture);

    assert!(
        direct_svg.starts_with("<svg") && direct_svg.contains("</svg>"),
        "direct SVG render should succeed for case {case}"
    );
    assert!(
        replay_svg.starts_with("<svg") && replay_svg.contains("</svg>"),
        "MMDS replay SVG render should succeed for case {case}"
    );
}

#[test]
fn registry_detects_all_diagram_types() {
    let registry = default_registry();

    assert_eq!(registry.detect("graph TD\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("flowchart LR\nA-->B"), Some("flowchart"));
    assert_eq!(registry.detect("classDiagram\nclass User"), Some("class"));
    assert_eq!(
        registry.detect("sequenceDiagram\nA->>B: hi"),
        Some("sequence")
    );
}

#[test]
fn simple_render_trace_is_enabled_when_rust_log_requests_mmdflux_trace() {
    assert_render_trace_enabled_when_requested();

    let output = render_with_registry("graph TD\nA-->B", OutputFormat::Text);
    assert!(output.contains("A"));
}

#[test]
fn all_diagram_types_render_text() {
    let cases = [
        ("graph TD\nA-->B", "A"),
        ("classDiagram\nclass User", "User"),
        ("sequenceDiagram\nAlice->>Bob: hi", "Alice"),
    ];

    for (input, expected) in cases {
        let output = render_with_registry(input, OutputFormat::Text);
        assert!(
            output.contains(expected),
            "output missing expected content for {}",
            input
        );
    }
}

#[test]
fn flowchart_renders_all_formats() {
    let input = "graph TD\nA[Start]-->B[End]";
    let text = render_with_registry(input, OutputFormat::Text);
    assert!(text.contains("Start"));
    assert!(text.contains("End"));
    assert!(text.contains('│'));

    let ascii = render_with_registry(input, OutputFormat::Ascii);
    assert!(ascii.contains("Start"));
    assert!(!ascii.contains('│'));

    let svg = render_with_registry(input, OutputFormat::Svg);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("</svg>"));
}

#[test]
fn svg_shapes_render_expected_elements() {
    let input = r#"graph TD
    A[Rectangle]-->B(Rounded)
    B-->C{Diamond}
    C-->D((Circle))"#;
    let svg = render_flowchart_svg(input);

    assert!(svg.contains("<rect"));
    assert!(svg.contains("rx="));
    assert!(svg.contains("ry="));
    assert!(svg.contains("<polygon"));
    assert!(svg.contains("<ellipse"));
}

#[test]
fn svg_edges_and_subgraphs_render() {
    let input = r#"graph TD
    subgraph sg[Group]
        A-->A
    end
    B-.->C"#;
    let svg = render_flowchart_svg(input);

    assert!(svg.contains("class=\"subgraph\""));
    assert!(svg.contains("Group"));
    assert!(svg.matches("<path").count() >= 2);
    assert!(svg.contains("stroke-dasharray"));
}

#[test]
fn registry_render_smoke() {
    let text = render_with_registry("graph TD\nA-->B", OutputFormat::Text);
    assert!(text.contains("A"));

    let svg = render_with_registry("graph TD\nA-->B", OutputFormat::Svg);
    assert!(svg.starts_with("<svg"));
}

#[test]
fn generated_mermaid_from_mmds_renders_through_registry() {
    let payload = include_str!("fixtures/mmds/generation/basic-flow.json");
    let mermaid = generate_mermaid_from_str(payload).expect("should generate Mermaid");
    let rendered = render_with_registry(&mermaid, OutputFormat::Text);
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("End"));
}

#[test]
fn subgraph_endpoint_fixture_set_renders_through_direct_and_mmds_paths() {
    for case in ["subgraph_as_node_edge", "subgraph_to_subgraph_edge"] {
        assert_direct_and_mmds_svg_smoke(case);
    }
}

#[test]
fn issue_58_lr_fixtures_preserve_node_borders() {
    let cases = [
        ("git_workflow.mmd", vec!["┐┐", "┌└"]),
        ("backward_loop_lr.mmd", vec!["└└"]),
        (
            "architecture_graph_lr_terminal_contracts.mmd",
            vec!["┐┐", "└──────────┘│", "└─────────┘│"],
        ),
    ];

    for (fixture, forbidden) in cases {
        let rendered = render_flowchart_text_fixture(fixture);
        for pattern in forbidden {
            assert!(
                !rendered.contains(pattern),
                "fixture {fixture} should not contain border-collision pattern {pattern:?}\n{rendered}"
            );
        }
    }
}

#[test]
fn issue_58_td_dense_fixture_preserves_node_borders() {
    let rendered =
        render_flowchart_text_fixture_as_td("architecture_graph_lr_terminal_contracts.mmd");

    for pattern in ["┐┐", "│ registry │┐", "└──────────┘│", "└─────────┘│"]
    {
        assert!(
            !rendered.contains(pattern),
            "TD variant should not contain border-collision pattern {pattern:?}\n{rendered}"
        );
    }
}

#[test]
fn subgraph_direction_mixed_cross_boundary_edge_stays_off_borders() {
    let rendered = render_flowchart_text_fixture("subgraph_direction_mixed.mmd");

    for pattern in ["┌─ Bottom to Top ─┤", "│      │ C │◄─────┤"] {
        assert!(
            !rendered.contains(pattern),
            "subgraph_direction_mixed should not ride a subgraph border with pattern {pattern:?}\n{rendered}"
        );
    }

    for pattern in ["│  │ A │─►│ B │──┐│", "│      │ C │◄─────┼┘"]
    {
        assert!(
            rendered.contains(pattern),
            "subgraph_direction_mixed should keep the exterior continuation visible with pattern {pattern:?}\n{rendered}"
        );
    }
}
