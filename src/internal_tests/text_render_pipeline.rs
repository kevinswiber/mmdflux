use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::graph::geometry::{FPoint, FRect, GraphGeometry, LayoutEdge, PositionedNode};
use crate::graph::{Diagram, Direction, Edge, Node, Shape};
use crate::render::graph::{TextRenderOptions, render_text_from_geometry};
use crate::{OutputFormat, RenderConfig, TextColorMode};

#[test]
fn text_owner_local_smoke_renders_text_output() {
    let (diagram, geometry) = smoke_text_fixture();
    let text = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());

    assert!(text.contains("Start"));
    assert!(text.contains("End"));
}

fn smoke_text_fixture() -> (Diagram, GraphGeometry) {
    smoke_graph_geometry()
}

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error))
}

fn render_flowchart_fixture(name: &str) -> String {
    render_flowchart_fixture_with_options(name, OutputFormat::Text, TextColorMode::Plain)
}

fn render_flowchart_fixture_ascii(name: &str) -> String {
    render_flowchart_fixture_with_options(name, OutputFormat::Ascii, TextColorMode::Plain)
}

fn render_flowchart_fixture_with_options(
    name: &str,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> String {
    let input = load_flowchart_fixture(name);
    crate::render_diagram(
        &input,
        format,
        &RenderConfig {
            text_color_mode,
            ..RenderConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("Failed to render flowchart fixture {name}: {error}"))
}

fn assert_flowchart_snapshot(name: &str) {
    let output = render_flowchart_fixture(name);
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

fn smoke_graph_geometry() -> (Diagram, GraphGeometry) {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));

    let nodes = HashMap::from([
        (
            "A".to_string(),
            PositionedNode {
                id: "A".to_string(),
                rect: FRect::new(50.0, 25.0, 40.0, 20.0),
                shape: Shape::Rectangle,
                label: "Start".to_string(),
                parent: None,
            },
        ),
        (
            "B".to_string(),
            PositionedNode {
                id: "B".to_string(),
                rect: FRect::new(50.0, 75.0, 40.0, 20.0),
                shape: Shape::Rectangle,
                label: "End".to_string(),
                parent: None,
            },
        ),
    ]);

    let geometry = GraphGeometry {
        nodes,
        edges: vec![LayoutEdge {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: Some(vec![FPoint::new(50.0, 45.0), FPoint::new(50.0, 75.0)]),
            preserve_orthogonal_topology: false,
        }],
        subgraphs: HashMap::new(),
        self_edges: vec![],
        direction: Direction::TopDown,
        node_directions: HashMap::from([
            ("A".to_string(), Direction::TopDown),
            ("B".to_string(), Direction::TopDown),
        ]),
        bounds: FRect::new(0.0, 0.0, 100.0, 100.0),
        reversed_edges: vec![],
        engine_hints: None,
        grid_projection: None,
        rerouted_edges: HashSet::new(),
        enhanced_backward_routing: false,
    };

    (diagram, geometry)
}

mod owner_local_fixture_regressions {
    use super::*;

    #[test]
    fn ascii_only_mode() {
        let unicode_output = render_flowchart_fixture("simple.mmd");
        let ascii_output = render_flowchart_fixture_ascii("simple.mmd");

        assert!(unicode_output.contains("Start"));
        assert!(ascii_output.contains("Start"));

        let unicode_chars = [
            '─', '│', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '╭', '╮', '╯', '╰',
        ];
        for ch in unicode_chars {
            assert!(
                !ascii_output.contains(ch),
                "ASCII output should not contain '{ch}'"
            );
        }
    }

    #[test]
    fn simple_renders() {
        let output = render_flowchart_fixture("simple.mmd");
        assert!(!output.is_empty());
        assert!(output.contains("Start"));
        assert!(output.contains("End"));
    }

    #[test]
    fn decision_renders_diamond() {
        let output = render_flowchart_fixture("decision.mmd");
        assert!(output.contains("Is it working?"));
        assert!(output.contains('<') || output.contains('>'));
    }

    #[test]
    fn shapes_render_distinctly() {
        let output = render_flowchart_fixture("shapes.mmd");
        assert!(output.contains("Rectangle Node"));
        assert!(output.contains("Rounded Node"));
        assert!(output.contains("Diamond Node"));
    }

    #[test]
    fn shapes_document_render_distinctly() {
        let output = render_flowchart_fixture("shapes_document.mmd");
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
        let output = render_flowchart_fixture("shapes_junction.mmd");
        assert!(output.contains('●'));
        assert!(output.contains('◉'));
        assert!(output.contains('⊗'));
    }

    #[test]
    fn shapes_special_render_bar_and_text() {
        let output = render_flowchart_fixture("shapes_special.mmd");
        assert!(
            output.contains('┃'),
            "Fork/join in LR should use heavy vertical bar"
        );
        assert!(output.contains("Note"));
    }

    #[test]
    fn shapes_junction_ascii_degrades() {
        let output = render_flowchart_fixture_ascii("shapes_junction.mmd");
        assert!(output.contains("o"));
        assert!(output.contains("(o)"));
        assert!(output.contains("x"));
    }

    #[test]
    fn text_render_uses_stroke_fill_and_label_colors_when_ansi_enabled() {
        let plain = render_flowchart_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Text,
            TextColorMode::Plain,
        );
        let ansi = render_flowchart_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Text,
            TextColorMode::Ansi,
        );

        assert!(ansi.contains("38;2;"));
        assert!(ansi.contains("48;2;"));
        assert_eq!(strip_ansi(&ansi), plain);
    }

    #[test]
    fn text_render_clears_fill_background_before_right_border() {
        let ansi = render_flowchart_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Text,
            TextColorMode::Ansi,
        );

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
        let plain = render_flowchart_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Ascii,
            TextColorMode::Plain,
        );
        let ansi = render_flowchart_fixture_with_options(
            "style-basic.mmd",
            OutputFormat::Ascii,
            TextColorMode::Ansi,
        );

        assert!(ansi.contains("\u{1b}["));
        assert_eq!(strip_ansi(&ansi), plain);
    }

    #[test]
    fn shapes_degenerate_render_labels() {
        let output = render_flowchart_fixture("shapes_degenerate.mmd");
        for label in [
            "Cloud", "Bolt", "Bang", "Icon", "Hour", "Tri", "Flip", "Notch",
        ] {
            assert!(output.contains(label));
        }
    }

    #[test]
    fn edge_styles_render() {
        let output = render_flowchart_fixture("edge_styles.mmd");
        assert!(output.contains("Solid"));
        assert!(output.contains("Dotted"));
        assert!(output.contains("Thick"));
    }

    #[test]
    fn left_right_renders_horizontally() {
        let output = render_flowchart_fixture("left_right.mmd");
        let lines: Vec<&str> = output.lines().collect();
        let height = lines.len();
        let width = lines.iter().map(|line| line.len()).max().unwrap_or(0);
        assert!(
            width > height,
            "LR layout should be wider than tall: {}x{}",
            width,
            height
        );
    }

    #[test]
    fn chain_renders_all_nodes() {
        let output = render_flowchart_fixture("chain.mmd");
        assert!(output.contains("Step 1"));
        assert!(output.contains("Step 2"));
        assert!(output.contains("Step 3"));
        assert!(output.contains("Step 4"));
    }

    #[test]
    fn git_workflow_renders() {
        let output = render_flowchart_fixture("git_workflow.mmd");
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
    fn backward_in_subgraph_lr_matches_snapshot() {
        assert_flowchart_snapshot("backward_in_subgraph_lr.mmd");
    }

    #[test]
    fn http_request_renders() {
        let output = render_flowchart_fixture("http_request.mmd");
        assert!(!output.is_empty());
        let has_nodes = output.contains("Client")
            || output.contains("Server")
            || output.contains("Process")
            || output.contains("Response");
        assert!(has_nodes, "Should contain at least one node label");
        assert!(
            output.contains('<') || output.contains('>'),
            "Should have decision node (diamond shape uses < or > chars)"
        );
    }

    #[test]
    fn ci_pipeline_renders() {
        let output = render_flowchart_fixture("ci_pipeline.mmd");
        assert!(output.contains("Build"));
        assert!(output.contains("Test"));
        assert!(output.contains("Deploy?"));
    }

    #[test]
    fn complex_renders_without_panic() {
        let output = render_flowchart_fixture("complex.mmd");
        assert!(!output.is_empty());
        assert!(output.contains("Input"));
        assert!(output.contains("Output"));
    }

    #[test]
    fn render_with_subgraph_produces_borders() {
        let output = crate::render_diagram(
            "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n",
            OutputFormat::Text,
            &RenderConfig::default(),
        )
        .unwrap();

        assert!(
            output.contains('\u{250c}') || output.contains('+'),
            "output should contain top-left corner: {output}"
        );
        assert!(
            output.contains('\u{2518}') || output.contains('+'),
            "output should contain bottom-right corner: {output}"
        );
        assert!(
            output.contains("Group"),
            "output should contain title: {output}"
        );
    }

    #[test]
    fn render_simple_diagram_unchanged() {
        let output = crate::render_diagram(
            "graph TD\nA --> B\n",
            OutputFormat::Text,
            &RenderConfig::default(),
        )
        .unwrap();

        assert!(
            output.contains('A'),
            "output should contain node A: {output}"
        );
        assert!(
            output.contains('B'),
            "output should contain node B: {output}"
        );
    }

    #[test]
    fn ascii_issue_21_backward_edge_does_not_clip_right_edge() {
        let output = render_flowchart_fixture_ascii("callgraph_feedback_cycle.mmd");

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
}
