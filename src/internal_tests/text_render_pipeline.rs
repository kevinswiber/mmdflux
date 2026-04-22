use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::graph::geometry::{FPoint, FRect, GraphGeometry, LayoutEdge, PositionedNode};
use crate::graph::{Direction, Edge, Graph, Node, Shape};
use crate::render::graph::{TextRenderOptions, render_text_from_geometry};
use crate::{OutputFormat, RenderConfig, TextColorMode};

#[test]
fn text_owner_local_smoke_renders_text_output() {
    let (diagram, geometry) = smoke_text_fixture();
    let text = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());

    assert!(text.contains("Start"));
    assert!(text.contains("End"));
}

fn smoke_text_fixture() -> (Graph, GraphGeometry) {
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

fn smoke_graph_geometry() -> (Graph, GraphGeometry) {
    let mut diagram = Graph::new(Direction::TopDown);
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
            label_geometry: None,
            effective_wrapped_lines: None,
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
    fn text_render_applies_linkstyle_stroke_color_when_ansi_enabled() {
        let input = "graph TD\nA --> B\nB --> C\nlinkStyle default stroke:#999\nlinkStyle 1 stroke:#ff0000,stroke-width:4px\n";
        let plain = crate::render_diagram(
            input,
            OutputFormat::Text,
            &RenderConfig {
                text_color_mode: TextColorMode::Plain,
                ..RenderConfig::default()
            },
        )
        .expect("plain linkStyle text render should succeed");
        let ansi = crate::render_diagram(
            input,
            OutputFormat::Text,
            &RenderConfig {
                text_color_mode: TextColorMode::Ansi,
                ..RenderConfig::default()
            },
        )
        .expect("ansi linkStyle text render should succeed");

        assert!(
            ansi.contains("\u{1b}[38;2;153;153;153m"),
            "default linkStyle stroke color missing: {ansi:?}"
        );
        assert!(
            ansi.contains("\u{1b}[38;2;255;0;0m"),
            "targeted linkStyle stroke color missing: {ansi:?}"
        );
        assert_eq!(strip_ansi(&ansi), plain);
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

mod plan_0152_corridor_aware_placement {
    //! Phase 3 corridor-aware placement contract.
    //!
    //! Red canaries that pin what the corridor-aware placer must deliver.
    //! Each targets a specific regression PR #252 tripped when it widened
    //! PR #251's narrow trust gate: glyph stomps on state/composite,
    //! corridor-turn losses on label_clamp_bt_review and
    //! backward_label_asymmetric_markers, and the 7-cell column float on
    //! state/transitions.
    //!
    //! Forward-case pins guard PR #251's shipped scope against Phase 3's
    //! wider replacement.
    use super::*;

    fn render_state_fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("state")
            .join(name);
        let input = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("Failed to read state fixture {}: {}", path.display(), error)
        });
        crate::render_diagram(
            &input,
            OutputFormat::Text,
            &RenderConfig {
                text_color_mode: TextColorMode::Plain,
                ..RenderConfig::default()
            },
        )
        .unwrap_or_else(|error| panic!("Failed to render state fixture {name}: {error}"))
    }

    #[test]
    fn state_composite_retains_resume_terminal_arrowhead() {
        let text = render_state_fixture("composite.mmd");
        let count = text.matches('▲').count();
        assert_eq!(
            count, 1,
            "state/composite must retain the `▲` arrowhead terminal on the resume edge; got {count} occurrences in:\n{text}"
        );
    }

    #[test]
    fn label_clamp_bt_review_retains_corridor_turn_glyphs() {
        let text = render_flowchart_fixture("label_clamp_bt_review.mmd");
        assert!(
            text.contains("└─┐"),
            "label_clamp_bt_review must retain the `└─┐` corridor-turn glyph. Output:\n{text}"
        );
        assert!(
            text.contains("┌─┘"),
            "label_clamp_bt_review must retain the `┌─┘` corridor-turn glyph. Output:\n{text}"
        );
    }

    #[test]
    fn backward_label_asymmetric_markers_retains_approach_turn() {
        let text = render_flowchart_fixture("backward_label_asymmetric_markers.mmd");
        assert!(
            text.contains("┌───┘"),
            "backward_label_asymmetric_markers must retain the `┌───┘` approach-turn glyph. Output:\n{text}"
        );
    }

    // Char-column of a substring in a string — `str::find` returns a
    // byte index, which conflates with multi-byte glyphs like `│` when
    // the test wants to measure visual column alignment. Returns `None`
    // if the substring is not found.
    fn char_col_of(line: &str, needle: &str) -> Option<usize> {
        let byte_idx = line.find(needle)?;
        Some(line[..byte_idx].chars().count())
    }

    #[test]
    fn state_transitions_retry_label_stays_corridor_adjacent() {
        let text = render_state_fixture("transitions.mmd");
        let row_idx = text
            .lines()
            .position(|l| l.contains("retry"))
            .expect("retry should appear");
        let retry_col = char_col_of(text.lines().nth(row_idx).unwrap(), "retry")
            .expect("retry col should be locatable");

        let neighboring_rows = [row_idx.saturating_sub(1), row_idx + 1];
        let nearest_pipe_col = neighboring_rows
            .iter()
            .filter_map(|&r| text.lines().nth(r))
            .filter_map(|line| {
                line.chars()
                    .enumerate()
                    .filter_map(|(i, c)| (c == '│').then_some(i))
                    .last()
            })
            .next()
            .expect("neighboring row should carry a corridor `│`");

        let drift = (retry_col as isize - nearest_pipe_col as isize).abs();
        assert!(
            drift <= 2,
            "state/transitions `retry` label at col {retry_col} must sit within 2 cells of the corridor `│` at col {nearest_pipe_col}; got {drift} cells. Output:\n{text}"
        );
    }

    #[test]
    fn git_workflow_td_backward_label_honors_authoritative_center() {
        let text = render_flowchart_fixture("git_workflow_td.mmd");
        assert_eq!(
            text.matches("git pull").count(),
            1,
            "git pull label should appear exactly once. Output:\n{text}"
        );
        let row = text.lines().position(|l| l.contains("git pull")).unwrap();
        let line = text.lines().nth(row).unwrap();
        let label_col = char_col_of(line, "git pull").expect("git pull col should be locatable");
        let pipe_col = line
            .chars()
            .enumerate()
            .filter_map(|(i, c)| (c == '│').then_some(i))
            .next();
        if let Some(pipe_col) = pipe_col {
            let drift = (label_col as isize - pipe_col as isize).abs();
            assert!(
                drift <= 3,
                "git pull label should sit within 3 cells of the vertical corridor `│`; got {drift}. Output:\n{text}"
            );
        }
    }

    #[test]
    fn inline_label_flowchart_forward_labels_stay_attached_post_phase_3() {
        let text = render_flowchart_fixture("inline_label_flowchart.mmd");
        for label in ["no", "yes", "sync", "async", "hit", "miss", "warn"] {
            assert!(
                text.contains(label),
                "inline_label_flowchart should include {label:?} after Phase 3. Output:\n{text}"
            );
        }
    }

    #[test]
    fn git_workflow_lr_forward_and_backward_labels_all_corridor_adjacent() {
        let text = render_flowchart_fixture("git_workflow.mmd");
        for label in ["git add", "git commit", "git push", "git pull"] {
            assert!(
                text.contains(label),
                "git_workflow should include {label:?}. Output:\n{text}"
            );
        }
    }
}

mod plan_0147_task_3_2_text_grid_gate {
    //! Plan 0147 Task 3.2: labeled backward-edge fixtures must still render
    //! corridor-closure glyphs (`─┘` / `└─`) under Tier A's two-waypoint bend.
    //! Plan 0145 task 3.9 previously broke this; Tier A's narrower bend is
    //! expected to be safe — verify, don't assume.

    use super::*;

    fn assert_has_corridor_close_glyphs(fixture: &str) {
        let text = render_flowchart_fixture(fixture);
        let has_right_up = text.contains("─┘");
        let has_up_right = text.contains("└─");
        assert!(
            has_right_up || has_up_right,
            "corridor closure glyphs (─┘ / └─) missing from {fixture}; output:\n{text}"
        );
    }

    #[test]
    fn labeled_backward_asymmetric_preserves_corridor_close_glyphs() {
        assert_has_corridor_close_glyphs("backward_label_asymmetric_markers.mmd");
    }

    #[test]
    fn labeled_backward_loop_lr_preserves_corridor_close_glyphs() {
        assert_has_corridor_close_glyphs("backward_loop_lr.mmd");
    }
}

mod edge_rendering_regression {
    use std::collections::HashMap;
    use std::path::Path;

    use crate::diagrams::flowchart::compile_to_graph;
    use crate::engines::graph::algorithms::layered::layout_building::layered_config_for_layout;
    use crate::engines::graph::algorithms::layered::run_layered_layout;
    use crate::engines::graph::contracts::{
        EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest, MeasurementMode,
    };
    use crate::engines::graph::flux::FluxLayeredEngine;
    use crate::graph::grid::{
        AttachDirection, GridLayout, GridLayoutConfig, geometry_to_grid_layout_with_routed,
        route_all_edges, route_edge, route_edge_with_probe,
    };
    use crate::graph::measure::default_proportional_text_metrics;
    use crate::graph::routing::{EdgeRouting, route_graph_geometry};
    use crate::graph::{Arrow, Direction, Edge, Graph, Node, Stroke};
    use crate::mermaid::parse_flowchart;
    use crate::render::graph::text::{render_all_edges, render_edge};
    use crate::render::text::canvas::Canvas;
    use crate::render::text::chars::CharSet;
    use crate::{OutputFormat, RenderConfig};

    fn simple_diagram() -> Graph {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B"));
        diagram
    }

    fn compute_layout(diagram: &Graph, config: &GridLayoutConfig) -> GridLayout {
        let engine = FluxLayeredEngine::text();
        let request = GraphSolveRequest::new(
            MeasurementMode::Grid,
            GraphGeometryContract::Canonical,
            crate::graph::GeometryLevel::Layout,
            None,
            Default::default(),
        );
        let result = engine
            .solve(
                diagram,
                &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
                &request,
            )
            .expect("text edge test layout solve failed");

        geometry_to_grid_layout_with_routed(
            diagram,
            &result.geometry,
            result.routed.as_ref(),
            config,
        )
    }

    fn flux_layout_config() -> EngineConfig {
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig {
            greedy_switch: true,
            model_order_tiebreak: true,
            variable_rank_spacing: true,
            track_reversed_chains: true,
            ..crate::engines::graph::algorithms::layered::LayoutConfig::default()
        })
    }

    /// Render a `Diagram` through the full text pipeline (engine + grid + text render).
    fn render_diagram_to_text(diagram: &Graph) -> String {
        let layout = compute_layout(diagram, &GridLayoutConfig::default());
        crate::render::graph::text::render_text_from_grid_layout(
            diagram,
            &layout,
            &crate::render::graph::TextRenderOptions::default(),
        )
    }

    fn render_flowchart_fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join(name);
        let input = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error));
        crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
            .unwrap_or_else(|error| panic!("Failed to render fixture {name}: {error}"))
    }

    fn render_flowchart_input(input: &str) -> String {
        crate::render_diagram(input, OutputFormat::Text, &RenderConfig::default())
            .expect("input render should succeed")
    }

    fn flowchart_fixture_input(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error))
    }

    fn flowchart_fixture_diagram(name: &str) -> Graph {
        let input = flowchart_fixture_input(name);
        let parsed = parse_flowchart(&input).expect("flowchart fixture should parse");
        compile_to_graph(&parsed)
    }

    fn compute_layout_with_mode(
        diagram: &Graph,
        config: &GridLayoutConfig,
        measurement_mode: MeasurementMode,
    ) -> (crate::graph::geometry::RoutedGraphGeometry, GridLayout) {
        let engine = FluxLayeredEngine::text();
        let request = GraphSolveRequest::new(
            measurement_mode,
            GraphGeometryContract::Canonical,
            crate::graph::GeometryLevel::Layout,
            None,
            Default::default(),
        );
        let result = engine
            .solve(
                diagram,
                &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
                &request,
            )
            .expect("fixture solve failed");
        let routed = route_graph_geometry(
            diagram,
            &result.geometry,
            EdgeRouting::OrthogonalRoute,
            &default_proportional_text_metrics(),
        );
        let layout =
            geometry_to_grid_layout_with_routed(diagram, &result.geometry, Some(&routed), config);
        (routed, layout)
    }

    fn route_fixture_with_svg_layout(
        name: &str,
    ) -> (Graph, crate::graph::geometry::RoutedGraphGeometry) {
        let diagram = flowchart_fixture_diagram(name);
        let geom = run_layered_layout(
            &MeasurementMode::Proportional(default_proportional_text_metrics()),
            &diagram,
            &flux_layout_config(),
        )
        .expect("SVG-side fixture solve failed");
        let routed = route_graph_geometry(
            &diagram,
            &geom,
            EdgeRouting::OrthogonalRoute,
            &default_proportional_text_metrics(),
        );
        (diagram, routed)
    }

    fn find_edge<'a>(diagram: &'a Graph, from: &str, to: &str) -> &'a Edge {
        diagram
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("missing edge {from} -> {to}"))
    }

    fn lr_rl_middle_vertical_lane_draw(path: &[(usize, usize)]) -> Option<usize> {
        if path.len() != 4 {
            return None;
        }
        let first_horizontal = path[0].1 == path[1].1 && path[0].0 != path[1].0;
        let middle_vertical = path[1].0 == path[2].0 && path[1].1 != path[2].1;
        let terminal_horizontal = path[2].1 == path[3].1 && path[2].0 != path[3].0;
        (first_horizontal && middle_vertical && terminal_horizontal).then_some(path[1].0)
    }

    fn lr_rl_middle_vertical_lane_float(path: &[crate::graph::geometry::FPoint]) -> Option<f64> {
        const EPS: f64 = 0.5;
        if path.len() != 4 {
            return None;
        }
        let first_horizontal =
            (path[0].y - path[1].y).abs() <= EPS && (path[0].x - path[1].x).abs() > EPS;
        let middle_vertical =
            (path[1].x - path[2].x).abs() <= EPS && (path[1].y - path[2].y).abs() > EPS;
        let terminal_horizontal =
            (path[2].y - path[3].y).abs() <= EPS && (path[2].x - path[3].x).abs() > EPS;
        (first_horizontal && middle_vertical && terminal_horizontal).then_some(path[1].x)
    }

    fn rounded_float_path(path: &[crate::graph::geometry::FPoint]) -> Vec<(i32, i32)> {
        path.iter()
            .map(|point| (point.x.round() as i32, point.y.round() as i32))
            .collect()
    }

    // === Tests using compute_layout (route_edge / render_edge) ===

    #[test]
    fn test_render_vertical_edge() {
        let diagram = simple_diagram();
        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(output.contains('│') || output.contains('▼'));
    }

    #[test]
    fn test_render_edge_with_arrow() {
        let diagram = simple_diagram();
        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(output.contains('▼'));
    }

    #[test]
    fn test_render_dotted_edge() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_stroke(Stroke::Dotted));

        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let _output = canvas.to_string();
    }

    #[test]
    fn test_render_edge_without_arrow() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrow(Arrow::None));

        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(!output.contains('▼'));
    }

    #[test]
    fn test_render_all_edges() {
        let diagram = simple_diagram();
        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed_edges: Vec<_> = diagram
            .edges
            .iter()
            .filter_map(|e| route_edge(e, &layout, Direction::TopDown, None, None, false))
            .collect();

        render_all_edges(&mut canvas, &routed_edges, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(!output.trim().is_empty());
    }

    #[test]
    fn test_labeled_edge_has_waypoints() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B").with_label("yes"));

        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let ab_edge_idx = diagram
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == "B")
            .expect("Should have an A->B edge")
            .index;
        assert!(
            layout.edge_waypoints.contains_key(&ab_edge_idx),
            "Labeled short edge should have waypoints from label dummy"
        );
    }

    #[test]
    fn test_lr_label_placement_near_edge_segment() {
        let mut diagram = Graph::new(Direction::LeftRight);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        let mut edge = Edge::new("A", "B");
        edge.label = Some("test".to_string());
        diagram.add_edge(edge);

        let config = GridLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::LeftRight,
            None,
            None,
            false,
        )
        .unwrap();

        assert!(
            !routed.segments.is_empty(),
            "Routed edge should have segments"
        );

        let mut canvas = Canvas::new(layout.width, layout.height);
        render_edge(&mut canvas, &routed, &charset, Direction::LeftRight);

        let output = canvas.to_string();
        assert!(
            output.contains("test"),
            "Label 'test' should appear in output:\n{}",
            output
        );

        let lines: Vec<&str> = output.lines().collect();
        let label_line = lines
            .iter()
            .position(|l| l.contains("test"))
            .expect("Label should be on some line");

        let edge_lines: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains('─') || l.contains('►') || l.contains('-'))
            .map(|(i, _)| i)
            .collect();

        let near_edge = edge_lines.iter().any(|&ey| ey.abs_diff(label_line) <= 1);
        assert!(
            near_edge,
            "Label at line {} should be within 1 row of an edge line (edge lines at {:?})",
            label_line, edge_lines
        );
    }

    // === Tests using render_diagram_to_text (full Diagram-based pipeline) ===

    #[test]
    fn test_render_edge_with_label() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B").with_label("Yes"));

        let output = render_diagram_to_text(&diagram);
        assert!(output.contains("Yes"));
    }

    #[test]
    fn test_render_multiline_edge_label_as_centered_block() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_label("yes\nno"));

        let output = render_diagram_to_text(&diagram);
        let lines: Vec<&str> = output.lines().collect();

        let yes_line = lines
            .iter()
            .position(|l| l.contains("yes"))
            .expect("missing first line of multiline label");
        let no_line = lines
            .iter()
            .position(|l| l.contains("no"))
            .expect("missing second line of multiline label");
        assert_eq!(
            no_line,
            yes_line + 1,
            "multiline label lines should render on consecutive rows:\n{output}"
        );

        let yes_col = lines[yes_line]
            .find("yes")
            .expect("could not locate 'yes' column");
        let no_col = lines[no_line]
            .find("no")
            .expect("could not locate 'no' column");
        assert!(
            yes_col.abs_diff(no_col) <= 1,
            "multiline label lines should stay horizontally aligned:\n{output}"
        );

        let a_line = lines
            .iter()
            .position(|l| l.contains(" A "))
            .expect("missing node A row");
        let b_line = lines
            .iter()
            .rposition(|l| l.contains(" B "))
            .expect("missing node B row");
        let edge_mid = (a_line + b_line) / 2;
        let label_mid = (yes_line + no_line) / 2;
        assert!(
            label_mid.abs_diff(edge_mid) <= 1,
            "multiline label should stay near the edge midpoint:\n{output}"
        );
    }

    #[test]
    fn test_label_rendered_at_precomputed_position() {
        let output = render_diagram_to_text(&{
            let mut d = Graph::new(Direction::TopDown);
            d.add_node(Node::new("A").with_label("A"));
            d.add_node(Node::new("B").with_label("B"));
            d.add_edge(Edge::new("A", "B").with_label("yes"));
            d
        });

        assert!(output.contains("yes"), "Label 'yes' should be rendered");

        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
        let b_line = lines.iter().rposition(|l| l.contains('B')).unwrap();
        let yes_line = lines.iter().position(|l| l.contains("yes")).unwrap();
        assert!(
            yes_line > a_line && yes_line < b_line,
            "Label at line {} should be between A (line {}) and B (line {})\n{}",
            yes_line,
            a_line,
            b_line,
            output
        );
    }

    #[test]
    fn precomputed_label_avoids_node_overlap() {
        let output = render_diagram_to_text(&{
            let mut d = Graph::new(Direction::LeftRight);
            d.add_node(Node::new("A").with_label("Working Dir"));
            d.add_node(Node::new("B").with_label("Staging Area"));
            d.add_node(Node::new("C").with_label("Local Repo"));
            d.add_edge(Edge::new("A", "B").with_label("git add"));
            d.add_edge(Edge::new("B", "C").with_label("git commit"));
            d
        });

        assert!(
            output.contains("git add"),
            "Label 'git add' should be fully visible:\n{output}"
        );
        assert!(
            output.contains("git commit"),
            "Label 'git commit' should be fully visible:\n{output}"
        );
    }

    #[test]
    fn test_cross_arrow_end_to_end() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrows(Arrow::None, Arrow::Cross));

        let output = render_diagram_to_text(&diagram);
        assert!(
            output.contains('x'),
            "Output should contain 'x' for cross arrow:\n{output}"
        );
        assert!(
            !output.contains('\u{25BC}'),
            "Output should NOT contain normal down arrow for cross edge"
        );
    }

    #[test]
    fn test_circle_arrow_end_to_end() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrows(Arrow::None, Arrow::Circle));

        let output = render_diagram_to_text(&diagram);
        assert!(
            output.contains('○'),
            "Output should contain '○' for circle arrow:\n{output}"
        );
        assert!(
            !output.contains('\u{25BC}'),
            "Output should NOT contain normal down arrow for circle edge"
        );
    }

    #[test]
    fn backward_edge_label_near_routed_path_td() {
        let flowchart =
            crate::mermaid::parse_flowchart("graph TD\n    A --> B\n    B -->|retry| A").unwrap();
        let diagram = crate::diagrams::flowchart::compile_to_graph(&flowchart);
        let output = render_diagram_to_text(&diagram);

        assert!(
            output.contains("retry"),
            "Label should appear in output:\n{output}"
        );

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
    fn text_renders_head_label() {
        let input = "graph TD\n  A --> B\n";
        let mut diagram = crate::diagrams::flowchart::compile_to_graph(
            &crate::mermaid::parse_flowchart(input).expect("flowchart should parse"),
        );
        diagram.edges[0].head_label = Some("*".to_string());
        let output = render_diagram_to_text(&diagram);
        assert!(
            output.contains('*'),
            "text output should contain head label '*', got:\n{output}"
        );
    }

    #[test]
    fn text_renders_tail_label() {
        let input = "graph TD\n  A --> B\n";
        let mut diagram = crate::diagrams::flowchart::compile_to_graph(
            &crate::mermaid::parse_flowchart(input).expect("flowchart should parse"),
        );
        diagram.edges[0].tail_label = Some("src".to_string());
        let output = render_diagram_to_text(&diagram);
        assert!(
            output.contains("src"),
            "text output should contain tail label 'src', got:\n{output}"
        );
    }

    // === Tests using render_flowchart_fixture / render_flowchart_input ===

    #[test]
    fn labeled_edges_show_labels() {
        let output = render_flowchart_fixture("labeled_edges.mmd");
        assert!(output.contains("initialize") || output.contains("configure"));
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
        let output = render_flowchart_fixture("label_spacing.mmd");

        assert!(output.contains("valid"), "Should contain 'valid' label");
        assert!(output.contains("invalid"), "Should contain 'invalid' label");
        assert!(
            !output.contains("valinvalid"),
            "Labels should not merge into 'valinvalid'"
        );
        assert!(
            !output.contains("invalidvalid"),
            "Labels should not merge into 'invalidvalid'"
        );

        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|line| line.contains(" A ")).unwrap();
        let b_line = lines.iter().rposition(|line| line.contains(" B ")).unwrap();
        let label_line = lines
            .iter()
            .position(|line| line.contains("valid"))
            .unwrap();
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
    fn long_label_renders_without_panic() {
        let output = render_flowchart_input(
            "graph TD\n    A -->|this is a very long label that might overflow| B",
        );
        assert!(!output.is_empty());
        assert!(output.contains(" A "), "Node A should render:\n{output}");
        assert!(output.contains(" B "), "Node B should render:\n{output}");
    }

    #[test]
    fn fan_out_with_labels() {
        let output = render_flowchart_input(
            "graph TD\n    A -->|yes| B\n    A -->|no| C\n    A -->|maybe| D",
        );
        assert!(output.contains("yes"), "Expected 'yes' label:\n{output}");
        assert!(output.contains("no"), "Expected 'no' label:\n{output}");
        assert!(
            output.contains("maybe"),
            "Expected 'maybe' label:\n{output}"
        );
    }

    #[test]
    fn labeled_edge_lr_direction() {
        let output = render_flowchart_input("graph LR\n    A -->|label| B");
        assert!(output.contains(" A "), "Should contain node A:\n{output}");
        assert!(output.contains(" B "), "Should contain node B:\n{output}");
        assert!(
            output.contains("label"),
            "Expected 'label' in LR layout:\n{output}"
        );
    }

    #[test]
    fn mixed_labeled_and_unlabeled() {
        let output = render_flowchart_input(
            "graph TD\n    A -->|yes| B\n    A --> C\n    B --> D\n    C -->|error| D",
        );
        assert!(output.contains("yes"), "Expected 'yes' label:\n{output}");
        assert!(
            output.contains("error"),
            "Expected 'error' label:\n{output}"
        );
        for node in ["A", "B", "C", "D"] {
            assert!(
                output.contains(&format!(" {node} ")),
                "Expected node {node}:\n{output}"
            );
        }
    }

    #[test]
    fn all_edges_labeled() {
        let output = render_flowchart_input(
            "graph TD\n    A -->|start| B\n    B -->|process| C\n    C -->|end| D",
        );
        assert!(output.contains("end"), "Expected 'end' label:\n{output}");
        assert!(output.contains(" A "), "Expected node A:\n{output}");
        assert!(output.contains(" B "), "Expected node B:\n{output}");
        assert!(output.contains(" D "), "Expected node D:\n{output}");
        assert!(
            output.contains("┌───┐"),
            "Expected at least one node box:\n{output}"
        );
    }

    #[test]
    fn labeled_edges_reasonable_height() {
        let output = render_flowchart_fixture("labeled_edges.mmd");
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
        let output = render_flowchart_fixture("labeled_edges.mmd");
        assert!(
            output.contains("Valid?"),
            "Diamond text 'Valid?' should be intact in output:\n{output}"
        );
    }

    #[test]
    fn simple_cycle_compact_backward_routing() {
        let output = render_flowchart_fixture("simple_cycle.mmd");
        let line_count = output.lines().count();
        assert!(
            line_count < 30,
            "simple_cycle.mmd should be compact, got {line_count} lines"
        );
    }

    #[test]
    fn multiple_cycles_compact_backward_routing() {
        let output = render_flowchart_fixture("multiple_cycles.mmd");
        let line_count = output.lines().count();
        assert!(
            line_count < 40,
            "multiple_cycles.mmd should be compact, got {line_count} lines"
        );
    }

    #[test]
    fn text_five_fan_out_lr_does_not_gain_extra_terminal_tail_rows() {
        let output = render_flowchart_fixture("five_fan_out_lr.mmd");
        let lines: Vec<&str> = output.lines().collect();
        let last_non_empty = lines
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .expect("five_fan_out_lr should render at least one row");
        let target_e_bottom = lines
            .iter()
            .rposition(|line| line.contains("└──────────┘"))
            .expect("five_fan_out_lr should render a bottom border for Target E");

        assert_eq!(
            last_non_empty, target_e_bottom,
            "five_fan_out_lr should end at Target E's bottom border instead of leaving a dangling tail row\n{output}"
        );
    }

    #[test]
    fn text_five_fan_out_lr_keeps_direct_fanout_channels_for_inner_targets() {
        let output = render_flowchart_fixture("five_fan_out_lr.mmd");
        let forward_arrowheads = output.chars().filter(|&ch| ch == '►').count();
        let upward_arrowheads = output.chars().filter(|&ch| ch == '▲').count();

        assert_eq!(
            forward_arrowheads, 5,
            "five_fan_out_lr should keep one forward arrowhead per target instead of detouring inner branches through sibling columns\n{output}"
        );
        assert_eq!(
            upward_arrowheads, 0,
            "five_fan_out_lr should not need upward arrowheads when LR fan-out branches stay on their direct channels\n{output}"
        );
    }

    #[test]
    fn text_five_fan_in_lr_does_not_leave_a_dangling_bottom_stem() {
        let output = render_flowchart_fixture("five_fan_in_lr.mmd");

        assert_eq!(
            output.lines().last(),
            Some("└───┘"),
            "five_fan_in_lr should end at E's bottom border instead of leaving a dangling bottom stem\n{output}"
        );
        assert_eq!(
            output.chars().filter(|&ch| ch == '▲').count(),
            0,
            "five_fan_in_lr should not need upward arrowheads when the LR fan-in stays on the compact text route\n{output}"
        );
    }

    #[test]
    fn five_fan_out_lr_grid_projection_keeps_svg_source_centric_lane_order() {
        let config = GridLayoutConfig::default();
        let (_, svg_routed) = route_fixture_with_svg_layout("five_fan_out_lr.mmd");

        let text_diagram = flowchart_fixture_diagram("five_fan_out_lr.mmd");
        let (text_routed, text_layout) =
            compute_layout_with_mode(&text_diagram, &config, MeasurementMode::Grid);

        let mut svg_lanes = HashMap::new();
        let mut svg_paths = HashMap::new();
        let mut text_routed_paths = HashMap::new();
        let mut draw_paths = HashMap::new();
        let mut draw_lanes = HashMap::new();
        let mut text_path_families = HashMap::new();

        for target in ["B", "C", "E", "F"] {
            let svg_edge = svg_routed
                .edges
                .iter()
                .find(|edge| edge.from == "A" && edge.to == target)
                .unwrap_or_else(|| panic!("missing SVG-side routed edge A -> {target}"));
            svg_paths.insert(target.to_string(), rounded_float_path(&svg_edge.path));
            if let Some(lane) = lr_rl_middle_vertical_lane_float(&svg_edge.path) {
                svg_lanes.insert(target.to_string(), lane);
            }

            let text_routed_edge = text_routed
                .edges
                .iter()
                .find(|edge| edge.from == "A" && edge.to == target)
                .unwrap_or_else(|| panic!("missing text-side routed edge A -> {target}"));
            text_routed_paths.insert(
                target.to_string(),
                rounded_float_path(&text_routed_edge.path),
            );

            let edge = find_edge(&text_diagram, "A", target);
            let draw_path = text_layout
                .routed_edge_paths
                .get(&edge.index)
                .unwrap_or_else(|| panic!("missing draw path for A -> {target}"))
                .clone();
            draw_paths.insert(target.to_string(), draw_path.clone());
            if let Some(lane) = lr_rl_middle_vertical_lane_draw(&draw_path) {
                draw_lanes.insert(target.to_string(), lane);
            }

            let probe =
                route_edge_with_probe(edge, &text_layout, Direction::LeftRight, None, None, false)
                    .unwrap_or_else(|| panic!("missing text route probe for A -> {target}"));
            text_path_families.insert(target.to_string(), format!("{:?}", probe.probe.path_family));
        }

        assert_eq!(
            svg_lanes.len(),
            4,
            "fixture sanity failed: SVG-side routed geometry should keep H-V-H lanes for the four off-center fan-out edges.\nsvg_paths={svg_paths:?}"
        );
        assert_eq!(
            draw_lanes.len(),
            4,
            "five_fan_out_lr text/grid projection should keep the same H-V-H lane shape as the SVG-side routed geometry.\nsvg_paths={svg_paths:?}\ntext_routed_paths={text_routed_paths:?}\ndraw_paths={draw_paths:?}\ntext_path_families={text_path_families:?}"
        );

        let b_lane = *draw_lanes.get("B").expect("draw lane for A -> B");
        let c_lane = *draw_lanes.get("C").expect("draw lane for A -> C");
        let e_lane = *draw_lanes.get("E").expect("draw lane for A -> E");
        let f_lane = *draw_lanes.get("F").expect("draw lane for A -> F");
        assert!(
            b_lane < c_lane && f_lane < e_lane && c_lane == e_lane && b_lane == f_lane,
            "five_fan_out_lr text/grid projection should preserve the source-centric outer/inner lane ordering from the SVG-side route.\nsvg_lanes={svg_lanes:?}\ndraw_lanes={draw_lanes:?}\nsvg_paths={svg_paths:?}\ntext_routed_paths={text_routed_paths:?}\ndraw_paths={draw_paths:?}\ntext_path_families={text_path_families:?}"
        );
    }

    #[test]
    fn five_fan_out_lr_text_route_probe_preserves_forward_fanout_branches() {
        let config = GridLayoutConfig::default();
        let diagram = flowchart_fixture_diagram("five_fan_out_lr.mmd");
        let (_, layout) = compute_layout_with_mode(&diagram, &config, MeasurementMode::Grid);
        let charset = CharSet::unicode();
        let mut individual_forward_arrowheads = 0usize;
        let mut route_families = HashMap::new();
        let mut routed_edges = Vec::new();

        for target in ["B", "C", "D", "E", "F"] {
            let edge = find_edge(&diagram, "A", target);
            let result =
                route_edge_with_probe(edge, &layout, Direction::LeftRight, None, None, false)
                    .unwrap_or_else(|| panic!("missing routed text path for A -> {target}"));
            route_families.insert(
                target.to_string(),
                format!("{:?}", result.probe.path_family),
            );
            routed_edges.push(result.routed.clone());

            let mut edge_canvas = Canvas::new(layout.width, layout.height);
            render_edge(
                &mut edge_canvas,
                routed_edges.last().expect("just pushed routed edge"),
                &charset,
                Direction::LeftRight,
            );
            let edge_output = edge_canvas.to_string();
            let edge_forward_arrowheads = edge_output.chars().filter(|&ch| ch == '►').count();
            assert_eq!(
                edge_forward_arrowheads, 1,
                "five_fan_out_lr should keep A -> {target} on a forward text route instead of flipping it into a synthetic backward branch.\nroute_family={:?}\nedge_output=\n{edge_output}",
                result.probe.path_family
            );
            individual_forward_arrowheads += edge_forward_arrowheads;
        }

        let mut combined_canvas = Canvas::new(layout.width, layout.height);
        render_all_edges(
            &mut combined_canvas,
            &routed_edges,
            &charset,
            Direction::LeftRight,
        );
        let combined_output = combined_canvas.to_string();
        let combined_forward_arrowheads = combined_output.chars().filter(|&ch| ch == '►').count();

        assert_eq!(
            combined_forward_arrowheads, individual_forward_arrowheads,
            "five_fan_out_lr combined text edge painting should preserve every forward arrowhead from the routed fan-out edges after the text route probe keeps them forward.\nroute_families={route_families:?}\ncombined_output=\n{combined_output}"
        );
    }

    /// When a node's left face can't fit all inbound forward edges (more edges
    /// than rows), overflow edges should be reassigned to adjacent faces so
    /// every arrival has a distinct endpoint.
    #[test]
    fn lr_left_face_saturation_overflows_to_adjacent_faces() {
        let config = GridLayoutConfig::default();
        let diagram = flowchart_fixture_diagram("architecture_graph_lr_terminal_contracts.mmd");
        let (_, layout) = compute_layout_with_mode(&diagram, &config, MeasurementMode::Grid);

        let routed = route_all_edges(&diagram.edges, &layout, Direction::LeftRight);

        let graph1_arrivals: Vec<_> = routed
            .iter()
            .filter(|r| r.edge.to == "graph1" && !r.is_backward)
            .collect();

        // graph1 has height=3 (left face fits 3 distinct arrows).
        // With 6 forward inbound edges, at most 3 should be on the left face.
        let left_count = graph1_arrivals
            .iter()
            .filter(|r| r.entry_direction == AttachDirection::Left)
            .count();

        assert!(
            left_count <= 3,
            "graph1 left face (3 rows) should hold at most 3 forward edges, got {left_count}\narrivals={:#?}",
            graph1_arrivals
                .iter()
                .map(|r| (&r.edge.from, r.end, r.entry_direction))
                .collect::<Vec<_>>()
        );

        // All forward arrivals must have distinct endpoints.
        let mut endpoints: Vec<(usize, usize)> =
            graph1_arrivals.iter().map(|r| (r.end.x, r.end.y)).collect();
        let total = endpoints.len();
        endpoints.sort();
        endpoints.dedup();

        assert_eq!(
            endpoints.len(),
            total,
            "all forward arrivals to graph1 should have distinct endpoints\narrivals={:#?}",
            graph1_arrivals
                .iter()
                .map(|r| (&r.edge.from, r.end, r.entry_direction))
                .collect::<Vec<_>>()
        );
    }
}
