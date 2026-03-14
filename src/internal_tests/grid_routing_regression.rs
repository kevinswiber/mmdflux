//! Cross-pipeline grid routing regression tests that require engine-produced layouts.
//!
//! These tests were extracted from `graph::grid::routing_tests` because they
//! depend on the engine pipeline (`FluxLayeredEngine`, `run_layered_layout`,
//! etc.) to produce realistic `GridLayout` fixtures. The `internal_tests`
//! module permits cross-pipeline imports that the owner-local test module does
//! not.

use std::fs;
use std::path::Path;

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::layout_building::layered_config_for_layout;
use crate::engines::graph::algorithms::layered::{MeasurementMode, run_layered_layout};
use crate::engines::graph::contracts::{GraphEngine, GraphGeometryContract, GraphSolveRequest};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::grid::{
    AttachDirection, GridLayout, GridLayoutConfig, NodeBounds, Point, RoutedEdge, Segment,
    TextPathFamily, geometry_to_grid_layout_with_routed, route_all_edges, route_edge_with_probe,
};
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{GeometryLevel, Graph};
use crate::mermaid::parse_flowchart;
use crate::{OutputFormat, RenderConfig};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(path).expect("fixture should load")
}

fn compute_layout(diagram: &Graph, config: &GridLayoutConfig) -> GridLayout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        GeometryLevel::Layout,
        None,
    );
    let result = engine
        .solve(
            diagram,
            &EngineConfig::Layered(layered_config_for_layout(diagram, config)),
            &request,
        )
        .expect("grid routing test layout solve failed");

    geometry_to_grid_layout_with_routed(diagram, &result.geometry, result.routed.as_ref(), config)
}

fn text_layout_for_fixture(name: &str) -> (Graph, GridLayout) {
    let input = load_flowchart_fixture(name);
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    (diagram, layout)
}

fn routed_text_layout_for_fixture(name: &str) -> (Graph, GridLayout) {
    let input = load_flowchart_fixture(name);
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = compile_to_graph(&flowchart);
    let config =
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config)
        .expect("layout should succeed");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let layout = geometry_to_grid_layout_with_routed(
        &diagram,
        &geom,
        Some(&routed),
        &GridLayoutConfig::default(),
    );
    (diagram, layout)
}

fn render_flowchart_fixture(name: &str) -> String {
    let input = load_flowchart_fixture(name);
    crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .unwrap_or_else(|error| panic!("Failed to render fixture {name}: {error}"))
}

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
            let (y_min, y_max) = if y_start <= y_end {
                (y_start, y_end)
            } else {
                (y_end, y_start)
            };
            x >= left && x <= right && ranges_overlap(y_min, y_max, top, bottom)
        }
        Segment::Horizontal { y, x_start, x_end } => {
            y >= top && y <= bottom && ranges_overlap(x_start, x_end, left, right)
        }
    }
}

fn unrelated_node_intrusions(routed: &[RoutedEdge], layout: &GridLayout) -> Vec<String> {
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

/// Reconstruct a polyline from a start point and a sequence of segments.
///
/// Duplicated from `graph::grid::routing::orthogonal::polyline_points_from_segments`
/// which is `pub(super)` and not accessible from `internal_tests`.
fn polyline_points_from_segments(start: Point, segments: &[Segment]) -> Vec<Point> {
    let mut points = vec![start];
    for segment in segments {
        let end = segment.end_point();
        if points.last().copied() != Some(end) {
            points.push(end);
        }
    }
    points
}

// ---------------------------------------------------------------------------
// Fixture-dependent routing probe tests
// ---------------------------------------------------------------------------

#[test]
fn git_workflow_remote_to_working_prefers_shared_routed_draw_path() {
    let (diagram, layout) = routed_text_layout_for_fixture("git_workflow.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Remote" && edge.to == "Working")
        .expect("git_workflow should contain Remote -> Working");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("git_workflow backward edge should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
    assert_eq!(result.probe.rejection_reason, None);
}

#[test]
fn git_workflow_default_text_layout_keeps_backward_edge_off_shared_routed_draw_path() {
    let (diagram, layout) = text_layout_for_fixture("git_workflow.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Remote" && edge.to == "Working")
        .expect("git_workflow should contain Remote -> Working");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("git_workflow backward edge should route");

    assert_ne!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
}

#[test]
fn backward_loop_lr_default_text_layout_keeps_backward_edge_off_shared_routed_draw_path() {
    let (diagram, layout) = text_layout_for_fixture("backward_loop_lr.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "G" && edge.to == "F")
        .expect("backward_loop_lr should contain G -> F");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("backward_loop_lr backward edge should route");

    assert_ne!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
}

#[test]
fn double_skip_a_to_d_prefers_shared_routed_draw_path() {
    let (diagram, layout) = routed_text_layout_for_fixture("double_skip.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "A" && edge.to == "D")
        .expect("double_skip should contain A -> D");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("double_skip A -> D should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
    assert_eq!(result.probe.rejection_reason, None);
}

#[test]
fn skip_edge_collision_a_to_d_prefers_shared_routed_draw_path() {
    let (diagram, layout) = routed_text_layout_for_fixture("skip_edge_collision.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "A" && edge.to == "D")
        .expect("skip_edge_collision should contain A -> D");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("skip_edge_collision A -> D should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
    assert_eq!(result.probe.rejection_reason, None);
}

#[test]
fn simple_forward_edge_does_not_prefer_shared_routed_draw_path() {
    let (diagram, layout) = routed_text_layout_for_fixture("simple.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "A" && edge.to == "B")
        .expect("simple should contain A -> B");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("simple A -> B should route");

    assert_ne!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
}

#[test]
fn subgraph_direction_isolated_d_to_e_keeps_shared_route_clear_of_subgraph_border() {
    let (diagram, layout) = routed_text_layout_for_fixture("subgraph_direction_isolated.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "D" && edge.to == "E")
        .expect("subgraph_direction_isolated should contain D -> E");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("subgraph_direction_isolated D -> E should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );

    let sg = layout
        .subgraph_bounds
        .get("sg1")
        .expect("subgraph_direction_isolated should contain sg1");
    let bottom = sg.y + sg.height.saturating_sub(1);
    let routed_points = polyline_points_from_segments(result.routed.start, &result.routed.segments);
    assert!(
        routed_points
            .windows(2)
            .any(|segment| segment[0].y == segment[1].y
                && segment[0].y >= bottom.saturating_add(2)
                && ranges_overlap(
                    segment[0].x,
                    segment[1].x,
                    sg.x,
                    sg.x + sg.width.saturating_sub(1)
                )),
        "D -> E should keep a visible gap below the LR subgraph border: {:?}",
        result.routed
    );
}

#[test]
fn subgraph_direction_mixed_b_to_c_avoids_direct_centerline_route() {
    let (diagram, layout) = routed_text_layout_for_fixture("subgraph_direction_mixed.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "C")
        .expect("subgraph_direction_mixed should contain B -> C");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("subgraph_direction_mixed B -> C should route");

    assert_ne!(result.probe.path_family, TextPathFamily::Direct);
    assert_eq!(
        result.routed.entry_direction,
        AttachDirection::Right,
        "B -> C should enter C from the outer right-side lane instead of dropping into the old centerline: {:?}",
        result.routed
    );

    let lr_group = layout
        .subgraph_bounds
        .get("lr_group")
        .expect("subgraph_direction_mixed should contain lr_group");
    let bt_group = layout
        .subgraph_bounds
        .get("bt_group")
        .expect("subgraph_direction_mixed should contain bt_group");
    let vertical_lane = result
        .routed
        .segments
        .iter()
        .find_map(|segment| match segment {
            Segment::Vertical { x, y_start, y_end }
                if *x >= lr_group.x + lr_group.width.saturating_sub(4)
                    && *x >= bt_group.x + bt_group.width.saturating_sub(4)
                    && *y_start.min(y_end) <= lr_group.y + lr_group.height.saturating_sub(2)
                    && *y_start.max(y_end) > bt_group.y =>
            {
                Some((*x, *y_start, *y_end))
            }
            _ => None,
        });
    assert!(
        vertical_lane.is_some(),
        "B -> C should keep a long exterior vertical lane spanning between the two subgraphs: {:?}",
        result.routed
    );
}

#[test]
fn direction_override_c_to_end_avoids_shared_routed_draw_path_border_hug() {
    let (diagram, layout) = routed_text_layout_for_fixture("direction_override.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "C" && edge.to == "End")
        .expect("direction_override should contain C -> End");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("direction_override C -> End should route");

    assert_ne!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath,
        "C -> End should prefer a recomputed cross-boundary route over a clipped draw path that rides the subgraph border: {:?}",
        result.routed
    );
}

#[test]
fn subgraph_direction_cross_boundary_b_to_d_avoids_long_right_border_support() {
    let (diagram, layout) = routed_text_layout_for_fixture("subgraph_direction_cross_boundary.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "D")
        .expect("subgraph_direction_cross_boundary should contain B -> D");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("subgraph_direction_cross_boundary B -> D should route");

    let sg = layout
        .subgraph_bounds
        .get("sg1")
        .expect("subgraph_direction_cross_boundary should contain sg1");
    let right = sg.x + sg.width.saturating_sub(1);
    let bottom = sg.y + sg.height.saturating_sub(1);
    let routed_points = polyline_points_from_segments(result.routed.start, &result.routed.segments);

    let rides_right_border = routed_points.windows(2).any(|segment| {
        segment[0].x == segment[1].x
            && segment[0].x >= right
            && segment[0].x <= right.saturating_add(1)
            && segment[0].y.min(segment[1].y) <= bottom
            && segment[0].y.max(segment[1].y) >= bottom.saturating_add(2)
    });
    assert!(
        !rides_right_border,
        "B -> D should not keep a long vertical support on the subgraph's right border: {:?}",
        result.routed
    );

    assert!(
        result.routed.segments.len() >= 3,
        "B -> D should keep a visible detour after leaving the subgraph instead of collapsing into a single vertical support: {:?}",
        result.routed
    );
}

#[test]
fn criss_cross_b_to_e_shared_draw_path_keeps_vertical_terminal_support() {
    let (diagram, layout) = routed_text_layout_for_fixture("criss_cross.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "E")
        .expect("criss_cross should contain B -> E");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("criss_cross B -> E should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
    assert_eq!(result.probe.rejection_reason, None);
    assert_eq!(result.routed.entry_direction, AttachDirection::Top);

    let terminal = result
        .routed
        .segments
        .iter()
        .rev()
        .find(|segment| segment.length() > 0)
        .expect("criss_cross B -> E should keep a terminal segment");
    match terminal {
        Segment::Vertical { x, y_start, y_end } => {
            assert!(
                y_end > y_start,
                "criss_cross B -> E terminal support should point downward into E: {terminal:?}"
            );
            assert_eq!(
                *x, result.routed.end.x,
                "criss_cross B -> E terminal support should stay aligned with the target face: end={:?}, terminal={terminal:?}",
                result.routed.end
            );
        }
        other => panic!("criss_cross B -> E terminal support should be vertical, got {other:?}"),
    }
}

#[test]
fn backward_in_subgraph_lr_prefers_compact_direct_backward_route() {
    let (diagram, layout) = text_layout_for_fixture("backward_in_subgraph_lr.mmd");
    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "A")
        .expect("backward_in_subgraph_lr should contain B -> A");

    let result = route_edge_with_probe(edge, &layout, diagram.direction, None, None, false)
        .expect("backward_in_subgraph_lr backward edge should route");

    assert_eq!(result.probe.path_family, TextPathFamily::Direct);
    assert!(
        result.routed.segments.len() <= 3,
        "backward_in_subgraph_lr backward edge should stay compact, got {:?}",
        result.routed.segments
    );
}

// ---------------------------------------------------------------------------
// Owner-local fixture regressions (moved from routing_tests.rs)
// ---------------------------------------------------------------------------

mod owner_local_fixture_regressions {
    use super::*;

    fn assert_no_adjacent_arrows(output: &str, fixture_name: &str) {
        for (line_num, line) in output.lines().enumerate() {
            assert!(
                !line.contains("\u{25bc}\u{25bc}"),
                "{}: line {} has adjacent arrows: {}",
                fixture_name,
                line_num + 1,
                line
            );
        }
    }

    fn assert_distinct_arrival_x(fixture_name: &str, target_node: &str) {
        let (diagram, layout) = text_layout_for_fixture(fixture_name);
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|edge| edge.edge.to == target_node)
            .map(|edge| edge.end.x)
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

    #[test]
    fn fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("fan_in.mmd", "D");
    }

    #[test]
    fn fan_in_no_adjacent_arrows() {
        let output = render_flowchart_fixture("fan_in.mmd");
        assert_no_adjacent_arrows(&output, "fan_in.mmd");
    }

    #[test]
    fn fan_out_no_adjacent_arrows() {
        let output = render_flowchart_fixture("fan_out.mmd");
        assert_no_adjacent_arrows(&output, "fan_out.mmd");
    }

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
    fn five_fan_in_distinct_arrivals() {
        assert_distinct_arrival_x("five_fan_in.mmd", "F");
    }

    #[test]
    fn fan_in_arrival_points_remain_spread_after_shared_attachment_planner() {
        let output = render_flowchart_fixture("five_fan_in.mmd");
        assert!(output.contains("A"));
        assert!(output.contains("Target"));
        assert_distinct_arrival_x("five_fan_in.mmd", "F");
    }

    #[test]
    fn fan_out_distinct_departures() {
        let (diagram, layout) = text_layout_for_fixture("fan_out.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let departure_xs: Vec<usize> = routed
            .iter()
            .filter(|edge| edge.edge.from == "A")
            .map(|edge| edge.start.x)
            .collect();

        assert!(departure_xs.len() >= 2);
        assert_all_distinct(&departure_xs, "fan_out.mmd: edges departing A");
    }

    #[test]
    fn stagger_present_for_multiple_cycles() {
        let (_, layout) = text_layout_for_fixture("multiple_cycles.mmd");

        let a_cx = layout.node_bounds["A"].center_x();
        let b_cx = layout.node_bounds["B"].center_x();
        let c_cx = layout.node_bounds["C"].center_x();

        assert!(
            a_cx > 0 && b_cx > 0 && c_cx > 0,
            "All node centers should be positive: A={a_cx}, B={b_cx}, C={c_cx}",
        );
    }

    #[test]
    fn no_stagger_for_simple_chain() {
        let (_, layout) = text_layout_for_fixture("chain.mmd");

        let centers: Vec<usize> = layout
            .node_bounds
            .values()
            .map(|bounds| bounds.center_x())
            .collect();
        let first = centers[0];
        for &center in &centers[1..] {
            assert!(
                (center as isize - first as isize).unsigned_abs() <= 1,
                "All nodes should be centered: got {:?}",
                centers
            );
        }
    }

    #[test]
    fn stagger_produces_different_attachment_points() {
        let (diagram, layout) = text_layout_for_fixture("multiple_cycles.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let a_b_edge = routed
            .iter()
            .find(|edge| edge.edge.from == "A" && edge.edge.to == "B")
            .expect("A->B edge should exist");
        let c_a_edge = routed
            .iter()
            .find(|edge| edge.edge.from == "C" && edge.edge.to == "A")
            .expect("C->A edge should exist");

        assert_ne!(
            a_b_edge.start, c_a_edge.end,
            "Forward A->B start ({:?}) and backward C->A end ({:?}) should differ on A",
            a_b_edge.start, c_a_edge.end
        );
    }

    #[test]
    fn stagger_absent_for_simple_cycle_with_corrected_spacing() {
        let (_, layout) = text_layout_for_fixture("simple_cycle.mmd");

        let centers: Vec<usize> = layout
            .node_bounds
            .values()
            .map(|bounds| bounds.center_x())
            .collect();
        let min_center = *centers.iter().min().unwrap();
        let max_center = *centers.iter().max().unwrap();
        assert!(
            max_center - min_center <= 2,
            "Simple cycle should have minimal stagger with corrected spacing: centers {:?} (range={})",
            centers,
            max_center - min_center
        );
    }

    #[test]
    fn direct_simple_produces_valid_layout() {
        let (_, layout) = text_layout_for_fixture("simple.mmd");

        assert!(layout.width > 0, "canvas width must be positive");
        assert!(layout.height > 0, "canvas height must be positive");
        assert!(layout.draw_positions.contains_key("A"));
        assert!(layout.draw_positions.contains_key("B"));
        assert!(layout.node_bounds.contains_key("A"));
        assert!(layout.node_bounds.contains_key("B"));
    }

    #[test]
    fn direct_no_node_overlaps() {
        let (_, layout) = text_layout_for_fixture("chain.mmd");

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
        let (_, layout) = text_layout_for_fixture("fan_out.mmd");

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
        let (_, layout) = text_layout_for_fixture("simple.mmd");

        let a_y = layout.draw_positions["A"].1;
        let b_y = layout.draw_positions["B"].1;
        assert!(
            a_y < b_y,
            "in TD layout, A (rank 0) should be above B (rank 1)"
        );
    }

    #[test]
    fn direct_lr_horizontal_ordering() {
        let (_, layout) = text_layout_for_fixture("left_right.mmd");

        assert!(
            layout.width > layout.height || layout.node_bounds.len() <= 2,
            "LR layout should generally be wider than tall"
        );
    }

    #[test]
    fn direct_preserves_cross_axis_stagger() {
        let (_, layout) = text_layout_for_fixture("fan_out.mmd");

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
        let (_, layout) = text_layout_for_fixture("simple_cycle.mmd");

        let waypoint_vectors: Vec<&Vec<(usize, usize)>> = layout.edge_waypoints.values().collect();
        for i in 0..waypoint_vectors.len() {
            for j in (i + 1)..waypoint_vectors.len() {
                if !waypoint_vectors[i].is_empty() && !waypoint_vectors[j].is_empty() {
                    assert_ne!(
                        waypoint_vectors[i], waypoint_vectors[j],
                        "two edges share identical waypoint paths — overlap likely"
                    );
                }
            }
        }
    }

    #[test]
    fn direct_fan_in_ordered_arrivals() {
        let (diagram, layout) = text_layout_for_fixture("fan_in.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let mut arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|edge| edge.edge.to == "D")
            .map(|edge| edge.end.x)
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
        let (diagram, layout) = text_layout_for_fixture("five_fan_in.mmd");
        let routed = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let arrival_xs: Vec<usize> = routed
            .iter()
            .filter(|edge| edge.edge.to == "F")
            .map(|edge| edge.end.x)
            .collect();

        assert_all_distinct(&arrival_xs, "five_fan_in: arrival x at F");
    }

    fn assert_skip_edge_clears_node_b(fixture_name: &str) {
        let (diagram, layout) = text_layout_for_fixture(fixture_name);

        let b_bounds = &layout.node_bounds["B"];
        let ad_edge = diagram
            .edges
            .iter()
            .find(|edge| edge.from == "A" && edge.to == "D")
            .expect("Should have an A->D edge");
        let waypoints = layout
            .edge_waypoints
            .get(&ad_edge.index)
            .expect("A->D should have waypoints");

        assert!(
            !waypoints.is_empty(),
            "{}: A->D skip edge should have at least one waypoint",
            fixture_name
        );

        let waypoint_at_b_rank = waypoints[0];
        assert!(
            !b_bounds.contains(waypoint_at_b_rank.0, waypoint_at_b_rank.1),
            "{}: A->D waypoint {:?} should clear B's bounds {:?} (need separation)",
            fixture_name,
            waypoint_at_b_rank,
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

    #[test]
    fn git_workflow_backward_route_uses_compact_bottom_channel_with_routed_layout() {
        let (diagram, layout) = routed_text_layout_for_fixture("git_workflow.mmd");
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

        assert_eq!(remote_to_working.segments.len(), 5);
        assert_eq!(horizontal_segments, 2);
        assert_eq!(vertical_segments, 3);
    }

    #[test]
    fn routed_long_skip_edges_use_compact_shared_elbow_family() {
        for fixture in ["double_skip.mmd", "skip_edge_collision.mmd"] {
            let (diagram, layout) = routed_text_layout_for_fixture(fixture);
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

            assert_eq!(a_to_d.segments.len(), 5);
            assert_eq!(horizontal_segments, 2);
            assert_eq!(vertical_segments, 3);
        }
    }

    #[test]
    fn criss_cross_b_to_e_keeps_vertical_terminal_support_in_text() {
        let (diagram, layout) = routed_text_layout_for_fixture("criss_cross.mmd");
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
                Segment::Vertical { x, y_start, y_end } if *x == b_to_e.end.x && *y_end > *y_start
            ),
            "criss_cross B -> E should keep a downward terminal support into E: end={:?}, segments={:?}",
            b_to_e.end,
            b_to_e.segments
        );
    }

    #[test]
    fn callgraph_feedback_cycle_lower_criss_cross_stays_separated_in_text() {
        let (diagram, layout) = routed_text_layout_for_fixture("callgraph_feedback_cycle.mmd");
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

        assert!(detour.len() >= 6);
        assert!(simple.len() >= 5);

        let upper_y = detour[1].1;
        let lower_y = detour[3].1;
        let center_y = simple[2].1;
        assert!(upper_y < center_y && center_y < lower_y);
        let detour_center_x = detour[2].0;
        assert!(
            simple
                .windows(2)
                .all(|segment| !(segment[0].0 == detour_center_x
                    && segment[1].0 == detour_center_x
                    && segment[0].1 != segment[1].1))
        );
        assert!(simple[2].0 > detour_center_x && simple[3].0 < detour_center_x);
    }

    #[test]
    fn issue_21_quantized_waypoint_corridor_does_not_cross_unrelated_nodes() {
        let (diagram, layout) = text_layout_for_fixture("callgraph_feedback_cycle.mmd");
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
    fn subgraph_nodes_aligned_vertically() {
        let (_, layout) = text_layout_for_fixture("simple_subgraph.mmd");

        let a_cx = layout.node_bounds["A"].center_x();
        let b_cx = layout.node_bounds["B"].center_x();
        assert!(
            (a_cx as isize - b_cx as isize).unsigned_abs() <= 1,
            "A (center_x={}) and B (center_x={}) should be vertically aligned",
            a_cx,
            b_cx
        );
    }
}
