mod common;

use std::fs;
use std::path::Path;

use mmdflux::format::{CornerStyle, Curve, RoutingStyle};
use mmdflux::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, DEFAULT_GRAPH_FONT_FAMILY,
    DEFAULT_PROPORTIONAL_FONT_SIZE, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
};
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    EngineAlgorithmId, GraphTextStyleConfig, OutputFormat, RenderConfig, SvgThemeConfig,
    SvgThemeMode, render_diagram,
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

fn load_dynamic_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("dynamic")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read dynamic flowchart fixture {}: {e}",
            path.display()
        )
    })
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
fn font_metrics_explicit_recorded_profile_matches_default_svg() {
    let input = load_flowchart_fixture("labeled_edges.mmd");
    let default_svg = render_svg(&input, &RenderConfig::default());
    let explicit_svg = render_svg(
        &input,
        &RenderConfig {
            font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    );

    assert_eq!(explicit_svg, default_svg);
}

#[test]
fn font_metrics_explicit_compatibility_profile_differs_from_default_svg() {
    let input = load_flowchart_fixture("labeled_edges.mmd");
    let default_svg = render_svg(&input, &RenderConfig::default());
    let compatibility_svg = render_svg(
        &input,
        &RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    );

    assert_ne!(compatibility_svg, default_svg);
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
fn provider_free_svg_rejects_custom_graph_font_style() {
    let config = RenderConfig {
        graph_text_style: Some(GraphTextStyleConfig::new(
            "Inter",
            DEFAULT_PROPORTIONAL_FONT_SIZE,
        )),
        ..RenderConfig::default()
    };

    let err = render_diagram("graph TD\nA-->B", OutputFormat::Svg, &config)
        .expect_err("custom provider-free graph font style should fail");

    assert!(err.message.contains("fontFamily"), "{err}");
    assert!(err.message.contains("dynamic text metrics"), "{err}");
    assert!(err.message.contains(DEFAULT_GRAPH_FONT_FAMILY), "{err}");
}

#[test]
fn provider_free_svg_rejects_mermaid_graph_font_styles() {
    let input = load_dynamic_flowchart_fixture("multi_font_styles.mmd");

    let err = render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect_err("Mermaid font styles require dynamic text metrics");

    assert!(err.message.contains("fontFamily"), "{err}");
    assert!(err.message.contains("dynamic text metrics"), "{err}");
}

#[test]
fn provider_free_svg_accepts_style_matching_static_profile_descriptor() {
    let config = RenderConfig {
        graph_text_style: Some(GraphTextStyleConfig::new(
            DEFAULT_GRAPH_FONT_FAMILY,
            DEFAULT_PROPORTIONAL_FONT_SIZE,
        )),
        ..RenderConfig::default()
    };

    let output = render_diagram("graph TD\nA-->B", OutputFormat::Svg, &config).unwrap();

    assert!(output.contains("<svg"));
}

#[test]
fn provider_free_svg_accepts_equivalent_static_profile_font_stack_spelling_byte_stable() {
    let input = "graph TD\nA[mmmm]-->B[iiii]";
    let default_output = render_diagram(input, OutputFormat::Svg, &RenderConfig::default())
        .expect("default SVG render");
    let config = RenderConfig {
        graph_text_style: Some(GraphTextStyleConfig::new(
            "Trebuchet MS, Verdana, Arial, sans-serif",
            DEFAULT_PROPORTIONAL_FONT_SIZE,
        )),
        ..RenderConfig::default()
    };

    let styled_output =
        render_diagram(input, OutputFormat::Svg, &config).expect("descriptor-matching style");

    assert_eq!(styled_output, default_output);
}

#[test]
fn provider_free_text_and_ascii_reject_custom_graph_font_style() {
    let config = RenderConfig {
        graph_text_style: Some(GraphTextStyleConfig::new(
            "Inter",
            DEFAULT_PROPORTIONAL_FONT_SIZE,
        )),
        ..RenderConfig::default()
    };

    for format in [OutputFormat::Text, OutputFormat::Ascii] {
        let err = render_diagram("graph TD\nA-->B", format, &config)
            .expect_err("terminal output should reject graph font style");
        assert!(err.message.contains("font style"), "{err}");
        assert!(err.message.contains("not supported"), "{err}");
    }
}

#[test]
fn default_svg_rendering_remains_byte_stable_after_graph_font_config_contract() {
    let input = include_str!("fixtures/flowchart/labeled_edges.mmd");
    let output = render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).unwrap();

    assert_eq!(
        output,
        include_str!("svg-snapshots/flowchart/labeled_edges.svg")
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
fn text_output_forces_compatibility_profile_for_wrap_metrics() {
    let input = include_str!("fixtures/flowchart/flowchart_code_flow.mmd");
    let default_text = render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap();
    let compatibility_text = render_diagram(
        input,
        OutputFormat::Text,
        &RenderConfig {
            font_metrics_profile: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    )
    .unwrap();
    let recorded_text = render_diagram(
        input,
        OutputFormat::Text,
        &RenderConfig {
            font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
            ..RenderConfig::default()
        },
    )
    .unwrap();

    assert_eq!(default_text, compatibility_text);
    assert_eq!(recorded_text, compatibility_text);
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
fn svg_subgraph_class_styles_container_rect() {
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef blue fill:#e1f5fe,stroke:#01579b,stroke-width:2px\nclass A blue\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.contains(r#"class="subgraph""#), "{svg}");
    assert!(svg.contains(r##"fill="#e1f5fe""##), "{svg}");
    assert!(svg.contains(r##"stroke="#01579b""##), "{svg}");
    assert!(svg.contains(r#"stroke-width="2px""#), "{svg}");
}

#[test]
fn unstyled_subgraph_container_rect_stays_byte_stable() {
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(
        svg.contains(
            r##"<rect class="subgraph" x="43.00" y="12.00" width="107.80" height="100.00" fill="none" stroke="#888" stroke-width="1.00" />"##
        ),
        "{svg}"
    );
}

#[test]
fn themed_subgraph_default_fill_follows_mermaid_cluster_bkg_not_node_fill() {
    // Mermaid `dark` theme: clusterBkg = #302F3D, surface (node_fill) = #1f2020.
    // The subgraph rect must consume cluster_bkg, not node_fill.
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\n";
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

    let subgraph_lines: Vec<&str> = svg
        .lines()
        .filter(|line| line.contains(r#"class="subgraph""#))
        .collect();
    assert!(!subgraph_lines.is_empty(), "{svg}");
    for line in &subgraph_lines {
        assert!(
            line.contains(r##"fill="#302f3d""##),
            "themed subgraph should pick up cluster_bkg #302f3d, got: {line}"
        );
        assert!(
            !line.contains(r##"fill="#1f2020""##),
            "themed subgraph must not fall back to node_fill #1f2020, got: {line}"
        );
    }
}

#[test]
fn explicit_subgraph_class_fill_wins_over_theme_cluster_bkg() {
    // Even when a theme would seed cluster_bkg, an author-specified classDef
    // fill must take precedence (preserves the precedence guarantee from #328).
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef blue fill:#e1f5fe,stroke:#01579b,stroke-width:2px\nclass A blue\n";
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

    assert!(svg.contains(r##"fill="#e1f5fe""##), "{svg}");
    assert!(
        !svg.contains(r##"fill="#302f3d""##),
        "themed cluster_bkg must yield to explicit classDef fill: {svg}"
    );
}

#[test]
fn dynamic_themed_subgraph_emits_cluster_bkg_css_variable() {
    // Dynamic theme mode wires `--cluster-bkg` so adapters can override the
    // subgraph default at runtime without re-rendering.
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\n";
    let svg = render_svg(
        input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                mode: SvgThemeMode::Dynamic,
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("--cluster-bkg:#302f3d"), "{svg}");
    assert!(
        svg.contains("--_cluster-bkg: var(--cluster-bkg);"),
        "dynamic style block should bridge --cluster-bkg into --_cluster-bkg: {svg}"
    );
    assert!(
        svg.contains("fill:var(--_cluster-bkg);"),
        "subgraph rect should carry the dynamic fill declaration: {svg}"
    );
    assert!(
        svg.contains(r#"data-svg-role="graph-subgraph-rect""#),
        "subgraph rect must tag data-svg-role=\"graph-subgraph-rect\" for adapter targeting: {svg}"
    );
}

#[test]
fn named_theme_keeps_cluster_bkg_when_caller_overrides_surface_slot() {
    // Custom surface override on a named theme must not drag cluster_bkg with
    // it — the named seed (#302f3d for dark) is the parity contract, and an
    // ad-hoc surface tweak is for nodes only.
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\n";
    let svg = render_svg(
        input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("dark".into()),
                surface: Some("#123456".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let subgraph_lines: Vec<&str> = svg
        .lines()
        .filter(|line| line.contains(r#"class="subgraph""#))
        .collect();
    assert!(!subgraph_lines.is_empty(), "{svg}");
    for line in &subgraph_lines {
        assert!(
            line.contains(r##"fill="#302f3d""##),
            "named theme cluster_bkg must survive a surface override: {line}"
        );
        assert!(
            !line.contains(r##"fill="#123456""##),
            "surface override must not bleed into subgraph fill: {line}"
        );
    }
}

#[test]
fn svg_subgraph_title_uses_visual_font_attrs_from_class_style() {
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef title font-style:italic,font-weight:700,color:#123456\nclass A title\n";
    let svg = render_svg(input, &RenderConfig::default());

    let title = regex::Regex::new(r##"<text [^>]*fill="#123456"[^>]*font-style="italic"[^>]*font-weight="700"[^>]*>Source</text>"##)
        .expect("title regex should compile");
    assert!(title.is_match(&svg), "{svg}");
}

#[test]
fn svg_subgraph_title_position_honors_svg_scale() {
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\n";
    let svg = render_svg(
        input,
        &RenderConfig {
            svg_scale: Some(2.0),
            ..RenderConfig::default()
        },
    );

    let rect = regex::Regex::new(r#"<rect class="subgraph" [^>]*\sy="([0-9.]+)""#)
        .expect("rect regex should compile");
    let text = regex::Regex::new(r#"<text [^>]*\sy="([0-9.]+)"[^>]*>Source</text>"#)
        .expect("text regex should compile");
    let rect_y: f64 = rect
        .captures(&svg)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse().ok())
        .expect("subgraph rect y should parse");
    let title_y: f64 = text
        .captures(&svg)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse().ok())
        .expect("subgraph title y should parse");

    assert!(
        (title_y - rect_y - 8.0).abs() < f64::EPSILON,
        "scaled title offset should be 16px * 2.0 * 0.25: {svg}"
    );
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
fn themed_flowchart_subgraph_uses_visible_stroke_and_title_slots() {
    let input = load_flowchart_fixture("subgraph_direction_mixed.mmd");
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

    let subgraph_rect_lines: Vec<&str> = svg
        .lines()
        .filter(|line| line.contains("class=\"subgraph\""))
        .collect();
    assert!(!subgraph_rect_lines.is_empty(), "{svg}");
    for line in &subgraph_rect_lines {
        assert!(
            line.contains("stroke=\"#cccccc\""),
            "subgraph rect should use the theme node_stroke slot (#cccccc), got: {line}\nfull svg: {svg}"
        );
    }
    for title in ["Left to Right", "Bottom to Top"] {
        let expected = format!("fill=\"#cccccc\">{title}</text>");
        assert!(
            svg.contains(&expected),
            "subgraph title `{title}` should use fill=#cccccc, full svg: {svg}"
        );
    }
    assert!(!svg.contains("#454545"), "{svg}");
    assert!(!svg.contains("#3b3b3b"), "{svg}");
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

    assert!(svg.contains("d=\"M64.68,166.00 L64.68,206.00\""), "{svg}");
    assert!(svg.contains("d=\"M207.82,62.00 L207.82,102.00\""), "{svg}");
    assert!(!svg.contains("d=\"M64.68,166.00 L64.68,216.00\""), "{svg}");
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

    assert!(svg.contains("d=\"M62.45,67.00 L62.45,112.00\""), "{svg}");
    assert!(!svg.contains("d=\"M62.45,66.00 L62.45,112.00\""), "{svg}");
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

    assert!(svg.contains("d=\"M43.78,172.00 L43.78,210.00\""), "{svg}");
    assert!(!svg.contains("d=\"M43.78,166.00 L43.78,216.00\""), "{svg}");
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
    // Two participants + activation rect = three rects at the theme's
    // surface fill. The note path now resolves through its own `note_fill`
    // slot (sticky yellow) instead of collapsing onto the surface, which is
    // the parity behavior #358 restored.
    assert_eq!(svg.matches("fill=\"#1f2020\"").count(), 3, "{svg}");
    assert!(svg.contains("fill=\"#fff5ad\""), "{svg}");
    assert!(!svg.contains("#ffffcc"), "{svg}");
    assert!(!svg.contains("#ddd"), "{svg}");
    assert!(!svg.contains("#424242"), "{svg}");
    assert!(!svg.contains("#454545"), "{svg}");
}

#[test]
fn sequence_svg_themed_note_fill_is_distinct_from_participant_fill() {
    // Parity coverage for #358: every themed render must paint notes with a
    // fill that is visibly distinct from the participant/node fill so the
    // sticky-note cue survives theming. Checks light and dark Mermaid base
    // themes so a future seed regression on either side trips the test.
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Note right of Bob: Important\n";

    // Every named Mermaid theme should produce a note fill that is distinct
    // from the participant rect fill. Beautiful palettes fall back to surface
    // and are covered by the broader theme suite once they each grow a tuned
    // `note_fill`; until then the named Mermaid themes are the parity floor.
    for theme_name in [
        "default",
        "dark",
        "forest",
        "neutral",
        "zinc-light",
        "zinc-dark",
    ] {
        let svg = render_svg(
            input,
            &RenderConfig {
                svg_theme: Some(SvgThemeConfig {
                    name: Some(theme_name.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        // Extract the note path fill (the only <path ... Z" fill="..." />).
        let note_marker = "Z\" fill=\"";
        let note_idx = svg.find(note_marker).unwrap_or_else(|| {
            panic!(
                "expected a closed-path note fill in themed sequence SVG for {theme_name}: {svg}"
            )
        });
        let after = &svg[note_idx + note_marker.len()..];
        let note_fill = &after[..after.find('"').expect("note fill should be quoted")];

        // Extract a participant rect fill: every theme renders the first
        // participant via `<rect ... fill="..." stroke=...`.
        let participant_marker = "<rect x=\"10.00\"";
        let part_idx = svg
            .find(participant_marker)
            .unwrap_or_else(|| panic!("expected participant rect for {theme_name}: {svg}"));
        let part_slice = &svg[part_idx..];
        let fill_marker = "fill=\"";
        let fill_idx = part_slice.find(fill_marker).expect("participant fill");
        let after_part = &part_slice[fill_idx + fill_marker.len()..];
        let participant_fill = &after_part[..after_part.find('"').expect("participant fill quote")];

        assert_ne!(
            note_fill, participant_fill,
            "{theme_name}: note fill and participant fill collapsed to the same value ({note_fill}); \
             #357 fixed visibility by routing both through `surface`, #358 restores the distinction"
        );
    }
}

#[test]
fn sequence_svg_untheme_path_preserves_legacy_sticky_yellow() {
    // Un-themed renders must keep the historical `#ffffcc` sticky-note fill
    // byte-identically. This is the byte-stability anchor #358 promises for
    // every fixture that does not opt into a theme.
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Note right of Bob: Important\n";
    let svg = render_svg(input, &RenderConfig::default());

    assert!(svg.contains("fill=\"#ffffcc\""), "{svg}");
    assert!(!svg.contains("fill=\"#fff5ad\""), "{svg}");
}

#[test]
fn sequence_svg_dynamic_theme_emits_note_fill_css_variable() {
    // Dynamic theme mode must surface `--note-fill` in the root style and
    // bridge it through `--_note-fill: var(--note-fill);` so adapters can
    // recolor sticky-notes at runtime without re-rendering.
    let input = "sequenceDiagram\n    Alice->>Bob: Hello\n    Note right of Bob: Important\n";
    let svg = render_svg(
        input,
        &RenderConfig {
            svg_theme: Some(SvgThemeConfig {
                name: Some("default".into()),
                mode: SvgThemeMode::Dynamic,
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    assert!(svg.contains("--note-fill:#fff5ad"), "{svg}");
    assert!(
        svg.contains("--_note-fill: var(--note-fill);"),
        "dynamic style block should bridge --note-fill into --_note-fill: {svg}"
    );
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
fn svg_node_with_rx_only_mirrors_ry_for_backwards_compatibility() {
    // Historical single-radius behaviour: when only `rx` is supplied, the SVG
    // emits `rx=<v> ry=<v>` so the rect renders as a uniform rounded corner.
    let input = "graph TD\n  classDef rounded rx:10\n  A[Box]:::rounded\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("rx=\"10\" ry=\"10\""),
        "expected mirrored rx/ry in SVG: {svg}"
    );
}

#[test]
fn svg_node_with_independent_ry_emits_distinct_radius() {
    let input = "graph TD\n  classDef ovalish rx:10,ry:20\n  A[Box]:::ovalish\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("rx=\"10\" ry=\"20\""),
        "expected independent rx and ry in SVG: {svg}"
    );
}

#[test]
fn svg_subgraph_with_independent_ry_emits_distinct_radius() {
    let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef rounded rx:10,ry:24\nclass A rounded\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("rx=\"10\" ry=\"24\""),
        "expected independent rx/ry on subgraph rect: {svg}"
    );
}

#[test]
fn svg_subgraph_with_rx_only_keeps_single_radius() {
    // Default subgraph styles (no explicit ry) MUST keep the historical
    // single-radius output for byte-identical replay.
    let input =
        "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef rounded rx:10\nclass A rounded\n";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains("rx=\"10\" ry=\"10\""),
        "expected mirrored rx/ry on subgraph rect: {svg}"
    );
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

#[test]
fn flowchart_subgraph_renders_g_wrapper_with_cluster_class() {
    let input = load_flowchart_fixture("subgraph_direction_mixed.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster" id=""#),
        "expected each subgraph to be wrapped in <g class=\"cluster\" id=\"...\">, got: {svg}"
    );
    let cluster_count = svg.matches(r#"<g class="cluster""#).count();
    assert_eq!(
        cluster_count, 2,
        "expected 2 subgraph wrappers, got {cluster_count}\n{svg}"
    );
}

#[test]
fn flowchart_subgraph_keeps_inner_rect_class_for_measurement_regex_compat() {
    let input = load_flowchart_fixture("subgraph_direction_mixed.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<rect class="subgraph""#),
        "inner rect must keep class=\"subgraph\" so dynamic measurement regex stays intact, got: {svg}"
    );
}

#[test]
fn flowchart_user_class_lands_on_subgraph_wrapper() {
    let input = load_flowchart_fixture("subgraph_user_class.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster blueFill" id="lr">"#),
        "expected wrapper with user class blueFill on subgraph lr, got: {svg}"
    );
}

#[test]
fn flowchart_user_class_does_not_leak_to_non_classed_subgraphs() {
    let input = "\
flowchart TD
    subgraph lr [Left to Right]
        A --> B
    end
    subgraph bt [Bottom to Top]
        C --> D
    end
    classDef blueFill fill:#9cf
    class lr blueFill
";
    let svg = render_svg(input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster blueFill" id="lr">"#),
        "{svg}"
    );
    assert!(svg.contains(r#"<g class="cluster" id="bt">"#), "{svg}");
    assert!(
        !svg.contains(r#"<g class="cluster blueFill" id="bt">"#),
        "{svg}"
    );
}

#[test]
fn flowchart_user_class_resolved_style_applies_to_inner_rect() {
    let input = load_flowchart_fixture("subgraph_user_class.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r##"fill="#9cf""##) || svg.contains(r##"fill="#9CF""##),
        "expected fill from classDef on inner rect, got: {svg}"
    );
}

#[test]
fn nested_subgraphs_each_get_their_own_wrapper_and_class() {
    let input = load_flowchart_fixture("subgraph_nested_classes.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster outerSkin" id="outer">"#),
        "outer wrapper missing or wrong class: {svg}"
    );
    assert!(
        svg.contains(r#"<g class="cluster innerSkin" id="inner">"#),
        "inner wrapper missing or wrong class: {svg}"
    );
}

#[test]
fn nested_subgraph_outer_class_does_not_inherit_to_inner() {
    let input = load_flowchart_fixture("subgraph_nested_classes.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    let inner_open = svg
        .find(r#"<g class="cluster innerSkin" id="inner">"#)
        .expect("inner wrapper exists");
    let inner_line_end = svg[inner_open..].find('>').unwrap() + inner_open;
    let inner_line = &svg[inner_open..=inner_line_end];
    assert!(
        !inner_line.contains("outerSkin"),
        "inner wrapper must not inherit outer class, got: {inner_line}"
    );
}

#[test]
fn nested_subgraph_wrappers_are_ordered_outer_before_inner() {
    let input = load_flowchart_fixture("subgraph_nested_classes.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    let outer = svg
        .find(r#"<g class="cluster outerSkin" id="outer">"#)
        .expect("outer wrapper present");
    let inner = svg
        .find(r#"<g class="cluster innerSkin" id="inner">"#)
        .expect("inner wrapper present");
    assert!(
        outer < inner,
        "outer wrapper should appear before inner in render order, got outer@{outer} inner@{inner}"
    );
}

#[test]
fn class_diagram_namespace_renders_g_wrapper() {
    let input = load_class_fixture("namespaces.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster" id=""#),
        "class namespace should emit <g class=\"cluster\" id=\"...\"> wrapper, got: {svg}"
    );
    let wrapper_count = svg.matches(r#"<g class="cluster""#).count();
    assert!(
        wrapper_count >= 2,
        "expected >=2 namespace wrappers in namespaces.mmd, got {wrapper_count}\n{svg}"
    );
}

#[test]
fn state_composite_region_renders_g_wrapper() {
    let input = load_state_fixture("composite.mmd");
    let svg = render_svg(&input, &RenderConfig::default());
    assert!(
        svg.contains(r#"<g class="cluster" id=""#),
        "state composite region should emit <g class=\"cluster\" id=\"...\"> wrapper, got: {svg}"
    );
}

#[test]
fn cross_family_wrappers_preserve_inner_rect_subgraph_class() {
    let class_svg = render_svg(
        &load_class_fixture("namespaces.mmd"),
        &RenderConfig::default(),
    );
    let state_svg = render_svg(
        &load_state_fixture("composite.mmd"),
        &RenderConfig::default(),
    );
    assert!(
        class_svg.contains(r#"<rect class="subgraph""#),
        "{class_svg}"
    );
    assert!(
        state_svg.contains(r#"<rect class="subgraph""#),
        "{state_svg}"
    );
}

#[test]
fn flowchart_subgraph_g_wrapper_closes_after_text() {
    let input = "flowchart TD\n    subgraph lr [Left to Right]\n        A --> B\n    end\n";
    let svg = render_svg(input, &RenderConfig::default());
    let open = svg
        .find(r#"<g class="cluster" id=""#)
        .expect("wrapper opens");
    let inner = &svg[open..];
    let close_rel = inner.find("</g>").expect("wrapper closes");
    let between = &inner[..close_rel];
    assert!(
        between.contains(r#"<rect class="subgraph""#),
        "rect must be inside wrapper: {between}"
    );
    assert!(
        between.contains(">Left to Right</text>"),
        "title must be inside wrapper: {between}"
    );
}
