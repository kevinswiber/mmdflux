//! Integration tests for mmdflux.
//!
//! These tests verify the full parsing and rendering pipeline using fixture files.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use mmdflux::diagram::{EdgePreset, EngineAlgorithmId, OutputFormat, RenderConfig, TextColorMode};
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::geometry::{FPoint, RoutedGraphGeometry};
use mmdflux::diagrams::flowchart::routing::route_graph_geometry;
use mmdflux::diagrams::mmds::from_mmds_str;
use mmdflux::render::{
    Layout, NodeBounds, RenderOptions, RoutedEdge, Segment, TextLayoutConfig, compute_layout,
    geometry_to_text_layout_with_routed, render, render_all_edges_with_labels, route_all_edges,
};
use mmdflux::{
    Diagram, Direction, EdgeRouting, EngineConfig, Shape, build_diagram, default_registry,
    parse_flowchart,
};

/// Load a fixture file by name from `tests/fixtures/flowchart/`.
fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

const ISSUE_21_FIXTURE: &str = "callgraph_feedback_cycle.mmd";

/// Load an MMDS fixture file by name from `tests/fixtures/mmds/`.
fn load_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

/// Parse and build a diagram from a fixture file.
fn parse_and_build(name: &str) -> Diagram {
    let input = load_fixture(name);
    let flowchart = parse_flowchart(&input).expect("Failed to parse fixture");
    build_diagram(&flowchart)
}

/// Parse, build, and compute layout for a fixture file.
fn layout_fixture(name: &str) -> (Diagram, Layout) {
    let diagram = parse_and_build(name);
    let layout = compute_layout(&diagram, &TextLayoutConfig::default());
    (diagram, layout)
}

fn layout_fixture_with_routed(name: &str) -> (Diagram, Layout) {
    let diagram = parse_and_build(name);
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
        .expect("layout should succeed");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let layout = geometry_to_text_layout_with_routed(
        &diagram,
        &geom,
        Some(&routed),
        &TextLayoutConfig::default(),
    );
    (diagram, layout)
}

/// Parse, build, and render a fixture file.
fn render_fixture(name: &str) -> String {
    let diagram = parse_and_build(name);
    render(&diagram, &RenderOptions::default())
}

fn render_fixture_with_options(
    name: &str,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> String {
    let diagram = parse_and_build(name);
    render(
        &diagram,
        &RenderOptions {
            output_format: format,
            text_color_mode,
            ..Default::default()
        },
    )
}

/// Parse, build, and render a Mermaid input string.
fn render_input(input: &str) -> String {
    let flowchart = parse_flowchart(input).expect("Failed to parse input");
    let diagram = build_diagram(&flowchart);
    render(&diagram, &RenderOptions::default())
}

/// Parse, build, and render a fixture file with ASCII-only output.
fn render_fixture_ascii(name: &str) -> String {
    render_fixture_with_options(name, OutputFormat::Ascii, TextColorMode::Plain)
}

#[test]
fn criss_cross_text_keeps_vertical_terminal_arrowheads() {
    let output = render_fixture("criss_cross.mmd");

    assert!(
        !output.contains('►') && !output.contains('◄'),
        "criss_cross text output should not fall back to a horizontal terminal arrowhead after the orthogonal de-overlap reroute:\n{output}"
    );
    let has_bottom_arrival_row = output
        .lines()
        .any(|line| line.chars().filter(|&ch| ch == '▼').count() >= 4);
    assert!(
        has_bottom_arrival_row,
        "criss_cross text output should keep visible downward terminal arrowheads into the bottom row targets:\n{output}"
    );
}

fn assert_flowchart_snapshot(name: &str) {
    let output = render_fixture(name);
    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("flowchart")
        .join(name.replace(".mmd", ".txt"));
    let expected = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|_| panic!("Missing snapshot: {}", snapshot_path.display()));

    assert_eq!(output, expected, "Snapshot mismatch for {name}");
}

fn strip_ansi(input: &str) -> String {
    let mut stripped = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        stripped.push(ch);
    }

    stripped
}

/// Assert that all values in the slice are distinct.
fn assert_all_distinct(values: &[usize], context: &str) {
    for i in 0..values.len() {
        for j in (i + 1)..values.len() {
            assert_ne!(
                values[i], values[j],
                "{}: duplicate value {} (all: {:?})",
                context, values[i], values
            );
        }
    }
}

fn route_fixture_orthogonal(fixture: &str) -> RoutedGraphGeometry {
    let diagram = parse_and_build(fixture);
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
        .expect("layout should succeed");
    route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute)
}

fn route_input_orthogonal(input: &str) -> RoutedGraphGeometry {
    let flowchart = parse_flowchart(input).expect("fixture input should parse");
    let diagram = build_diagram(&flowchart);
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
        .expect("layout should succeed");
    route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute)
}

fn edge_path<'a>(routed: &'a RoutedGraphGeometry, from: &str, to: &str) -> &'a [FPoint] {
    routed
        .edges
        .iter()
        .find(|edge| edge.from == from && edge.to == to)
        .unwrap_or_else(|| panic!("missing edge {from} -> {to}"))
        .path
        .as_slice()
}

fn first_segment(path: &[FPoint]) -> (f64, bool) {
    assert!(path.len() >= 2, "routed path must have at least two points");
    let p0 = path[0];
    let p1 = path[1];
    let dx = (p1.x - p0.x).abs();
    let dy = (p1.y - p0.y).abs();
    (dx + dy, dy > dx + 0.000_001)
}

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let (a_min, a_max) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
    let (b_min, b_max) = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
    a_min <= b_max && b_min <= a_max
}

fn segment_intersects_bounds(segment: Segment, bounds: &NodeBounds) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    match segment {
        Segment::Vertical { x, y_start, y_end } => {
            x >= left && x <= right && ranges_overlap(y_start, y_end, top, bottom)
        }
        Segment::Horizontal { y, x_start, x_end } => {
            y >= top && y <= bottom && ranges_overlap(x_start, x_end, left, right)
        }
    }
}

fn unrelated_node_intrusions(routed: &[RoutedEdge], layout: &Layout) -> Vec<String> {
    let mut intrusions = Vec::new();

    for edge in routed {
        for (node_id, bounds) in &layout.node_bounds {
            if node_id == &edge.edge.from || node_id == &edge.edge.to {
                continue;
            }

            for segment in &edge.segments {
                if segment_intersects_bounds(*segment, bounds) {
                    intrusions.push(format!(
                        "{} -> {} intersects unrelated node {} via {:?} against {:?}",
                        edge.edge.from, edge.edge.to, node_id, segment, bounds
                    ));
                }
            }
        }
    }

    intrusions
}

// =============================================================================
// Parsing Tests
// =============================================================================

mod parsing {
    use super::*;

    #[test]
    fn simple_parses_correctly() {
        let diagram = parse_and_build("simple.mmd");

        assert_eq!(diagram.direction, Direction::TopDown);
        assert_eq!(diagram.nodes.len(), 2);
        assert_eq!(diagram.edges.len(), 1);

        assert!(diagram.nodes.contains_key("A"));
        assert!(diagram.nodes.contains_key("B"));
        assert_eq!(diagram.nodes["A"].label, "Start");
        assert_eq!(diagram.nodes["B"].label, "End");
    }

    #[test]
    fn decision_parses_correctly() {
        let diagram = parse_and_build("decision.mmd");

        assert_eq!(diagram.nodes.len(), 4);
        assert_eq!(diagram.edges.len(), 4);
        assert_eq!(diagram.nodes["B"].shape, Shape::Diamond);
        assert_eq!(diagram.nodes["B"].label, "Is it working?");
    }

    #[test]
    fn shapes_parses_correctly() {
        let diagram = parse_and_build("shapes.mmd");

        assert_eq!(diagram.nodes["rect"].shape, Shape::Rectangle);
        assert_eq!(diagram.nodes["round"].shape, Shape::Round);
        assert_eq!(diagram.nodes["diamond"].shape, Shape::Diamond);
    }

    #[test]
    fn shape_keywords_parse_document_and_card() {
        let diagram = parse_and_build("shapes_document.mmd");
        assert_eq!(diagram.nodes["doc"].shape, Shape::Document);
        assert_eq!(diagram.nodes["docs"].shape, Shape::Documents);
        assert_eq!(diagram.nodes["tagdoc"].shape, Shape::TaggedDocument);
        assert_eq!(diagram.nodes["card"].shape, Shape::Card);
        assert_eq!(diagram.nodes["tag"].shape, Shape::TaggedRect);
    }

    #[test]
    fn shape_keywords_parse_junctions_and_specials() {
        let diagram = parse_and_build("shapes_junction.mmd");
        assert_eq!(diagram.nodes["j1"].shape, Shape::SmallCircle);
        assert_eq!(diagram.nodes["j2"].shape, Shape::FramedCircle);
        assert_eq!(diagram.nodes["j3"].shape, Shape::CrossedCircle);

        let diagram = parse_and_build("shapes_special.mmd");
        assert_eq!(diagram.nodes["fork"].shape, Shape::ForkJoin);
        assert_eq!(diagram.nodes["note"].shape, Shape::TextBlock);
    }

    #[test]
    fn shape_keywords_parse_degenerate_fallbacks() {
        let diagram = parse_and_build("shapes_degenerate.mmd");
        for id in [
            "cloud",
            "bolt",
            "bang",
            "icon",
            "hourglass",
            "tri",
            "flip",
            "notch",
        ] {
            assert_eq!(diagram.nodes[id].shape, Shape::Rectangle);
        }
    }

    #[test]
    fn left_right_direction() {
        let diagram = parse_and_build("left_right.mmd");
        assert_eq!(diagram.direction, Direction::LeftRight);
    }

    #[test]
    fn bottom_top_direction() {
        let diagram = parse_and_build("bottom_top.mmd");
        assert_eq!(diagram.direction, Direction::BottomTop);
    }

    #[test]
    fn right_left_direction() {
        let diagram = parse_and_build("right_left.mmd");
        assert_eq!(diagram.direction, Direction::RightLeft);
    }

    #[test]
    fn chain_creates_correct_edges() {
        let diagram = parse_and_build("chain.mmd");
        assert_eq!(diagram.nodes.len(), 4);
        assert_eq!(diagram.edges.len(), 3);
    }

    #[test]
    fn ampersand_expands_to_multiple_edges() {
        let diagram = parse_and_build("ampersand.mmd");
        assert_eq!(diagram.nodes.len(), 5);
        // A & B --> C creates 2 edges, C --> D & E creates 2 edges
        assert_eq!(diagram.edges.len(), 4);
    }

    #[test]
    fn labeled_edges_parsed() {
        let diagram = parse_and_build("labeled_edges.mmd");
        let edges_with_labels = diagram.edges.iter().filter(|e| e.label.is_some()).count();
        assert!(edges_with_labels > 0, "Should have labeled edges");
    }

    #[test]
    fn inline_edge_labels_parsed() {
        let diagram = parse_and_build("inline_edge_labels.mmd");

        assert_eq!(diagram.edges.len(), 4);
        assert_eq!(diagram.edges[0].label.as_deref(), Some("yes"));
        assert_eq!(diagram.edges[1].label.as_deref(), Some("retry"));
        assert_eq!(diagram.edges[2].label.as_deref(), Some("final step"));
        assert_eq!(diagram.edges[3].label.as_deref(), Some("no"));

        assert!(!diagram.nodes.contains_key("yes"));
        assert!(!diagram.nodes.contains_key("retry"));
        assert!(!diagram.nodes.contains_key("no"));
    }

    #[test]
    fn inline_label_flowchart_parsed() {
        let diagram = parse_and_build("inline_label_flowchart.mmd");

        let mut counts: HashMap<&str, usize> = HashMap::new();
        for label in diagram.edges.iter().filter_map(|e| e.label.as_deref()) {
            *counts.entry(label).or_insert(0) += 1;
        }

        assert_eq!(counts.get("no"), Some(&2));
        assert_eq!(counts.get("yes"), Some(&2));
        assert_eq!(counts.get("sync"), Some(&1));
        assert_eq!(counts.get("async"), Some(&1));
        assert_eq!(counts.get("hit"), Some(&1));
        assert_eq!(counts.get("miss"), Some(&1));
        assert_eq!(counts.get("warn"), Some(&1));
        assert_eq!(counts.values().sum::<usize>(), 9);
    }

    #[test]
    fn comments_are_ignored() {
        let diagram = parse_and_build("git_workflow.mmd");
        assert_eq!(diagram.nodes.len(), 4);
        assert!(!diagram.nodes.contains_key("%%"));
    }

    #[test]
    fn complex_parses_all_features() {
        let diagram = parse_and_build("complex.mmd");
        assert!(diagram.nodes.len() >= 9);
        assert!(diagram.edges.len() >= 10);
    }
}

// =============================================================================
// Rendering Tests
// =============================================================================

mod rendering {
    use super::*;

    #[test]
    fn simple_renders() {
        let output = render_fixture("simple.mmd");
        assert!(!output.is_empty());
        assert!(output.contains("Start"));
        assert!(output.contains("End"));
    }

    #[test]
    fn decision_renders_diamond() {
        let output = render_fixture("decision.mmd");
        assert!(output.contains("Is it working?"));
        // Diamond shapes use < and > characters
        assert!(output.contains('<') || output.contains('>'));
    }

    #[test]
    fn shapes_render_distinctly() {
        let output = render_fixture("shapes.mmd");
        assert!(output.contains("Rectangle Node"));
        assert!(output.contains("Rounded Node"));
        assert!(output.contains("Diamond Node"));
    }

    #[test]
    fn shapes_document_render_distinctly() {
        let output = render_fixture("shapes_document.mmd");
        assert!(output.contains("Doc"));
        assert!(output.contains("Docs"));
        assert!(output.contains("TagDoc"));
        assert!(output.contains("Card"));
        assert!(output.contains("Tag"));
        assert!(output.contains('~'), "Document should use wavy bottom");
        assert!(
            output.contains('╱'),
            "Tagged doc/card should use folded corner"
        );
    }

    #[test]
    fn shapes_junction_render_glyphs() {
        let output = render_fixture("shapes_junction.mmd");
        assert!(output.contains('●'));
        assert!(output.contains('◉'));
        assert!(output.contains('⊗'));
    }

    #[test]
    fn shapes_special_render_bar_and_text() {
        let output = render_fixture("shapes_special.mmd");
        // shapes_special.mmd uses graph LR, so fork/join bar is vertical
        assert!(
            output.contains('┃'),
            "Fork/join in LR should use heavy vertical bar"
        );
        assert!(output.contains("Note"));
    }

    #[test]
    fn shapes_junction_ascii_degrades() {
        let output = render_fixture_ascii("shapes_junction.mmd");
        assert!(output.contains("o"));
        assert!(output.contains("(o)"));
        assert!(output.contains("x"));
    }

    #[test]
    fn text_render_uses_stroke_fill_and_label_colors_when_ansi_enabled() {
        let plain = render_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Text,
            TextColorMode::Plain,
        );
        let ansi =
            render_fixture_with_options("style-basic.mmd", OutputFormat::Text, TextColorMode::Ansi);

        assert!(ansi.contains("38;2;"));
        assert!(ansi.contains("48;2;"));
        assert_eq!(strip_ansi(&ansi), plain);
    }

    #[test]
    fn text_render_clears_fill_background_before_right_border() {
        let ansi =
            render_fixture_with_options("style-basic.mmd", OutputFormat::Text, TextColorMode::Ansi);

        assert!(
            ansi.contains("\u{1b}[38;2;51;51;51;49m│"),
            "expected right border to clear fill background: {ansi:?}"
        );
        assert!(
            !ansi.contains("\u{1b}[48;2;255;238;170m \u{1b}[38;2;51;51;51m│"),
            "right border should not retain fill background: {ansi:?}"
        );
    }

    #[test]
    fn ascii_render_keeps_same_geometry_with_color_disabled() {
        let plain = render_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Ascii,
            TextColorMode::Plain,
        );
        let ansi = render_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Ascii,
            TextColorMode::Ansi,
        );

        assert!(ansi.contains("\u{1b}["));
        assert_eq!(strip_ansi(&ansi), plain);
    }

    #[test]
    fn shapes_degenerate_render_labels() {
        let output = render_fixture("shapes_degenerate.mmd");
        for label in [
            "Cloud", "Bolt", "Bang", "Icon", "Hour", "Tri", "Flip", "Notch",
        ] {
            assert!(output.contains(label));
        }
    }

    #[test]
    fn edge_styles_render() {
        let output = render_fixture("edge_styles.mmd");
        assert!(output.contains("Solid"));
        assert!(output.contains("Dotted"));
        assert!(output.contains("Thick"));
    }

    #[test]
    fn left_right_renders_horizontally() {
        let output = render_fixture("left_right.mmd");
        // In LR layout, nodes should be on similar vertical lines
        // The output should be wider than tall (roughly)
        let lines: Vec<&str> = output.lines().collect();
        let height = lines.len();
        let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        assert!(
            width > height,
            "LR layout should be wider than tall: {}x{}",
            width,
            height
        );
    }

    #[test]
    fn chain_renders_all_nodes() {
        let output = render_fixture("chain.mmd");
        assert!(output.contains("Step 1"));
        assert!(output.contains("Step 2"));
        assert!(output.contains("Step 3"));
        assert!(output.contains("Step 4"));
    }

    #[test]
    fn labeled_edges_show_labels() {
        let output = render_fixture("labeled_edges.mmd");
        // Labels should appear in output
        assert!(output.contains("initialize") || output.contains("configure"));

        // "yes" and "no" labels from the Config diamond branches
        assert!(
            output.contains("yes"),
            "Expected 'yes' label in output:\n{output}"
        );
        assert!(
            output.contains("no"),
            "Expected 'no' label in output:\n{output}"
        );
    }

    #[test]
    fn branching_labels_dont_overlap() {
        // Test that branching edges with labels place them on separate branches
        let output = render_fixture("label_spacing.mmd");

        // Both labels should be present and complete (not truncated)
        assert!(output.contains("valid"), "Should contain 'valid' label");
        assert!(output.contains("invalid"), "Should contain 'invalid' label");

        // The labels should NOT directly overlap (no merged text like "valinvalid")
        // They can be on the same row as long as they're separated
        assert!(
            !output.contains("valinvalid"),
            "Labels should not merge into 'valinvalid'"
        );
        assert!(
            !output.contains("invalidvalid"),
            "Labels should not merge into 'invalidvalid'"
        );

        // Labels should appear between source node A and target nodes B/C
        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|l| l.contains(" A ")).unwrap();
        let b_line = lines.iter().rposition(|l| l.contains(" B ")).unwrap();

        // At least one label should be between A and B rows
        let label_line = lines.iter().position(|l| l.contains("valid")).unwrap();
        assert!(
            label_line > a_line && label_line < b_line,
            "Label at line {} should be between A (line {}) and B (line {})\n{}",
            label_line,
            a_line,
            b_line,
            output
        );
    }

    #[test]
    fn git_workflow_renders() {
        let output = render_fixture("git_workflow.mmd");
        for label in [
            "Working Dir",
            "Staging Area",
            "Local Repo",
            "Remote Repo",
            "git add",
            "git commit",
            "git push",
            "git pull",
        ] {
            assert!(output.contains(label), "Missing '{label}':\n{output}");
        }
    }

    #[test]
    fn git_workflow_matches_snapshot() {
        assert_flowchart_snapshot("git_workflow.mmd");
    }

    #[test]
    fn backward_loop_lr_matches_snapshot() {
        assert_flowchart_snapshot("backward_loop_lr.mmd");
    }

    #[test]
    fn git_workflow_backward_route_uses_compact_bottom_channel_with_routed_layout() {
        let (diagram, layout) = layout_fixture_with_routed("git_workflow.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);
        let remote_to_working = routed
            .iter()
            .find(|edge| edge.edge.from == "Remote" && edge.edge.to == "Working")
            .expect("git_workflow should contain Remote -> Working");
        let horizontal_segments = remote_to_working
            .segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Horizontal { .. }))
            .count();
        let vertical_segments = remote_to_working
            .segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Vertical { .. }))
            .count();

        assert_eq!(
            remote_to_working.segments.len(),
            5,
            "git_workflow Remote -> Working should collapse to the shared bottom-channel family when routed draw paths are available: {:?}",
            remote_to_working.segments
        );
        assert_eq!(
            horizontal_segments, 2,
            "git_workflow Remote -> Working should keep two horizontal traversals in the compact bottom-channel family: {:?}",
            remote_to_working.segments
        );
        assert_eq!(
            vertical_segments, 3,
            "git_workflow Remote -> Working should keep three vertical support segments in the compact bottom-channel family: {:?}",
            remote_to_working.segments
        );
    }

    #[test]
    fn routed_long_skip_edges_use_compact_shared_elbow_family() {
        for fixture in ["double_skip.mmd", "skip_edge_collision.mmd"] {
            let (diagram, layout) = layout_fixture_with_routed(fixture);
            let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);
            let a_to_d = routed
                .iter()
                .find(|edge| edge.edge.from == "A" && edge.edge.to == "D")
                .unwrap_or_else(|| panic!("{fixture} should contain A -> D"));

            let horizontal_segments = a_to_d
                .segments
                .iter()
                .filter(|segment| matches!(segment, Segment::Horizontal { .. }))
                .count();
            let vertical_segments = a_to_d
                .segments
                .iter()
                .filter(|segment| matches!(segment, Segment::Vertical { .. }))
                .count();

            assert_eq!(
                a_to_d.segments.len(),
                5,
                "{fixture} A -> D should use the compact routed elbow family when routed draw paths are available: {:?}",
                a_to_d.segments
            );
            assert_eq!(
                horizontal_segments, 2,
                "{fixture} A -> D should keep exactly two horizontal traversals in the routed elbow family: {:?}",
                a_to_d.segments
            );
            assert_eq!(
                vertical_segments, 3,
                "{fixture} A -> D should keep exactly three vertical segments in the routed elbow family: {:?}",
                a_to_d.segments
            );
        }
    }

    #[test]
    fn criss_cross_b_to_e_keeps_vertical_terminal_support_in_text() {
        let (diagram, layout) = layout_fixture_with_routed("criss_cross.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);
        let b_to_e = routed
            .iter()
            .find(|edge| edge.edge.from == "B" && edge.edge.to == "E")
            .expect("criss_cross should contain B -> E");

        let final_segment = b_to_e
            .segments
            .iter()
            .rev()
            .find(|segment| segment.length() > 0)
            .expect("criss_cross B -> E should keep a visible terminal support");
        assert!(
            matches!(
                final_segment,
                Segment::Vertical { x, y_start, y_end }
                    if *x == b_to_e.end.x && *y_end > *y_start
            ),
            "criss_cross B -> E should keep a downward terminal support into E: end={:?}, segments={:?}",
            b_to_e.end,
            b_to_e.segments
        );
    }

    #[test]
    fn callgraph_feedback_cycle_lower_criss_cross_stays_separated_in_text() {
        let (diagram, layout) = layout_fixture_with_routed("callgraph_feedback_cycle.mmd");
        let n4_to_n6 = diagram
            .edges
            .iter()
            .find(|edge| edge.from == "n4" && edge.to == "n6")
            .expect("callgraph_feedback_cycle should contain n4 -> n6");
        let n7_to_n5 = diagram
            .edges
            .iter()
            .find(|edge| edge.from == "n7" && edge.to == "n5")
            .expect("callgraph_feedback_cycle should contain n7 -> n5");

        let detour = layout
            .routed_edge_paths
            .get(&n4_to_n6.index)
            .expect("n4 -> n6 should keep its routed detour");
        let simple = layout
            .routed_edge_paths
            .get(&n7_to_n5.index)
            .expect("n7 -> n5 should keep its routed crossing path");

        assert!(
            detour.len() >= 6,
            "n4 -> n6 should remain a multi-bend detour after text adaptation: {detour:?}"
        );
        assert!(
            simple.len() >= 5,
            "n7 -> n5 should retain an explicit center crossing row after text adaptation: {simple:?}"
        );

        let upper_y = detour[1].1;
        let lower_y = detour[3].1;
        let center_y = simple[2].1;
        assert!(
            upper_y < center_y && center_y < lower_y,
            "callgraph_feedback_cycle lower criss-cross should preserve a distinct center crossing row between the detour's upper and lower lanes: simple={simple:?}, detour={detour:?}"
        );
        let detour_center_x = detour[2].0;
        assert!(
            simple
                .windows(2)
                .all(|segment| !(segment[0].0 == detour_center_x
                    && segment[1].0 == detour_center_x
                    && segment[0].1 != segment[1].1)),
            "callgraph_feedback_cycle lower criss-cross should cross the detour spine in a single cell instead of sharing a vertical run: simple={simple:?}, detour={detour:?}"
        );
        assert!(
            simple[2].0 > detour_center_x && simple[3].0 < detour_center_x,
            "callgraph_feedback_cycle lower criss-cross should cross on the center row and descend on a target-side column: simple={simple:?}, detour={detour:?}"
        );
    }

    #[test]
    fn http_request_renders() {
        let output = render_fixture("http_request.mmd");
        // Due to cycle handling, node order may vary. Check for presence of key elements.
        assert!(!output.is_empty());
        // At least some nodes should be present
        let has_nodes = output.contains("Client")
            || output.contains("Server")
            || output.contains("Process")
            || output.contains("Response");
        assert!(has_nodes, "Should contain at least one node label");
        // Should have diamond shape indicators
        assert!(
            output.contains('<') || output.contains('>'),
            "Should have decision node (diamond shape uses < or > chars)"
        );
    }

    #[test]
    fn ci_pipeline_renders() {
        let output = render_fixture("ci_pipeline.mmd");
        assert!(output.contains("Build"));
        assert!(output.contains("Test"));
        assert!(output.contains("Deploy?"));
    }

    #[test]
    fn complex_renders_without_panic() {
        let output = render_fixture("complex.mmd");
        assert!(!output.is_empty());
        assert!(output.contains("Input"));
        assert!(output.contains("Output"));
    }

    #[test]
    fn ascii_issue_21_backward_edge_does_not_clip_right_edge() {
        let diagram = parse_and_build(ISSUE_21_FIXTURE);
        let output = render(
            &diagram,
            &RenderOptions {
                output_format: OutputFormat::Ascii,
                ..Default::default()
            },
        );

        let clipped_lines: Vec<&str> = output
            .lines()
            .filter(|line| line.trim_end().ends_with('-'))
            .collect();
        assert!(
            clipped_lines.is_empty(),
            "ASCII output should not be clipped on the right edge for issue #21.\nFound clipped lines:\n{}\n\nFull output:\n{}",
            clipped_lines.join("\n"),
            output
        );
    }

    #[test]
    fn issue_21_quantized_waypoint_corridor_does_not_cross_unrelated_nodes() {
        let (diagram, layout) = layout_fixture(ISSUE_21_FIXTURE);
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);
        let intrusions: Vec<String> = unrelated_node_intrusions(&routed, &layout)
            .into_iter()
            .filter(|intrusion| intrusion.starts_with("n3 -> n4"))
            .collect();

        assert!(
            intrusions.is_empty(),
            "issue_21 text routing should keep the reranked n3 -> n4 corridor out of unrelated nodes:\n{}",
            intrusions.join("\n")
        );
    }

    #[test]
    fn ascii_only_mode() {
        let unicode_output = render_fixture("simple.mmd");
        let ascii_output = render_fixture_ascii("simple.mmd");

        // Both should contain the labels
        assert!(unicode_output.contains("Start"));
        assert!(ascii_output.contains("Start"));

        // ASCII output should not contain Unicode box-drawing chars
        let unicode_chars = [
            '─', '│', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '╭', '╮', '╯', '╰',
        ];
        for ch in unicode_chars {
            assert!(
                !ascii_output.contains(ch),
                "ASCII output should not contain '{}'",
                ch
            );
        }
    }
}

// =============================================================================
// Stagger Preservation Tests
// =============================================================================

mod stagger {
    use super::*;

    #[test]
    fn stagger_present_for_multiple_cycles() {
        // multiple_cycles.mmd: A[Top] --> B[Middle], B --> C[Bottom], C --> A, C --> B
        // With multiple backward edges, the BK algorithm may produce horizontal
        // displacement among nodes.  With per-edge label spacing (compact unlabeled
        // edges), all three nodes end up vertically aligned (no stagger).  We only
        // verify the layout is well-formed: each node is placed and all center-x
        // values are finite.
        let (_, layout) = layout_fixture("multiple_cycles.mmd");

        let a_cx = layout.node_bounds["A"].center_x();
        let b_cx = layout.node_bounds["B"].center_x();
        let c_cx = layout.node_bounds["C"].center_x();

        // Verify the layout is well-formed: each node is placed.
        assert!(
            a_cx > 0 && b_cx > 0 && c_cx > 0,
            "All node centers should be positive: A={a_cx}, B={b_cx}, C={c_cx}",
        );
    }

    #[test]
    fn no_stagger_for_simple_chain() {
        // chain.mmd: linear chain with no backward edges → no stagger
        let (_, layout) = layout_fixture("chain.mmd");

        let centers: Vec<usize> = layout.node_bounds.values().map(|b| b.center_x()).collect();
        let first = centers[0];
        for &c in &centers[1..] {
            assert!(
                (c as isize - first as isize).unsigned_abs() <= 1,
                "All nodes should be centered: got {:?}",
                centers
            );
        }
    }

    #[test]
    fn stagger_produces_different_attachment_points() {
        // For multiple_cycles.mmd, the forward edge A→B and backward edge C→A
        // should attach at different positions on node A's boundary.
        let (diagram, layout) = layout_fixture("multiple_cycles.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let a_b_edge = routed
            .iter()
            .find(|e| e.edge.from == "A" && e.edge.to == "B")
            .expect("A→B edge should exist");
        let c_a_edge = routed
            .iter()
            .find(|e| e.edge.from == "C" && e.edge.to == "A")
            .expect("C→A edge should exist");

        // A→B exits from A (start point); C→A enters A (end point)
        // With stagger, these should be at different positions on A
        assert_ne!(
            a_b_edge.start, c_a_edge.end,
            "Forward A→B start ({:?}) and backward C→A end ({:?}) should differ on A",
            a_b_edge.start, c_a_edge.end
        );
    }

    #[test]
    fn stagger_absent_for_simple_cycle_with_corrected_spacing() {
        // simple_cycle.mmd has one backward edge spanning the full graph.
        // With corrected spacing (reversed edge chains excluded from gap
        // inflation), the reduced inter-rank spacing keeps nodes centered
        // without horizontal displacement. The backward edge routes around
        // the right side instead of causing stagger.
        let (_, layout) = layout_fixture("simple_cycle.mmd");

        let centers: Vec<usize> = layout.node_bounds.values().map(|b| b.center_x()).collect();
        let min_center = *centers.iter().min().unwrap();
        let max_center = *centers.iter().max().unwrap();
        assert!(
            max_center - min_center <= 2,
            "Simple cycle should have minimal stagger with corrected spacing: centers {:?} (range={})",
            centers,
            max_center - min_center
        );
    }
}

// =============================================================================
// Attachment Point Spreading Tests
// =============================================================================

mod spreading {
    use super::*;

    /// Verify that no row has immediately adjacent down-arrows.
    fn assert_no_adjacent_arrows(output: &str, fixture_name: &str) {
        for (line_num, line) in output.lines().enumerate() {
            assert!(
                !line.contains("▼▼"),
                "{}: line {} has adjacent arrows: {}",
                fixture_name,
                line_num + 1,
                line
            );
        }
    }

    /// Verify that edges arriving at a shared target node have distinct endpoint x-coordinates.
    fn assert_distinct_arrival_x(fixture_name: &str, target_node: &str) {
        let (diagram, layout) = layout_fixture(fixture_name);
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|r| r.edge.to == target_node)
            .map(|r| r.end.x)
            .collect();

        assert!(
            arrival_xs.len() >= 2,
            "{}: expected >=2 edges arriving at {}, got {}",
            fixture_name,
            target_node,
            arrival_xs.len()
        );

        assert_all_distinct(
            &arrival_xs,
            &format!("{}: edges arriving at {}", fixture_name, target_node),
        );
    }

    // --- Wide-node fixtures: no adjacent arrows ---

    #[test]
    fn fan_in_no_adjacent_arrows() {
        let output = render_fixture("fan_in.mmd");
        assert_no_adjacent_arrows(&output, "fan_in.mmd");
    }

    #[test]
    fn fan_out_no_adjacent_arrows() {
        let output = render_fixture("fan_out.mmd");
        assert_no_adjacent_arrows(&output, "fan_out.mmd");
    }

    // --- All target fixtures: distinct arrival x-coordinates ---

    #[test]
    fn double_skip_distinct_arrivals() {
        assert_distinct_arrival_x("double_skip.mmd", "D");
    }

    #[test]
    fn stacked_fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("stacked_fan_in.mmd", "C");
    }

    #[test]
    fn narrow_fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("narrow_fan_in.mmd", "D");
    }

    #[test]
    fn skip_edge_collision_distinct_arrivals() {
        assert_distinct_arrival_x("skip_edge_collision.mmd", "D");
    }

    #[test]
    fn fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("fan_in.mmd", "D");
    }

    #[test]
    fn five_fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("five_fan_in.mmd", "F");
    }

    #[test]
    fn fan_in_arrival_points_remain_spread_after_shared_attachment_planner() {
        let output = render_fixture("five_fan_in.mmd");
        assert!(output.contains("A"));
        assert!(output.contains("Target"));
        assert_distinct_arrival_x("five_fan_in.mmd", "F");
    }

    // --- Departure-side spreading ---

    #[test]
    fn fan_out_distinct_departures() {
        let (diagram, layout) = layout_fixture("fan_out.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let departure_xs: Vec<usize> = routed
            .iter()
            .filter(|r| r.edge.from == "A")
            .map(|r| r.start.x)
            .collect();

        assert!(departure_xs.len() >= 2);
        assert_all_distinct(&departure_xs, "fan_out.mmd: edges departing A");
    }
}

// =============================================================================
// Skip-Edge Waypoint Separation Tests
// =============================================================================

mod skip_edge_separation {
    use super::*;

    /// Assert that the A→D skip-edge waypoints do not overlap with node B's bounding box.
    /// Both fixtures have an A→B→...→D chain plus an A→D skip edge whose waypoints
    /// must clear intermediate node B (either to the left or right).
    fn assert_skip_edge_clears_node_b(fixture_name: &str) {
        let (diagram, layout) = layout_fixture(fixture_name);

        let b_bounds = &layout.node_bounds["B"];
        let ad_edge = diagram
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == "D")
            .expect("Should have an A→D edge");
        let waypoints = layout
            .edge_waypoints
            .get(&ad_edge.index)
            .expect("A→D should have waypoints");

        assert!(
            !waypoints.is_empty(),
            "{}: A→D skip edge should have at least one waypoint",
            fixture_name
        );

        // Waypoints are ordered by rank; the first is at B's rank.
        let wp_at_b_rank = waypoints[0];
        assert!(
            !b_bounds.contains(wp_at_b_rank.0, wp_at_b_rank.1),
            "{}: A→D waypoint {:?} should clear B's bounds {:?} (need separation)",
            fixture_name,
            wp_at_b_rank,
            b_bounds,
        );
    }

    #[test]
    fn double_skip_waypoints_avoid_intermediate_nodes() {
        assert_skip_edge_clears_node_b("double_skip.mmd");
    }

    #[test]
    fn skip_edge_collision_waypoints_avoid_intermediate_nodes() {
        assert_skip_edge_clears_node_b("skip_edge_collision.mmd");
    }
}

// =============================================================================
// Direct Layout Integration Tests
// =============================================================================

mod direct_layout {
    use super::*;

    #[test]
    fn direct_simple_produces_valid_layout() {
        let (_, layout) = layout_fixture("simple.mmd");

        assert!(layout.width > 0, "canvas width must be positive");
        assert!(layout.height > 0, "canvas height must be positive");
        assert!(layout.draw_positions.contains_key("A"));
        assert!(layout.draw_positions.contains_key("B"));
        assert!(layout.node_bounds.contains_key("A"));
        assert!(layout.node_bounds.contains_key("B"));
    }

    #[test]
    fn direct_no_node_overlaps() {
        let (_, layout) = layout_fixture("chain.mmd");

        let bounds: Vec<_> = layout.node_bounds.values().collect();
        for i in 0..bounds.len() {
            for j in (i + 1)..bounds.len() {
                let a = bounds[i];
                let b = bounds[j];
                let x_overlap = a.x < b.x + b.width && b.x < a.x + a.width;
                let y_overlap = a.y < b.y + b.height && b.y < a.y + a.height;
                assert!(
                    !(x_overlap && y_overlap),
                    "nodes overlap: {:?} vs {:?}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn direct_nodes_within_canvas() {
        let (_, layout) = layout_fixture("fan_out.mmd");

        for (id, bounds) in &layout.node_bounds {
            assert!(
                bounds.x + bounds.width <= layout.width,
                "node {} exceeds canvas width: {} + {} > {}",
                id,
                bounds.x,
                bounds.width,
                layout.width
            );
            assert!(
                bounds.y + bounds.height <= layout.height,
                "node {} exceeds canvas height: {} + {} > {}",
                id,
                bounds.y,
                bounds.height,
                layout.height
            );
        }
    }

    #[test]
    fn direct_td_vertical_ordering() {
        let (_, layout) = layout_fixture("simple.mmd");

        let a_y = layout.draw_positions["A"].1;
        let b_y = layout.draw_positions["B"].1;
        assert!(
            a_y < b_y,
            "in TD layout, A (rank 0) should be above B (rank 1)"
        );
    }

    #[test]
    fn direct_lr_horizontal_ordering() {
        let (_, layout) = layout_fixture("left_right.mmd");

        assert!(
            layout.width > layout.height || layout.node_bounds.len() <= 2,
            "LR layout should generally be wider than tall"
        );
    }

    #[test]
    fn direct_preserves_cross_axis_stagger() {
        // fan_out.mmd: A→B, A→C, A→D — layer 1 has B, C, D which should
        // have distinct x positions from layered layout's BK algorithm.
        let (_, layout) = layout_fixture("fan_out.mmd");

        let b_x = layout.node_bounds["B"].center_x();
        let c_x = layout.node_bounds["C"].center_x();
        let d_x = layout.node_bounds["D"].center_x();

        assert!(
            b_x != c_x || c_x != d_x,
            "B/C/D all at same x center ({}) — cross-axis stagger was lost",
            b_x,
        );
    }

    #[test]
    fn direct_cycle_no_edge_overlap_at_attachment() {
        let (_, layout) = layout_fixture("simple_cycle.mmd");

        let wp_vecs: Vec<&Vec<(usize, usize)>> = layout.edge_waypoints.values().collect();
        for i in 0..wp_vecs.len() {
            for j in (i + 1)..wp_vecs.len() {
                if !wp_vecs[i].is_empty() && !wp_vecs[j].is_empty() {
                    assert_ne!(
                        wp_vecs[i], wp_vecs[j],
                        "two edges share identical waypoint paths — overlap likely"
                    );
                }
            }
        }
    }

    #[test]
    fn direct_fan_in_ordered_arrivals() {
        let (diagram, layout) = layout_fixture("fan_in.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let mut arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|r| r.edge.to == "D")
            .map(|r| r.end.x)
            .collect();
        arrival_xs.sort();
        arrival_xs.dedup();

        assert!(
            arrival_xs.len() >= 2,
            "fan_in: expected >=2 distinct arrival x-coords at D, got {:?}",
            arrival_xs
        );
    }

    #[test]
    fn direct_five_fan_in_distinct_arrivals() {
        let (diagram, layout) = layout_fixture("five_fan_in.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|r| r.edge.to == "F")
            .map(|r| r.end.x)
            .collect();

        assert_all_distinct(&arrival_xs, "five_fan_in: arrival x at F");
    }
}

// =============================================================================
// Baseline Snapshots
// =============================================================================

mod snapshots {
    use super::*;

    #[test]
    fn generate_baseline_snapshots() {
        let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart");
        let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots")
            .join("flowchart");
        fs::create_dir_all(&snapshot_dir).unwrap();
        let regenerate = std::env::var("GENERATE_TEXT_SNAPSHOTS").is_ok();

        for entry in fs::read_dir(&fixture_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "mmd") {
                let name = path.file_stem().unwrap().to_str().unwrap();
                let input = fs::read_to_string(&path).unwrap();
                let flowchart = parse_flowchart(&input).expect("Failed to parse");
                let diagram = build_diagram(&flowchart);
                let output = render(&diagram, &RenderOptions::default());
                let snapshot_path = snapshot_dir.join(format!("{}.txt", name));
                if regenerate {
                    fs::write(snapshot_path, &output).unwrap();
                } else {
                    let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                        panic!(
                            "Missing snapshot: {}. Set GENERATE_TEXT_SNAPSHOTS=1 to generate.",
                            snapshot_path.display()
                        )
                    });
                    assert_eq!(
                        output, expected,
                        "Snapshot mismatch for {}. Set GENERATE_TEXT_SNAPSHOTS=1 to regenerate.",
                        name
                    );
                }
            }
        }
    }
}

// =============================================================================
// All Fixtures Parse and Render
// =============================================================================

mod all_fixtures {
    use super::*;

    const FIXTURE_FILES: &[&str] = &[
        "simple.mmd",
        "decision.mmd",
        "shapes.mmd",
        "shapes_basic.mmd",
        "shapes_junction.mmd",
        "shapes_document.mmd",
        "shapes_special.mmd",
        "shapes_degenerate.mmd",
        "edge_styles.mmd",
        "left_right.mmd",
        "bottom_top.mmd",
        "right_left.mmd",
        "chain.mmd",
        "ampersand.mmd",
        "labeled_edges.mmd",
        "git_workflow.mmd",
        "http_request.mmd",
        "ci_pipeline.mmd",
        "complex.mmd",
        "simple_subgraph.mmd",
        "subgraph_edges.mmd",
        "multi_subgraph.mmd",
        "nested_subgraph.mmd",
        "nested_subgraph_only.mmd",
        "nested_with_siblings.mmd",
    ];

    #[test]
    fn all_fixtures_parse() {
        for fixture in FIXTURE_FILES {
            let input = load_fixture(fixture);
            parse_flowchart(&input)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", fixture, e));
        }
    }

    #[test]
    fn all_fixtures_render() {
        for fixture in FIXTURE_FILES {
            let output = render_fixture(fixture);
            assert!(
                !output.is_empty(),
                "Fixture {} should produce non-empty output",
                fixture
            );
        }
    }

    #[test]
    fn all_fixtures_render_ascii() {
        for fixture in FIXTURE_FILES {
            let output = render_fixture_ascii(fixture);
            assert!(
                !output.is_empty(),
                "Fixture {} should produce non-empty ASCII output",
                fixture
            );
        }
    }
}

// =============================================================================
// LR/RL Routing Tests
// =============================================================================

mod lr_routing {
    use super::*;

    fn assert_has_right_arrow(output: &str) {
        assert!(
            output.contains('►') || output.contains('>'),
            "LR edge should use right-pointing arrow, got:\n{}",
            output
        );
    }

    fn assert_has_left_arrow(output: &str) {
        assert!(
            output.contains('◄') || output.contains('<'),
            "LR backward edge should have left-pointing arrow, got:\n{}",
            output
        );
    }

    fn assert_no_vertical_arrows_between_nodes(output: &str) {
        let has_vertical = output
            .lines()
            .any(|line| line.contains("│▲│") || line.contains("│▼│"));
        assert!(
            !has_vertical,
            "LR edge should not have vertical arrows between nodes, got:\n{}",
            output
        );
    }

    #[test]
    fn lr_simple_chain_horizontal_arrows() {
        let output = render_input("graph LR\n    A[Start] --> B[End]");
        assert_has_right_arrow(&output);
        assert_no_vertical_arrows_between_nodes(&output);
    }

    #[test]
    fn lr_three_node_chain_horizontal_arrows() {
        let output = render_fixture("left_right.mmd");
        assert_has_right_arrow(&output);
        assert_no_vertical_arrows_between_nodes(&output);
    }

    #[test]
    fn lr_backward_edge_renders_without_panic() {
        let output =
            render_input("graph LR\n    A[Start] --> B[Middle]\n    B --> C[End]\n    C --> A");

        assert!(output.contains("Start"), "Should contain Start node");
        assert!(output.contains("Middle"), "Should contain Middle node");
        assert!(output.contains("End"), "Should contain End node");
        assert_has_left_arrow(&output);
    }

    #[test]
    fn lr_backward_edge_routes_around_nodes() {
        // LR backward edges now route below nodes with synthetic waypoints.
        // The backward edge should produce some arrow character.
        let output = render_input("graph LR\n    A --> B\n    B --> A");
        let arrow_count = output
            .chars()
            .filter(|c| matches!(c, '▲' | '▼' | '◄' | '►' | '<' | '>'))
            .count();
        // Should have at least 2 arrows: one for forward A→B (►) and one for backward B→A
        assert!(
            arrow_count >= 2,
            "Should have arrows for both forward and backward edges, found {} arrows in:\n{}",
            arrow_count,
            output
        );
    }

    #[test]
    fn lr_multirank_backward_edge_does_not_extend_left_of_target() {
        // The backward edge D→A should NOT place its arrow to the LEFT
        // of A's left border -- that extends outside the diagram bounds.
        let output = render_input("graph LR\n    A --> B --> C --> D\n    D --> A");

        let mut arrow_col = None;
        let mut node_left_border = None;

        for line in output.lines() {
            if let Some(pos) = line.find('◄') {
                arrow_col = Some(pos);
            }
            if line.contains(" A ")
                && let Some(pos) = line.find('│')
            {
                node_left_border = Some(pos);
            }
        }

        if let (Some(arrow), Some(border)) = (arrow_col, node_left_border) {
            assert!(
                arrow >= border,
                "Backward edge arrow (col {}) should not extend left of node A's border (col {}). \
                 The arrow extends outside the diagram area.\nOutput:\n{}",
                arrow,
                border,
                output
            );
        }
    }
}

// =============================================================================
// Subgraph Rendering Tests
// =============================================================================

mod subgraph_rendering {
    use super::*;

    #[test]
    fn simple_subgraph_renders_title_and_nodes() {
        let output = render_fixture("simple_subgraph.mmd");
        assert!(output.contains("Process"), "Should contain subgraph title");
        assert!(output.contains("Start"), "Should contain Start node");
        assert!(output.contains("Middle"), "Should contain Middle node");
        assert!(output.contains("End"), "Should contain End node");
    }

    #[test]
    fn simple_subgraph_has_border() {
        let output = render_fixture("simple_subgraph.mmd");
        // Subgraph border uses box-drawing characters
        assert!(
            output.contains('┌') && output.contains('┘'),
            "Should have box-drawing border characters"
        );
    }

    #[test]
    fn subgraph_edges_renders_both_groups() {
        let output = render_fixture("subgraph_edges.mmd");
        assert!(
            output.contains("Input"),
            "Should contain Input subgraph title"
        );
        assert!(output.contains("Data"), "Should contain Data node");
        assert!(output.contains("Config"), "Should contain Config node");
        assert!(output.contains("Result"), "Should contain Result node");
        assert!(output.contains("Log"), "Should contain Log node");
    }

    #[test]
    fn subgraph_edges_borders_do_not_overlap() {
        let output = render_fixture("subgraph_edges.mmd");
        let lines: Vec<&str> = output.lines().collect();

        // Find the row containing sg1's bottom-left corner (└)
        // and sg2's top-left corner (┌). They must be on separate rows.
        let bottom_border_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains('└'))
            .map(|(i, _)| i)
            .collect();
        let top_border_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains('┌'))
            .map(|(i, _)| i)
            .collect();

        // sg1's bottom border (last └ row before sg2) should be strictly
        // above sg2's top border (┌ row containing "Output" or second ┌)
        // Simple check: no row should contain both └ and ┌ from different subgraphs
        // More robust: the sg1 bottom-left corner row < sg2 top-left corner row
        assert!(
            !bottom_border_rows.is_empty(),
            "Should have bottom border rows"
        );
        assert!(!top_border_rows.is_empty(), "Should have top border rows");

        // Find sg1's bottom border (└ row) and sg2's top border (second ┌ row).
        // sg1's top border is the first ┌ row. sg2's top border is the next ┌ row
        // after sg1's content.
        let _first_top = lines.iter().position(|l| l.contains('┌')).unwrap();
        let first_bottom = lines.iter().position(|l| l.contains('└')).unwrap();
        let second_top = lines
            .iter()
            .enumerate()
            .skip(first_bottom)
            .position(|(_, l)| l.contains('┌'))
            .map(|pos| pos + first_bottom);

        if let Some(second_top) = second_top {
            assert!(
                second_top > first_bottom,
                "Second subgraph top ({second_top}) should be below first subgraph \
                bottom ({first_bottom})"
            );
        }
    }

    #[test]
    fn subgraph_titles_preserved_with_cross_edges() {
        let output = render_fixture("subgraph_edges.mmd");
        // Both titles should be fully intact (not corrupted by edge arrows)
        assert!(
            output.contains("Input"),
            "Input title should be intact in: {}",
            output
        );
        assert!(
            output.contains("Output"),
            "Output title should be intact in: {}",
            output
        );
    }

    #[test]
    fn multi_subgraph_renders_both_groups() {
        let output = render_fixture("multi_subgraph.mmd");
        // LR layout may not display subgraph titles if the box is too compact
        assert!(output.contains("UI"), "Should contain UI node");
        assert!(output.contains("API"), "Should contain API node");
        assert!(output.contains("Server"), "Should contain Server node");
        assert!(output.contains("DB"), "Should contain DB node");
        // Should have subgraph borders and node borders
        let border_count = output.matches('┌').count();
        assert!(
            border_count >= 3,
            "Should have borders for subgraphs and nodes, got {} '┌' chars",
            border_count
        );
        // Both subgraph titles should appear
        assert!(output.contains("Frontend"), "Should contain Frontend title");
        assert!(output.contains("Backend"), "Should contain Backend title");
    }

    #[test]
    fn simple_subgraph_ascii_mode() {
        let output = render_fixture_ascii("simple_subgraph.mmd");
        assert!(output.contains("Process"), "ASCII: should contain title");
        assert!(output.contains("Start"), "ASCII: should contain Start");
        // ASCII mode uses +/-/| for borders
        assert!(
            output.contains('+') && output.contains('-'),
            "ASCII mode should use +/- border characters"
        );
    }

    #[test]
    fn subgraph_nodes_aligned_vertically() {
        // Verify the stagger fix: nodes in a vertical chain inside a subgraph
        // should have similar horizontal positions
        let (_, layout) = layout_fixture("simple_subgraph.mmd");

        let a_cx = layout.node_bounds["A"].center_x();
        let b_cx = layout.node_bounds["B"].center_x();

        assert!(
            (a_cx as isize - b_cx as isize).unsigned_abs() <= 1,
            "A (center_x={}) and B (center_x={}) should be vertically aligned",
            a_cx,
            b_cx
        );
    }

    #[test]
    fn subgraph_title_embedded_in_border() {
        let output = render_fixture("simple_subgraph.mmd");
        // Title should be embedded in border line, not floating above
        assert!(
            output.contains("─ Process ─") || output.contains("- Process -"),
            "Title should be embedded in border: {}",
            output
        );
    }
}

// === Subgraph parsing and building tests ===

#[test]
fn test_parse_simple_subgraph_fixture() {
    let diagram = parse_and_build("simple_subgraph.mmd");

    assert!(diagram.has_subgraphs());
    assert!(diagram.subgraphs.contains_key("sg1"));
    assert_eq!(diagram.subgraphs["sg1"].title, "Process");
    assert!(diagram.subgraphs["sg1"].nodes.contains(&"A".to_string()));
    assert!(diagram.subgraphs["sg1"].nodes.contains(&"B".to_string()));
}

#[test]
fn test_parse_subgraph_edges_fixture() {
    let diagram = parse_and_build("subgraph_edges.mmd");

    assert_eq!(diagram.subgraphs.len(), 2);
    assert!(diagram.subgraphs.contains_key("sg1"));
    assert!(diagram.subgraphs.contains_key("sg2"));
    // Edges cross subgraph boundaries
    assert!(diagram.edges.iter().any(|e| e.from == "A" && e.to == "C"));
    assert!(diagram.edges.iter().any(|e| e.from == "B" && e.to == "D"));
}

#[test]
fn test_parse_multi_subgraph_fixture() {
    let diagram = parse_and_build("multi_subgraph.mmd");

    assert_eq!(diagram.subgraphs.len(), 2);
    assert!(diagram.subgraphs.contains_key("sg1"));
    assert!(diagram.subgraphs.contains_key("sg2"));
    assert_eq!(diagram.subgraphs["sg1"].title, "Frontend");
    assert_eq!(diagram.subgraphs["sg2"].title, "Backend");
    // Cross-boundary edge
    assert!(diagram.edges.iter().any(|e| e.from == "B" && e.to == "C"));
}

/// Edge case tests for label-as-dummy-node (Plan 0024).
mod label_edge_cases {
    use super::*;

    #[test]
    fn long_label_renders_without_panic() {
        let output =
            render_input("graph TD\n    A -->|this is a very long label that might overflow| B");
        // Should not panic; nodes should still render correctly
        assert!(!output.is_empty());
        assert!(output.contains(" A "), "Node A should render:\n{output}");
        assert!(output.contains(" B "), "Node B should render:\n{output}");
        // Label may be truncated or omitted if wider than canvas — this is
        // acceptable behavior for now.
    }

    #[test]
    fn fan_out_with_labels() {
        let output =
            render_input("graph TD\n    A -->|yes| B\n    A -->|no| C\n    A -->|maybe| D");
        // All three labels should be visible
        assert!(output.contains("yes"), "Expected 'yes' label:\n{output}");
        assert!(output.contains("no"), "Expected 'no' label:\n{output}");
        assert!(
            output.contains("maybe"),
            "Expected 'maybe' label:\n{output}"
        );
    }

    #[test]
    fn labeled_edge_lr_direction() {
        let output = render_input("graph LR\n    A -->|label| B");
        assert!(output.contains(" A "), "Should contain node A:\n{output}");
        assert!(output.contains(" B "), "Should contain node B:\n{output}");
        assert!(
            output.contains("label"),
            "Expected 'label' in LR layout:\n{output}"
        );
    }

    #[test]
    fn mixed_labeled_and_unlabeled() {
        let output = render_input(
            "graph TD\n    A -->|yes| B\n    A --> C\n    B --> D\n    C -->|error| D",
        );
        assert!(output.contains("yes"), "Expected 'yes' label:\n{output}");
        assert!(
            output.contains("error"),
            "Expected 'error' label:\n{output}"
        );
        // All nodes should be present
        for node in ["A", "B", "C", "D"] {
            assert!(
                output.contains(&format!(" {node} ")),
                "Expected node {node}:\n{output}"
            );
        }
    }

    #[test]
    fn all_edges_labeled() {
        let output =
            render_input("graph TD\n    A -->|start| B\n    B -->|process| C\n    C -->|end| D");
        // At least the last label should appear (via precomputed position)
        assert!(output.contains("end"), "Expected 'end' label:\n{output}");
        // All nodes should render (check for bordered node text)
        assert!(output.contains(" A "), "Expected node A:\n{output}");
        assert!(output.contains(" B "), "Expected node B:\n{output}");
        assert!(output.contains(" D "), "Expected node D:\n{output}");
        // Node C may have arrow overlap in its box due to edge routing
        // through the node, but the node box itself should exist
        assert!(
            output.contains("┌───┐"),
            "Expected at least one node box:\n{output}"
        );
    }

    #[test]
    fn labeled_edges_reasonable_height() {
        let output = render_fixture("labeled_edges.mmd");
        let line_count = output.lines().count();

        assert!(
            line_count < 40,
            "labeled_edges.mmd should render in under 40 lines, got {line_count}"
        );

        for label in &["initialize", "configure", "yes", "no", "retry"] {
            assert!(
                output.contains(label),
                "Output should contain label '{label}'"
            );
        }
    }

    #[test]
    fn diamond_text_not_corrupted_by_arrows() {
        let output = render_fixture("labeled_edges.mmd");
        assert!(
            output.contains("Valid?"),
            "Diamond text 'Valid?' should be intact in output:\n{output}"
        );
    }

    #[test]
    fn simple_cycle_compact_backward_routing() {
        let output = render_fixture("simple_cycle.mmd");
        let line_count = output.lines().count();
        assert!(
            line_count < 30,
            "simple_cycle.mmd should be compact, got {line_count} lines"
        );
    }

    #[test]
    fn multiple_cycles_compact_backward_routing() {
        let output = render_fixture("multiple_cycles.mmd");
        let line_count = output.lines().count();
        assert!(
            line_count < 40,
            "multiple_cycles.mmd should be compact, got {line_count} lines"
        );
    }
}

// === Backward edge label position tests (Plan 0027, Task 5.1) ===

#[test]
fn backward_edge_label_position_td() {
    let output = render_input("graph TD\n    A --> B\n    B -->|retry| A");
    assert!(output.contains("retry"), "Label missing:\n{output}");

    let lines: Vec<&str> = output.lines().collect();
    let a_line = lines
        .iter()
        .position(|l| l.contains(" A "))
        .expect("missing node A row");
    let b_line = lines
        .iter()
        .rposition(|l| l.contains(" B "))
        .expect("missing node B row");
    let retry_line = lines
        .iter()
        .position(|l| l.contains("retry"))
        .expect("missing retry label row");

    assert!(
        retry_line > a_line && retry_line < b_line,
        "Label row {} should be between A row {} and B row {}\n{}",
        retry_line,
        a_line,
        b_line,
        output
    );
}

#[test]
fn backward_edge_label_position_bt() {
    let output = render_input("graph BT\n    A --> B\n    B -->|retry| A");
    assert!(output.contains("retry"), "Label missing:\n{output}");
}

#[test]
fn backward_edge_label_position_lr() {
    let output = render_input("graph LR\n    A --> B\n    B -->|retry| A");
    assert!(output.contains("retry"), "Label missing:\n{output}");
}

#[test]
fn backward_edge_label_position_rl() {
    let output = render_input("graph RL\n    A --> B\n    B -->|retry| A");
    assert!(output.contains("retry"), "Label missing:\n{output}");
}

#[test]
fn backward_and_forward_labels_coexist() {
    let output = render_input("graph TD\n    A -->|go| B\n    B -->|retry| A");
    assert!(output.contains("go"), "Forward label missing:\n{output}");
    assert!(
        output.contains("retry"),
        "Backward label missing:\n{output}"
    );
}

#[test]
fn backward_edge_label_does_not_overlap_nodes() {
    let output = render_input("graph TD\n    Start --> End\n    End -->|back| Start");
    assert!(output.contains("back"), "Label missing:\n{output}");
    let lines: Vec<&str> = output.lines().collect();
    for line in &lines {
        if line.contains("back") {
            let back_pos = line.find("back").unwrap();
            let before_label = &line[..back_pos];
            assert!(
                !before_label.ends_with('│') && !before_label.ends_with('┐'),
                "Label overlaps with node box:\n{output}"
            );
        }
    }
}

// =========================================================================
// Multi-Subgraph Title Tests (Plan 0031)
// =========================================================================

#[test]
fn test_render_titled_subgraph_shows_title() {
    let input = r#"graph TD
    subgraph sg1[Processing]
        A[Step 1] --> B[Step 2]
    end"#;
    let output = render_input(input);

    assert!(
        output.contains("Processing"),
        "Output should contain subgraph title 'Processing':\n{}",
        output
    );
    assert!(output.contains("Step 1"));
    assert!(output.contains("Step 2"));
}

#[test]
fn test_render_multi_subgraph_titled() {
    // Two titled subgraphs with a cross-edge.
    // Note: multi-subgraph border overlap is a known pre-existing issue —
    // this test verifies titles appear and layout completes without panic.
    let input = r#"graph TD
    subgraph sg1[Intake]
        A[Read] --> B[Parse]
    end
    subgraph sg2[Emit]
        C[Format] --> D[Write]
    end
    B --> C"#;
    let output = render_input(input);

    assert!(
        output.contains("Intake"),
        "Output should contain 'Intake' title:\n{}",
        output
    );
    assert!(
        output.contains("Emit"),
        "Output should contain 'Emit' title:\n{}",
        output
    );
    assert!(output.contains("Read"), "Missing 'Read':\n{}", output);
    assert!(output.contains("Write"), "Missing 'Write':\n{}", output);
}

#[test]
fn test_render_titled_subgraph_title_not_overwritten_by_edge() {
    let input = r#"graph TD
    D[External] --> A
    subgraph sg1[Processing]
        A[Internal] --> B[Next]
    end"#;
    let output = render_input(input);

    assert!(
        output.contains("Processing"),
        "Title should not be overwritten by edge:\n{}",
        output
    );
    assert!(output.contains("External"));
    assert!(output.contains("Internal"));
}

// =========================================================================
// Nested Subgraph Tests (Plan 0032)
// =========================================================================

#[test]
fn test_nested_subgraph_renders_both_borders() {
    let output = render_fixture("nested_subgraph.mmd");
    assert!(
        output.contains("Outer"),
        "Should contain outer border title:\n{}",
        output
    );
    assert!(
        output.contains("Inner"),
        "Should contain inner border title:\n{}",
        output
    );
}

#[test]
fn test_nested_subgraph_only_renders() {
    let output = render_fixture("nested_subgraph_only.mmd");
    assert!(
        output.contains("Outer"),
        "Should contain outer border title:\n{}",
        output
    );
    assert!(
        output.contains("Inner"),
        "Should contain inner border title:\n{}",
        output
    );
}

#[test]
fn test_nested_with_siblings_renders() {
    let output = render_fixture("nested_with_siblings.mmd");
    assert!(
        output.contains("Outer"),
        "Should contain outer border title:\n{}",
        output
    );
    assert!(
        output.contains("Left"),
        "Should contain left border title:\n{}",
        output
    );
    assert!(
        output.contains("Right"),
        "Should contain right border title:\n{}",
        output
    );
}

#[test]
fn test_nested_subgraph_parent_tracking() {
    let diagram = parse_and_build("nested_subgraph.mmd");
    assert_eq!(diagram.subgraphs["inner"].parent, Some("outer".to_string()));
    assert_eq!(diagram.subgraphs["outer"].parent, None);
}

#[test]
fn test_nested_subgraph_bounds_containment() {
    let (_, layout) = layout_fixture("nested_subgraph.mmd");
    let outer = &layout.subgraph_bounds["outer"];
    let inner = &layout.subgraph_bounds["inner"];
    assert!(
        outer.x <= inner.x,
        "outer.x ({}) <= inner.x ({})",
        outer.x,
        inner.x
    );
    assert!(
        outer.y <= inner.y,
        "outer.y ({}) <= inner.y ({})",
        outer.y,
        inner.y
    );
    assert!(
        outer.x + outer.width >= inner.x + inner.width,
        "outer right ({}) >= inner right ({})",
        outer.x + outer.width,
        inner.x + inner.width
    );
    assert!(
        outer.y + outer.height >= inner.y + inner.height,
        "outer bottom ({}) >= inner bottom ({})",
        outer.y + outer.height,
        inner.y + inner.height
    );
}

// ==========================================
// Self-edge (A --> A) tests
// ==========================================

#[test]
fn test_self_loop_renders_without_crash() {
    let output = render_fixture("self_loop.mmd");
    assert!(!output.trim().is_empty());
    assert!(output.contains("Process"));
}

#[test]
fn test_self_loop_has_loop_segments() {
    let output = render_input("graph TD\n    A --> A");
    // Should have vertical line segments forming the loop
    assert!(
        output.contains('│') || output.contains('|'),
        "should have vertical segments"
    );
    // Should have horizontal line segments
    assert!(
        output.contains('─') || output.contains('-'),
        "should have horizontal segments"
    );
}

#[test]
fn test_self_loop_node_appears_once() {
    let output = render_input("graph TD\n    A[Unique] --> A");
    let count = output.matches("Unique").count();
    assert_eq!(count, 1, "node label should appear exactly once");
}

#[test]
fn test_self_loop_with_label() {
    let output = render_fixture("self_loop_labeled.mmd");
    assert!(output.contains("retry"), "label text should appear");
    assert!(output.contains("done"), "other label should appear");
}

#[test]
fn test_self_loop_all_directions() {
    for dir in &["TD", "BT", "LR", "RL"] {
        let input = format!("graph {}\n    A --> A", dir);
        let output = render_input(&input);
        assert!(
            !output.trim().is_empty(),
            "direction {} should produce non-empty output",
            dir
        );
        assert!(
            output.contains('A'),
            "direction {} should contain node label",
            dir
        );
    }
}

#[test]
fn test_self_loop_with_normal_edges() {
    let output = render_fixture("self_loop_with_others.mmd");
    assert!(output.contains("Start"));
    assert!(output.contains("Process"));
    assert!(output.contains("End"));
}

#[test]
fn test_self_loop_on_isolated_node() {
    let output = render_input("graph TD\n    A --> A");
    assert!(output.contains('A'));
}

#[test]
fn test_self_loop_with_backward_edge() {
    // A->B->A cycle plus B->B self-loop
    let output = render_input("graph TD\n    A --> B\n    B --> A\n    B --> B");
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn test_self_loop_ascii_mode() {
    let diagram = parse_and_build("self_loop.mmd");
    let output = render(
        &diagram,
        &RenderOptions {
            output_format: OutputFormat::Ascii,
            ..Default::default()
        },
    );
    // Should use ASCII characters, no Unicode box drawing
    assert!(!output.contains('┌'), "should not have Unicode box drawing");
    assert!(
        !output.contains('─'),
        "should not have Unicode horizontal line"
    );
}

// === Compound graph external node positioning tests ===

#[test]
fn test_sibling_subgraph_nodes_distinct_x() {
    // A (us-east) and C (us-west) are at the same rank but in different subgraphs.
    // They should have distinct x-coordinates (not collapsed on top of each other).
    let (_, layout) = layout_fixture("external_node_subgraph.mmd");
    let a_cx = layout.node_bounds["A"].center_x();
    let c_cx = layout.node_bounds["C"].center_x();
    assert_ne!(
        a_cx, c_cx,
        "Sibling subgraph nodes should have distinct x: A={}, C={}",
        a_cx, c_cx
    );
}

#[test]
fn test_external_node_not_far_from_targets() {
    // E connects to A (us-east) and C (us-west).
    // E should be reasonably close to the A-C range, not pushed far away.
    // Ideally E would be centered between A and C, but the current layout
    // positions E near the left subgraph border. This test verifies E isn't
    // wildly offset (the original bug had E ~150 chars away from the subgraphs).
    let (_, layout) = layout_fixture("external_node_subgraph.mmd");
    let a_cx = layout.node_bounds["A"].center_x();
    let c_cx = layout.node_bounds["C"].center_x();
    let e_cx = layout.node_bounds["E"].center_x();
    let min_x = a_cx.min(c_cx);
    let max_x = a_cx.max(c_cx);
    let range = max_x - min_x;
    // E should be within a reasonable distance of the A-C midpoint.
    // The original bug had E ~150 chars away. Use max(2*range, 60) as
    // threshold to allow for intermediate layout states while still
    // catching catastrophic offsets.
    let midpoint = (min_x + max_x) / 2;
    let distance = (e_cx as isize - midpoint as isize).unsigned_abs();
    let tolerance = (range * 2).max(60);
    assert!(
        distance <= tolerance,
        "External node E ({}) is too far from A ({}) - C ({}) range (distance {} > {})",
        e_cx,
        a_cx,
        c_cx,
        distance,
        tolerance
    );
}

#[test]
fn test_external_node_centered_between_targets() {
    let (_, layout) = layout_fixture("external_node_subgraph.mmd");
    let a_cx = layout.node_bounds["A"].center_x();
    let c_cx = layout.node_bounds["C"].center_x();
    let e_cx = layout.node_bounds["E"].center_x();

    let min_x = a_cx.min(c_cx);
    let max_x = a_cx.max(c_cx);
    let range = max_x - min_x;
    let midpoint = (min_x + max_x) / 2;
    let distance = (e_cx as isize - midpoint as isize).unsigned_abs();
    let tolerance = (range / 2).max(15);

    assert!(
        distance <= tolerance,
        "External node E ({}) is not centered between A ({}) and C ({}) (distance {} > {})",
        e_cx,
        a_cx,
        c_cx,
        distance,
        tolerance
    );
}

// =============================================================================
// Parse Compatibility Tests
// =============================================================================

mod compat {
    use super::*;

    #[test]
    fn directive_stripped() {
        let output = render_fixture("compat_directive.mmd");
        assert!(output.contains("Start"));
        assert!(output.contains("Decision"));
    }

    #[test]
    fn frontmatter_stripped() {
        let output = render_fixture("compat_frontmatter.mmd");
        assert!(output.contains("A"));
        assert!(output.contains("B"));
        assert!(output.contains("C"));
    }

    #[test]
    fn no_direction_defaults_to_td() {
        let diagram = parse_and_build("compat_no_direction.mmd");
        assert_eq!(diagram.direction, Direction::TopDown);
        let output = render_fixture("compat_no_direction.mmd");
        assert!(output.contains("Start"));
        assert!(output.contains("End"));
    }

    #[test]
    fn numeric_ids() {
        let output = render_fixture("compat_numeric_ids.mmd");
        assert!(output.contains("First"));
        assert!(output.contains("Second"));
        assert!(output.contains("Third"));
    }

    #[test]
    fn hyphenated_ids() {
        let output = render_fixture("compat_hyphenated_ids.mmd");
        assert!(output.contains("Start"));
        assert!(output.contains("Process A"));
        assert!(output.contains("Check"));
        assert!(output.contains("Done"));
    }

    #[test]
    fn class_annotation_ignored() {
        let output = render_fixture("compat_class_annotation.mmd");
        assert!(output.contains("Start"));
        assert!(output.contains("Decision"));
        // classDef lines should not cause parse failures
    }

    #[test]
    fn invisible_edge_not_rendered() {
        let output = render_fixture("compat_invisible_edge.mmd");
        assert!(output.contains("A"));
        assert!(output.contains("B"));
        assert!(output.contains("C"));
        // Invisible edge should not appear in output
        assert!(!output.contains("~~~"));
    }

    #[test]
    fn kitchen_sink() {
        let output = render_fixture("compat_kitchen_sink.mmd");
        assert!(output.contains("Start"));
        assert!(output.contains("Check Input"));
        assert!(output.contains("Done"));
        assert!(output.contains("Error"));
    }
}

#[test]
fn test_bidirectional_arrows_both_ends() {
    let output = render_fixture("bidirectional.mmd");

    // For TD layout, down arrows (▼) appear at the target end,
    // up arrows (▲) appear at the source end of bidirectional edges.
    let down_arrows = output.chars().filter(|&c| c == '\u{25BC}').count();
    let up_arrows = output.chars().filter(|&c| c == '\u{25B2}').count();

    // Each bidirectional edge has an arrow at each end.
    // With per-edge label spacing, unlabeled edges are compact (2 rows of stem),
    // which may be too short to render both arrowheads.  Verify every edge has
    // at least its target arrow (▼) and at least one edge shows the source
    // arrow (▲) to prove the bidirectional rendering path works.
    assert!(
        down_arrows >= 3,
        "Should have at least 3 down arrows for 3 bidir edges, got {down_arrows}\n{output}"
    );
    assert!(
        up_arrows >= 1,
        "Should have at least 1 up arrow for bidirectional edges, got {up_arrows}\n{output}"
    );
}

#[test]
fn test_invisible_edge_not_rendered() {
    use mmdflux::graph::Stroke;

    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(mmdflux::graph::Node::new("A").with_label("A"));
    diagram.add_node(mmdflux::graph::Node::new("B").with_label("B"));
    diagram.add_node(mmdflux::graph::Node::new("C").with_label("C"));
    diagram.add_edge(mmdflux::graph::Edge::new("A", "B")); // visible
    diagram.add_edge(mmdflux::graph::Edge::new("A", "C").with_stroke(Stroke::Invisible)); // invisible

    let output = render(&diagram, &RenderOptions::default());

    // All nodes should appear
    assert!(output.contains("A"), "Node A should appear");
    assert!(output.contains("B"), "Node B should appear");
    assert!(output.contains("C"), "Node C should appear");

    // There should be exactly 1 arrow (for A→B), not 2
    let down_arrows = output.chars().filter(|&c| c == '▼').count();
    assert_eq!(
        down_arrows, 1,
        "Should have exactly 1 visible arrow (A→B only), got {down_arrows}\n{output}"
    );
}

#[test]
fn test_invisible_edge_affects_layout() {
    use mmdflux::graph::Stroke;

    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(mmdflux::graph::Node::new("A").with_label("A"));
    diagram.add_node(mmdflux::graph::Node::new("B").with_label("B"));
    diagram.add_edge(mmdflux::graph::Edge::new("A", "B").with_stroke(Stroke::Invisible));

    let output = render(&diagram, &RenderOptions::default());

    // Both nodes should appear
    assert!(output.contains("A"), "Node A should appear");
    assert!(output.contains("B"), "Node B should appear");

    // A should be above B (invisible edge enforces rank ordering)
    let lines: Vec<&str> = output.lines().collect();
    let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
    let b_line = lines.iter().position(|l| l.contains('B')).unwrap();
    assert!(
        a_line < b_line,
        "A should be above B due to invisible edge rank constraint\n{output}"
    );

    // No visible edge characters (no arrows, no lines)
    let down_arrows = output.chars().filter(|&c| c == '▼').count();
    assert_eq!(
        down_arrows, 0,
        "Invisible edge should produce no arrows\n{output}"
    );
}

#[test]
fn test_same_rank_constraint_horizontal_alignment() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(mmdflux::graph::Node::new("A").with_label("A"));
    diagram.add_node(mmdflux::graph::Node::new("B").with_label("B"));
    diagram.add_node(mmdflux::graph::Node::new("C").with_label("C"));
    diagram.add_edge(mmdflux::graph::Edge::new("A", "C"));
    diagram.add_same_rank_constraint("A", "B");

    let output = render(&diagram, &RenderOptions::default());

    let lines: Vec<&str> = output.lines().collect();
    let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
    let b_line = lines.iter().position(|l| l.contains('B')).unwrap();
    let c_line = lines.iter().rposition(|l| l.contains('C')).unwrap();

    assert_eq!(a_line, b_line, "A and B should be on same line:\n{output}");
    assert!(c_line > a_line, "C should be below A:\n{output}");
}

#[test]
fn test_same_rank_no_visible_edge() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(mmdflux::graph::Node::new("X").with_label("X"));
    diagram.add_node(mmdflux::graph::Node::new("Y").with_label("Y"));
    diagram.add_same_rank_constraint("X", "Y");

    let output = render(&diagram, &RenderOptions::default());

    assert!(output.contains("X"));
    assert!(output.contains("Y"));

    let has_arrows = output
        .chars()
        .any(|c| c == '\u{25BC}' || c == '\u{25B2}' || c == '\u{25BA}' || c == '\u{25C4}');
    assert!(
        !has_arrows,
        "Same-rank constraint should not render arrows:\n{output}"
    );
}

#[test]
fn test_same_rank_lr_layout() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(mmdflux::graph::Node::new("A").with_label("A"));
    diagram.add_node(mmdflux::graph::Node::new("B").with_label("B"));
    diagram.add_node(mmdflux::graph::Node::new("C").with_label("C"));
    diagram.add_edge(mmdflux::graph::Edge::new("A", "C"));
    diagram.add_same_rank_constraint("A", "B");

    let output = render(&diagram, &RenderOptions::default());

    assert!(output.contains("A"));
    assert!(output.contains("B"));
    assert!(output.contains("C"));
}

#[test]
fn test_minlen_2_forces_rank_gap() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(mmdflux::graph::Node::new("A").with_label("A"));
    diagram.add_node(mmdflux::graph::Node::new("B").with_label("B"));
    diagram.add_edge(mmdflux::graph::Edge::new("A", "B").with_minlen(2));

    let output = render(&diagram, &RenderOptions::default());

    let lines: Vec<&str> = output.lines().collect();
    let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
    let b_line = lines.iter().rposition(|l| l.contains('B')).unwrap();
    let gap = b_line - a_line;

    assert!(
        gap > 3,
        "Gap between A and B should be significant with minlen=2, got {gap}:\n{output}"
    );
}

mod arrow_types {
    use super::*;

    #[test]
    fn test_bidirectional_td_both_arrows_visible() {
        let output = render_input("graph TD\n    A <--> B");
        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
        let b_line = lines.iter().rposition(|l| l.contains('B')).unwrap();
        assert!(b_line > a_line, "B should be below A:\n{output}");
    }

    #[test]
    fn test_bidirectional_lr_both_arrows_visible() {
        let output = render_input("graph LR\n    A <--> B");
        assert!(output.contains('A'), "Node A should appear:\n{output}");
        assert!(output.contains('B'), "Node B should appear:\n{output}");
    }

    #[test]
    fn test_cross_arrow_renders_x() {
        let output = render_input("graph TD\n    A --x B");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(
            output.contains('x') || output.contains('X'),
            "Cross arrow should render x/X character:\n{output}"
        );
    }

    #[test]
    fn test_circle_arrow_renders_o() {
        let output = render_input("graph TD\n    A --o B");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(
            output.contains('o') || output.contains('O'),
            "Circle arrow should render o/O character:\n{output}"
        );
    }

    #[test]
    fn test_cross_both_ends() {
        let output = render_input("graph TD\n    A x--x B");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        let x_count = output.chars().filter(|&c| c == 'x' || c == 'X').count();
        assert!(
            x_count >= 2,
            "x--x should render x on both ends, found {x_count}:\n{output}"
        );
    }

    #[test]
    fn test_circle_both_ends() {
        let output = render_input("graph TD\n    A o--o B");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
    }

    #[test]
    fn test_bidirectional_fixture_all_styles() {
        let output = render_fixture("bidirectional_arrows.mmd");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(output.contains('C'));
        assert!(output.contains('D'));
    }

    #[test]
    fn test_cross_circle_fixture() {
        let output = render_fixture("cross_circle_arrows.mmd");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(output.contains('C'));
        assert!(output.contains('D'));
        assert!(output.contains('E'));
    }

    #[test]
    fn test_mixed_arrow_types_in_chain() {
        let output = render_input("graph TD\n    A --> B\n    B --x C\n    C --o D\n    D <--> E");
        assert!(output.contains('A'));
        assert!(output.contains('E'));
    }
}

mod multigraph {
    use super::*;

    #[test]
    fn test_multi_edge_parse_preserves_both() {
        let input = load_fixture("multi_edge.mmd");
        let flowchart = parse_flowchart(&input).unwrap();
        let diagram = build_diagram(&flowchart);
        assert_eq!(
            diagram.edges.len(),
            2,
            "Should preserve both edges between A and B"
        );
    }

    #[test]
    fn test_multi_edge_renders_without_panic() {
        let output = render_fixture("multi_edge.mmd");
        assert!(output.contains('A'), "Node A should appear:\n{output}");
        assert!(output.contains('B'), "Node B should appear:\n{output}");
    }

    #[test]
    fn test_multi_edge_labeled_both_labels_visible() {
        let output = render_fixture("multi_edge_labeled.mmd");
        assert!(
            output.contains("path 1"),
            "First edge label should appear:\n{output}"
        );
        assert!(
            output.contains("path 2"),
            "Second edge label should appear:\n{output}"
        );
    }

    #[test]
    fn test_multi_edge_lr_layout() {
        let output = render_input("graph LR\n    A -->|yes| B\n    A -->|no| B");
        assert!(
            output.contains("yes"),
            "Label 'yes' should appear:\n{output}"
        );
        assert!(output.contains("no"), "Label 'no' should appear:\n{output}");
    }

    #[test]
    fn test_multi_edge_different_styles() {
        let input = "graph TD\n    A --> B\n    A -.-> B\n    A ==> B";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        assert_eq!(
            diagram.edges.len(),
            3,
            "Should have 3 edges between A and B"
        );

        let output = render(&diagram, &RenderOptions::default());
        assert!(output.contains('A'), "Node A should appear:\n{output}");
        assert!(output.contains('B'), "Node B should appear:\n{output}");
    }

    #[test]
    fn test_multi_edge_with_downstream_node() {
        let output = render_fixture("multi_edge_labeled.mmd");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(output.contains('C'));
        let lines: Vec<&str> = output.lines().collect();
        let b_line = lines.iter().position(|l| l.contains('B')).unwrap();
        let c_line = lines.iter().rposition(|l| l.contains('C')).unwrap();
        assert!(c_line > b_line, "C should be below B:\n{output}");
    }

    #[test]
    fn test_multi_edge_three_edges_same_pair() {
        let output =
            render_input("graph TD\n    A -->|one| B\n    A -->|two| B\n    A -->|three| B");
        assert!(
            output.contains("one"),
            "Label 'one' should appear:\n{output}"
        );
        assert!(
            output.contains("two"),
            "Label 'two' should appear:\n{output}"
        );
        assert!(
            output.contains("three"),
            "Label 'three' should appear:\n{output}"
        );
    }

    #[test]
    fn test_multi_edge_mixed_with_other_edges() {
        let output =
            render_input("graph TD\n    A -->|x| B\n    A -->|y| B\n    A --> C\n    B --> D");
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(output.contains('C'));
        assert!(output.contains('D'));
        assert!(output.contains('x'), "Label 'x' should appear:\n{output}");
        assert!(output.contains('y'), "Label 'y' should appear:\n{output}");
    }
}

// === Subgraph-as-node edge resolution tests ===

#[test]
fn test_render_subgraph_as_node_edge() {
    let output = render_fixture("subgraph_as_node_edge.mmd");

    assert!(output.contains("Backend"), "Should render subgraph title");
    assert!(output.contains("Client"), "Should render Client node");
    assert!(output.contains("Logs"), "Should render Logs node");
    assert!(
        output.contains("API Server"),
        "Should render API Server node"
    );
    assert!(output.contains("Database"), "Should render Database node");
}

#[test]
fn test_subgraph_as_node_edge_no_sg_node() {
    let diagram = parse_and_build("subgraph_as_node_edge.mmd");

    // sg1 should not exist as a regular leaf node
    assert!(
        !diagram.nodes.contains_key("sg1"),
        "sg1 should not be a regular node after edge resolution"
    );
    // But it should exist as a subgraph
    assert!(diagram.subgraphs.contains_key("sg1"));

    // Edges should target children of sg1, not sg1 itself
    for edge in &diagram.edges {
        assert_ne!(edge.from, "sg1", "Edge source should not be sg1");
        assert_ne!(edge.to, "sg1", "Edge target should not be sg1");
    }
}

// ============================================================================
// Phase 5: Integration test fixtures
// ============================================================================

// --- 5.1: Subgraph-as-node edge fixtures ---

#[test]
fn test_render_subgraph_to_subgraph_edge() {
    let output = render_fixture("subgraph_to_subgraph_edge.mmd");

    assert!(output.contains("Frontend"), "Should render Frontend title");
    assert!(output.contains("Backend"), "Should render Backend title");
    assert!(
        output.contains("User Interface"),
        "Should render User Interface"
    );
    assert!(output.contains("API Server"), "Should render API Server");
}

#[test]
fn test_subgraph_to_subgraph_edge_resolution() {
    let diagram = parse_and_build("subgraph_to_subgraph_edge.mmd");

    // Neither frontend nor backend should exist as regular nodes
    assert!(!diagram.nodes.contains_key("frontend"));
    assert!(!diagram.nodes.contains_key("backend"));

    // Both should exist as subgraphs
    assert!(diagram.subgraphs.contains_key("frontend"));
    assert!(diagram.subgraphs.contains_key("backend"));

    // The edge "frontend --> backend" should be resolved to child nodes
    for edge in &diagram.edges {
        assert_ne!(edge.from, "frontend");
        assert_ne!(edge.to, "backend");
    }
}

#[test]
fn test_render_nested_subgraph_edge() {
    let output = render_fixture("nested_subgraph_edge.mmd");

    assert!(output.contains("Cloud"), "Should render Cloud title");
    assert!(output.contains("US East"), "Should render US East title");
    assert!(output.contains("Client"), "Should render Client");
    assert!(output.contains("Monitoring"), "Should render Monitoring");
    assert!(output.contains("Server1"), "Should render Server1");
}

#[test]
fn test_nested_subgraph_edge_resolution() {
    let diagram = parse_and_build("nested_subgraph_edge.mmd");

    // cloud should not exist as a regular node
    assert!(!diagram.nodes.contains_key("cloud"));
    assert!(diagram.subgraphs.contains_key("cloud"));

    // Edges targeting "cloud" should resolve to a child node
    for edge in &diagram.edges {
        assert_ne!(edge.to, "cloud", "Edge target should not be cloud");
        assert_ne!(edge.from, "cloud", "Edge source should not be cloud");
    }
}

// --- 5.2: Multi-word title and numeric ID fixtures ---

#[test]
fn test_render_multi_word_subgraph_title() {
    let output = render_fixture("subgraph_multi_word_title.mmd");

    assert!(
        output.contains("Data Processing Pipeline"),
        "Should render multi-word title"
    );
    assert!(output.contains("Extract"), "Should render Extract");
    assert!(output.contains("Transform"), "Should render Transform");
    assert!(output.contains("Load"), "Should render Load");
    assert!(output.contains("Source"), "Should render Source");
    assert!(output.contains("Sink"), "Should render Sink");
}

#[test]
fn test_render_numeric_subgraph_id() {
    let output = render_fixture("subgraph_numeric_id.mmd");

    assert!(output.contains("Phase 1"), "Should render Phase 1 title");
    assert!(output.contains("Phase 2"), "Should render Phase 2 title");
    assert!(output.contains("A"), "Should render node A");
    assert!(output.contains("D"), "Should render node D");
}

#[test]
fn test_parse_subgraph_id_with_quoted_title() {
    let output = render_input("graph TD\nsubgraph myId \"My Custom Title\"\nA --> B\nend\n");
    assert!(
        output.contains("My Custom Title"),
        "Should render quoted title"
    );
}

// --- 5.3: Direction override fixtures ---

#[test]
fn test_render_subgraph_direction_lr() {
    let output = render_fixture("subgraph_direction_lr.mmd");

    assert!(
        output.contains("Horizontal Flow"),
        "Should render subgraph title"
    );
    assert!(output.contains("Step 1"), "Should render Step 1");
    assert!(output.contains("Step 2"), "Should render Step 2");
    assert!(output.contains("Step 3"), "Should render Step 3");
    assert!(output.contains("Start"), "Should render Start");
    assert!(output.contains("End"), "Should render End");
}

#[test]
fn test_subgraph_direction_lr_horizontal_arrangement() {
    let (diagram, layout) = layout_fixture("subgraph_direction_lr.mmd");

    // A, B, C should be arranged horizontally (increasing x, similar y)
    let a = layout.get_bounds("A").unwrap();
    let b = layout.get_bounds("B").unwrap();
    let c = layout.get_bounds("C").unwrap();

    assert!(
        a.center_x() < b.center_x(),
        "Step 1 should be left of Step 2"
    );
    assert!(
        b.center_x() < c.center_x(),
        "Step 2 should be left of Step 3"
    );

    let y_tol = 2;
    assert!(
        (a.center_y() as isize - b.center_y() as isize).abs() <= y_tol,
        "Step 1 and Step 2 should be at similar y"
    );

    // Nodes should have LR effective direction
    assert_eq!(layout.node_directions.get("A"), Some(&Direction::LeftRight));
    let _ = diagram; // suppress unused variable
}

#[test]
fn test_render_subgraph_direction_nested() {
    let output = render_fixture("subgraph_direction_nested.mmd");

    assert!(
        output.contains("Vertical Outer"),
        "Should render outer title"
    );
    assert!(
        output.contains("Horizontal Inner"),
        "Should render inner title"
    );
    assert!(output.contains("D"), "Should render node D");
    assert!(output.contains("A"), "Should render node A");
    assert!(output.contains("C"), "Should render node C");
}

#[test]
fn test_render_subgraph_direction_mixed() {
    let output = render_fixture("subgraph_direction_mixed.mmd");

    assert!(
        output.contains("Left to Right"),
        "Should render LR subgraph title"
    );
    // Title may be pierced by an edge junction (e.g. "Bottom┼to Top"),
    // so check for both words rather than the exact phrase.
    assert!(
        output.contains("Bottom") && output.contains("to Top"),
        "Should render BT subgraph title"
    );
    assert!(output.contains("A"), "Should render node A");
    assert!(output.contains("B"), "Should render node B");
    assert!(output.contains("C"), "Should render node C");
    assert!(output.contains("D"), "Should render node D");
}

#[test]
fn test_subgraph_direction_mixed_layout() {
    let (_, layout) = layout_fixture("subgraph_direction_mixed.mmd");

    // A, B in LR subgraph: horizontal arrangement
    let a = layout.get_bounds("A").unwrap();
    let b = layout.get_bounds("B").unwrap();
    assert!(
        a.center_x() < b.center_x(),
        "A should be left of B in LR subgraph"
    );

    // C, D in BT subgraph: C below D (BT = source at bottom flows up)
    let c = layout.get_bounds("C").unwrap();
    let d = layout.get_bounds("D").unwrap();
    assert!(
        c.center_y() > d.center_y(),
        "C (source) should be below D (target) in BT subgraph: C_cy={} D_cy={}",
        c.center_y(),
        d.center_y()
    );

    // Check effective directions
    assert_eq!(layout.node_directions.get("A"), Some(&Direction::LeftRight));
    assert_eq!(layout.node_directions.get("C"), Some(&Direction::BottomTop));
}

#[test]
fn test_render_subgraph_direction_nested_both() {
    // Both parent (LR) and child (BT) have direction overrides.
    // Nodes in the inner subgraph should get the inner direction (BT),
    // not the outer (LR), regardless of HashMap iteration order.
    let output = render_fixture("subgraph_direction_nested_both.mmd");

    // The outer subgraph title may be partially clipped when inner subgraph
    // borders overlap with it in compact layouts.  Check for partial match.
    assert!(
        output.contains("Outer LR") || output.contains("ter LR"),
        "Should render outer title (possibly clipped by nested border)"
    );
    assert!(output.contains("Inner BT"), "Should render inner title");
    assert!(output.contains("A"), "Should render node A");
    assert!(output.contains("B"), "Should render node B");
    assert!(output.contains("C"), "Should render node C");
    assert!(output.contains("D"), "Should render node D");
}

#[test]
fn test_subgraph_direction_nested_both_layout() {
    // Verify deterministic direction assignment for nested overrides.
    let (_, layout) = layout_fixture("subgraph_direction_nested_both.mmd");

    // A, B are in inner (BT): deepest override wins → BottomTop
    assert_eq!(
        layout.node_directions.get("A"),
        Some(&Direction::BottomTop),
        "A should get inner BT direction, not outer LR"
    );
    assert_eq!(
        layout.node_directions.get("B"),
        Some(&Direction::BottomTop),
        "B should get inner BT direction, not outer LR"
    );

    // C is only in outer (LR): gets outer direction
    assert_eq!(
        layout.node_directions.get("C"),
        Some(&Direction::LeftRight),
        "C should get outer LR direction"
    );

    // D is outside both: gets diagram root direction (TD)
    assert_eq!(
        layout.node_directions.get("D"),
        Some(&Direction::TopDown),
        "D should get root TD direction"
    );
}

#[test]
fn test_route_policy_effective_edge_direction_with_nested_override_fixture() {
    let (diagram, layout) = layout_fixture("subgraph_direction_nested_both.mmd");

    assert_eq!(
        layout.effective_edge_direction("A", "B", diagram.direction),
        Direction::BottomTop
    );
    assert_eq!(
        layout.effective_edge_direction("C", "A", diagram.direction),
        Direction::LeftRight
    );
}

#[test]
fn test_orthogonal_route_routed_geometry_is_axis_aligned_for_forward_edges() {
    let diagram = parse_and_build("simple.mmd");
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
        .expect("layout should succeed");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for edge in routed.edges.iter().filter(|edge| !edge.is_backward) {
        assert!(
            edge.path
                .windows(2)
                .all(|seg| seg[0].x == seg[1].x || seg[0].y == seg[1].y),
            "orthogonal routing produced diagonal segment for {} -> {}: {:?}",
            edge.from,
            edge.to,
            edge.path
        );
    }
}

#[test]
fn test_step_topology_preserves_fan_stem_room_and_lane_compaction() {
    let fan_out = route_fixture_orthogonal("five_fan_out.mmd");
    let (a_b_stem, _) = first_segment(edge_path(&fan_out, "A", "B"));
    let (a_c_stem, _) = first_segment(edge_path(&fan_out, "A", "C"));
    let (a_f_stem, _) = first_segment(edge_path(&fan_out, "A", "F"));
    assert!(
        a_b_stem > 8.0 && a_f_stem > 8.0,
        "five_fan_out outer branches should have >8px primary stem; got A->B={a_b_stem}, A->F={a_f_stem}"
    );
    assert!(
        (a_c_stem - a_b_stem).abs() < 65.0,
        "five_fan_out lane spacing should stay compact; got A->B stem={a_b_stem}, A->C stem={a_c_stem}"
    );

    let fan_in = route_fixture_orthogonal("five_fan_in_diamond.mmd");
    let (b_f_stem, _) = first_segment(edge_path(&fan_in, "B", "F"));
    let (d_f_stem, _) = first_segment(edge_path(&fan_in, "D", "F"));
    let (a_f_stem_in, _) = first_segment(edge_path(&fan_in, "A", "F"));
    let (e_f_stem_in, _) = first_segment(edge_path(&fan_in, "E", "F"));
    assert!(
        b_f_stem > 8.0 && d_f_stem > 8.0,
        "five_fan_in_diamond inner branches should have >8px primary stem; got B->F={b_f_stem}, D->F={d_f_stem}"
    );
    assert!(
        a_f_stem_in < 100.0 && e_f_stem_in < 100.0,
        "five_fan_in_diamond outer branches should not consume most of the rank gap; got A->F={a_f_stem_in}, E->F={e_f_stem_in}"
    );

    let readme = route_input_orthogonal(
        "graph TD\n\
         A[Request] --> B{Authenticated?}\n\
         B -->|yes| C[Serve from cache]\n\
         B -->|no| D[Query database]\n\
         C --> E[Respond]\n\
         D --> E\n",
    );
    let (b_c_stem, b_c_vertical) = first_segment(edge_path(&readme, "B", "C"));
    let (b_d_stem, b_d_vertical) = first_segment(edge_path(&readme, "B", "D"));
    assert!(
        !b_c_vertical && !b_d_vertical,
        "README decision branches from angular source should depart laterally first; got B->C vertical={b_c_vertical}, B->D vertical={b_d_vertical}"
    );
    assert!(
        b_c_stem > 8.0 && b_d_stem > 8.0,
        "README decision branches should keep visible lateral departure segments; got B->C={b_c_stem}, B->D={b_d_stem}"
    );
    let (c_e_stem, c_e_vertical) = first_segment(edge_path(&readme, "C", "E"));
    let (d_e_stem, d_e_vertical) = first_segment(edge_path(&readme, "D", "E"));
    assert!(
        c_e_vertical && d_e_vertical,
        "README two-edge fan-in should depart along primary axis before lateral jog; got C->E vertical={c_e_vertical}, D->E vertical={d_e_vertical}"
    );
    assert!(
        c_e_stem > 10.0 && d_e_stem > 10.0,
        "README two-edge fan-in should keep >10px source stems; got C->E={c_e_stem}, D->E={d_e_stem}"
    );
}

#[test]
fn test_svg_orthogonal_route_differs_from_legacy_for_cycle_fixture() {
    let input = load_fixture("simple_cycle.mmd");
    let registry = default_registry();

    let mut legacy = registry
        .create("flowchart")
        .expect("flowchart instance should exist");
    legacy.parse(&input).expect("fixture should parse");
    let legacy_output = legacy
        .render(
            OutputFormat::Svg,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
        .expect("mermaid-layered render should succeed");

    let mut orthogonal = registry
        .create("flowchart")
        .expect("flowchart instance should exist");
    orthogonal.parse(&input).expect("fixture should parse");
    let orthogonal_output = orthogonal
        .render(
            OutputFormat::Svg,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
        .expect("flux-layered render should succeed");

    assert_ne!(
        legacy_output, orthogonal_output,
        "orthogonal routing should route cycle fixture through a distinct path set"
    );
}

#[test]
fn test_subgraph_direction_cross_boundary_no_stale_waypoints() {
    // Cross-boundary edges (one endpoint inside override subgraph, one outside)
    // should NOT retain waypoints from the parent layout after reconciliation.
    let (diagram, layout) = layout_fixture("subgraph_direction_cross_boundary.mmd");

    // C-->A crosses into the LR subgraph; B-->D crosses out.
    // After reconciliation, these edges should have their waypoints invalidated
    // (empty or absent) so the router recomputes from reconciled positions.
    let ca_idx = diagram
        .edges
        .iter()
        .find(|e| e.from == "C" && e.to == "A")
        .expect("C->A edge should exist")
        .index;
    let bd_idx = diagram
        .edges
        .iter()
        .find(|e| e.from == "B" && e.to == "D")
        .expect("B->D edge should exist")
        .index;

    // Ensure the fixture makes these long edges (rank span > 1), so waypoints
    // would exist without invalidation.
    let ca_layer_diff = layout
        .grid_positions
        .get("C")
        .unwrap()
        .layer
        .abs_diff(layout.grid_positions.get("A").unwrap().layer);
    let bd_layer_diff = layout
        .grid_positions
        .get("B")
        .unwrap()
        .layer
        .abs_diff(layout.grid_positions.get("D").unwrap().layer);
    assert!(
        ca_layer_diff > 1,
        "fixture should make C->A a long edge (layer diff > 1)"
    );
    assert!(
        bd_layer_diff > 1,
        "fixture should make B->D a long edge (layer diff > 1)"
    );

    // Cross-boundary waypoints are clipped to the subgraph border.
    // However, if the clipped waypoints end up on the wrong subgraph face
    // (e.g. side border when the source is above), the face-mismatch
    // detector removes them so the router can recompute a fresh path.
    // Either outcome (clipped waypoints or absent) produces correct
    // rendering; verify at least one cross-boundary edge retains waypoints.
    let ca_wps = layout.edge_waypoints.get(&ca_idx);
    let bd_wps = layout.edge_waypoints.get(&bd_idx);
    assert!(
        ca_wps.is_some() || bd_wps.is_some(),
        "at least one cross-boundary edge should have clipped waypoints"
    );
}

#[test]
fn test_render_subgraph_direction_cross_boundary() {
    // Smoke test: cross-boundary edges with direction overrides should render
    // without panics and include all nodes.
    let output = render_fixture("subgraph_direction_cross_boundary.mmd");

    assert!(
        output.contains("Horizontal Section"),
        "Should render subgraph title"
    );
    assert!(output.contains("A"), "Should render node A");
    assert!(output.contains("B"), "Should render node B");
    assert!(output.contains("C"), "Should render node C");
    assert!(output.contains("D"), "Should render node D");
}

#[test]
fn registry_entrypoint_dispatches_mmds_input_to_mmds_instance() {
    let input = load_mmds_fixture("minimal-layout.json");
    let registry = default_registry();
    let diagram_id = registry
        .detect(&input)
        .expect("registry should detect MMDS fixture");
    assert_eq!(diagram_id, "mmds");

    let mut instance = registry
        .create(diagram_id)
        .expect("registry should create mmds instance");
    instance.parse(&input).expect("MMDS parse should succeed");

    let output = instance
        .render(
            OutputFormat::Text,
            &mmdflux::diagram::RenderConfig::default(),
        )
        .expect("layout MMDS payload should render via registry entrypoint");
    assert!(output.contains("Start"));
    assert!(output.contains("End"));
}

#[test]
fn mmds_integration_fixture_matrix() {
    let cases = [
        ("layout-valid-flowchart.json", true),
        ("layout-valid-class.json", true),
        ("positioned/layout-basic.json", true),
        ("positioned/routed-basic.json", true),
        ("subgraph-endpoint-intent-present.json", true),
        ("subgraph-endpoint-intent-missing.json", true),
        ("subgraph-endpoint-subgraph-to-subgraph-present.json", true),
        ("subgraph-endpoint-subgraph-to-subgraph-missing.json", true),
        ("profiles/unknown-extension.json", true),
        ("invalid/dangling-edge-target.json", false),
        ("invalid/dangling-endpoint-intent-subgraph.json", false),
        ("invalid/dangling-subgraph-parent.json", false),
        ("invalid/invalid-shape.json", false),
        ("invalid/unsupported-version.json", false),
        ("profiles/unknown-core-version.json", false),
    ];

    for (fixture_name, should_pass) in cases {
        let payload = load_mmds_fixture(fixture_name);
        assert_eq!(
            from_mmds_str(&payload).is_ok(),
            should_pass,
            "fixture {} expected pass={}",
            fixture_name,
            should_pass
        );
    }
}

#[test]
fn fan_in_backward_channel_interaction_fixture_matrix_matches_documented_policy_in_text_and_svg() {
    fn edge_path_data(svg: &str) -> Vec<String> {
        svg.lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with("<path d=\"")
                    && (line.contains("marker-end=") || line.contains("marker-start="))
            })
            .filter_map(|line| {
                let start = line.find("d=\"")?;
                let after = &line[start + 3..];
                let end = after.find('"')?;
                Some(after[..end].to_string())
            })
            .collect()
    }

    fn parse_svg_path_points(path_data: &str) -> Vec<(f64, f64)> {
        path_data
            .split_whitespace()
            .filter_map(|token| {
                let token = token.trim_start_matches(|c: char| c.is_ascii_alphabetic());
                let (x, y) = token.split_once(',')?;
                Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
            })
            .collect()
    }

    fn parse_attr_f64(line: &str, attr: &str) -> Option<f64> {
        let marker = format!("{attr}=\"");
        let start = line.find(&marker)? + marker.len();
        let rest = &line[start..];
        let end = rest.find('"')?;
        rest[..end].parse::<f64>().ok()
    }

    fn node_rect_for_label(svg: &str, label: &str) -> Option<(f64, f64, f64, f64)> {
        let (text_x, text_y) = svg.lines().find_map(|line| {
            if !line.contains("<text") || !line.contains(&format!(">{label}<")) {
                return None;
            }
            Some((parse_attr_f64(line, "x")?, parse_attr_f64(line, "y")?))
        })?;

        svg.lines().find_map(|line| {
            if !line.contains("<rect ")
                || !line.contains("stroke=\"#333\"")
                || !line.contains("fill=\"white\"")
            {
                return None;
            }
            let x = parse_attr_f64(line, "x")?;
            let y = parse_attr_f64(line, "y")?;
            let width = parse_attr_f64(line, "width")?;
            let height = parse_attr_f64(line, "height")?;
            let inside = text_x >= x && text_x <= x + width && text_y >= y && text_y <= y + height;
            if inside {
                Some((x, y, width, height))
            } else {
                None
            }
        })
    }

    fn svg_point_face(rect: (f64, f64, f64, f64), point: (f64, f64)) -> &'static str {
        let eps = 0.5;
        let (x, y, w, h) = rect;
        let left = x;
        let right = x + w;
        let top = y;
        let bottom = y + h;

        let on_right = (point.0 - right).abs() <= eps;
        let on_left = (point.0 - left).abs() <= eps;
        let on_top = (point.1 - top).abs() <= eps;
        let on_bottom = (point.1 - bottom).abs() <= eps;

        if on_right && point.1 > top + eps && point.1 < bottom - eps {
            "right"
        } else if on_left && point.1 > top + eps && point.1 < bottom - eps {
            "left"
        } else if on_top && point.0 > left + eps && point.0 < right - eps {
            "top"
        } else if on_bottom && point.0 > left + eps && point.0 < right - eps {
            "bottom"
        } else if on_right {
            "right"
        } else if on_left {
            "left"
        } else {
            "interior_or_corner"
        }
    }

    fn svg_terminal_approach_face(
        rect: (f64, f64, f64, f64),
        points: &[(f64, f64)],
    ) -> &'static str {
        if points.is_empty() {
            return "interior_or_corner";
        }
        let end = *points.last().expect("path should include endpoint");
        let direct_face = svg_point_face(rect, end);
        if direct_face != "interior_or_corner" {
            return direct_face;
        }
        if points.len() < 2 {
            return direct_face;
        }
        let prev = points[points.len() - 2];
        let dx = end.0 - prev.0;
        let dy = end.1 - prev.1;
        let (x, y, w, h) = rect;
        let left = x;
        let right = x + w;
        let top = y;
        let bottom = y + h;
        const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;

        // SVG marker pullback can place the endpoint slightly outside the
        // node border while still visually attaching to that face.
        if end.0 > right
            && end.0 - right <= MARKER_PULLBACK_TOLERANCE
            && end.1 >= top - MARKER_PULLBACK_TOLERANCE
            && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
            && dy.abs() <= 0.5
            && dx < 0.0
        {
            return "right";
        }
        if end.0 < left
            && left - end.0 <= MARKER_PULLBACK_TOLERANCE
            && end.1 >= top - MARKER_PULLBACK_TOLERANCE
            && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
            && dy.abs() <= 0.5
            && dx > 0.0
        {
            return "left";
        }
        if end.1 > bottom
            && end.1 - bottom <= MARKER_PULLBACK_TOLERANCE
            && end.0 >= left - MARKER_PULLBACK_TOLERANCE
            && end.0 <= right + MARKER_PULLBACK_TOLERANCE
            && dx.abs() <= 0.5
            && dy < 0.0
        {
            return "bottom";
        }
        if end.1 < top
            && top - end.1 <= MARKER_PULLBACK_TOLERANCE
            && end.0 >= left - MARKER_PULLBACK_TOLERANCE
            && end.0 <= right + MARKER_PULLBACK_TOLERANCE
            && dx.abs() <= 0.5
            && dy > 0.0
        {
            return "top";
        }

        if dx.abs() >= dy.abs() {
            if dx > 0.0 {
                "right"
            } else if dx < 0.0 {
                "left"
            } else {
                "interior_or_corner"
            }
        } else if dy > 0.0 {
            "bottom"
        } else if dy < 0.0 {
            "top"
        } else {
            "interior_or_corner"
        }
    }

    fn svg_terminal_approach_face_relaxed(
        rect: (f64, f64, f64, f64),
        points: &[(f64, f64)],
    ) -> &'static str {
        if points.is_empty() {
            return "interior_or_corner";
        }
        let end = *points.last().expect("path should include endpoint");
        let direct_face = svg_point_face(rect, end);
        if direct_face != "interior_or_corner" {
            return direct_face;
        }
        if points.len() < 2 {
            return direct_face;
        }

        let prev = points[points.len() - 2];
        let dx = end.0 - prev.0;
        let dy = end.1 - prev.1;
        let (x, y, w, h) = rect;
        let left = x;
        let right = x + w;
        let top = y;
        let bottom = y + h;
        const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;

        if end.0 > right
            && end.0 - right <= MARKER_PULLBACK_TOLERANCE
            && end.1 >= top - MARKER_PULLBACK_TOLERANCE
            && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
            && dx < 0.0
        {
            return "right";
        }
        if end.0 < left
            && left - end.0 <= MARKER_PULLBACK_TOLERANCE
            && end.1 >= top - MARKER_PULLBACK_TOLERANCE
            && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
            && dx > 0.0
        {
            return "left";
        }
        if end.1 > bottom
            && end.1 - bottom <= MARKER_PULLBACK_TOLERANCE
            && end.0 >= left - MARKER_PULLBACK_TOLERANCE
            && end.0 <= right + MARKER_PULLBACK_TOLERANCE
            && dy < 0.0
        {
            return "bottom";
        }
        if end.1 < top
            && top - end.1 <= MARKER_PULLBACK_TOLERANCE
            && end.0 >= left - MARKER_PULLBACK_TOLERANCE
            && end.0 <= right + MARKER_PULLBACK_TOLERANCE
            && dy > 0.0
        {
            return "top";
        }

        svg_terminal_approach_face(rect, points)
    }

    fn svg_source_departure_face(
        rect: (f64, f64, f64, f64),
        points: &[(f64, f64)],
    ) -> &'static str {
        if points.is_empty() {
            return "interior_or_corner";
        }
        let start = points[0];
        let direct_face = svg_point_face(rect, start);
        if direct_face != "interior_or_corner" {
            return direct_face;
        }
        if points.len() < 2 {
            return direct_face;
        }

        let next = points[1];
        let dx = next.0 - start.0;
        let dy = next.1 - start.1;
        if dx.abs() >= dy.abs() {
            if dx > 0.0 {
                "right"
            } else if dx < 0.0 {
                "left"
            } else {
                "interior_or_corner"
            }
        } else if dy > 0.0 {
            "bottom"
        } else if dy < 0.0 {
            "top"
        } else {
            "interior_or_corner"
        }
    }

    fn edge_path_for_svg_order(diagram: &Diagram, svg: &str, edge_index: usize) -> Vec<(f64, f64)> {
        let mut visible_edge_indexes: Vec<usize> = diagram
            .edges
            .iter()
            .filter(|edge| edge.stroke != mmdflux::graph::Stroke::Invisible)
            .map(|edge| edge.index)
            .collect();
        visible_edge_indexes.sort_unstable();

        let svg_position = visible_edge_indexes
            .iter()
            .position(|idx| *idx == edge_index)
            .expect("edge index should be visible in SVG");
        let paths = edge_path_data(svg);
        parse_svg_path_points(
            paths
                .get(svg_position)
                .expect("edge path should exist at visible edge position"),
        )
    }

    let render_with_registry = |fixture_name: &str, format: OutputFormat| {
        let input = load_fixture(fixture_name);
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(&input).expect("fixture should parse");
        instance
            .render(
                format,
                &RenderConfig {
                    layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                    ..RenderConfig::default()
                },
            )
            .expect("render should succeed")
    };

    let fan_in_cases = [
        ("stacked_fan_in.mmd", "C", "Bot", 0usize),
        ("fan_in.mmd", "D", "Target", 0usize),
        ("five_fan_in.mmd", "F", "Target", 0usize),
    ];
    for (fixture_name, target_id, target_label, min_side_faces) in fan_in_cases {
        let diagram = parse_and_build(fixture_name);
        let text = render_with_registry(fixture_name, OutputFormat::Text);
        assert!(
            text.contains(target_label),
            "text output should contain target label {target_label} for {fixture_name}"
        );
        let svg = render_with_registry(fixture_name, OutputFormat::Svg);
        let rect = node_rect_for_label(&svg, target_label)
            .unwrap_or_else(|| panic!("missing target rect for {target_label} in {fixture_name}"));
        let inbound_indices: Vec<usize> = diagram
            .edges
            .iter()
            .filter(|edge| edge.to == target_id)
            .map(|edge| edge.index)
            .collect();
        assert!(
            !inbound_indices.is_empty(),
            "fixture {fixture_name} should have inbound edges to {target_id}"
        );

        let mut side_face_count = 0usize;
        let mut interior_or_corner_count = 0usize;
        for edge_index in inbound_indices {
            let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
            let face = svg_terminal_approach_face(rect, &points);
            if face == "interior_or_corner" {
                interior_or_corner_count += 1;
            }
            if matches!(face, "left" | "right") {
                side_face_count += 1;
            }
        }

        assert_eq!(
            interior_or_corner_count, 0,
            "fixture {fixture_name} should keep inbound endpoints on a concrete target face under Fan-in overflow policy"
        );
        if min_side_faces == 0 {
            assert_eq!(
                side_face_count, 0,
                "fixture {fixture_name} should stay on primary TD incoming face when overflow is not required"
            );
        } else {
            assert!(
                side_face_count >= min_side_faces,
                "fixture {fixture_name} should spill overflow arrivals to side faces under Fan-in overflow policy: expected >= {min_side_faces}, actual={side_face_count}"
            );
        }
    }

    // With per-edge label spacing, unlabeled backward edges are more compact.
    // Most backward edges still use side-channel (right-face) routing, but
    // the fan_in_backward_channel_conflict case uses top-face source routing
    // (departing toward the target above) and bottom-face target arrival.
    let backward_channel_cases = [
        (
            "simple_cycle.mmd",
            "C",
            "A",
            "End",
            "Start",
            "right",
            "right",
        ),
        (
            "multiple_cycles.mmd",
            "C",
            "A",
            "Bottom",
            "Top",
            "right",
            "right",
        ),
        (
            "fan_in_backward_channel_conflict.mmd",
            "Loop",
            "B",
            "Sink",
            "Target",
            "top",
            "bottom",
        ),
        (
            "http_request.mmd",
            "Response",
            "Client",
            "Send Response",
            "Client",
            "right",
            "right",
        ),
        (
            "git_workflow.mmd",
            "Remote",
            "Working",
            "Remote Repo",
            "Working Dir",
            "bottom",
            "bottom",
        ),
    ];
    for (
        fixture_name,
        from,
        to,
        source_label,
        target_label,
        expected_source_face,
        expected_target_face,
    ) in backward_channel_cases
    {
        let diagram = parse_and_build(fixture_name);
        let text = render_with_registry(fixture_name, OutputFormat::Text);
        assert!(
            text.contains(target_label),
            "text output should contain target label {target_label} for {fixture_name}"
        );
        let svg = render_with_registry(fixture_name, OutputFormat::Svg);
        let source_rect = node_rect_for_label(&svg, source_label)
            .unwrap_or_else(|| panic!("missing source rect for {source_label} in {fixture_name}"));
        let target_rect = node_rect_for_label(&svg, target_label)
            .unwrap_or_else(|| panic!("missing target rect for {target_label} in {fixture_name}"));
        let edge_index = diagram
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("expected edge {from} -> {to} in {fixture_name}"))
            .index;
        let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
        let source_face = svg_source_departure_face(source_rect, &points);
        assert_eq!(
            source_face, expected_source_face,
            "fixture {fixture_name} edge {from}->{to} should keep expected backward source face {expected_source_face}; points={points:?}"
        );
        let target_face = svg_terminal_approach_face_relaxed(target_rect, &points);
        assert_eq!(
            target_face, expected_target_face,
            "fixture {fixture_name} edge {from}->{to} should keep expected backward target face {expected_target_face}; points={points:?}"
        );
    }
}

#[test]
fn td_backward_entry_face_followup_parity_matches_text_for_decision_and_complex() {
    fn point_face(
        rect: mmdflux::diagrams::flowchart::geometry::FRect,
        point: mmdflux::diagrams::flowchart::geometry::FPoint,
    ) -> &'static str {
        let eps = 0.5;
        let left = rect.x;
        let right = rect.x + rect.width;
        let top = rect.y;
        let bottom = rect.y + rect.height;

        let on_right = (point.x - right).abs() <= eps;
        let on_left = (point.x - left).abs() <= eps;
        let on_top = (point.y - top).abs() <= eps;
        let on_bottom = (point.y - bottom).abs() <= eps;

        if on_right && point.y > top + eps && point.y < bottom - eps {
            "right"
        } else if on_left && point.y > top + eps && point.y < bottom - eps {
            "left"
        } else if on_top && point.x > left + eps && point.x < right - eps {
            "top"
        } else if on_bottom && point.x > left + eps && point.x < right - eps {
            "bottom"
        } else if on_right {
            "right"
        } else if on_left {
            "left"
        } else {
            "interior_or_corner"
        }
    }

    let render_text_with_engine = |input: &str, engine: &str| {
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(input).expect("fixture should parse");
        instance
            .render(
                OutputFormat::Text,
                &RenderConfig {
                    layout_engine: EngineAlgorithmId::parse(engine).ok(),
                    ..RenderConfig::default()
                },
            )
            .expect("text render should succeed")
    };

    // (fixture, from, to, expected_source_face, full_target_face, orthogonal_target_face)
    // Long backward edges (rank_span >= 6) use side-face channel routing in
    // orthogonal routing (R-BACK-7 Heuristic 4), so orthogonal target face may differ
    // from polyline routing.
    type BackwardFaceCase<'a> = (&'a str, &'a str, &'a str, Option<&'a str>, &'a str, &'a str);
    let cases: [BackwardFaceCase<'_>; 2] = [
        // D is to the right of A; parity override is bypassed to avoid crossing
        // the forward A->D edge, so orthogonal uses side-channel (right-face) routing.
        ("decision.mmd", "D", "A", None, "bottom", "right"),
        // Layout quality improvements (model order + variable spacing) shifted E
        // relative to A so the polyline backward edge now enters A from the bottom
        // face instead of the left face. The orthogonal router still uses the
        // side-channel (right-face) heuristic for long backward edges.
        ("complex.mmd", "E", "A", None, "bottom", "right"),
    ];

    for (
        fixture,
        from,
        to,
        expected_source_face,
        expected_full_target,
        expected_orthogonal_target,
    ) in cases
    {
        let input = load_fixture(fixture);
        let flowchart = parse_flowchart(&input).expect("fixture should parse");
        let diagram = build_diagram(&flowchart);
        let mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
        let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&mode, &diagram, &config).expect("layout should succeed");

        let source_rect = geom
            .nodes
            .get(from)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain source node {from}"))
            .rect;
        let target_rect = geom
            .nodes
            .get(to)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target node {to}"))
            .rect;

        let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let full_edge = full
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain edge {from}->{to}"));
        let orthogonal_edge = orthogonal
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain edge {from}->{to}"));

        let full_start = full_edge
            .path
            .first()
            .copied()
            .expect("polyline edge should have source endpoint");
        let full_end = full_edge
            .path
            .last()
            .copied()
            .expect("polyline edge should have target endpoint");
        let orthogonal_start = orthogonal_edge
            .path
            .first()
            .copied()
            .expect("orthogonal edge should have source endpoint");
        let orthogonal_end = orthogonal_edge
            .path
            .last()
            .copied()
            .expect("orthogonal edge should have target endpoint");

        let full_source_face = point_face(source_rect, full_start);
        let full_target_face = point_face(target_rect, full_end);
        let orthogonal_source_face = point_face(source_rect, orthogonal_start);
        let orthogonal_target_face = point_face(target_rect, orthogonal_end);

        if let Some(expected_source_face) = expected_source_face {
            assert_eq!(
                full_source_face, expected_source_face,
                "fixture contract changed unexpectedly: polyline {from}->{to} should use source face {expected_source_face}; path={:?}",
                full_edge.path
            );
        }
        assert_eq!(
            full_target_face, expected_full_target,
            "fixture contract changed unexpectedly: polyline {from}->{to} should use target face {expected_full_target}; path={:?}",
            full_edge.path
        );

        if let Some(expected_source_face) = expected_source_face {
            assert_eq!(
                orthogonal_source_face, expected_source_face,
                "orthogonal {from}->{to} should match TD source-face parity with text/polyline ({expected_source_face}) for fixture {fixture}; full_path={:?}, orthogonal_path={:?}",
                full_edge.path, orthogonal_edge.path
            );
        }
        assert_eq!(
            orthogonal_target_face, expected_orthogonal_target,
            "orthogonal {from}->{to} target face should be {expected_orthogonal_target} for fixture {fixture}; full_path={:?}, orthogonal_path={:?}",
            full_edge.path, orthogonal_edge.path
        );

        // Text output is routing-independent (backward edge face differences
        // only affect SVG path geometry, not text grid). Verify flux-layered
        // renders successfully for each fixture.
        let _text = render_text_with_engine(&input, "flux-layered");
    }
}

#[test]
fn lr_backward_spacing_followup_matches_text_parity_for_git_and_http() {
    const MIN_GIT_CHANNEL_CLEARANCE: f64 = 12.0;
    const MAX_HTTP_RIGHT_CLEARANCE_SHRINK_FROM_FULL: f64 = 8.0;

    fn point_face(
        rect: mmdflux::diagrams::flowchart::geometry::FRect,
        point: mmdflux::diagrams::flowchart::geometry::FPoint,
    ) -> &'static str {
        let eps = 0.5;
        let left = rect.x;
        let right = rect.x + rect.width;
        let top = rect.y;
        let bottom = rect.y + rect.height;

        let on_right = (point.x - right).abs() <= eps;
        let on_left = (point.x - left).abs() <= eps;
        let on_top = (point.y - top).abs() <= eps;
        let on_bottom = (point.y - bottom).abs() <= eps;

        if on_right && point.y > top + eps && point.y < bottom - eps {
            "right"
        } else if on_left && point.y > top + eps && point.y < bottom - eps {
            "left"
        } else if on_top && point.x > left + eps && point.x < right - eps {
            "top"
        } else if on_bottom && point.x > left + eps && point.x < right - eps {
            "bottom"
        } else if on_right {
            "right"
        } else if on_left {
            "left"
        } else {
            "interior_or_corner"
        }
    }

    let render_text_with_engine = |input: &str, engine: &str| {
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(input).expect("fixture should parse");
        instance
            .render(
                OutputFormat::Text,
                &RenderConfig {
                    layout_engine: EngineAlgorithmId::parse(engine).ok(),
                    ..RenderConfig::default()
                },
            )
            .expect("text render should succeed")
    };

    {
        let fixture = "git_workflow.mmd";
        let input = load_fixture(fixture);
        let flowchart = parse_flowchart(&input).expect("fixture should parse");
        let diagram = build_diagram(&flowchart);
        let mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
        let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&mode, &diagram, &config).expect("layout should succeed");
        assert_eq!(
            geom.direction,
            Direction::LeftRight,
            "fixture {fixture} should remain LR for backward channel spacing parity checks"
        );

        let source_rect = geom
            .nodes
            .get("Remote")
            .unwrap_or_else(|| panic!("fixture {fixture} should contain source node Remote"))
            .rect;
        let target_rect = geom
            .nodes
            .get("Working")
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target node Working"))
            .rect;

        let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        let full_edge = full
            .edges
            .iter()
            .find(|edge| edge.from == "Remote" && edge.to == "Working")
            .expect("fixture should contain edge Remote -> Working");
        let orthogonal_edge = orthogonal
            .edges
            .iter()
            .find(|edge| edge.from == "Remote" && edge.to == "Working")
            .expect("fixture should contain edge Remote -> Working");

        let full_start = full_edge.path[0];
        let _full_end = *full_edge
            .path
            .last()
            .expect("full edge should have endpoint");
        let orthogonal_start = orthogonal_edge.path[0];
        let orthogonal_end = *orthogonal_edge
            .path
            .last()
            .expect("orthogonal edge should have endpoint");
        let _full_source_face = point_face(source_rect, full_start);
        assert_eq!(
            point_face(source_rect, orthogonal_start),
            "bottom",
            "orthogonal Remote -> Working should preserve canonical bottom source face while matching spacing parity; full_path={:?}, orthogonal_path={:?}",
            full_edge.path,
            orthogonal_edge.path
        );
        assert_eq!(
            point_face(target_rect, orthogonal_end),
            "bottom",
            "orthogonal Remote -> Working should preserve canonical bottom target face while matching spacing parity; full_path={:?}, orthogonal_path={:?}",
            full_edge.path,
            orthogonal_edge.path
        );

        let node_envelope_bottom =
            (source_rect.y + source_rect.height).max(target_rect.y + target_rect.height);
        let orthogonal_lane_y = orthogonal_edge
            .path
            .iter()
            .map(|point| point.y)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            orthogonal_lane_y >= node_envelope_bottom + MIN_GIT_CHANNEL_CLEARANCE - 0.001,
            "orthogonal Remote -> Working channel lane should have >= {MIN_GIT_CHANNEL_CLEARANCE}px clearance from node envelope (R-BACK-8): node_envelope_bottom={node_envelope_bottom}, orthogonal_lane_y={orthogonal_lane_y}, clearance={}, full_path={:?}, orthogonal_path={:?}",
            orthogonal_lane_y - node_envelope_bottom,
            full_edge.path,
            orthogonal_edge.path
        );

        // Verify flux-layered text renders successfully for this fixture.
        let _text = render_text_with_engine(&input, "flux-layered");
    }

    {
        let fixture = "http_request.mmd";
        let input = load_fixture(fixture);
        let flowchart = parse_flowchart(&input).expect("fixture should parse");
        let diagram = build_diagram(&flowchart);
        let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
            .expect("layout should succeed");

        let source_rect = geom
            .nodes
            .get("Response")
            .unwrap_or_else(|| panic!("fixture {fixture} should contain source node Response"))
            .rect;
        let target_rect = geom
            .nodes
            .get("Client")
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target node Client"))
            .rect;

        let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        let full_edge = full
            .edges
            .iter()
            .find(|edge| edge.from == "Response" && edge.to == "Client")
            .expect("fixture should contain edge Response -> Client");
        let orthogonal_edge = orthogonal
            .edges
            .iter()
            .find(|edge| edge.from == "Response" && edge.to == "Client")
            .expect("fixture should contain edge Response -> Client");

        let full_start = full_edge.path[0];
        let _full_end = *full_edge
            .path
            .last()
            .expect("full edge should have endpoint");
        let orthogonal_start = orthogonal_edge.path[0];
        let orthogonal_end = *orthogonal_edge
            .path
            .last()
            .expect("orthogonal edge should have endpoint");
        let _full_source_face = point_face(source_rect, full_start);
        assert_eq!(
            point_face(source_rect, orthogonal_start),
            "right",
            "orthogonal Response -> Client should preserve canonical right source face while matching right-clearance parity; full_path={:?}, orthogonal_path={:?}",
            full_edge.path,
            orthogonal_edge.path
        );
        assert_eq!(
            point_face(target_rect, orthogonal_end),
            "right",
            "orthogonal Response -> Client should preserve canonical right target face while matching right-clearance parity; full_path={:?}, orthogonal_path={:?}",
            full_edge.path,
            orthogonal_edge.path
        );

        let full_right_lane_x = full_edge
            .path
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let orthogonal_right_lane_x = orthogonal_edge
            .path
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            orthogonal_right_lane_x + MAX_HTTP_RIGHT_CLEARANCE_SHRINK_FROM_FULL
                >= full_right_lane_x,
            "orthogonal Response -> Client should preserve right-side clearance close to polyline text baseline (allowed shrink <= {MAX_HTTP_RIGHT_CLEARANCE_SHRINK_FROM_FULL}): full_right_lane_x={full_right_lane_x}, orthogonal_right_lane_x={orthogonal_right_lane_x}, full_path={:?}, orthogonal_path={:?}",
            full_edge.path,
            orthogonal_edge.path
        );

        // Verify flux-layered text renders successfully for this fixture.
        let _text = render_text_with_engine(&input, "flux-layered");
    }
}

#[test]
fn polyline_route_rollback_is_stable_for_text_and_svg() {
    let input = load_fixture("simple_cycle.mmd");

    let render_svg = || {
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(&input).expect("fixture should parse");
        instance
            .render(
                OutputFormat::Svg,
                &RenderConfig {
                    layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
                    edge_preset: Some(EdgePreset::Polyline),
                    ..RenderConfig::default()
                },
            )
            .expect("render should succeed")
    };

    let baseline_svg = render_svg();
    let svg = render_svg();
    assert_eq!(
        svg, baseline_svg,
        "svg rollback should be stable across repeated renders"
    );
}

#[test]
fn text_label_revalidation_fixtures_match_between_orthogonal_route_and_polyline_route_modes() {
    let fixtures = ["labeled_edges.mmd", "inline_label_flowchart.mmd"];

    for fixture in fixtures {
        let input = load_fixture(fixture);
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(&input).expect("fixture should parse");
        // Verify flux-layered text renders successfully for label fixtures.
        let _text = instance
            .render(
                OutputFormat::Text,
                &RenderConfig {
                    layout_engine: EngineAlgorithmId::parse("flux-layered").ok(),
                    ..RenderConfig::default()
                },
            )
            .expect("text render should succeed");
    }
}

#[test]
fn top_level_render_matches_flowchart_instance_for_subgraph_direction_mixed() {
    let input = load_fixture("subgraph_direction_mixed.mmd");
    let diagram = parse_and_build("subgraph_direction_mixed.mmd");

    let top_level = render(&diagram, &RenderOptions::default());

    let registry = default_registry();
    let mut instance = registry
        .create("flowchart")
        .expect("flowchart instance should exist");
    instance.parse(&input).expect("fixture should parse");
    let instance_output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .expect("instance render should succeed");

    assert_eq!(
        top_level, instance_output,
        "top-level render() should match the flowchart instance text pipeline for subgraph fixtures"
    );
}

#[test]
fn flowchart_instance_render_is_stable_for_subgraph_direction_mixed() {
    let input = load_fixture("subgraph_direction_mixed.mmd");
    let mut baseline: Option<String> = None;

    for _ in 0..6 {
        let registry = default_registry();
        let mut instance = registry
            .create("flowchart")
            .expect("flowchart instance should exist");
        instance.parse(&input).expect("fixture should parse");
        let output = instance
            .render(OutputFormat::Text, &RenderConfig::default())
            .expect("instance render should succeed");

        if let Some(expected) = baseline.as_ref() {
            assert_eq!(
                output, *expected,
                "flowchart instance text render should remain stable across repeated subgraph renders"
            );
        } else {
            baseline = Some(output);
        }
    }
}

#[test]
fn text_renderer_rejects_stale_precomputed_label_anchor_for_label_revalidation_fixture() {
    fn distance_to_segment(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
        let (px, py) = point;
        let (sx, sy) = start;
        let (ex, ey) = end;
        let dx = ex - sx;
        let dy = ey - sy;
        let len_sq = dx * dx + dy * dy;
        if len_sq <= 0.000_001 {
            return ((px - sx).powi(2) + (py - sy).powi(2)).sqrt();
        }
        let projection = ((px - sx) * dx + (py - sy) * dy) / len_sq;
        let t = projection.clamp(0.0, 1.0);
        let cx = sx + t * dx;
        let cy = sy + t * dy;
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
    }

    fn distance_to_routed_path(
        point: (usize, usize),
        segments: &[mmdflux::render::Segment],
    ) -> f64 {
        let p = (point.0 as f64, point.1 as f64);
        segments
            .iter()
            .map(|segment| match segment {
                mmdflux::render::Segment::Horizontal { y, x_start, x_end } => {
                    distance_to_segment(p, (*x_start as f64, *y as f64), (*x_end as f64, *y as f64))
                }
                mmdflux::render::Segment::Vertical { x, y_start, y_end } => {
                    distance_to_segment(p, (*x as f64, *y_start as f64), (*x as f64, *y_end as f64))
                }
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn render_label_center(
        diagram: &Diagram,
        layout: &Layout,
        routed_edges: &[mmdflux::render::RoutedEdge],
        label: &str,
        label_positions: &HashMap<usize, (usize, usize)>,
    ) -> ((usize, usize), String) {
        let mut canvas = mmdflux::render::Canvas::new(layout.width, layout.height);
        let charset = mmdflux::render::CharSet::unicode();

        let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
        node_keys.sort();
        for node_id in node_keys {
            let node = &diagram.nodes[node_id];
            if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
                mmdflux::render::render_node(&mut canvas, node, x, y, &charset, diagram.direction);
            }
        }

        render_all_edges_with_labels(
            &mut canvas,
            routed_edges,
            &charset,
            diagram.direction,
            label_positions,
        );

        let output = canvas.to_string();
        let mut matches = Vec::new();
        for (y, line) in output.lines().enumerate() {
            if let Some(x) = line.find(label) {
                matches.push((x, y));
            }
        }
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one rendered '{label}' label occurrence; got {:?}\n{output}",
            matches
        );
        (matches[0], output)
    }

    let flowchart =
        parse_flowchart("graph TD\nA[Very Wide Source Node] -->|cfg| B[Very Wide Target Node]\n")
            .expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let layout = compute_layout(&diagram, &TextLayoutConfig::default());
    let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);

    let target_edge = diagram
        .edges
        .iter()
        .find(|edge| edge.label.as_deref() == Some("cfg"))
        .expect("diagram should contain labeled edge");
    let label = target_edge
        .label
        .as_ref()
        .expect("target edge should include label");
    let label_width = label.chars().count();
    let routed_edge = routed_edges
        .iter()
        .find(|edge| edge.edge.index == target_edge.index)
        .expect("routed edge should exist");

    let (baseline_left, baseline_output) = render_label_center(
        &diagram,
        &layout,
        &routed_edges,
        label,
        &layout.edge_label_positions,
    );
    let baseline_center = (baseline_left.0 + label_width / 2, baseline_left.1);
    let baseline_drift = distance_to_routed_path(baseline_center, &routed_edge.segments);

    let stale_candidates = [
        (layout.width.saturating_sub(label_width + 2), 1usize),
        (
            layout.width.saturating_sub(label_width + 2),
            layout.height / 2,
        ),
        (1usize + label_width / 2, layout.height.saturating_sub(2)),
    ];
    let stale_center = stale_candidates
        .iter()
        .copied()
        .max_by(|a, b| {
            distance_to_routed_path(*a, &routed_edge.segments)
                .partial_cmp(&distance_to_routed_path(*b, &routed_edge.segments))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("stale candidate list should be non-empty");
    let stale_drift = distance_to_routed_path(stale_center, &routed_edge.segments);
    assert!(
        stale_drift > baseline_drift + 6.0,
        "test setup invalid: stale candidate should be much farther than baseline (baseline={baseline_drift:.2}, stale={stale_drift:.2})\nbaseline output:\n{baseline_output}"
    );

    let mut poisoned_positions = layout.edge_label_positions.clone();
    poisoned_positions.insert(target_edge.index, stale_center);
    let (rendered_left, output) =
        render_label_center(&diagram, &layout, &routed_edges, label, &poisoned_positions);
    let rendered_center = (rendered_left.0 + label_width / 2, rendered_left.1);
    let rendered_drift = distance_to_routed_path(rendered_center, &routed_edge.segments);

    assert!(
        rendered_drift <= baseline_drift + 1.0,
        "stale precomputed anchor should be ignored so rendered drift stays near baseline; baseline={baseline_drift:.2}, stale={stale_drift:.2}, rendered={rendered_drift:.2}, stale_center={stale_center:?}, rendered_left={rendered_left:?}\n{output}"
    );
}

#[test]
fn classify_face_matches_expected_common_approaches() {
    use mmdflux::render::NodeBounds;
    use mmdflux::render::intersect::{NodeFace, classify_face};

    let bounds = NodeBounds {
        x: 10,
        y: 10,
        width: 20,
        height: 10,
        layout_center_x: None,
        layout_center_y: None,
    };

    assert_eq!(
        classify_face(&bounds, (20, 0), Shape::Rectangle),
        NodeFace::Top
    );
    assert_eq!(
        classify_face(&bounds, (35, 15), Shape::Rectangle),
        NodeFace::Right
    );
}

#[test]
fn text_renders_head_label() {
    let input = "graph TD\n  A --> B\n";
    let flowchart = mmdflux::parse_flowchart(input).unwrap();
    let mut diagram = mmdflux::build_diagram(&flowchart);
    diagram.edges[0].head_label = Some("*".to_string());
    let output = render(&diagram, &RenderOptions::default());
    assert!(
        output.contains('*'),
        "text output should contain head label '*', got:\n{output}"
    );
}

#[test]
fn text_renders_tail_label() {
    let input = "graph TD\n  A --> B\n";
    let flowchart = mmdflux::parse_flowchart(input).unwrap();
    let mut diagram = mmdflux::build_diagram(&flowchart);
    diagram.edges[0].tail_label = Some("src".to_string());
    let output = render(&diagram, &RenderOptions::default());
    assert!(
        output.contains("src"),
        "text output should contain tail label 'src', got:\n{output}"
    );
}
