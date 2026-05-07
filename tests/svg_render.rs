mod common;

use std::fs;
use std::path::Path;

use mmdflux::format::{CornerStyle, Curve, RoutingStyle};
use mmdflux::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
};
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    EngineAlgorithmId, OutputFormat, RenderConfig, SvgThemeConfig, SvgThemeMode, render_diagram,
};

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read flowchart fixture {}: {e}", path.display()))
}

fn load_class_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("class")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read class fixture {}: {e}", path.display()))
}

fn load_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read MMDS fixture {}: {e}", path.display()))
}

fn load_sequence_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sequence")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read sequence fixture {}: {e}", path.display()))
}

fn load_state_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("state")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read state fixture {}: {e}", path.display()))
}

fn render_svg(input: &str, config: &RenderConfig) -> String {
    render_diagram(input, OutputFormat::Svg, config).expect("SVG render should succeed")
}

#[test]
fn font_metrics_explicit_compatibility_profile_matches_default_svg() {
    let input = load_flowchart_fixture("labeled_edges.mmd");
    let default_svg = render_svg(&input, &RenderConfig::default());
    let explicit_svg = render_svg(
        &input,
        &RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    );

    assert_eq!(explicit_svg, default_svg);
}

#[test]
fn font_metrics_unsupported_profile_fails_svg_before_output() {
    let err = render_diagram(
        "graph TD\nA-->B",
        OutputFormat::Svg,
        &RenderConfig {
            font_metrics_profile: Some("mermaid-sans-v1".to_string()),
            ..RenderConfig::default()
        },
    )
    .expect_err("unsupported profile should fail before SVG output");

    assert!(
        err.message
            .contains("unsupported text metrics profile 'mermaid-sans-v1'"),
        "{err}"
    );
}

#[test]
fn mmdflux_sans_svg_uses_recorded_profile_for_rendered_label_backgrounds() {
    let input = "graph TD\nA -->|mmmm| B";
    let recorded = render_svg(
        input,
        &RenderConfig {
            font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    );

    assert!(
        recorded.contains("width=\"61.31\" height=\"28.00\" fill=\"white\" />"),
        "{recorded}"
    );
    assert!(
        !recorded.contains("width=\"59.97\" height=\"28.00\" fill=\"white\" />"),
        "{recorded}"
    );
}

#[test]
fn text_output_ignores_recorded_font_profile_for_now() {
    let input = "graph TD\nA[mmmm] --> B[iiii]";
    let default_text = render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap();
    let recorded_text = render_diagram(
        input,
        OutputFormat::Text,
        &RenderConfig {
            font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    )
    .unwrap();

    assert_eq!(recorded_text, default_text);
}

#[test]
fn basic_flowchart_svg_has_root_text_and_arrow_marker() {
    let input = "graph TD\nA[Start] --> B[End]\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("End"));
    assert!(svg.contains("marker-end="));
    assert!(svg.contains("<path d=\""));
}

#[test]
fn simple_arrow_flowchart_only_emits_arrowhead_def() {
    let svg = render_svg("graph TD\nA-->B\n", &RenderConfig::default());

    assert!(svg.contains("id=\"arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"crosshead\""), "{svg}");
    assert!(!svg.contains("id=\"circlehead\""), "{svg}");
    assert!(!svg.contains("id=\"diamondhead\""), "{svg}");
    assert!(!svg.contains("id=\"open-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"open-diamondhead\""), "{svg}");
}

#[test]
fn mixed_arrow_flowchart_only_emits_referenced_marker_defs_once() {
    let input = load_flowchart_fixture("cross_circle_arrows.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert_eq!(svg.matches("id=\"crosshead\"").count(), 1, "{svg}");
    assert_eq!(svg.matches("id=\"circlehead\"").count(), 1, "{svg}");
    assert!(!svg.contains("id=\"arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"diamondhead\""), "{svg}");
    assert!(!svg.contains("id=\"open-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"open-diamondhead\""), "{svg}");
}

#[test]
fn graph_circle_markers_use_default_canvas_fill() {
    let input = load_flowchart_fixture("cross_circle_arrows.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("id=\"circlehead\""), "{svg}");
    assert!(
        svg.contains(
            "<circle cx=\"6\" cy=\"6\" r=\"5\" stroke=\"#333\" stroke-width=\"1\" fill=\"white\""
        ),
        "{svg}"
    );
}

#[test]
fn themed_graph_circle_markers_use_theme_background_fill() {
    let input = load_class_fixture("lollipop_interfaces.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("id=\"circlehead\""), "{svg}");
    assert!(
        svg.contains(
            "<circle cx=\"6\" cy=\"6\" r=\"5\" stroke=\"#d3d3d3\" stroke-width=\"1\" fill=\"#333333\""
        ),
        "{svg}"
    );
}

#[test]
fn graph_circle_marker_paths_stop_at_circle_border() {
    let input = load_class_fixture("lollipop_interfaces.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("d=\"M64.82,166.00 L64.82,206.00\""), "{svg}");
    assert!(svg.contains("d=\"M210.80,62.00 L210.80,102.00\""), "{svg}");
    assert!(!svg.contains("d=\"M64.82,166.00 L64.82,216.00\""), "{svg}");
}

#[test]
fn class_open_arrow_markers_are_unfilled() {
    let input = load_class_fixture("interface_realization.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("id=\"open-arrowhead\""), "{svg}");
    assert!(
        svg.contains("<polygon points=\"0,0 10.00,5.00 0,10.00\" fill=\"none\" stroke=\"#333\""),
        "{svg}"
    );
}

#[test]
fn class_open_arrow_paths_stop_at_triangle_border() {
    let input = load_class_fixture("simple.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("d=\"M62.13,67.00 L62.13,112.00\""), "{svg}");
    assert!(!svg.contains("d=\"M62.13,66.00 L62.13,112.00\""), "{svg}");
}

#[test]
fn class_open_diamond_markers_are_unfilled() {
    let input = load_class_fixture("two_way_relations.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("id=\"open-diamondhead\""), "{svg}");
    assert!(
        svg.contains(
            "<polygon points=\"0,6.00 6.00,0 12.00,6.00 6.00,12.00\" fill=\"none\" stroke=\"#333\""
        ),
        "{svg}"
    );
}

#[test]
fn class_open_diamond_paths_stop_at_diamond_border() {
    let input = load_class_fixture("two_way_relations.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("d=\"M42.45,172.00 L42.45,210.00\""), "{svg}");
    assert!(!svg.contains("d=\"M42.45,166.00 L42.45,216.00\""), "{svg}");
}

#[test]
fn svg_theme_changes_graph_root_and_node_colors() {
    let svg = render_svg(
        "graph TD\nA-->B\n",
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("background-color: #333333;"), "{svg}");
    assert!(svg.contains("fill=\"#1f2020\""), "{svg}");
    assert!(svg.contains("stroke=\"#d3d3d3\""), "{svg}");
    assert!(svg.contains("fill=\"#cccccc\">A</text>"), "{svg}");
}

#[test]
fn svg_theme_dynamic_mode_emits_root_variables_and_hex_fallbacks_for_graphs() {
    let svg = render_svg(
        "graph TD\nA-->B\n",
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                mode: SvgThemeMode::Dynamic,
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("--bg:#333333"), "{svg}");
    assert!(svg.contains("--fg:#cccccc"), "{svg}");
    assert!(svg.contains("<style>"), "{svg}");
    assert!(svg.contains("--_node-fill: var(--surface);"), "{svg}");
    assert!(svg.contains("fill=\"#1f2020\""), "{svg}");
    assert!(svg.contains("stroke=\"#d3d3d3\""), "{svg}");
}

#[test]
fn svg_theme_applies_to_state_diagrams_via_graph_family_runtime() {
    let input = load_state_fixture("simple.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("background-color: #333333;"), "{svg}");
    assert!(svg.contains("fill=\"#1f2020\""), "{svg}");
    assert!(svg.contains("fill=\"#cccccc\">Idle</text>"), "{svg}");
}

#[test]
fn svg_theme_preserves_node_style_precedence_through_runtime() {
    let input = load_flowchart_fixture("style-basic.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("background-color: #333333;"), "{svg}");
    assert!(svg.contains("fill=\"#ffeeaa\""), "{svg}");
    assert!(svg.contains("stroke=\"#333\""), "{svg}");
    assert!(svg.contains("fill=\"#111\">Alpha</text>"), "{svg}");
}

#[test]
fn mermaid_theme_hints_render_themed_svg_for_compatibility_fixtures() {
    for fixture in ["compat_frontmatter.mmd", "compat_directive.mmd"] {
        let input = load_flowchart_fixture(fixture);
        let svg = render_svg(&input, &RenderConfig::default());

        assert!(
            svg.contains("background-color: #333333;"),
            "{fixture}\n{svg}"
        );
        assert!(svg.contains("fill=\"#1f2020\""), "{fixture}\n{svg}");
        assert!(svg.contains("fill=\"#cccccc\""), "{fixture}\n{svg}");
    }
}

#[test]
fn svg_runtime_honors_supported_style_options() {
    let input = load_flowchart_fixture("complex.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            layout_engine: Some(
                EngineAlgorithmId::parse("flux-layered")
                    .expect("flux-layered engine id should parse"),
            ),
            routing_style: Some(RoutingStyle::Orthogonal),
            curve: Some(Curve::Linear(CornerStyle::Rounded)),
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<text"));
    assert!(svg.contains("<path d=\""));
}

#[test]
fn basic_sequence_svg_has_participants_and_arrows() {
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Bob-->>Alice: Hi\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Alice"));
    assert!(svg.contains("Bob"));
    assert!(svg.contains("Hello"));
    assert!(svg.contains("Hi"));
    assert!(svg.contains("marker-end="));
    assert!(svg.contains("stroke-dasharray=\"5,5\"")); // lifelines
    assert!(svg.contains("stroke-dasharray=\"6,4\"")); // dashed message
}

#[test]
fn simple_sequence_only_emits_filled_arrowhead_def() {
    let svg = render_svg(
        "sequenceDiagram\n    Alice->>Bob: Hello\n",
        &RenderConfig::default(),
    );

    assert!(svg.contains("id=\"seq-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-open-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-crosshead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-async-arrowhead\""), "{svg}");
}

#[test]
fn plain_sequence_messages_emit_no_marker_defs() {
    let svg = render_svg(
        "sequenceDiagram\n    Alice->Bob: Hello\n    Bob-->Alice: World\n",
        &RenderConfig::default(),
    );

    assert!(!svg.contains("id=\"seq-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-open-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-crosshead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-async-arrowhead\""), "{svg}");
    assert!(!svg.contains("marker-end="), "{svg}");
}

#[test]
fn mixed_sequence_only_emits_referenced_marker_defs_once() {
    let input = load_sequence_fixture("all_arrows.mmd")
        .replace("    A-xB: Solid cross\n", "")
        .replace("    A--xB: Dashed cross\n", "");
    let svg = render_svg(&input, &RenderConfig::default());

    assert_eq!(svg.matches("id=\"seq-arrowhead\"").count(), 1, "{svg}");
    assert_eq!(
        svg.matches("id=\"seq-async-arrowhead\"").count(),
        1,
        "{svg}"
    );
    assert!(!svg.contains("id=\"seq-open-arrowhead\""), "{svg}");
    assert!(!svg.contains("id=\"seq-crosshead\""), "{svg}");
}

#[test]
fn sequence_async_arrow_markers_are_unfilled() {
    let input = load_sequence_fixture("async_arrow.mmd");
    let svg = render_svg(&input, &RenderConfig::default());

    assert!(svg.contains("id=\"seq-async-arrowhead\""), "{svg}");
    assert!(
        svg.contains(
            "<marker id=\"seq-async-arrowhead\" viewBox=\"0 0 10 10\" refX=\"0\" refY=\"5\""
        ),
        "{svg}"
    );
    assert!(
        svg.contains("<path d=\"M 0 0 L 10 5 L 0 10\" fill=\"none\" stroke=\"#333\""),
        "{svg}"
    );
}

#[test]
fn sequence_async_paths_stop_at_marker_back_edge() {
    let input = load_sequence_fixture("all_arrows.mmd");
    let svg = render_svg(
        &input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(
        svg.contains("<line x1=\"30.45\" y1=\"374.00\" x2=\"170.45\" y2=\"374.00\""),
        "{svg}"
    );
    assert!(
        svg.contains("<line x1=\"30.45\" y1=\"424.00\" x2=\"170.45\" y2=\"424.00\""),
        "{svg}"
    );
}

#[test]
fn sequence_svg_self_message_renders_path() {
    let input = "sequenceDiagram\n    Alice->>Alice: Think\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<path d=\"M"));
    assert!(svg.contains("Think"));
}

#[test]
fn sequence_svg_note_renders_note_box() {
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Note right of Bob: Important\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Important"));
    assert!(svg.contains("#ffffcc")); // note fill color
}

#[test]
fn sequence_svg_activation_renders_rect() {
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    activate Bob\n    Bob-->>Alice: Hi\n    deactivate Bob\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("activations")); // group class
    assert!(svg.contains("#ddd")); // activation fill
}

#[test]
fn sequence_svg_theme_changes_note_and_activation_colors() {
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Note right of Bob: Important\n    activate Bob\n    Bob-->>Alice: Hi\n    deactivate Bob\n";
    let svg = render_svg(
        input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("background-color: #333333;"), "{svg}");
    assert!(svg.contains("fill=\"#1f2020\""), "{svg}");
    assert!(svg.contains("fill=\"#424242\""), "{svg}");
    assert!(svg.contains("fill=\"#454545\""), "{svg}");
    assert!(!svg.contains("#ffffcc"), "{svg}");
    assert!(!svg.contains("#ddd"), "{svg}");
}

#[test]
fn positioned_mmds_payload_renders_svg_through_runtime() {
    let payload = load_mmds_fixture("positioned/routed-fan-in-ports.json");
    let svg = render_svg(
        &payload,
        &RenderConfig {
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("marker-end="));
    assert!(svg.contains("<path d=\""));
}

#[test]
fn positioned_mmds_payload_honors_explicit_svg_theme() {
    let payload = load_mmds_fixture("positioned/routed-fan-in-ports.json");
    let svg = render_svg(
        &payload,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                ..Default::default()
            }),
            path_simplification: PathSimplification::None,
            ..RenderConfig::default()
        },
    );

    assert!(svg.contains("background-color: #333333;"), "{svg}");
    assert!(svg.contains("fill=\"#1f2020\""), "{svg}");
    assert!(svg.contains("stroke=\"#cccccc\""), "{svg}");
}

// --- classDef / class / ::: styling ---

#[test]
fn classdef_annotation_svg_has_fill_colors() {
    let input = include_str!("fixtures/flowchart/compat_class_annotation.mmd");
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("fill=\"#ff0\""),
        "expected highlight fill: {svg}"
    );
    assert!(
        svg.contains("fill=\"#0f0\""),
        "expected success fill: {svg}"
    );
    assert!(svg.contains("fill=\"#f00\""), "expected error fill: {svg}");
}

#[test]
fn classdef_class_stmt_svg_has_colors() {
    let input = include_str!("fixtures/flowchart/classdef_class_stmt.mmd");
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("fill=\"#f00\""),
        "expected fill from class statement: {svg}"
    );
}

#[test]
fn classdef_precedence_style_wins() {
    let input = include_str!("fixtures/flowchart/classdef_precedence.mmd");
    let svg = render_svg(input, &RenderConfig::default());
    // B should have fill="#0f0" (style overrides classDef)
    assert!(
        svg.contains("fill=\"#0f0\""),
        "style should override classDef: {svg}"
    );
    // A should still have classDef fill
    assert!(
        svg.contains("fill=\"#ddd\""),
        "classDef fill should apply to A: {svg}"
    );
}

#[test]
fn state_classdef_basic_svg_has_colors() {
    let input = include_str!("fixtures/state/classdef_basic.mmd");
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("fill=\"#bfb\""),
        "expected active fill in SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"#fbb\""),
        "expected error fill in SVG: {svg}"
    );
}

#[test]
fn state_classdef_composite_svg_has_colors() {
    let input = include_str!("fixtures/state/classdef_composite.mmd");
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("fill=\"#ff0\""),
        "expected highlight fill in SVG: {svg}"
    );
}

// --- Extended CSS properties ---

#[test]
fn svg_node_with_font_weight() {
    let input = "graph TD\n  classDef bold font-weight:bold\n  A:::bold\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("font-weight=\"bold\""),
        "expected font-weight in SVG: {svg}"
    );
}

#[test]
fn svg_node_with_stroke_width() {
    let input = "graph TD\n  classDef thick stroke-width:3px\n  A:::thick\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("stroke-width=\"3px\""),
        "expected stroke-width in SVG: {svg}"
    );
}

#[test]
fn svg_node_with_rx() {
    let input = "graph TD\n  classDef rounded rx:10\n  A[Box]:::rounded\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(svg.contains("rx=\"10\""), "expected rx in SVG: {svg}");
}

#[test]
fn svg_node_with_stroke_dasharray() {
    let input = "graph TD\n  classDef dashed stroke-dasharray:5,3\n  A:::dashed\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("stroke-dasharray=\"5,3\""),
        "expected stroke-dasharray in SVG: {svg}"
    );
}

#[test]
fn svg_node_with_font_style() {
    let input = "graph TD\n  classDef italic font-style:italic\n  A:::italic\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("font-style=\"italic\""),
        "expected font-style in SVG: {svg}"
    );
}
