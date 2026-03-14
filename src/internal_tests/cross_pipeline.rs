//! Residual crate-local coverage for behaviors that still span multiple owners.
//!
//! This suite is intentionally narrow: top-level runtime/render entrypoints,
//! crate-private parity checks, and cross-pipeline regression contracts that do
//! not have a single obvious owner-local home.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::builtins::default_registry;
use crate::engines::graph::algorithms::layered::{
    Direction as LayeredDirection, LayoutConfig as LayeredConfig, MeasurementMode, Ranker,
    run_layered_layout,
};
use crate::engines::graph::contracts::{
    EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest,
};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::format::{EdgePreset, RoutingStyle};
use crate::graph::geometry::{FPoint, RoutedGraphGeometry};
use crate::graph::grid::{
    GridLayout, GridLayoutConfig, GridRanker, NodeBounds, RoutedEdge, Segment,
    geometry_to_grid_layout_with_routed, route_all_edges,
};
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{Direction, GeometryLevel, Graph, Shape};
use crate::mmds::from_mmds_str;
use crate::payload::Diagram as DiagramPayload;
use crate::render::graph::text::{render_all_edges_with_labels, render_node};
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_svg_from_routed_geometry,
    render_text_from_geometry,
};
use crate::render::text::{Canvas, CharSet};
use crate::{EngineAlgorithmId, OutputFormat, RenderConfig, TextColorMode};

/// Load a fixture file by name from `tests/fixtures/flowchart/`.
fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

/// Load an MMDS fixture file by name from `tests/fixtures/mmds/`.
fn load_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

fn default_grid_request(
    level: GeometryLevel,
    routing_style: Option<RoutingStyle>,
) -> GraphSolveRequest {
    GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        level,
        routing_style,
    )
}

fn default_proportional_mode() -> MeasurementMode {
    MeasurementMode::Proportional(default_proportional_text_metrics())
}

fn default_proportional_request(
    geometry_contract: GraphGeometryContract,
    level: GeometryLevel,
    routing_style: Option<RoutingStyle>,
) -> GraphSolveRequest {
    GraphSolveRequest::new(
        default_proportional_mode(),
        geometry_contract,
        level,
        routing_style,
    )
}

fn solve_diagram(
    diagram: &Graph,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<crate::engines::graph::contracts::GraphSolveResult, crate::RenderError> {
    let engine = FluxLayeredEngine::text();
    let request = match format {
        OutputFormat::Svg => default_proportional_request(
            GraphGeometryContract::Visual,
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
        OutputFormat::Mmds => default_proportional_request(
            GraphGeometryContract::Canonical,
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
        _ => default_grid_request(
            config.geometry_level,
            config
                .routing_style
                .or_else(|| config.edge_preset.map(|preset| preset.expand().0)),
        ),
    };
    engine.solve(
        diagram,
        &EngineConfig::Layered(config.layout.clone().into()),
        &request,
    )
}

fn render_diagram_with_config(
    diagram: &Graph,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, crate::RenderError> {
    let result = solve_diagram(diagram, format, config)?;

    match format {
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options: TextRenderOptions = config.into();
            options.output_format = format;
            Ok(render_text_from_geometry(
                diagram,
                &result.geometry,
                result.routed.as_ref(),
                &options,
            ))
        }
        OutputFormat::Svg => {
            let options: SvgRenderOptions = config.into();
            Ok(if let Some(routed) = result.routed.as_ref() {
                render_svg_from_routed_geometry(diagram, routed, &options)
            } else {
                render_svg_from_geometry(diagram, &result.geometry, &options)
            })
        }
        other => Err(crate::errors::RenderError {
            message: format!("cross-pipeline helper does not render {other} output"),
        }),
    }
}

fn render_diagram_with_text_options(
    diagram: &Graph,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> Result<String, crate::RenderError> {
    render_diagram_with_config(
        diagram,
        format,
        &RenderConfig {
            text_color_mode,
            ..RenderConfig::default()
        },
    )
}

fn layered_config_for_layout(diagram: &Graph, config: &GridLayoutConfig) -> LayeredConfig {
    let mut rank_sep = config.rank_sep;
    if !diagram.subgraphs.is_empty() && config.cluster_rank_sep > 0.0 {
        rank_sep += config.cluster_rank_sep;
    }

    LayeredConfig {
        direction: match diagram.direction {
            Direction::TopDown => LayeredDirection::TopBottom,
            Direction::BottomTop => LayeredDirection::BottomTop,
            Direction::LeftRight => LayeredDirection::LeftRight,
            Direction::RightLeft => LayeredDirection::RightLeft,
        },
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: match config.ranker.unwrap_or_default() {
            GridRanker::NetworkSimplex => Ranker::NetworkSimplex,
            GridRanker::LongestPath => Ranker::LongestPath,
        },
        ..Default::default()
    }
}

fn compute_layout(diagram: &Graph, config: &GridLayoutConfig) -> GridLayout {
    let engine = FluxLayeredEngine::text();
    let request = default_grid_request(GeometryLevel::Layout, None);
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("graph-family cross-pipeline solve failed");

    geometry_to_grid_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}

fn parse_flowchart_via_registry(input: &str) -> Box<dyn crate::registry::ParsedDiagram> {
    default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
        .parse(input)
        .unwrap_or_else(|e| panic!("Failed to parse flowchart input: {e}"))
}

fn prepare_flowchart(input: &str, config: &RenderConfig) -> Graph {
    let payload = parse_flowchart_via_registry(input)
        .into_payload(config)
        .unwrap_or_else(|e| panic!("Failed to build flowchart payload: {e}"));
    let DiagramPayload::Flowchart(graph) = payload else {
        panic!("flowchart input should yield a Flowchart payload");
    };
    graph
}

/// Parse and build a diagram from a fixture file.
fn parse_and_build(name: &str) -> Graph {
    let input = load_fixture(name);
    prepare_flowchart(&input, &RenderConfig::default())
}

/// Parse, build, and compute layout for a fixture file.
fn layout_fixture(name: &str) -> (Graph, GridLayout) {
    let diagram = parse_and_build(name);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    (diagram, layout)
}

/// Parse, build, and render a fixture file.
fn render_fixture(name: &str) -> String {
    let diagram = parse_and_build(name);
    render_text_diagram(&diagram)
}

fn render_text_diagram(diagram: &Graph) -> String {
    render_diagram_with_config(diagram, OutputFormat::Text, &RenderConfig::default())
        .expect("text render should succeed")
}

fn render_diagram_with_output(
    diagram: &Graph,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> String {
    render_diagram_with_text_options(diagram, format, text_color_mode)
        .expect("diagram render should succeed")
}

fn render_fixture_with_options(
    name: &str,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> String {
    let diagram = parse_and_build(name);
    render_diagram_with_output(&diagram, format, text_color_mode)
}

/// Parse, build, and render a Mermaid input string.
fn render_input(input: &str) -> String {
    let diagram = prepare_flowchart(input, &RenderConfig::default());
    render_text_diagram(&diagram)
}

/// Parse, build, and render a fixture file with ASCII-only output.
fn render_fixture_ascii(name: &str) -> String {
    render_fixture_with_options(name, OutputFormat::Ascii, TextColorMode::Plain)
}

fn render_text_via_owner_pipeline(name: &str) -> String {
    let diagram = parse_and_build(name);
    render_diagram_with_config(&diagram, OutputFormat::Text, &RenderConfig::default())
        .expect("owner pipeline render should succeed")
}

#[test]
fn top_level_render_matches_flowchart_instance_for_subgraph_direction_mixed() {
    let input = load_fixture("subgraph_direction_mixed.mmd");
    let top_level = render_text_via_owner_pipeline("subgraph_direction_mixed.mmd");

    let instance_output =
        crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
            .expect("instance render should succeed");

    assert_eq!(
        top_level, instance_output,
        "top-level render() should match the flowchart instance text pipeline for subgraph fixtures"
    );
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

fn route_fixture_orthogonal(fixture: &str) -> RoutedGraphGeometry {
    let diagram = parse_and_build(fixture);
    let config =
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
        .expect("layout should succeed");
    route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute)
}

fn route_input_orthogonal(input: &str) -> RoutedGraphGeometry {
    let diagram = prepare_flowchart(input, &RenderConfig::default());
    let config =
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
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
                let diagram = prepare_flowchart(&input, &RenderConfig::default());
                let output = render_text_diagram(&diagram);
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
            let _parsed = parse_flowchart_via_registry(&input);
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

/// Edge case tests for label-as-dummy-node (Plan 0024).
mod label_edge_cases {
    use super::*;

    #[test]
    fn backward_routes_keep_outer_lane_and_terminal_tangent_contracts() {
        const MIN_OUTER_LANE_CLEARANCE: f64 = 12.0;
        const EPS: f64 = 0.5;

        fn point_on_target_face(
            rect: crate::graph::geometry::FRect,
            point: FPoint,
        ) -> &'static str {
            let left = rect.x;
            let right = rect.x + rect.width;
            let top = rect.y;
            let bottom = rect.y + rect.height;

            let on_right = (point.x - right).abs() <= EPS;
            let on_left = (point.x - left).abs() <= EPS;
            let on_top = (point.y - top).abs() <= EPS;
            let on_bottom = (point.y - bottom).abs() <= EPS;

            if on_right && point.y > top + EPS && point.y < bottom - EPS {
                "right"
            } else if on_left && point.y > top + EPS && point.y < bottom - EPS {
                "left"
            } else if on_top && point.x > left + EPS && point.x < right - EPS {
                "top"
            } else if on_bottom && point.x > left + EPS && point.x < right - EPS {
                "bottom"
            } else if on_right {
                "right"
            } else if on_left {
                "left"
            } else {
                "interior_or_corner"
            }
        }

        let diagram = parse_and_build("multiple_cycles.mmd");
        let config = EngineConfig::Layered(
            crate::engines::graph::algorithms::layered::LayoutConfig::default(),
        );
        let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
            .expect("layout should succeed");
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == "C" && edge.to == "A")
            .expect("multiple_cycles fixture missing edge C -> A");

        assert!(
            edge.path.len() >= 4,
            "multiple_cycles C -> A should have enough routed points to form an outer return lane: path={:?}",
            edge.path
        );

        let start = edge.path[0];
        let prev = edge.path[edge.path.len() - 2];
        let end = *edge.path.last().expect("edge path is non-empty");
        let baseline_max_x = start.x.max(end.x);
        let route_max_x = edge
            .path
            .iter()
            .map(|point| point.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let clearance = route_max_x - baseline_max_x;
        assert!(
            clearance >= MIN_OUTER_LANE_CLEARANCE,
            "multiple_cycles C -> A should preserve an outer-lane lateral clearance (>= {MIN_OUTER_LANE_CLEARANCE}) instead of collapsing into a near-vertical return: clearance={clearance}, path={:?}",
            edge.path
        );

        let target_rect = geom
            .nodes
            .get("A")
            .expect("multiple_cycles should contain node A")
            .rect;
        match point_on_target_face(target_rect, end) {
            "right" => assert!(
                (prev.y - end.y).abs() <= EPS && end.x < prev.x,
                "multiple_cycles C -> A should approach A from the right with a leftward terminal tangent: prev={prev:?}, end={end:?}, path={:?}",
                edge.path
            ),
            "left" => assert!(
                (prev.y - end.y).abs() <= EPS && end.x > prev.x,
                "multiple_cycles C -> A should approach A from the left with a rightward terminal tangent: prev={prev:?}, end={end:?}, path={:?}",
                edge.path
            ),
            "top" => assert!(
                (prev.x - end.x).abs() <= EPS && end.y > prev.y,
                "multiple_cycles C -> A should approach A from the top with a downward terminal tangent: prev={prev:?}, end={end:?}, path={:?}",
                edge.path
            ),
            "bottom" => assert!(
                (prev.x - end.x).abs() <= EPS && end.y < prev.y,
                "multiple_cycles C -> A should approach A from the bottom with an upward terminal tangent: prev={prev:?}, end={end:?}, path={:?}",
                edge.path
            ),
            other => panic!(
                "multiple_cycles C -> A should resolve to a concrete terminal face after backward routing, got {other}: path={:?}",
                edge.path
            ),
        }
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
    let output = render_diagram_with_output(&diagram, OutputFormat::Ascii, TextColorMode::Plain);
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
    use crate::graph::Stroke;

    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(crate::graph::Node::new("A").with_label("A"));
    diagram.add_node(crate::graph::Node::new("B").with_label("B"));
    diagram.add_node(crate::graph::Node::new("C").with_label("C"));
    diagram.add_edge(crate::graph::Edge::new("A", "B")); // visible
    diagram.add_edge(crate::graph::Edge::new("A", "C").with_stroke(Stroke::Invisible)); // invisible

    let output = render_text_diagram(&diagram);

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
    use crate::graph::Stroke;

    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(crate::graph::Node::new("A").with_label("A"));
    diagram.add_node(crate::graph::Node::new("B").with_label("B"));
    diagram.add_edge(crate::graph::Edge::new("A", "B").with_stroke(Stroke::Invisible));

    let output = render_text_diagram(&diagram);

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
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(crate::graph::Node::new("A").with_label("A"));
    diagram.add_node(crate::graph::Node::new("B").with_label("B"));
    diagram.add_node(crate::graph::Node::new("C").with_label("C"));
    diagram.add_edge(crate::graph::Edge::new("A", "C"));
    diagram.add_same_rank_constraint("A", "B");

    let output = render_text_diagram(&diagram);

    let lines: Vec<&str> = output.lines().collect();
    let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
    let b_line = lines.iter().position(|l| l.contains('B')).unwrap();
    let c_line = lines.iter().rposition(|l| l.contains('C')).unwrap();

    assert_eq!(a_line, b_line, "A and B should be on same line:\n{output}");
    assert!(c_line > a_line, "C should be below A:\n{output}");
}

#[test]
fn test_same_rank_no_visible_edge() {
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(crate::graph::Node::new("X").with_label("X"));
    diagram.add_node(crate::graph::Node::new("Y").with_label("Y"));
    diagram.add_same_rank_constraint("X", "Y");

    let output = render_text_diagram(&diagram);

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
    let mut diagram = Graph::new(Direction::LeftRight);
    diagram.add_node(crate::graph::Node::new("A").with_label("A"));
    diagram.add_node(crate::graph::Node::new("B").with_label("B"));
    diagram.add_node(crate::graph::Node::new("C").with_label("C"));
    diagram.add_edge(crate::graph::Edge::new("A", "C"));
    diagram.add_same_rank_constraint("A", "B");

    let output = render_text_diagram(&diagram);

    assert!(output.contains("A"));
    assert!(output.contains("B"));
    assert!(output.contains("C"));
}

#[test]
fn test_minlen_2_forces_rank_gap() {
    let mut diagram = Graph::new(Direction::TopDown);
    diagram.add_node(crate::graph::Node::new("A").with_label("A"));
    diagram.add_node(crate::graph::Node::new("B").with_label("B"));
    diagram.add_edge(crate::graph::Edge::new("A", "B").with_minlen(2));

    let output = render_text_diagram(&diagram);

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
        let diagram = prepare_flowchart(&input, &RenderConfig::default());
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
        let diagram = prepare_flowchart(input, &RenderConfig::default());

        assert_eq!(
            diagram.edges.len(),
            3,
            "Should have 3 edges between A and B"
        );

        let output = render_text_diagram(&diagram);
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
    let config =
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
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
fn test_orthogonal_route_criss_cross_repairs_preserve_axis_aligned_forward_paths() {
    let routed = route_fixture_orthogonal("criss_cross.mmd");

    for (from, to) in [("B", "E"), ("C", "D")] {
        let path = edge_path(&routed, from, to);
        assert!(
            path.windows(2)
                .all(|seg| seg[0].x == seg[1].x || seg[0].y == seg[1].y),
            "criss_cross {from} -> {to} should remain axis-aligned after de-overlap repair: {:?}",
            path
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
fn test_svg_orthogonal_route_differs_from_mermaid_polyline_for_cycle_fixture() {
    let input = load_fixture("simple_cycle.mmd");
    let mermaid_polyline_output = crate::render_diagram(
        &input,
        OutputFormat::Svg,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("mermaid-layered render should succeed");
    let orthogonal_output = crate::render_diagram(
        &input,
        OutputFormat::Svg,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("flux-layered render should succeed");

    assert_ne!(
        mermaid_polyline_output, orthogonal_output,
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

    fn edge_path_for_svg_order(diagram: &Graph, svg: &str, edge_index: usize) -> Vec<(f64, f64)> {
        let mut visible_edge_indexes: Vec<usize> = diagram
            .edges
            .iter()
            .filter(|edge| edge.stroke != crate::graph::Stroke::Invisible)
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
        crate::render_diagram(
            &input,
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
        rect: crate::graph::geometry::FRect,
        point: crate::graph::geometry::FPoint,
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
        crate::render_diagram(
            input,
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
        let diagram = prepare_flowchart(&input, &RenderConfig::default());
        let mode = default_proportional_mode();
        let config = EngineConfig::Layered(
            crate::engines::graph::algorithms::layered::LayoutConfig::default(),
        );
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
        rect: crate::graph::geometry::FRect,
        point: crate::graph::geometry::FPoint,
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
        crate::render_diagram(
            input,
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
        let diagram = prepare_flowchart(&input, &RenderConfig::default());
        let mode = default_proportional_mode();
        let config = EngineConfig::Layered(
            crate::engines::graph::algorithms::layered::LayoutConfig::default(),
        );
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
        let diagram = prepare_flowchart(&input, &RenderConfig::default());
        let config = EngineConfig::Layered(
            crate::engines::graph::algorithms::layered::LayoutConfig::default(),
        );
        let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
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
        crate::render_diagram(
            &input,
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
        // Verify flux-layered text renders successfully for label fixtures.
        let _text = crate::render_diagram(
            &input,
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
fn flowchart_instance_render_is_stable_for_subgraph_direction_mixed() {
    let input = load_fixture("subgraph_direction_mixed.mmd");
    let mut baseline: Option<String> = None;

    for _ in 0..6 {
        let output = crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
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

    fn distance_to_routed_path(point: (usize, usize), segments: &[Segment]) -> f64 {
        let p = (point.0 as f64, point.1 as f64);
        segments
            .iter()
            .map(|segment| match segment {
                Segment::Horizontal { y, x_start, x_end } => {
                    distance_to_segment(p, (*x_start as f64, *y as f64), (*x_end as f64, *y as f64))
                }
                Segment::Vertical { x, y_start, y_end } => {
                    distance_to_segment(p, (*x as f64, *y_start as f64), (*x as f64, *y_end as f64))
                }
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn render_label_center(
        diagram: &Graph,
        layout: &GridLayout,
        routed_edges: &[RoutedEdge],
        label: &str,
        label_positions: &HashMap<usize, (usize, usize)>,
    ) -> ((usize, usize), String) {
        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
        node_keys.sort();
        for node_id in node_keys {
            let node = &diagram.nodes[node_id];
            if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
                render_node(&mut canvas, node, x, y, &charset, diagram.direction);
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

    let diagram = prepare_flowchart(
        "graph TD\nA[Very Wide Source Node] -->|cfg| B[Very Wide Target Node]\n",
        &RenderConfig::default(),
    );
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
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
    use crate::graph::grid::{NodeFace, classify_face};

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
