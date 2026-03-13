use std::fs;
use std::path::Path;

use super::super::attachments::plan_attachments;
use super::attachment_resolution::{
    compute_attachment_plan as compute_attachment_plan_module,
    resolve_attachment_points as resolve_attachment_points_module,
};
use super::draw_path::{
    normalize_draw_path_points, repair_draw_path_segment_collisions, route_edge_from_draw_path,
};
use super::orthogonal::{
    build_orthogonal_path as build_orthogonal_path_module,
    compute_vertical_first_path as compute_vertical_first_path_module,
    polyline_points_from_segments,
};
use super::path_selection::should_use_routed_draw_path;
use super::route_variants::route_backward_with_synthetic_waypoints as route_backward_with_synthetic_waypoints_module;
use super::self_edges::route_self_edge as route_self_edge_module;
use super::*;
use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::layout_building::layered_config_for_layout;
use crate::engines::graph::algorithms::layered::{MeasurementMode, run_layered_layout};
use crate::engines::graph::contracts::{GraphEngine, GraphGeometryContract, GraphSolveRequest};
use crate::engines::graph::flux::FluxLayeredEngine;
use crate::graph::attachment::{
    Face, OverflowSide, canonical_backward_channel_face, classify_face_float, edge_faces,
    fan_in_overflow_face_for_slot, fan_in_primary_face_capacity, point_on_face_float,
    resolve_overflow_backward_channel_conflict,
};
use crate::graph::grid::{
    GridLayout, GridLayoutConfig, GridPos, NodeBounds, NodeFace, SelfEdgeDrawData, SubgraphBounds,
    geometry_to_grid_layout_with_routed,
};
use crate::graph::routing::{EdgeRouting, build_orthogonal_path_float, route_graph_geometry};
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Diagram, Direction, Edge, Node};
use crate::mermaid::parse_flowchart;
use crate::{OutputFormat, RenderConfig};

fn simple_td_diagram() -> Diagram {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram
}

fn load_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(path).expect("fixture should load")
}

fn routed_text_layout_for_fixture(name: &str) -> (Diagram, GridLayout) {
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

fn text_layout_for_fixture(name: &str) -> (Diagram, GridLayout) {
    let input = load_flowchart_fixture(name);
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = compile_to_graph(&flowchart);
    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    (diagram, layout)
}

fn compute_layout(diagram: &Diagram, config: &GridLayoutConfig) -> GridLayout {
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        crate::GeometryLevel::Layout,
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

#[test]
fn test_route_edge_straight_vertical() {
    let diagram = simple_td_diagram();
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let edge = &diagram.edges[0];
    let routed = route_edge(edge, &layout, Direction::TopDown, None, None, false).unwrap();

    // Should have at least one segment
    assert!(!routed.segments.is_empty());

    // For vertically aligned nodes, routing may coalesce the connector and main
    // runs into a single colinear segment. All remaining segments should be
    // vertical and share the same x coordinate.
    if routed.start.x == routed.end.x {
        assert!(
            !routed.segments.is_empty(),
            "Expected at least 1 segment, got {}",
            routed.segments.len()
        );
        for seg in &routed.segments {
            match seg {
                Segment::Vertical { x, .. } => {
                    assert_eq!(
                        *x, routed.start.x as usize,
                        "Vertical segment should be colinear with start/end"
                    );
                }
                _ => panic!(
                    "Expected all vertical segments for colinear nodes, got {:?}",
                    seg
                ),
            }
        }
    }
}

#[test]
fn test_route_edge_with_bend() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("Branch1"));
    diagram.add_node(Node::new("C").with_label("Branch2"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "C"));

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Route edge from A to C (which will be offset horizontally)
    let edge = &diagram.edges[1];
    let routed = route_edge(edge, &layout, Direction::TopDown, None, None, false).unwrap();

    // If nodes are not aligned, should have multiple segments
    if routed.start.x != routed.end.x {
        assert!(routed.segments.len() > 1);
    }
}

#[test]
fn test_route_all_edges() {
    let diagram = simple_td_diagram();
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let routed = route_all_edges(&diagram.edges, &layout, Direction::TopDown);

    assert_eq!(routed.len(), 1);
}

#[test]
fn test_attachment_directions_td() {
    let (out_dir, in_dir) = attachment_directions(Direction::TopDown);
    assert!(matches!(out_dir, AttachDirection::Bottom));
    assert!(matches!(in_dir, AttachDirection::Top));
}

#[test]
fn test_attachment_directions_lr() {
    let (out_dir, in_dir) = attachment_directions(Direction::LeftRight);
    assert!(matches!(out_dir, AttachDirection::Right));
    assert!(matches!(in_dir, AttachDirection::Left));
}

#[test]
fn test_point_creation() {
    let p = Point::new(10, 20);
    assert_eq!(p.x, 10);
    assert_eq!(p.y, 20);
}

#[test]
fn test_straight_vertical_path() {
    let start = Point::new(10, 5);
    let end = Point::new(10, 15);
    let segments = compute_vertical_first_path(start, end);

    assert_eq!(segments.len(), 1);
    match segments[0] {
        Segment::Vertical { x, y_start, y_end } => {
            assert_eq!(x, 10);
            assert_eq!(y_start, 5);
            assert_eq!(y_end, 15);
        }
        _ => panic!("Expected vertical segment"),
    }
}

#[test]
fn test_z_shaped_vertical_path() {
    let start = Point::new(5, 5);
    let end = Point::new(15, 15);
    let segments = compute_vertical_first_path(start, end);

    assert_eq!(segments.len(), 3);
    assert!(matches!(segments[0], Segment::Vertical { .. }));
    assert!(matches!(segments[1], Segment::Horizontal { .. }));
    assert!(matches!(segments[2], Segment::Vertical { .. }));
}

// Backward edge detection tests

fn make_bounds(x: usize, y: usize) -> NodeBounds {
    make_bounds_sized(x, y, 10, 3)
}

fn make_bounds_sized(x: usize, y: usize, width: usize, height: usize) -> NodeBounds {
    NodeBounds {
        x,
        y,
        width,
        height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

fn ranges_overlap(a1: usize, a2: usize, b1: usize, b2: usize) -> bool {
    let (a_min, a_max) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
    let (b_min, b_max) = if b1 <= b2 { (b1, b2) } else { (b2, b1) };
    a_min <= b_max && b_min <= a_max
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

fn minimal_layout(
    bounds: &[(&str, NodeBounds)],
    routed_paths: &[(usize, Vec<(usize, usize)>)],
) -> GridLayout {
    let node_bounds: std::collections::HashMap<String, NodeBounds> = bounds
        .iter()
        .map(|(id, bounds)| ((*id).to_string(), *bounds))
        .collect();
    let draw_positions: std::collections::HashMap<String, (usize, usize)> = bounds
        .iter()
        .map(|(id, bounds)| ((*id).to_string(), (bounds.x, bounds.y)))
        .collect();
    let grid_positions: std::collections::HashMap<String, GridPos> = bounds
        .iter()
        .enumerate()
        .map(|(idx, (id, _))| {
            (
                (*id).to_string(),
                GridPos {
                    layer: idx,
                    pos: idx,
                },
            )
        })
        .collect();
    let node_shapes: std::collections::HashMap<String, Shape> = bounds
        .iter()
        .map(|(id, _)| ((*id).to_string(), Shape::Rectangle))
        .collect();
    let routed_edge_paths: std::collections::HashMap<usize, Vec<(usize, usize)>> = routed_paths
        .iter()
        .map(|(edge_idx, points)| (*edge_idx, points.clone()))
        .collect();

    GridLayout {
        grid_positions,
        draw_positions,
        node_bounds,
        width: 120,
        height: 80,
        h_spacing: 4,
        v_spacing: 3,
        edge_waypoints: std::collections::HashMap::new(),
        routed_edge_paths,
        preserve_routed_path_topology: std::collections::HashSet::new(),
        edge_label_positions: std::collections::HashMap::new(),
        node_shapes,
        subgraph_bounds: std::collections::HashMap::new(),
        self_edges: Vec::new(),
        node_directions: std::collections::HashMap::new(),
    }
}

fn default_routing_overrides() -> RoutingOverrides {
    RoutingOverrides {
        src_attach: None,
        tgt_attach: None,
        src_face: None,
        tgt_face: None,
        src_first_vertical: false,
    }
}

#[test]
fn test_is_backward_edge_td_forward() {
    // In TD layout, source above target is forward
    let from = make_bounds(10, 0);
    let to = make_bounds(10, 10);
    assert!(!is_backward_edge(&from, &to, Direction::TopDown));
}

#[test]
fn test_is_backward_edge_td_backward() {
    // In TD layout, source below target is backward
    let from = make_bounds(10, 10);
    let to = make_bounds(10, 0);
    assert!(is_backward_edge(&from, &to, Direction::TopDown));
}

#[test]
fn test_is_backward_edge_bt_forward() {
    // In BT layout, source below target is forward
    let from = make_bounds(10, 10);
    let to = make_bounds(10, 0);
    assert!(!is_backward_edge(&from, &to, Direction::BottomTop));
}

#[test]
fn test_is_backward_edge_bt_backward() {
    // In BT layout, source above target is backward
    let from = make_bounds(10, 0);
    let to = make_bounds(10, 10);
    assert!(is_backward_edge(&from, &to, Direction::BottomTop));
}

#[test]
fn test_is_backward_edge_lr_forward() {
    // In LR layout, source left of target is forward
    let from = make_bounds(0, 10);
    let to = make_bounds(20, 10);
    assert!(!is_backward_edge(&from, &to, Direction::LeftRight));
}

#[test]
fn test_is_backward_edge_lr_backward() {
    // In LR layout, source right of target is backward
    let from = make_bounds(20, 10);
    let to = make_bounds(0, 10);
    assert!(is_backward_edge(&from, &to, Direction::LeftRight));
}

#[test]
fn test_is_backward_edge_rl_forward() {
    // In RL layout, source right of target is forward
    let from = make_bounds(20, 10);
    let to = make_bounds(0, 10);
    assert!(!is_backward_edge(&from, &to, Direction::RightLeft));
}

#[test]
fn test_is_backward_edge_rl_backward() {
    // In RL layout, source left of target is backward
    let from = make_bounds(0, 10);
    let to = make_bounds(20, 10);
    assert!(is_backward_edge(&from, &to, Direction::RightLeft));
}

#[test]
fn test_is_backward_edge_same_position() {
    // Same position is not backward (edge case)
    let from = make_bounds(10, 10);
    let to = make_bounds(10, 10);
    assert!(!is_backward_edge(&from, &to, Direction::TopDown));
    assert!(!is_backward_edge(&from, &to, Direction::BottomTop));
    assert!(!is_backward_edge(&from, &to, Direction::LeftRight));
    assert!(!is_backward_edge(&from, &to, Direction::RightLeft));
}

// Orthogonalization tests

#[test]
fn test_orthogonalize_segment_vertical() {
    // Vertical segment should stay vertical
    let from = Point::new(10, 5);
    let to = Point::new(10, 15);
    let segments = orthogonalize_segment(from, to, true);

    assert_eq!(segments.len(), 1);
    match segments[0] {
        Segment::Vertical { x, y_start, y_end } => {
            assert_eq!(x, 10);
            assert_eq!(y_start, 5);
            assert_eq!(y_end, 15);
        }
        _ => panic!("Expected vertical segment"),
    }
}

#[test]
fn test_orthogonalize_segment_horizontal() {
    // Horizontal segment should stay horizontal
    let from = Point::new(5, 10);
    let to = Point::new(20, 10);
    let segments = orthogonalize_segment(from, to, true);

    assert_eq!(segments.len(), 1);
    match segments[0] {
        Segment::Horizontal { y, x_start, x_end } => {
            assert_eq!(y, 10);
            assert_eq!(x_start, 5);
            assert_eq!(x_end, 20);
        }
        _ => panic!("Expected horizontal segment"),
    }
}

#[test]
fn test_orthogonalize_segment_diagonal_vertical_first() {
    // Diagonal segment with vertical-first preference
    let from = Point::new(5, 5);
    let to = Point::new(15, 20);
    let segments = orthogonalize_segment(from, to, true);

    assert_eq!(segments.len(), 2);
    // First: vertical from (5,5) to (5,20)
    match segments[0] {
        Segment::Vertical { x, y_start, y_end } => {
            assert_eq!(x, 5);
            assert_eq!(y_start, 5);
            assert_eq!(y_end, 20);
        }
        _ => panic!("Expected vertical segment first"),
    }
    // Second: horizontal from (5,20) to (15,20)
    match segments[1] {
        Segment::Horizontal { y, x_start, x_end } => {
            assert_eq!(y, 20);
            assert_eq!(x_start, 5);
            assert_eq!(x_end, 15);
        }
        _ => panic!("Expected horizontal segment second"),
    }
}

#[test]
fn test_orthogonalize_segment_diagonal_horizontal_first() {
    // Diagonal segment with horizontal-first preference
    let from = Point::new(5, 5);
    let to = Point::new(15, 20);
    let segments = orthogonalize_segment(from, to, false);

    assert_eq!(segments.len(), 2);
    // First: horizontal from (5,5) to (15,5)
    match segments[0] {
        Segment::Horizontal { y, x_start, x_end } => {
            assert_eq!(y, 5);
            assert_eq!(x_start, 5);
            assert_eq!(x_end, 15);
        }
        _ => panic!("Expected horizontal segment first"),
    }
    // Second: vertical from (15,5) to (15,20)
    match segments[1] {
        Segment::Vertical { x, y_start, y_end } => {
            assert_eq!(x, 15);
            assert_eq!(y_start, 5);
            assert_eq!(y_end, 20);
        }
        _ => panic!("Expected vertical segment second"),
    }
}

#[test]
fn test_orthogonalize_empty_waypoints() {
    let waypoints: Vec<(usize, usize)> = vec![];
    let segments = orthogonalize(&waypoints, Direction::TopDown);
    assert!(segments.is_empty());
}

#[test]
fn test_orthogonalize_single_waypoint() {
    // Single waypoint = no segments (need at least 2 points)
    let waypoints = vec![(10, 10)];
    let segments = orthogonalize(&waypoints, Direction::TopDown);
    assert!(segments.is_empty());
}

#[test]
fn test_orthogonalize_two_waypoints_aligned() {
    let waypoints = vec![(10, 5), (10, 15)];
    let segments = orthogonalize(&waypoints, Direction::TopDown);

    assert_eq!(segments.len(), 1);
    assert!(matches!(segments[0], Segment::Vertical { x: 10, .. }));
}

#[test]
fn test_orthogonalize_two_waypoints_diagonal() {
    let waypoints = vec![(5, 5), (15, 20)];
    let segments = orthogonalize(&waypoints, Direction::TopDown);

    // TD is vertical-first, so should be 2 segments
    assert_eq!(segments.len(), 2);
    assert!(matches!(segments[0], Segment::Vertical { .. }));
    assert!(matches!(segments[1], Segment::Horizontal { .. }));
}

#[test]
fn test_orthogonalize_three_waypoints() {
    let waypoints = vec![(5, 5), (15, 10), (25, 20)];
    let segments = orthogonalize(&waypoints, Direction::TopDown);

    // Two diagonal segments → 4 segments total (2 per diagonal)
    assert_eq!(segments.len(), 4);
}

#[test]
fn test_build_orthogonal_path_no_waypoints() {
    let start = Point::new(10, 5);
    let end = Point::new(20, 15);
    let waypoints: Vec<(usize, usize)> = vec![];

    let segments = build_orthogonal_path(start, &waypoints, end, Direction::TopDown);

    // Direct diagonal path → 2 segments (vertical-first for TD)
    assert_eq!(segments.len(), 2);
    assert!(matches!(segments[0], Segment::Vertical { .. }));
    assert!(matches!(segments[1], Segment::Horizontal { .. }));
}

#[test]
fn test_build_orthogonal_path_with_waypoints() {
    let start = Point::new(10, 5);
    let waypoints = vec![(15, 10), (20, 15)];
    let end = Point::new(25, 20);

    let segments = build_orthogonal_path(start, &waypoints, end, Direction::TopDown);

    // start→wp1: diagonal (2 segs), wp1→wp2: diagonal (2 segs), wp2→end: diagonal (2 segs)
    // Total: 6 segments
    assert_eq!(segments.len(), 6);
}

#[test]
fn test_build_orthogonal_path_aligned_waypoints() {
    let start = Point::new(10, 5);
    let waypoints = vec![(10, 10), (10, 15)]; // All on same x
    let end = Point::new(10, 20);

    let segments = build_orthogonal_path(start, &waypoints, end, Direction::TopDown);

    // All aligned vertically → 3 vertical segments
    assert_eq!(segments.len(), 3);
    for seg in segments {
        assert!(matches!(seg, Segment::Vertical { x: 10, .. }));
    }
}

#[test]
fn test_build_orthogonal_path_lr_direction() {
    let start = Point::new(5, 10);
    let end = Point::new(20, 15);
    let waypoints: Vec<(usize, usize)> = vec![];

    let segments = build_orthogonal_path(start, &waypoints, end, Direction::LeftRight);

    // LR uses horizontal-first but note: build_orthogonal_path uses
    // orthogonalize_segment (not build_orthogonal_path_for_direction),
    // so it produces H-V for LR (horizontal-first = !vertical_first)
    assert_eq!(segments.len(), 2);
    assert!(matches!(segments[0], Segment::Horizontal { .. }));
    assert!(matches!(segments[1], Segment::Vertical { .. }));
}

#[test]
fn attachment_plan_stays_stable_for_shared_face_edges() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("Left"));
    diagram.add_node(Node::new("C").with_label("Right"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "C"));

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let plan = compute_attachment_plan_module(&diagram.edges, &layout, Direction::TopDown);

    assert!(
        !plan.is_empty(),
        "shared-face routing should keep attachment overrides stable"
    );
}

#[test]
fn attachment_resolution_module_keeps_lr_consensus_y() {
    let from = make_bounds_sized(0, 2, 10, 3);
    let to = make_bounds_sized(20, 4, 10, 5);
    let ep = EdgeEndpoints {
        from_bounds: from,
        from_shape: Shape::Rectangle,
        to_bounds: to,
        to_shape: Shape::Rectangle,
    };

    let (src, tgt) = resolve_attachment_points_module(None, None, &ep, &[], Direction::LeftRight);

    assert_eq!(src.1, tgt.1);
}

#[test]
fn orthogonal_builder_module_preserves_vertical_first_z_path() {
    let start = Point::new(5, 5);
    let end = Point::new(15, 15);
    let vertical_first = compute_vertical_first_path_module(start, end);
    let through_waypoints =
        build_orthogonal_path_module(start, &[(10, 10)], end, Direction::TopDown);

    assert!(matches!(vertical_first[0], Segment::Vertical { .. }));
    assert!(matches!(through_waypoints[0], Segment::Vertical { .. }));
}

// Backward edge routing tests

#[test]
fn test_route_backward_edge_td() {
    // Create a diagram with a cycle: A -> B -> A
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B")); // Forward
    diagram.add_edge(Edge::new("B", "A")); // Backward

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Route the backward edge
    let backward_edge = &diagram.edges[1];
    let routed = route_edge(
        backward_edge,
        &layout,
        Direction::TopDown,
        None,
        None,
        false,
    )
    .unwrap();

    // Backward edge uses synthetic waypoints. With per-edge label spacing
    // (no labels here), nodes are at adjacent ranks — the backward path enters
    // the target from the right side.
    assert_eq!(routed.entry_direction, AttachDirection::Right);

    // Should have segments connecting B to A
    assert!(!routed.segments.is_empty());
}

#[test]
fn route_edge_reports_shared_routed_draw_path_when_backward_draw_path_is_used() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Bottom"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "A"));

    let source = make_bounds_sized(20, 20, 10, 3);
    let target = make_bounds_sized(20, 0, 10, 3);
    let draw_path = vec![(30, 21), (40, 21), (40, 1), (30, 1)];
    let layout = minimal_layout(
        &[("A", target), ("B", source)],
        &[(diagram.edges[1].index, draw_path)],
    );

    let result = route_edge_with_probe(
        &diagram.edges[1],
        &layout,
        Direction::TopDown,
        None,
        None,
        false,
    )
    .expect("backward routed draw path should route");

    assert_eq!(
        result.probe.path_family,
        TextPathFamily::SharedRoutedDrawPath
    );
    assert_eq!(result.probe.rejection_reason, None);
}

#[test]
fn lr_backward_waypoints_prefer_waypoint_inferred_bottom_faces() {
    let from = make_bounds_sized(20, 2, 10, 3);
    let to = make_bounds_sized(4, 2, 10, 3);
    let endpoints = EdgeEndpoints {
        from_bounds: from,
        from_shape: Shape::Rectangle,
        to_bounds: to,
        to_shape: Shape::Rectangle,
    };

    let waypoints = vec![(20, 7), (14, 7)];

    let (src_attach, tgt_attach) =
        resolve_attachment_points_module(None, None, &endpoints, &waypoints, Direction::LeftRight);

    assert_eq!(src_attach.1, from.y + from.height - 1);
    assert_eq!(tgt_attach.1, to.y + to.height - 1);
}

#[test]
fn route_edge_reports_rejection_reason_when_draw_path_hits_unrelated_node() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Bottom"));
    diagram.add_node(Node::new("C").with_label("Blocker"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "A"));

    let source = make_bounds_sized(20, 20, 10, 3);
    let target = make_bounds_sized(20, 0, 10, 3);
    let blocker = make_bounds_sized(38, 10, 8, 4);
    let draw_path = vec![(30, 21), (40, 21), (40, 1), (30, 1)];
    let layout = minimal_layout(
        &[("A", target), ("B", source), ("C", blocker)],
        &[(diagram.edges[1].index, draw_path)],
    );

    let result = route_edge_with_probe(
        &diagram.edges[1],
        &layout,
        Direction::TopDown,
        None,
        None,
        false,
    )
    .expect("backward edge should fall back after draw-path rejection");

    assert_eq!(result.probe.path_family, TextPathFamily::SyntheticBackward);
    assert_eq!(
        result.probe.rejection_reason,
        Some(TextPathRejection::SegmentCollision)
    );
}

#[test]
fn route_variants_module_routes_backward_waypoints() {
    let edge = Edge::new("B", "A");
    let endpoints = EdgeEndpoints {
        from_bounds: make_bounds_sized(20, 20, 10, 3),
        from_shape: Shape::Rectangle,
        to_bounds: make_bounds_sized(20, 0, 10, 3),
        to_shape: Shape::Rectangle,
    };

    let routed = route_backward_with_synthetic_waypoints_module(
        &edge,
        &endpoints,
        &[(35, 21), (35, 1)],
        Direction::TopDown,
        default_routing_overrides(),
    )
    .expect("synthetic backward waypoint route should succeed");

    assert!(routed.is_backward);
    assert!(!routed.segments.is_empty());
}

#[test]
fn self_edge_module_preserves_top_down_loop_entry_direction() {
    let edge = Edge::new("A", "A");
    let self_edge = SelfEdgeDrawData {
        node_id: "A".to_string(),
        edge_index: 0,
        points: vec![(10, 5), (14, 5), (14, 9), (10, 9), (10, 5)],
    };

    let routed = route_self_edge_module(&self_edge, &edge, Direction::TopDown);

    assert!(routed.is_self_edge);
    assert_eq!(routed.entry_direction, AttachDirection::Right);
}

#[test]
fn path_selection_prefers_structured_forward_routed_draw_path() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Bottom"));
    diagram.add_edge(Edge::new("A", "B"));

    let source = make_bounds_sized(20, 0, 10, 3);
    let target = make_bounds_sized(20, 20, 10, 3);
    let draw_path = vec![(25, 2), (25, 6), (35, 6), (35, 16), (25, 16), (25, 20)];
    let mut layout = minimal_layout(&[("A", source), ("B", target)], &[(0, draw_path)]);
    layout
        .edge_waypoints
        .insert(diagram.edges[0].index, vec![(25, 6), (35, 16)]);

    assert!(should_use_routed_draw_path(
        &diagram.edges[0],
        &layout,
        &source,
        &target,
        diagram.direction,
        None,
    ));
}

#[test]
fn draw_path_route_reports_segment_collision_from_helper_module() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Bottom"));
    diagram.add_node(Node::new("C").with_label("Blocker"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "A"));

    let source = make_bounds_sized(20, 20, 10, 3);
    let target = make_bounds_sized(20, 0, 10, 3);
    let blocker = make_bounds_sized(38, 10, 8, 4);
    let draw_path = vec![(30, 21), (40, 21), (40, 1), (30, 1)];
    let layout = minimal_layout(
        &[("A", target), ("B", source), ("C", blocker)],
        &[(diagram.edges[1].index, draw_path.clone())],
    );
    let endpoints = EdgeEndpoints {
        from_bounds: source,
        from_shape: Shape::Rectangle,
        to_bounds: target,
        to_shape: Shape::Rectangle,
    };

    let rejection = route_edge_from_draw_path(
        &diagram.edges[1],
        &layout,
        &endpoints,
        &draw_path,
        Direction::TopDown,
        default_routing_overrides(),
    )
    .expect_err("blocking draw path should reject instead of routing through blocker");

    assert_eq!(rejection, TextPathRejection::SegmentCollision);
}

#[test]
fn draw_path_collision_repair_detours_around_blocker() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Left"));
    diagram.add_node(Node::new("B").with_label("Right"));
    diagram.add_node(Node::new("C").with_label("Blocker"));
    diagram.add_edge(Edge::new("A", "B"));

    let source = make_bounds_sized(0, 4, 8, 3);
    let target = make_bounds_sized(20, 4, 8, 3);
    let blocker = make_bounds_sized(10, 4, 4, 3);
    let layout = minimal_layout(&[("A", source), ("B", target), ("C", blocker)], &[]);
    let repaired =
        repair_draw_path_segment_collisions(&[(7, 5), (21, 5)], &layout, &diagram.edges[0]);

    assert_eq!(repaired, vec![(7, 5), (7, 3), (21, 3), (21, 5)]);
}

#[test]
fn draw_path_normalization_repairs_terminal_staircase() {
    let normalized = normalize_draw_path_points(
        &[(12, 0), (12, 2), (12, 5), (18, 5), (18, 8)],
        Direction::TopDown,
    );

    assert_eq!(
        normalized,
        vec![(12, 0), (12, 2), (12, 4), (18, 4), (18, 8)]
    );
}

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
fn test_route_backward_edge_lr() {
    // Create a horizontal layout with a cycle
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B")); // Forward
    diagram.add_edge(Edge::new("B", "A")); // Backward

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Route the backward edge
    let backward_edge = &diagram.edges[1];
    let routed = route_edge(
        backward_edge,
        &layout,
        Direction::LeftRight,
        None,
        None,
        false,
    )
    .unwrap();

    // Short LR backward edges should stay compact: a lower side-lane direct
    // route is enough when no corridor obstruction exists.
    assert_eq!(routed.entry_direction, AttachDirection::Right);
    assert!(
        !routed.segments.is_empty(),
        "backward edge should produce at least one segment"
    );
    assert!(
        routed
            .segments
            .iter()
            .all(|segment| matches!(segment, Segment::Horizontal { .. })),
        "short LR backward edge should stay horizontal, got {:?}",
        routed.segments
    );
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

#[test]
fn compact_lr_backward_route_yields_to_subgraph_border_in_corridor() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "A"));

    let from = make_bounds_sized(20, 2, 8, 3);
    let to = make_bounds_sized(4, 2, 8, 3);
    let mut layout = minimal_layout(&[("A", to), ("B", from)], &[]);
    layout.subgraph_bounds.insert(
        "blocker".to_string(),
        SubgraphBounds {
            x: 14,
            y: 1,
            width: 5,
            height: 6,
            title: "Blocker".to_string(),
            depth: 0,
        },
    );

    let result = route_edge_with_probe(
        &diagram.edges[1],
        &layout,
        diagram.direction,
        None,
        None,
        false,
    )
    .expect(
        "compact LR backward edge should still route when a subgraph border blocks the direct lane",
    );

    assert_eq!(result.probe.path_family, TextPathFamily::SyntheticBackward);
}

#[test]
fn test_forward_edge_entry_direction_td() {
    // Forward edges should have standard entry direction
    let diagram = simple_td_diagram();
    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let edge = &diagram.edges[0];
    let routed = route_edge(edge, &layout, Direction::TopDown, None, None, false).unwrap();

    // TD forward edges enter from Top
    assert_eq!(routed.entry_direction, AttachDirection::Top);
}

#[test]
fn test_forward_edge_entry_direction_lr() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_label("Start"));
    diagram.add_node(Node::new("B").with_label("End"));
    diagram.add_edge(Edge::new("A", "B"));

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let edge = &diagram.edges[0];
    let routed = route_edge(edge, &layout, Direction::LeftRight, None, None, false).unwrap();

    // LR forward edges enter from Left
    assert_eq!(routed.entry_direction, AttachDirection::Left);
}

#[test]
fn test_multiple_backward_edges_route_successfully() {
    // Create diagram with two backward edges going to different targets
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Middle"));
    diagram.add_node(Node::new("C").with_label("Bottom"));
    diagram.add_edge(Edge::new("A", "B")); // Forward
    diagram.add_edge(Edge::new("B", "C")); // Forward
    diagram.add_edge(Edge::new("C", "A")); // Backward to A
    diagram.add_edge(Edge::new("C", "B")); // Backward to B

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    // Route both backward edges — they should both produce valid paths
    let edge_c_to_a = &diagram.edges[2];
    let edge_c_to_b = &diagram.edges[3];
    let routed_c_a = route_edge(edge_c_to_a, &layout, Direction::TopDown, None, None, false);
    let routed_c_b = route_edge(edge_c_to_b, &layout, Direction::TopDown, None, None, false);

    assert!(routed_c_a.is_some(), "Backward edge C->A should route");
    assert!(routed_c_b.is_some(), "Backward edge C->B should route");

    // Both should have segments
    assert!(!routed_c_a.unwrap().segments.is_empty());
    assert!(!routed_c_b.unwrap().segments.is_empty());
}

// --- Waypoint-based backward edge tests ---

#[test]
fn test_backward_edge_with_waypoints_td() {
    // Backward edge spanning 2+ ranks should use waypoints
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Middle"));
    diagram.add_node(Node::new("C").with_label("Bottom"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "C"));
    diagram.add_edge(Edge::new("C", "A")); // Backward spanning 2 ranks

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let backward_edge = &diagram.edges[2];
    let routed = route_edge(
        backward_edge,
        &layout,
        Direction::TopDown,
        None,
        None,
        false,
    )
    .unwrap();

    assert!(
        routed.segments.len() >= 2,
        "Backward edge should have routing segments, got {}",
        routed.segments.len()
    );
}

#[test]
fn test_short_backward_edge_uses_synthetic_waypoints() {
    // B→A backward edge spanning 1 rank — no dummies, no layout waypoints
    // With synthetic waypoints, should route around the right side of nodes
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_label("Top"));
    diagram.add_node(Node::new("B").with_label("Bottom"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "A")); // Backward, 1 rank

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let backward_edge = &diagram.edges[1];
    let routed = route_edge(
        backward_edge,
        &layout,
        Direction::TopDown,
        None,
        None,
        false,
    );
    assert!(routed.is_some(), "Backward edge should route successfully");

    let routed = routed.unwrap();
    // With synthetic waypoints routing around the right side, we should see a
    // multi-segment orthogonal path rather than a direct connector.
    assert!(
        routed.segments.len() >= 3,
        "Backward edge with synthetic waypoints should have >= 3 segments, got {}",
        routed.segments.len()
    );
}

#[test]
fn test_backward_edge_lr_with_waypoints() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_label("Left"));
    diagram.add_node(Node::new("B").with_label("Mid"));
    diagram.add_node(Node::new("C").with_label("Right"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("B", "C"));
    diagram.add_edge(Edge::new("C", "A")); // Backward, spans 2 ranks

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);

    let backward_edge = &diagram.edges[2];
    let routed = route_edge(
        backward_edge,
        &layout,
        Direction::LeftRight,
        None,
        None,
        false,
    );
    assert!(
        routed.is_some(),
        "LR backward edge should route successfully"
    );
}

#[test]
fn test_backward_edge_expands_canvas_for_routing() {
    // Backward edges add canvas margin for synthetic waypoint routing
    let mut diagram_with_cycle = Diagram::new(Direction::TopDown);
    diagram_with_cycle.add_node(Node::new("A").with_label("Top"));
    diagram_with_cycle.add_node(Node::new("B").with_label("Bottom"));
    diagram_with_cycle.add_edge(Edge::new("A", "B"));
    diagram_with_cycle.add_edge(Edge::new("B", "A")); // Backward

    let mut diagram_no_cycle = Diagram::new(Direction::TopDown);
    diagram_no_cycle.add_node(Node::new("A").with_label("Top"));
    diagram_no_cycle.add_node(Node::new("B").with_label("Bottom"));
    diagram_no_cycle.add_edge(Edge::new("A", "B"));

    let config = GridLayoutConfig::default();
    let layout_cycle = compute_layout(&diagram_with_cycle, &config);
    let layout_no_cycle = compute_layout(&diagram_no_cycle, &config);

    assert!(
        layout_cycle.width > layout_no_cycle.width,
        "Backward edge should expand canvas width for routing margin. With cycle: {}, without: {}",
        layout_cycle.width,
        layout_no_cycle.width
    );
}

// --- LR zero-gap entry direction test ---

#[test]
fn test_lr_zero_gap_entry_direction() {
    // When compute_layout_direct places nodes with minimal gap, the
    // offset start and end points can coincide. The entry direction
    // should still match the layout direction (Left for LR).
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("Input").with_label("User Input"));
    diagram.add_node(Node::new("Process").with_label("Process Data"));
    diagram.add_node(Node::new("Output").with_label("Display Result"));
    diagram.add_edge(Edge::new("Input", "Process"));
    diagram.add_edge(Edge::new("Process", "Output"));

    let config = GridLayoutConfig::default();
    let layout = compute_layout(&diagram, &config);
    let routed_edges = route_all_edges(&diagram.edges, &layout, Direction::LeftRight);

    for routed in &routed_edges {
        assert_eq!(
            routed.entry_direction,
            AttachDirection::Left,
            "LR edge {}->{} should enter from Left, not {:?}",
            routed.edge.from,
            routed.edge.to,
            routed.entry_direction,
        );
    }
}

// --- Consensus-Y tests for LR/RL attachment points (Task 1.1) ---

#[test]
fn test_lr_attachment_consensus_y_same_height() {
    let from = make_bounds_sized(0, 2, 10, 3);
    let to = make_bounds_sized(20, 4, 10, 3);
    let ep = EdgeEndpoints {
        from_bounds: from,
        from_shape: Shape::Rectangle,
        to_bounds: to,
        to_shape: Shape::Rectangle,
    };
    let (src, tgt) = resolve_attachment_points_module(None, None, &ep, &[], Direction::LeftRight);
    assert_eq!(
        src.1, tgt.1,
        "LR attachment points should have consensus y, got src.y={} tgt.y={}",
        src.1, tgt.1
    );
}

#[test]
fn test_lr_attachment_consensus_y_different_height() {
    let from = make_bounds_sized(0, 2, 10, 3);
    let to = make_bounds_sized(20, 3, 10, 5);
    let ep = EdgeEndpoints {
        from_bounds: from,
        from_shape: Shape::Rectangle,
        to_bounds: to,
        to_shape: Shape::Rectangle,
    };
    let (src, tgt) = resolve_attachment_points_module(None, None, &ep, &[], Direction::LeftRight);
    assert_eq!(
        src.1, tgt.1,
        "LR attachment points should have consensus y even with different heights, got src.y={} tgt.y={}",
        src.1, tgt.1
    );
}

#[test]
fn test_rl_attachment_consensus_y() {
    let from = make_bounds_sized(20, 2, 10, 3);
    let to = make_bounds_sized(0, 4, 10, 3);
    let ep = EdgeEndpoints {
        from_bounds: from,
        from_shape: Shape::Rectangle,
        to_bounds: to,
        to_shape: Shape::Rectangle,
    };
    let (src, tgt) = resolve_attachment_points_module(None, None, &ep, &[], Direction::RightLeft);
    assert_eq!(
        src.1, tgt.1,
        "RL attachment points should have consensus y, got src.y={} tgt.y={}",
        src.1, tgt.1
    );
}

// --- generate_backward_waypoints tests ---

#[test]
fn test_generate_backward_waypoints_td() {
    // TD layout: source (B) at y=6, target (A) at y=0 — backward
    let src = make_bounds_sized(4, 6, 8, 3);
    let tgt = make_bounds_sized(4, 0, 8, 3);

    let waypoints = generate_backward_waypoints(&src, &tgt, Direction::TopDown);

    assert!(!waypoints.is_empty(), "should produce waypoints");
    // Waypoints should be to the right of both nodes
    let max_right = (src.x + src.width).max(tgt.x + tgt.width);
    for wp in &waypoints {
        assert!(
            wp.0 > max_right,
            "waypoint x={} should be right of nodes (max_right={})",
            wp.0,
            max_right
        );
    }
}

#[test]
fn test_generate_backward_waypoints_lr() {
    // LR layout: source (B) at x=12, target (A) at x=0 — backward
    let src = make_bounds_sized(12, 2, 8, 3);
    let tgt = make_bounds_sized(0, 2, 8, 3);

    let waypoints = generate_backward_waypoints(&src, &tgt, Direction::LeftRight);

    assert!(!waypoints.is_empty(), "should produce waypoints");
    // Waypoints should be below both nodes
    let max_bottom = (src.y + src.height).max(tgt.y + tgt.height);
    for wp in &waypoints {
        assert!(
            wp.1 > max_bottom,
            "waypoint y={} should be below nodes (max_bottom={})",
            wp.1,
            max_bottom
        );
    }
}

#[test]
fn test_generate_backward_waypoints_forward_returns_empty() {
    // Forward edge in TD: src above target — not backward
    let src = make_bounds_sized(4, 0, 8, 3);
    let tgt = make_bounds_sized(4, 6, 8, 3);

    let waypoints = generate_backward_waypoints(&src, &tgt, Direction::TopDown);
    assert!(
        waypoints.is_empty(),
        "forward edge should return empty waypoints"
    );
}

#[test]
fn test_generate_backward_waypoints_bt() {
    // BT layout: source at y=0 (visually bottom), target at y=6 (visually top) — backward
    let src = make_bounds_sized(4, 0, 8, 3);
    let tgt = make_bounds_sized(4, 6, 8, 3);

    let waypoints = generate_backward_waypoints(&src, &tgt, Direction::BottomTop);

    assert!(
        !waypoints.is_empty(),
        "should produce waypoints for BT backward"
    );
    let max_right = (src.x + src.width).max(tgt.x + tgt.width);
    for wp in &waypoints {
        assert!(wp.0 > max_right, "BT waypoint should be right of nodes");
    }
}

#[test]
fn test_generate_backward_waypoints_rl() {
    // RL layout: source at x=0, target at x=12 — backward
    let src = make_bounds_sized(0, 2, 8, 3);
    let tgt = make_bounds_sized(12, 2, 8, 3);

    let waypoints = generate_backward_waypoints(&src, &tgt, Direction::RightLeft);

    assert!(
        !waypoints.is_empty(),
        "should produce waypoints for RL backward"
    );
    let max_bottom = (src.y + src.height).max(tgt.y + tgt.height);
    for wp in &waypoints {
        assert!(wp.1 > max_bottom, "RL waypoint should be below nodes");
    }
}

// === Segment helper tests (Task 1.1) ===

#[test]
fn vertical_segment_length() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.length(), 10);
}

#[test]
fn vertical_segment_length_reversed() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 20,
        y_end: 10,
    };
    assert_eq!(seg.length(), 10);
}

#[test]
fn horizontal_segment_length() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 5,
        x_end: 15,
    };
    assert_eq!(seg.length(), 10);
}

#[test]
fn horizontal_segment_length_reversed() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 15,
        x_end: 5,
    };
    assert_eq!(seg.length(), 10);
}

#[test]
fn zero_length_segment() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 10,
    };
    assert_eq!(seg.length(), 0);
}

#[test]
fn start_point_vertical() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.start_point(), Point { x: 5, y: 10 });
}

#[test]
fn end_point_vertical() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.end_point(), Point { x: 5, y: 20 });
}

#[test]
fn start_point_horizontal() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 5,
        x_end: 15,
    };
    assert_eq!(seg.start_point(), Point { x: 5, y: 3 });
}

#[test]
fn end_point_horizontal() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 5,
        x_end: 15,
    };
    assert_eq!(seg.end_point(), Point { x: 15, y: 3 });
}

#[test]
fn point_at_offset_zero_is_start() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.point_at_offset(0), seg.start_point());
}

#[test]
fn point_at_offset_length_is_end() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.point_at_offset(seg.length()), seg.end_point());
}

#[test]
fn point_at_offset_midpoint_vertical() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.point_at_offset(5), Point { x: 5, y: 15 });
}

#[test]
fn point_at_offset_midpoint_horizontal() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 0,
        x_end: 10,
    };
    assert_eq!(seg.point_at_offset(5), Point { x: 5, y: 3 });
}

#[test]
fn point_at_offset_reversed_vertical() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 20,
        y_end: 10,
    };
    assert_eq!(seg.point_at_offset(5), Point { x: 5, y: 15 });
}

#[test]
fn point_at_offset_reversed_horizontal() {
    let seg = Segment::Horizontal {
        y: 3,
        x_start: 15,
        x_end: 5,
    };
    assert_eq!(seg.point_at_offset(5), Point { x: 10, y: 3 });
}

#[test]
fn point_at_offset_clamped_beyond_length() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 20,
    };
    assert_eq!(seg.point_at_offset(100), seg.end_point());
}

#[test]
fn point_at_offset_zero_length_segment() {
    let seg = Segment::Vertical {
        x: 5,
        y_start: 10,
        y_end: 10,
    };
    assert_eq!(seg.point_at_offset(0), Point { x: 5, y: 10 });
}

#[test]
fn routing_core_edge_faces_all_directions_forward_and_backward() {
    assert_eq!(
        edge_faces(Direction::TopDown, false),
        (Face::Bottom, Face::Top)
    );
    assert_eq!(
        edge_faces(Direction::TopDown, true),
        (Face::Top, Face::Bottom)
    );

    assert_eq!(
        edge_faces(Direction::BottomTop, false),
        (Face::Top, Face::Bottom)
    );
    assert_eq!(
        edge_faces(Direction::BottomTop, true),
        (Face::Bottom, Face::Top)
    );

    assert_eq!(
        edge_faces(Direction::LeftRight, false),
        (Face::Right, Face::Left)
    );
    assert_eq!(
        edge_faces(Direction::LeftRight, true),
        (Face::Left, Face::Right)
    );

    assert_eq!(
        edge_faces(Direction::RightLeft, false),
        (Face::Left, Face::Right)
    );
    assert_eq!(
        edge_faces(Direction::RightLeft, true),
        (Face::Right, Face::Left)
    );
}

#[test]
fn routing_core_classify_face_float_prefers_major_axis() {
    let center = FPoint::new(50.0, 50.0);
    let rect = FRect::new(40.0, 40.0, 20.0, 20.0);
    assert_eq!(
        classify_face_float(center, rect, FPoint::new(50.0, 10.0)),
        Face::Top
    );
    assert_eq!(
        classify_face_float(center, rect, FPoint::new(90.0, 50.0)),
        Face::Right
    );
}

#[test]
fn routing_core_point_on_face_float_uses_fraction_and_clamps() {
    let rect = FRect::new(10.0, 20.0, 40.0, 20.0);
    assert_eq!(
        point_on_face_float(rect, Face::Top, 0.0),
        FPoint::new(10.0, 20.0)
    );
    assert_eq!(
        point_on_face_float(rect, Face::Top, 1.0),
        FPoint::new(50.0, 20.0)
    );
    assert_eq!(
        point_on_face_float(rect, Face::Right, -2.0),
        FPoint::new(50.0, 20.0)
    );
    assert_eq!(
        point_on_face_float(rect, Face::Left, 2.0),
        FPoint::new(10.0, 40.0)
    );
}

#[test]
fn routing_core_face_conversions_are_explicit_and_lossless() {
    assert_eq!(Face::from_node_face(NodeFace::Top), Face::Top);
    assert_eq!(Face::from_node_face(NodeFace::Bottom), Face::Bottom);
    assert_eq!(Face::from_node_face(NodeFace::Left), Face::Left);
    assert_eq!(Face::from_node_face(NodeFace::Right), Face::Right);

    assert_eq!(Face::Top.to_node_face(), NodeFace::Top);
    assert_eq!(Face::Bottom.to_node_face(), NodeFace::Bottom);
    assert_eq!(Face::Left.to_node_face(), NodeFace::Left);
    assert_eq!(Face::Right.to_node_face(), NodeFace::Right);
}

#[test]
fn routing_core_build_orthogonal_path_float_emits_axis_aligned_segments() {
    let points = build_orthogonal_path_float(
        FPoint::new(10.0, 10.0),
        FPoint::new(80.0, 60.0),
        Direction::TopDown,
        &[],
    );

    assert!(points.windows(2).all(|seg| {
        (seg[0].x - seg[1].x).abs() < f64::EPSILON || (seg[0].y - seg[1].y).abs() < f64::EPSILON
    }));
}

#[test]
fn overflow_backward_channel_precedence_prefers_backward_channel_over_overflow_target_slot() {
    assert_eq!(
        fan_in_primary_face_capacity(Direction::TopDown),
        4,
        "TD/Bt primary capacity should remain in sync with contract"
    );
    assert_eq!(
        fan_in_primary_face_capacity(Direction::LeftRight),
        2,
        "LR/RL primary capacity should remain in sync with contract"
    );

    assert_eq!(
        canonical_backward_channel_face(Direction::TopDown),
        Face::Right,
        "TD backward canonical channel should be right"
    );
    assert_eq!(
        canonical_backward_channel_face(Direction::LeftRight),
        Face::Bottom,
        "LR backward canonical channel should be bottom"
    );

    assert_eq!(
        fan_in_overflow_face_for_slot(Direction::TopDown, OverflowSide::LeftOrTop),
        Face::Left,
        "TD overflow slot 0 should map to left"
    );
    assert_eq!(
        fan_in_overflow_face_for_slot(Direction::TopDown, OverflowSide::RightOrBottom),
        Face::Right,
        "TD overflow slot 1 should map to right"
    );

    assert_eq!(
        resolve_overflow_backward_channel_conflict(
            Direction::TopDown,
            true,
            true,
            Some(Face::Right),
            Face::Right
        ),
        Face::Right
    );
    assert_eq!(
        resolve_overflow_backward_channel_conflict(
            Direction::TopDown,
            true,
            true,
            Some(Face::Top),
            Face::Top
        ),
        Face::Right
    );
    assert_eq!(
        resolve_overflow_backward_channel_conflict(
            Direction::TopDown,
            false,
            false,
            Some(Face::Right),
            Face::Right
        ),
        Face::Right
    );
}

#[test]
fn overflow_backward_channel_precedence_resolver_is_stable_and_direction_aware() {
    let cases = [
        (
            Direction::TopDown,
            true,
            true,
            Some(Face::Right),
            Face::Right,
            Face::Right,
        ),
        (
            Direction::TopDown,
            false,
            true,
            Some(Face::Right),
            Face::Right,
            Face::Left,
        ),
        (
            Direction::TopDown,
            false,
            true,
            Some(Face::Left),
            Face::Left,
            Face::Left,
        ),
        (
            Direction::LeftRight,
            true,
            true,
            Some(Face::Bottom),
            Face::Bottom,
            Face::Bottom,
        ),
        (
            Direction::LeftRight,
            false,
            true,
            Some(Face::Bottom),
            Face::Bottom,
            Face::Top,
        ),
        (
            Direction::LeftRight,
            false,
            false,
            Some(Face::Bottom),
            Face::Bottom,
            Face::Bottom,
        ),
    ];

    for (direction, is_backward, has_backward_conflict, overflow_face, proposed, expected) in cases
    {
        let first = resolve_overflow_backward_channel_conflict(
            direction,
            is_backward,
            has_backward_conflict,
            overflow_face,
            proposed,
        );
        let second = resolve_overflow_backward_channel_conflict(
            direction,
            is_backward,
            has_backward_conflict,
            overflow_face,
            proposed,
        );
        assert_eq!(
            first, expected,
            "resolver returned wrong face for direction={direction:?}, backward={is_backward}, conflict={has_backward_conflict}, overflow={overflow_face:?}, proposed={proposed:?}"
        );
        assert_eq!(
            first, second,
            "resolver must be deterministic across repeated evaluation"
        );
    }
}

#[test]
fn plan_attachments_spreads_edges_monotonically_on_same_face() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A"));
    diagram.add_node(Node::new("B"));
    diagram.add_node(Node::new("C"));
    diagram.add_node(Node::new("D"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "C"));
    diagram.add_edge(Edge::new("A", "D"));

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let plan = plan_attachments(&diagram.edges, &layout, Direction::TopDown);
    let fractions = plan.source_fractions_for("A", Face::Bottom);

    assert!(
        fractions.windows(2).all(|w| w[0] <= w[1]),
        "source fractions must be monotonic: {fractions:?}"
    );
}

#[test]
fn plan_attachments_is_stable_for_equal_cross_axis_positions() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A"));
    diagram.add_node(Node::new("B"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "B"));

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let first = plan_attachments(&diagram.edges, &layout, Direction::TopDown);
    let second = plan_attachments(&diagram.edges, &layout, Direction::TopDown);

    assert_eq!(first, second);
}

#[test]
fn shared_planner_adapter_spreads_fan_in_arrivals() {
    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A"));
    diagram.add_node(Node::new("B"));
    diagram.add_node(Node::new("C"));
    diagram.add_node(Node::new("D"));
    diagram.add_node(Node::new("E"));
    diagram.add_node(Node::new("F"));
    diagram.add_edge(Edge::new("A", "B"));
    diagram.add_edge(Edge::new("A", "C"));
    diagram.add_edge(Edge::new("A", "D"));
    diagram.add_edge(Edge::new("A", "E"));
    diagram.add_edge(Edge::new("A", "F"));

    let layout = compute_layout(&diagram, &GridLayoutConfig::default());
    let overrides =
        compute_attachment_plan_from_shared_planner(&diagram.edges, &layout, Direction::TopDown);

    let mut src_x: Vec<usize> = diagram
        .edges
        .iter()
        .filter(|edge| edge.from == "A")
        .filter_map(|edge| {
            overrides
                .get(&edge.index)
                .and_then(|ov| ov.source.map(|p| p.0))
        })
        .collect();
    src_x.sort_unstable();
    src_x.dedup();

    assert!(
        src_x.len() > 1,
        "fan-in/fan-out source attachments should be spread: {src_x:?}"
    );
}

mod owner_local_fixture_regressions {
    use super::*;

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
            .expect("A→B edge should exist");
        let c_a_edge = routed
            .iter()
            .find(|edge| edge.edge.from == "C" && edge.edge.to == "A")
            .expect("C→A edge should exist");

        assert_ne!(
            a_b_edge.start, c_a_edge.end,
            "Forward A→B start ({:?}) and backward C→A end ({:?}) should differ on A",
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

        let waypoint_at_b_rank = waypoints[0];
        assert!(
            !b_bounds.contains(waypoint_at_b_rank.0, waypoint_at_b_rank.1),
            "{}: A→D waypoint {:?} should clear B's bounds {:?} (need separation)",
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
