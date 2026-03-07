//! Contract tests for the routed geometry pipeline.
//!
//! Verifies that `route_graph_geometry` produces correct `RoutedGraphGeometry`
//! from engine-produced `GraphGeometry`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use mmdflux::diagram::EdgeRouting;
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::geometry::*;
use mmdflux::diagrams::flowchart::routing::{route_graph_geometry, snap_path_to_grid_preview};
use mmdflux::{EngineConfig, OutputFormat, RenderConfig, build_diagram, parse_flowchart};

/// Flux-layered LayoutConfig with all enhancements enabled.
fn flux_layout_config() -> EngineConfig {
    EngineConfig::Layered(mmdflux::layered::types::LayoutConfig {
        greedy_switch: true,
        model_order_tiebreak: true,
        variable_rank_spacing: true,
        track_reversed_chains: true,
        ..mmdflux::layered::types::LayoutConfig::default()
    })
}

/// Parse input and produce (Diagram, GraphGeometry) via the layout engine.
fn layout_test(input: &str) -> (mmdflux::Diagram, GraphGeometry) {
    let fc = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&fc);
    let config = flux_layout_config();
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();
    (diagram, geom)
}

fn layout_fixture(name: &str) -> (mmdflux::Diagram, GraphGeometry) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    layout_test(&input)
}

fn layout_test_svg(input: &str) -> (mmdflux::Diagram, GraphGeometry) {
    let fc = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&fc);
    let mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
    let config = flux_layout_config();
    let geom = run_layered_layout(&mode, &diagram, &config).unwrap();
    (diagram, geom)
}

fn layout_fixture_svg(name: &str) -> (mmdflux::Diagram, GraphGeometry) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    layout_test_svg(&input)
}

const ROUTE_EPS: f64 = 0.000_001;

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= ROUTE_EPS
}

fn segment_is_axis_aligned(a: FPoint, b: FPoint) -> bool {
    approx_eq(a.x, b.x) || approx_eq(a.y, b.y)
}

fn segment_is_non_degenerate(a: FPoint, b: FPoint) -> bool {
    !approx_eq(a.x, b.x) || !approx_eq(a.y, b.y)
}

fn points_approx_equal(a: FPoint, b: FPoint) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
}

fn bend_count(path: &[FPoint]) -> usize {
    path.len().saturating_sub(2)
}

fn point_distance(a: FPoint, b: FPoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn distance_point_to_segment(point: FPoint, a: FPoint, b: FPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= ROUTE_EPS {
        return point_distance(point, a);
    }

    let projection = ((point.x - a.x) * dx + (point.y - a.y) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = FPoint::new(a.x + t * dx, a.y + t * dy);
    point_distance(point, closest)
}

fn distance_point_to_path(point: FPoint, path: &[FPoint]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return point_distance(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

#[derive(Debug)]
struct RoutedStyleMonitorReport {
    scanned_styled_edges: usize,
    violations: Vec<String>,
    summary_line: String,
}

fn min_segment_len(path: &[FPoint]) -> f64 {
    path.windows(2)
        .map(|segment| point_distance(segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

fn style_segment_monitor_report_for_routed_geometry(
    fixtures: &[&str],
    min_segment_threshold: f64,
) -> RoutedStyleMonitorReport {
    use mmdflux::graph::Stroke;

    let mut scanned_styled_edges = 0usize;
    let mut violations = Vec::new();

    for fixture in fixtures {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        for edge in diagram
            .edges
            .iter()
            .filter(|edge| matches!(edge.stroke, Stroke::Dotted | Stroke::Thick))
        {
            let routed_edge = routed
                .edges
                .iter()
                .find(|candidate| candidate.index == edge.index)
                .unwrap_or_else(|| {
                    panic!(
                        "fixture {fixture} should route styled edge index {}",
                        edge.index
                    )
                });
            let min_segment = min_segment_len(&routed_edge.path);
            scanned_styled_edges += 1;
            if min_segment < min_segment_threshold {
                violations.push(format!(
                    "{fixture} {}->{} stroke={:?} min_segment={min_segment:.2} threshold={min_segment_threshold:.2}",
                    edge.from, edge.to, edge.stroke
                ));
            }
        }
    }

    RoutedStyleMonitorReport {
        scanned_styled_edges,
        summary_line: format!(
            "style_monitor_routed scanned={} violations={} threshold={:.2}",
            scanned_styled_edges,
            violations.len(),
            min_segment_threshold
        ),
        violations,
    }
}

fn labeled_edge_label_drift_failures(
    diagram: &mmdflux::Diagram,
    routed: &RoutedGraphGeometry,
    max_distance: f64,
) -> Vec<String> {
    let mut failures = Vec::new();
    for edge in diagram.edges.iter().filter(|edge| edge.label.is_some()) {
        let routed_edge = routed
            .edges
            .iter()
            .find(|candidate| candidate.index == edge.index)
            .unwrap_or_else(|| panic!("missing routed edge for index {}", edge.index));
        let Some(label_position) = routed_edge.label_position else {
            failures.push(format!(
                "{} -> {} (index {}) has edge label but no routed label_position",
                edge.from, edge.to, edge.index
            ));
            continue;
        };
        let drift = distance_point_to_path(label_position, &routed_edge.path);
        if drift > max_distance {
            failures.push(format!(
                "{} -> {} label {:?} drift={drift:.2} exceeds {max_distance:.2}; label_position={label_position:?}, path={:?}",
                edge.from,
                edge.to,
                edge.label,
                routed_edge.path
            ));
        }
    }
    failures
}

fn point_inside_rect(rect: FRect, point: FPoint) -> bool {
    let eps = 0.01;
    point.x > rect.x + eps
        && point.x < rect.x + rect.width - eps
        && point.y > rect.y + eps
        && point.y < rect.y + rect.height - eps
}

/// Check if an axis-aligned segment strictly passes through the interior of a rect.
/// Returns true when any interior portion of the segment overlaps the rect's interior.
fn axis_aligned_segment_crosses_rect_interior(a: FPoint, b: FPoint, rect: FRect) -> bool {
    let eps = 0.5;
    let left = rect.x + eps;
    let right = rect.x + rect.width - eps;
    let top = rect.y + eps;
    let bottom = rect.y + rect.height - eps;
    if left >= right || top >= bottom {
        return false;
    }

    // Horizontal segment
    if (a.y - b.y).abs() < eps {
        let seg_y = a.y;
        if seg_y <= top || seg_y >= bottom {
            return false;
        }
        let seg_min_x = a.x.min(b.x);
        let seg_max_x = a.x.max(b.x);
        seg_max_x > left && seg_min_x < right
    }
    // Vertical segment
    else if (a.x - b.x).abs() < eps {
        let seg_x = a.x;
        if seg_x <= left || seg_x >= right {
            return false;
        }
        let seg_min_y = a.y.min(b.y);
        let seg_max_y = a.y.max(b.y);
        seg_max_y > top && seg_min_y < bottom
    } else {
        false
    }
}

fn point_on_target_face(rect: FRect, point: FPoint) -> &'static str {
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

fn face_corner_inset_margin(rect: FRect, point: FPoint) -> Option<f64> {
    let eps = 0.5;
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    let on_top = (point.y - top).abs() <= eps;
    let on_bottom = (point.y - bottom).abs() <= eps;
    let on_left = (point.x - left).abs() <= eps;
    let on_right = (point.x - right).abs() <= eps;

    if on_top || on_bottom {
        return Some((point.x - left).min(right - point.x));
    }
    if on_left || on_right {
        return Some((point.y - top).min(bottom - point.y));
    }
    None
}

fn terminal_support_is_normal_to_attached_rect_face(
    rect: FRect,
    prev: FPoint,
    end: FPoint,
) -> bool {
    let eps = 0.01;
    let on_top = (end.y - rect.y).abs() <= eps;
    let on_bottom = (end.y - (rect.y + rect.height)).abs() <= eps;
    let on_left = (end.x - rect.x).abs() <= eps;
    let on_right = (end.x - (rect.x + rect.width)).abs() <= eps;

    let vertical_segment = (prev.x - end.x).abs() <= eps && (prev.y - end.y).abs() > eps;
    let horizontal_segment = (prev.y - end.y).abs() <= eps && (prev.x - end.x).abs() > eps;

    (on_top || on_bottom) && vertical_segment || (on_left || on_right) && horizontal_segment
}

fn source_support_is_normal_to_attached_rect_face(
    rect: FRect,
    start: FPoint,
    next: FPoint,
) -> bool {
    let eps = 0.01;
    let on_top = (start.y - rect.y).abs() <= eps;
    let on_bottom = (start.y - (rect.y + rect.height)).abs() <= eps;
    let on_left = (start.x - rect.x).abs() <= eps;
    let on_right = (start.x - (rect.x + rect.width)).abs() <= eps;

    let vertical_segment = (start.x - next.x).abs() <= eps && (start.y - next.y).abs() > eps;
    let horizontal_segment = (start.y - next.y).abs() <= eps && (start.x - next.x).abs() > eps;

    (on_top || on_bottom) && vertical_segment || (on_left || on_right) && horizontal_segment
}

fn has_coincident_horizontal_overlap(path_a: &[FPoint], path_b: &[FPoint]) -> bool {
    const EPS: f64 = 0.5;
    for seg_a in path_a.windows(2) {
        let a0 = seg_a[0];
        let a1 = seg_a[1];
        let a_is_horizontal = (a0.y - a1.y).abs() <= EPS && (a0.x - a1.x).abs() > EPS;
        if !a_is_horizontal {
            continue;
        }
        let a_min_x = a0.x.min(a1.x);
        let a_max_x = a0.x.max(a1.x);
        for seg_b in path_b.windows(2) {
            let b0 = seg_b[0];
            let b1 = seg_b[1];
            let b_is_horizontal = (b0.y - b1.y).abs() <= EPS && (b0.x - b1.x).abs() > EPS;
            if !b_is_horizontal || (a0.y - b0.y).abs() > EPS {
                continue;
            }
            let b_min_x = b0.x.min(b1.x);
            let b_max_x = b0.x.max(b1.x);
            let overlap = a_max_x.min(b_max_x) - a_min_x.max(b_min_x);
            if overlap > EPS {
                return true;
            }
        }
    }
    false
}

fn has_coincident_vertical_overlap(path_a: &[FPoint], path_b: &[FPoint]) -> bool {
    const EPS: f64 = 0.5;
    for seg_a in path_a.windows(2) {
        let a0 = seg_a[0];
        let a1 = seg_a[1];
        let a_is_vertical = (a0.x - a1.x).abs() <= EPS && (a0.y - a1.y).abs() > EPS;
        if !a_is_vertical {
            continue;
        }
        let a_min_y = a0.y.min(a1.y);
        let a_max_y = a0.y.max(a1.y);
        for seg_b in path_b.windows(2) {
            let b0 = seg_b[0];
            let b1 = seg_b[1];
            let b_is_vertical = (b0.x - b1.x).abs() <= EPS && (b0.y - b1.y).abs() > EPS;
            if !b_is_vertical || (a0.x - b0.x).abs() > EPS {
                continue;
            }
            let b_min_y = b0.y.min(b1.y);
            let b_max_y = b0.y.max(b1.y);
            let overlap = a_max_y.min(b_max_y) - a_min_y.max(b_min_y);
            if overlap > EPS {
                return true;
            }
        }
    }
    false
}

fn td_bt_middle_horizontal_lane(path: &[FPoint]) -> Option<f64> {
    const EPS: f64 = 0.5;
    if path.len() != 4 {
        return None;
    }
    let first_vertical =
        (path[0].x - path[1].x).abs() <= EPS && (path[0].y - path[1].y).abs() > EPS;
    let middle_horizontal =
        (path[1].y - path[2].y).abs() <= EPS && (path[1].x - path[2].x).abs() > EPS;
    let terminal_vertical =
        (path[2].x - path[3].x).abs() <= EPS && (path[2].y - path[3].y).abs() > EPS;
    (first_vertical && middle_horizontal && terminal_vertical).then_some(path[1].y)
}

fn lr_rl_middle_vertical_lane(path: &[FPoint]) -> Option<f64> {
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

fn td_bt_primary_horizontal_lane(path: &[FPoint]) -> Option<f64> {
    const EPS: f64 = 0.5;
    let mut best: Option<(f64, f64)> = None;
    for seg in path.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let is_horizontal = (a.y - b.y).abs() <= EPS && (a.x - b.x).abs() > EPS;
        if !is_horizontal {
            continue;
        }
        let len = (a.x - b.x).abs();
        let replace = match best {
            Some((_, best_len)) => len > best_len,
            None => true,
        };
        if replace {
            best = Some((a.y, len));
        }
    }
    best.map(|(y, _)| y)
}

fn path_has_source_turnback_spike(path: &[FPoint]) -> bool {
    if path.len() < 4 {
        return false;
    }

    let p0 = path[0];
    let p1 = path[1];
    let p2 = path[2];

    points_approx_equal(p0, p2)
        && segment_is_axis_aligned(p0, p1)
        && segment_is_axis_aligned(p1, p2)
        && segment_is_non_degenerate(p0, p1)
        && segment_is_non_degenerate(p1, p2)
}

fn path_has_immediate_axial_turnback(path: &[FPoint]) -> bool {
    if path.len() < 3 {
        return false;
    }

    path.windows(3).any(|w| {
        let a = w[0];
        let b = w[1];
        let c = w[2];
        if !segment_is_axis_aligned(a, b) || !segment_is_axis_aligned(b, c) {
            return false;
        }

        let dx1 = b.x - a.x;
        let dy1 = b.y - a.y;
        let dx2 = c.x - b.x;
        let dy2 = c.y - b.y;
        let cross = dx1 * dy2 - dy1 * dx2;
        if cross.abs() > ROUTE_EPS {
            return false;
        }

        let dot = dx1 * dx2 + dy1 * dy2;
        dot < -ROUTE_EPS
    })
}

fn path_has_primary_axis_reversal(path: &[FPoint], direction: mmdflux::Direction) -> bool {
    path.windows(2).any(|segment| {
        let a = segment[0];
        let b = segment[1];
        match direction {
            mmdflux::Direction::TopDown => {
                (a.x - b.x).abs() <= ROUTE_EPS
                    && (a.y - b.y).abs() > ROUTE_EPS
                    && (b.y - a.y) < -ROUTE_EPS
            }
            mmdflux::Direction::BottomTop => {
                (a.x - b.x).abs() <= ROUTE_EPS
                    && (a.y - b.y).abs() > ROUTE_EPS
                    && (b.y - a.y) > ROUTE_EPS
            }
            mmdflux::Direction::LeftRight => {
                (a.y - b.y).abs() <= ROUTE_EPS
                    && (a.x - b.x).abs() > ROUTE_EPS
                    && (b.x - a.x) < -ROUTE_EPS
            }
            mmdflux::Direction::RightLeft => {
                (a.y - b.y).abs() <= ROUTE_EPS
                    && (a.x - b.x).abs() > ROUTE_EPS
                    && (b.x - a.x) > ROUTE_EPS
            }
        }
    })
}

fn effective_edge_direction_for_test(
    node_directions: &std::collections::HashMap<String, mmdflux::Direction>,
    from: &str,
    to: &str,
    fallback: mmdflux::Direction,
) -> mmdflux::Direction {
    let src_dir = node_directions.get(from).copied().unwrap_or(fallback);
    let tgt_dir = node_directions.get(to).copied().unwrap_or(fallback);
    if src_dir == tgt_dir {
        src_dir
    } else {
        fallback
    }
}

// -----------------------------------------------------------------------
// Task 1.1: Routed geometry contract tests
// -----------------------------------------------------------------------

#[test]
fn routed_geometry_has_correct_node_count() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nB-->C");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    assert_eq!(routed.nodes.len(), 3);
    assert!(routed.nodes.contains_key("A"));
    assert!(routed.nodes.contains_key("B"));
    assert!(routed.nodes.contains_key("C"));
}

#[test]
fn routed_geometry_has_correct_edge_count() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nB-->C");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    assert_eq!(routed.edges.len(), 2);
}

#[test]
fn routed_edges_have_non_empty_paths() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nB-->C");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    for edge in &routed.edges {
        assert!(
            edge.path.len() >= 2,
            "edge {} -> {} should have at least 2 path points, got {}",
            edge.from,
            edge.to,
            edge.path.len()
        );
    }
}

#[test]
fn routed_geometry_preserves_label_positions() {
    let (diagram, geom) = layout_test("graph TD\nA--label-->B");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    let edge = &routed.edges[0];
    assert!(
        edge.label_position.is_some(),
        "labeled edge should have a label position"
    );
}

const LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT: f64 = 2.0;

#[test]
fn orthogonal_labels_remain_attached_to_active_segments_labeled_edges() {
    let (diagram, geom) = layout_fixture_svg("labeled_edges.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let failures = labeled_edge_label_drift_failures(
        &diagram,
        &routed,
        LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
    );
    assert!(
        failures.is_empty(),
        "Label revalidation regression: labeled_edges has off-path labels:\n{}",
        failures.join("\n")
    );
}

#[test]
fn orthogonal_labels_remain_attached_to_active_segments_inline_label_flowchart() {
    let (diagram, geom) = layout_fixture_svg("inline_label_flowchart.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let failures = labeled_edge_label_drift_failures(
        &diagram,
        &routed,
        LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
    );
    assert!(
        failures.is_empty(),
        "Label revalidation regression: inline_label_flowchart has off-path labels:\n{}",
        failures.join("\n")
    );
}

#[test]
fn stale_label_anchor_is_replaced_with_valid_route_anchor() {
    let (diagram, geom) = layout_fixture_svg("labeled_edges.mmd");
    let stale_edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Config" && edge.to == "Error")
        .expect("fixture should contain Config -> Error")
        .index;
    let mut stale_geom = geom.clone();
    let original_anchor = {
        let edge = stale_geom
            .edges
            .iter_mut()
            .find(|edge| edge.index == stale_edge_index)
            .expect("fixture should contain layout edge for Config -> Error");
        let anchor = edge
            .label_position
            .expect("fixture should carry layout label anchor for Config -> Error");
        // Force a deterministic stale anchor so this contract does not depend
        // on incidental route shape changes from unrelated source-stem tweaks.
        let stale_anchor = FPoint::new(anchor.x + 60.0, anchor.y + 60.0);
        edge.label_position = Some(stale_anchor);
        stale_anchor
    };

    let routed = route_graph_geometry(&diagram, &stale_geom, EdgeRouting::OrthogonalRoute);
    let routed_edge = routed
        .edges
        .iter()
        .find(|edge| edge.index == stale_edge_index)
        .expect("routed geometry should contain Config -> Error");
    let validated_anchor = routed_edge
        .label_position
        .expect("validated routed label anchor should be present");

    let original_drift = distance_point_to_path(original_anchor, &routed_edge.path);
    let validated_drift = distance_point_to_path(validated_anchor, &routed_edge.path);
    assert!(
        original_drift > LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
        "fixture contract invalid: original anchor should be stale for this test (drift={original_drift}, path={:?})",
        routed_edge.path
    );
    assert!(
        validated_drift <= LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
        "validated anchor should be on/near active segment after fallback (drift={validated_drift}, anchor={validated_anchor:?}, path={:?})",
        routed_edge.path
    );
}

#[test]
fn routed_geometry_preserves_direction() {
    let (diagram, geom) = layout_test("graph LR\nA-->B");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    assert_eq!(routed.direction, mmdflux::Direction::LeftRight);
}

#[test]
fn routed_geometry_preserves_bounds() {
    let (diagram, geom) = layout_test("graph TD\nA-->B");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    assert!(routed.bounds.width > 0.0);
    assert!(routed.bounds.height > 0.0);
}

#[test]
fn routed_geometry_preserves_subgraphs() {
    let (diagram, geom) = layout_test("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    assert!(!routed.subgraphs.is_empty());
    let sg = &routed.subgraphs["sg1"];
    assert_eq!(sg.title, "Group");
    assert!(sg.rect.width > 0.0);
}

#[test]
fn routed_geometry_marks_backward_edges() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nB-->A");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    // At least one edge should be marked backward (the cycle)
    let backward_count = routed.edges.iter().filter(|e| e.is_backward).count();
    assert!(
        backward_count >= 1,
        "cycle should produce at least one backward edge"
    );
}

#[test]
fn routed_self_edges_have_paths() {
    let (diagram, geom) = layout_test("graph TD\nA-->A");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    assert_eq!(routed.self_edges.len(), 1);
    assert!(
        routed.self_edges[0].path.len() >= 2,
        "self-edge should have at least 2 path points"
    );
    assert_eq!(routed.self_edges[0].node_id, "A");
}

// -----------------------------------------------------------------------
// Task 1.2: Edge routing tests
// -----------------------------------------------------------------------

#[test]
fn engine_provided_mode_uses_layout_path_hints() {
    let (diagram, geom) = layout_test("graph TD\nA-->B");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::EngineProvided);

    let edge = &routed.edges[0];
    // EngineProvided should use the engine-provided path hints directly
    assert!(edge.path.len() >= 2);

    // The path should match the layout_path_hint from the geometry
    let layout_hint = geom.edges[0].layout_path_hint.as_ref().unwrap();
    assert_eq!(edge.path.len(), layout_hint.len());
    for (rp, lp) in edge.path.iter().zip(layout_hint.iter()) {
        assert_eq!(rp.x, lp.x);
        assert_eq!(rp.y, lp.y);
    }
}

#[test]
fn polyline_route_mode_produces_valid_paths() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nB-->C\nA-->C");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    assert_eq!(routed.edges.len(), 3);
    for edge in &routed.edges {
        assert!(
            edge.path.len() >= 2,
            "edge {} -> {} should have valid path",
            edge.from,
            edge.to,
        );
    }
}

#[test]
fn edge_routings_produce_same_structure() {
    let (diagram, geom) = layout_test("graph TD\nA-->B");

    let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    let pass = route_graph_geometry(&diagram, &geom, EdgeRouting::EngineProvided);

    // Both modes should produce the same structural output
    assert_eq!(full.nodes.len(), pass.nodes.len());
    assert_eq!(full.edges.len(), pass.edges.len());
    assert_eq!(full.self_edges.len(), pass.self_edges.len());
    assert_eq!(full.subgraphs.len(), pass.subgraphs.len());
}

#[test]
fn routed_edges_preserve_subgraph_references() {
    let (diagram, geom) = layout_test("graph TD\nsubgraph sg1[Group]\nA\nend\nB-->sg1");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);

    // Check that subgraph references are preserved in routed edges.
    // If the edge connects to a subgraph-as-node, the reference should be preserved.
    if let Some(e) = routed
        .edges
        .iter()
        .find(|e| e.from_subgraph.is_some() || e.to_subgraph.is_some())
    {
        assert!(e.to_subgraph.is_some() || e.from_subgraph.is_some());
    }
}

// -----------------------------------------------------------------------
// Task 4.1: Orthogonal routing contracts
// -----------------------------------------------------------------------

#[test]
fn orthogonal_router_produces_axis_aligned_forward_paths() {
    let (diagram, geom) = layout_test("graph TD\nA-->B\nA-->C\nB-->D\nC-->D");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for edge in routed.edges.iter().filter(|edge| !edge.is_backward) {
        assert!(
            edge.path
                .windows(2)
                .all(|seg| seg[0].x == seg[1].x || seg[0].y == seg[1].y),
            "edge {} -> {} has diagonal segment in orthogonal routing: {:?}",
            edge.from,
            edge.to,
            edge.path
        );
    }
}

#[test]
fn snap_path_to_grid_preserves_start_and_end_nodes() {
    let path = vec![
        FPoint::new(10.2, 20.8),
        FPoint::new(10.2, 40.4),
        FPoint::new(35.7, 40.4),
    ];
    let snapped = snap_path_to_grid_preview(&path, 1.0, 1.0);

    assert_eq!(snapped.first(), Some(&FPoint::new(10.0, 21.0)));
    assert_eq!(snapped.last(), Some(&FPoint::new(36.0, 40.0)));
}

#[test]
fn orthogonal_route_preserves_core_routed_geometry_contracts() {
    for fixture in ["simple.mmd", "chain.mmd", "simple_cycle.mmd"] {
        let (diagram, geom) = layout_fixture(fixture);
        let legacy = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
        let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        assert_eq!(
            orthogonal.edges.len(),
            legacy.edges.len(),
            "edge count diverged for fixture {fixture}"
        );
        assert_eq!(
            orthogonal.self_edges.len(),
            legacy.self_edges.len(),
            "self-edge count diverged for fixture {fixture}"
        );

        for (u, l) in orthogonal.edges.iter().zip(legacy.edges.iter()) {
            assert_eq!(u.index, l.index, "edge index mismatch in fixture {fixture}");
            assert_eq!(u.from, l.from, "edge source mismatch in fixture {fixture}");
            assert_eq!(u.to, l.to, "edge target mismatch in fixture {fixture}");
            assert_eq!(
                u.is_backward, l.is_backward,
                "backward-edge flag mismatch in fixture {fixture}"
            );
            assert!(
                u.path.len() >= 2,
                "orthogonal path too short for {} -> {} in fixture {fixture}",
                u.from,
                u.to
            );
        }
    }
}

#[test]
fn orthogonal_route_contracts_are_axis_aligned_and_non_degenerate() {
    let (diagram, geom) = layout_fixture("simple_cycle.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for edge in &routed.edges {
        assert!(
            edge.path.len() >= 2,
            "edge {} -> {} has too few points: {:?}",
            edge.from,
            edge.to,
            edge.path
        );

        for seg in edge.path.windows(2) {
            let a = seg[0];
            let b = seg[1];
            assert!(
                segment_is_axis_aligned(a, b),
                "edge {} -> {} contains diagonal segment: {:?}",
                edge.from,
                edge.to,
                edge.path
            );
            assert!(
                segment_is_non_degenerate(a, b),
                "edge {} -> {} contains duplicate or zero-length segment: {:?}",
                edge.from,
                edge.to,
                edge.path
            );
        }
    }
}

#[test]
fn orthogonal_route_contracts_preserve_terminal_support_segment() {
    let (diagram, geom) = layout_fixture("ampersand.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::TopDown);

    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    for edge in routed.edges.iter().filter(|edge| !edge.is_backward) {
        assert!(
            edge.path.len() >= 2,
            "edge {} -> {} must have at least two points",
            edge.from,
            edge.to
        );
        let prev = edge.path[edge.path.len() - 2];
        let end = edge.path[edge.path.len() - 1];
        let dx = (end.x - prev.x).abs();
        let dy = (end.y - prev.y).abs();

        assert!(
            dy > ROUTE_EPS,
            "edge {} -> {} terminal segment is zero-length: {:?}",
            edge.from,
            edge.to,
            edge.path
        );
        assert!(
            dx <= ROUTE_EPS,
            "edge {} -> {} terminal segment is not vertical in TD: {:?}",
            edge.from,
            edge.to,
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_contracts_are_deterministic_for_repeated_runs() {
    let (diagram, geom) = layout_fixture("multi_subgraph_direction_override.mmd");
    let first = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let second = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    assert_eq!(first.edges.len(), second.edges.len());
    for (lhs, rhs) in first.edges.iter().zip(second.edges.iter()) {
        assert_eq!(lhs.index, rhs.index);
        assert_eq!(lhs.from, rhs.from);
        assert_eq!(lhs.to, rhs.to);
        assert_eq!(lhs.path, rhs.path);
    }
}

#[test]
fn orthogonal_route_multi_subgraph_bmid_to_f_keeps_terminal_support_clearance() {
    let (diagram, geom) = layout_fixture("multi_subgraph_direction_override.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain Bmid -> F");

    assert!(
        edge.path.len() >= 2,
        "Bmid -> F should have routed path points: {:?}",
        edge.path
    );

    let prev = edge.path[edge.path.len() - 2];
    let end = edge.path[edge.path.len() - 1];
    let dx = (end.x - prev.x).abs();
    let dy = (end.y - prev.y).abs();

    assert!(
        dx <= ROUTE_EPS,
        "Bmid -> F terminal segment should stay vertical in TD: {:?}",
        edge.path
    );
    assert!(
        dy >= 12.0,
        "Bmid -> F terminal support should preserve >=12px clearance before endpoint: dy={dy}, path={:?}",
        edge.path
    );
}

#[test]
fn orthogonal_route_fan_in_lr_target_endpoints_stay_on_or_outside_target_border() {
    let (diagram, geom) = layout_fixture("fan_in_lr.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let target_rect = geom
        .nodes
        .get("D")
        .expect("fan_in_lr should contain target node D")
        .rect;

    for edge in routed
        .edges
        .iter()
        .filter(|edge| (edge.from == "A" || edge.from == "C") && edge.to == "D")
    {
        assert!(
            edge.path.len() >= 2,
            "edge should contain at least two points: {:?}",
            edge.path
        );
        let prev = edge.path[edge.path.len() - 2];
        let end = *edge.path.last().expect("edge should have routed points");
        assert!(
            !point_inside_rect(target_rect, end),
            "orthogonal routed endpoint should not be inside target rect for {} -> {}: end={:?}, target_rect={:?}, path={:?}",
            edge.from,
            edge.to,
            end,
            target_rect,
            edge.path
        );
        assert!(
            terminal_support_is_normal_to_attached_rect_face(target_rect, prev, end),
            "fan_in_lr terminal segment should approach D on the face-normal axis for {} -> {}: prev={:?}, end={:?}, target_rect={:?}, path={:?}",
            edge.from,
            edge.to,
            prev,
            end,
            target_rect,
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_ports_keep_minimum_corner_inset_for_fan_edges() {
    const MIN_CORNER_INSET: f64 = 8.0;
    let cases = [
        ("fan_out.mmd", "A", "B"),
        ("fan_out.mmd", "A", "D"),
        ("fan_in.mmd", "A", "D"),
        ("fan_in.mmd", "C", "D"),
        ("fan_in_lr.mmd", "A", "D"),
        ("fan_in_lr.mmd", "C", "D"),
    ];

    for (fixture, from, to) in cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain edge {from} -> {to}"));

        assert!(
            edge.path.len() >= 2,
            "fixture {fixture} edge {from}->{to} should have at least 2 routed points: {:?}",
            edge.path
        );

        let source_rect = geom
            .nodes
            .get(from)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain source {from}"))
            .rect;
        let target_rect = geom
            .nodes
            .get(to)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target {to}"))
            .rect;

        let start = edge.path[0];
        let end = *edge.path.last().expect("edge should contain endpoint");
        let source_margin = face_corner_inset_margin(source_rect, start).unwrap_or_else(|| {
            panic!(
                "fixture {fixture} edge {from}->{to} source endpoint should lie on source face: start={start:?}, source_rect={source_rect:?}, path={:?}",
                edge.path
            )
        });
        let target_margin = face_corner_inset_margin(target_rect, end).unwrap_or_else(|| {
            panic!(
                "fixture {fixture} edge {from}->{to} target endpoint should lie on target face: end={end:?}, target_rect={target_rect:?}, path={:?}",
                edge.path
            )
        });

        assert!(
            source_margin >= MIN_CORNER_INSET,
            "fixture {fixture} edge {from}->{to} source inset too small: margin={source_margin:.2}, expected>={MIN_CORNER_INSET:.2}, source_rect={source_rect:?}, start={start:?}, path={:?}",
            edge.path
        );
        assert!(
            target_margin >= MIN_CORNER_INSET,
            "fixture {fixture} edge {from}->{to} target inset too small: margin={target_margin:.2}, expected>={MIN_CORNER_INSET:.2}, target_rect={target_rect:?}, end={end:?}, path={:?}",
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_http_request_backward_edge_preserves_client_side_face_attachment() {
    let (diagram, geom) = layout_fixture("http_request.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "Response" && edge.to == "Client")
        .expect("fixture should contain Response -> Client");
    assert!(
        edge.path.len() >= 2,
        "Response -> Client should have at least two routed points: {:?}",
        edge.path
    );

    let end = *edge.path.last().expect("edge should have endpoint");
    let client_rect = geom
        .nodes
        .get("Client")
        .expect("fixture should contain Client")
        .rect;
    let right = client_rect.x + client_rect.width;
    let bottom = client_rect.y + client_rect.height;

    let dist_to_right = (right - end.x).abs();
    let dist_to_bottom = (bottom - end.y).abs();
    assert!(
        dist_to_right + 0.5 < dist_to_bottom,
        "Response -> Client endpoint should favor Client right face over bottom face in orthogonal routing: end={end:?}, client_rect={client_rect:?}, dist_to_right={dist_to_right}, dist_to_bottom={dist_to_bottom}, path={:?}",
        edge.path
    );
    assert!(
        end.y < bottom - 0.5,
        "Response -> Client endpoint should not collapse to bottom corner in orthogonal routing: end={end:?}, client_rect={client_rect:?}, path={:?}",
        edge.path
    );
}

#[test]
fn orthogonal_route_contracts_keep_primary_axis_departure_stem_for_off_center_td_source_ports() {
    let (diagram, geom) = layout_fixture("compat_kitchen_sink.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::TopDown);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for (from, to) in [("check-1", "process-A"), ("check-1", "error-1")] {
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture should contain {from} -> {to}"));
        assert!(
            edge.path.len() >= 3,
            "{from} -> {to} should have at least three routed points: {:?}",
            edge.path
        );

        let source_node = geom
            .nodes
            .get(from)
            .unwrap_or_else(|| panic!("fixture should contain source {from}"));
        let source_rect = source_node.rect;
        let start = edge.path[0];
        let next = edge.path[1];
        let center_x = source_rect.x + source_rect.width / 2.0;
        let source_offset = (start.x - center_x).abs();

        let is_angular = matches!(
            source_node.shape,
            mmdflux::graph::Shape::Diamond | mmdflux::graph::Shape::Hexagon
        );
        if !is_angular {
            let min_off_center = 1.0;
            assert!(
                source_offset >= min_off_center,
                "fixture expectation invalid: {from} -> {to} source should be off-center, got offset={source_offset}, min={min_off_center}, path={:?}",
                edge.path
            );
            assert!(
                (next.x - start.x).abs() <= ROUTE_EPS && (next.y - start.y).abs() > ROUTE_EPS,
                "off-center TD source should keep a primary-axis departure stem before sweeping for {from} -> {to}: start={start:?}, next={next:?}, path={:?}",
                edge.path
            );
        } else {
            assert!(
                (next.y - start.y).abs() <= ROUTE_EPS && (next.x - start.x).abs() > ROUTE_EPS,
                "angular TD source should depart laterally before bending toward target for {from} -> {to}: start={start:?}, next={next:?}, path={:?}",
                edge.path
            );
        }
    }
}

#[test]
fn orthogonal_route_contracts_keep_primary_stem_before_outward_td_fan_out_sweeps() {
    let (diagram, geom) = layout_fixture("fan_out.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::TopDown);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for (from, to) in [("A", "B"), ("A", "D")] {
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture should contain {from} -> {to}"));
        assert!(
            edge.path.len() >= 3,
            "{from} -> {to} should have at least three routed points: {:?}",
            edge.path
        );

        let source_rect = geom
            .nodes
            .get(from)
            .unwrap_or_else(|| panic!("fixture should contain source {from}"))
            .rect;
        let start = edge.path[0];
        let next = edge.path[1];
        let third = edge.path[2];
        let center_x = source_rect.x + source_rect.width / 2.0;
        let source_offset = start.x - center_x;
        assert!(
            source_offset.abs() >= 1.0,
            "fixture expectation invalid: {from} -> {to} source should be off-center, offset={source_offset}, path={:?}",
            edge.path
        );
        assert!(
            (next.x - start.x).abs() <= ROUTE_EPS && (next.y - start.y).abs() > ROUTE_EPS,
            "fan-out edge {from} -> {to} should keep a short primary-axis source stem before sweeping: start={start:?}, next={next:?}, path={:?}",
            edge.path
        );
        assert!(
            (third.y - next.y).abs() <= ROUTE_EPS && (third.x - next.x).abs() > ROUTE_EPS,
            "fan-out edge {from} -> {to} should sweep laterally after the source stem: next={next:?}, third={third:?}, path={:?}",
            edge.path
        );
        assert!(
            (third.x - next.x).signum() == source_offset.signum(),
            "fan-out edge {from} -> {to} should sweep outward from source center: source_offset={source_offset}, second_dx={}, path={:?}",
            third.x - next.x,
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_contracts_keep_directional_source_exits_for_selected_fixtures() {
    type EdgeExpectation = (&'static str, &'static str, f64);
    type FixtureExpectations = (&'static str, &'static [EdgeExpectation]);

    let td_cases: &[FixtureExpectations] = &[
        ("decision.mmd", &[("B", "D", 1.0)]),
        ("complex.mmd", &[("B", "D", 5.0)]),
        ("double_skip.mmd", &[("A", "D", 1.0)]),
    ];

    for (fixture, edges) in td_cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        assert_eq!(
            geom.direction,
            mmdflux::Direction::TopDown,
            "fixture {fixture} should be TD for outward-first source contract"
        );
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        for (from, to, min_offset) in *edges {
            let edge = routed
                .edges
                .iter()
                .find(|edge| edge.from == *from && edge.to == *to)
                .unwrap_or_else(|| panic!("fixture {fixture} missing edge {from} -> {to}"));
            assert!(
                edge.path.len() >= 2,
                "fixture {fixture} edge {from} -> {to} should have at least two points: {:?}",
                edge.path
            );

            let source_rect = geom
                .nodes
                .get(*from)
                .unwrap_or_else(|| panic!("fixture {fixture} missing source node {from}"))
                .rect;
            let start = edge.path[0];
            let next = edge.path[1];
            let center_x = source_rect.x + source_rect.width / 2.0;
            let source_offset = start.x - center_x;
            let source_shape = geom
                .nodes
                .get(*from)
                .unwrap_or_else(|| panic!("fixture {fixture} missing source node {from}"))
                .shape;
            assert!(
                source_offset.abs() >= *min_offset,
                "fixture expectation invalid: {from} -> {to} should start noticeably off-center (offset={source_offset}, min_offset={min_offset}) in {fixture}, path={:?}",
                edge.path
            );

            let first_dx = next.x - start.x;
            let first_dy = next.y - start.y;
            let first_is_primary = first_dx.abs() <= ROUTE_EPS && first_dy.abs() > ROUTE_EPS;
            let first_is_lateral_outward = first_dy.abs() <= ROUTE_EPS
                && first_dx.abs() > ROUTE_EPS
                && first_dx.signum() == source_offset.signum();
            let angular_source = matches!(
                source_shape,
                mmdflux::Shape::Diamond | mmdflux::Shape::Hexagon
            );
            if edge.path.len() >= 3 {
                assert!(
                    first_is_primary || (angular_source && first_is_lateral_outward),
                    "fixture {fixture} edge {from} -> {to} should leave source on TD primary axis first when a bend is present (except angular sources which may depart laterally outward): start={start:?}, next={next:?}, shape={source_shape:?}, path={:?}",
                    edge.path
                );
            } else {
                assert!(
                    first_is_primary,
                    "fixture {fixture} edge {from} -> {to} compact direct path should remain a primary-axis source support segment in TD: start={start:?}, next={next:?}, path={:?}",
                    edge.path
                );
            }
        }
    }

    let (diagram, geom) = layout_fixture_svg("git_workflow.mmd");
    assert_eq!(
        geom.direction,
        mmdflux::Direction::LeftRight,
        "git_workflow fixture should be LR"
    );
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    for (from, to) in [("Working", "Staging"), ("Local", "Remote")] {
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("git_workflow missing edge {from} -> {to}"));
        assert!(
            edge.path.len() >= 2,
            "git_workflow edge {from} -> {to} should have at least two points: {:?}",
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];
        assert!(
            (next.y - start.y).abs() <= ROUTE_EPS && (next.x - start.x).abs() > ROUTE_EPS,
            "git_workflow edge {from} -> {to} should leave source on LR primary axis first: start={start:?}, next={next:?}, path={:?}",
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_contracts_avoid_source_turnback_spikes_for_selected_fixtures() {
    let cases = [
        ("decision.mmd", "A", "B"),
        ("complex.mmd", "B", "D"),
        ("double_skip.mmd", "A", "D"),
        ("git_workflow.mmd", "Working", "Staging"),
    ];

    for (fixture, from, to) in cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} missing edge {from} -> {to}"));
        assert!(
            !path_has_source_turnback_spike(&edge.path),
            "fixture {fixture} edge {from} -> {to} should not contain source-local A-B-A turnback spike: path={:?}",
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_contracts_avoid_immediate_axial_turnbacks() {
    let cases = [
        ("multiple_cycles.mmd", "C", "A"),
        ("git_workflow.mmd", "Remote", "Working"),
        ("simple_cycle.mmd", "C", "A"),
        ("backward_in_subgraph.mmd", "B", "A"),
    ];

    for (fixture, from, to) in cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} missing edge {from} -> {to}"));
        assert!(
            !path_has_immediate_axial_turnback(&edge.path),
            "fixture {fixture} edge {from} -> {to} should not contain immediate axial turnbacks: path={:?}",
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_contracts_preserve_backward_cycle_outer_lane_clearance() {
    const MIN_OUTER_LANE_CLEARANCE: f64 = 12.0;

    let (diagram, geom) = layout_fixture_svg("multiple_cycles.mmd");
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
}

// -----------------------------------------------------------------------
// Task 1.2: Shared float-route heuristics
// -----------------------------------------------------------------------

#[test]
fn shared_builder_prefers_terminal_segment_matching_layout_entry_axis() {
    let (diagram, geom) = layout_fixture("direction_override.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for edge in routed.edges.iter().filter(|edge| !edge.is_backward) {
        let expected_direction = effective_edge_direction_for_test(
            &geom.node_directions,
            &edge.from,
            &edge.to,
            geom.direction,
        );
        let prev = edge.path[edge.path.len() - 2];
        let end = edge.path[edge.path.len() - 1];
        let x_aligned = approx_eq(prev.x, end.x);
        let y_aligned = approx_eq(prev.y, end.y);

        match expected_direction {
            mmdflux::Direction::TopDown | mmdflux::Direction::BottomTop => assert!(
                x_aligned && !y_aligned,
                "edge {} -> {} should enter on vertical terminal segment for {:?}, got {:?}",
                edge.from,
                edge.to,
                expected_direction,
                edge.path
            ),
            mmdflux::Direction::LeftRight | mmdflux::Direction::RightLeft => assert!(
                y_aligned && !x_aligned,
                "edge {} -> {} should enter on horizontal terminal segment for {:?}, got {:?}",
                edge.from,
                edge.to,
                expected_direction,
                edge.path
            ),
        }
    }
}

#[test]
fn orthogonal_route_nested_override_cross_boundary_edge_matches_lr_face_parity() {
    let fixture = "subgraph_direction_nested_both.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    let source_rect = geom
        .nodes
        .get("C")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain source node C"))
        .rect;
    let target_rect = geom
        .nodes
        .get("A")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain target node A"))
        .rect;

    let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let full_edge = full
        .edges
        .iter()
        .find(|edge| edge.from == "C" && edge.to == "A")
        .expect("fixture should contain C -> A in polyline mode");
    let orthogonal_edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "C" && edge.to == "A")
        .expect("fixture should contain C -> A in orthogonal mode");

    let full_start = full_edge
        .path
        .first()
        .copied()
        .expect("polyline C -> A should have source endpoint");
    let full_end = full_edge
        .path
        .last()
        .copied()
        .expect("polyline C -> A should have target endpoint");
    let orthogonal_start = orthogonal_edge
        .path
        .first()
        .copied()
        .expect("orthogonal C -> A should have source endpoint");
    let orthogonal_end = orthogonal_edge
        .path
        .last()
        .copied()
        .expect("orthogonal C -> A should have target endpoint");

    let full_source_face = point_on_target_face(source_rect, full_start);
    let full_target_face = point_on_target_face(target_rect, full_end);
    let orthogonal_source_face = point_on_target_face(source_rect, orthogonal_start);
    let orthogonal_target_face = point_on_target_face(target_rect, orthogonal_end);

    assert_eq!(
        full_source_face, "right",
        "fixture contract invalid: polyline C -> A should depart C from east/right face: path={:?}",
        full_edge.path
    );
    assert_eq!(
        full_target_face, "left",
        "fixture contract invalid: polyline C -> A should enter A from west/left face: path={:?}",
        full_edge.path
    );
    assert_eq!(
        orthogonal_source_face, full_source_face,
        "orthogonal C -> A should match polyline source face in nested override cross-boundary routing: full={full_source_face}, orthogonal={orthogonal_source_face}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path, orthogonal_edge.path
    );
    assert_eq!(
        orthogonal_target_face, full_target_face,
        "orthogonal C -> A should match polyline target face in nested override cross-boundary routing: full={full_target_face}, orthogonal={orthogonal_target_face}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path, orthogonal_edge.path
    );

    let n = orthogonal_edge.path.len();
    assert!(
        n >= 2,
        "orthogonal C -> A should include at least one segment: path={:?}",
        orthogonal_edge.path
    );
    let prev = orthogonal_edge.path[n - 2];
    assert!(
        approx_eq(prev.y, orthogonal_end.y) && !approx_eq(prev.x, orthogonal_end.x),
        "orthogonal C -> A should enter A on a horizontal LR terminal segment: prev={prev:?}, end={orthogonal_end:?}, path={:?}",
        orthogonal_edge.path
    );
}

#[test]
fn shared_builder_reduces_midfield_jogs_for_large_horizontal_offset_edges() {
    let (diagram, geom) = layout_fixture("decision.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "D")
        .expect("expected B -> D edge in decision fixture");
    let horizontal_offset = (edge.path[0].x - edge.path[edge.path.len() - 1].x).abs();

    // With corrected reversed-chain-edge tracking, gap inflation is reduced,
    // lowering the horizontal offset slightly (from ~33 to ~27).
    assert!(
        horizontal_offset > 25.0,
        "test fixture no longer has large horizontal offset: {horizontal_offset}"
    );
    assert!(
        bend_count(&edge.path) <= 2,
        "expected congestion heuristic to reduce bends for B -> D, got path {:?}",
        edge.path
    );
}

#[test]
fn shared_builder_keeps_alignment_tolerance_stable_for_near_aligned_points() {
    let (diagram, mut geom) = layout_test("graph TD\nA-->B");
    let hint = geom.edges[0]
        .layout_path_hint
        .clone()
        .expect("layout path hint should exist");
    let start = hint[0];
    let end = hint[hint.len() - 1];
    let y_span = end.y - start.y;

    geom.edges[0].layout_path_hint = Some(vec![
        start,
        FPoint::new(start.x + 0.4, start.y + y_span * 0.33),
        FPoint::new(start.x - 0.4, start.y + y_span * 0.66),
        end,
    ]);

    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let edge = &routed.edges[0];
    assert!(
        bend_count(&edge.path) <= 2,
        "near-aligned jitter should not produce extra elbows, got {:?}",
        edge.path
    );
    assert!(
        edge.path
            .windows(2)
            .all(|seg| segment_is_axis_aligned(seg[0], seg[1])),
        "near-aligned jitter produced non-orthogonal segment: {:?}",
        edge.path
    );
}

#[test]
fn orthogonal_route_contracts_keep_td_source_ports_normal_and_compact() {
    let cases: &[(&str, &[(&str, &str)])] = &[(
        "compat_kitchen_sink.mmd",
        &[
            ("start-node", "check-1"),
            ("process-A", "end-node"),
            ("error-1", "end-node"),
        ],
    )];

    for (fixture, edges) in cases {
        let (diagram, geom) = layout_fixture(fixture);
        assert_eq!(
            geom.direction,
            mmdflux::Direction::TopDown,
            "fixture {fixture} should be TD for source-support contract"
        );
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        for (from, to) in *edges {
            let edge = routed
                .edges
                .iter()
                .find(|edge| edge.from == *from && edge.to == *to)
                .unwrap_or_else(|| panic!("fixture {fixture} missing edge {from} -> {to}"));
            assert!(
                edge.path.len() >= 2,
                "fixture {fixture} edge {from} -> {to} should have at least two points: {:?}",
                edge.path
            );
            let source_rect = geom
                .nodes
                .get(*from)
                .unwrap_or_else(|| panic!("fixture {fixture} missing source node {from}"))
                .rect;
            let start = edge.path[0];
            let next = edge.path[1];
            let center_x = source_rect.x + source_rect.width / 2.0;
            let source_offset = (start.x - center_x).abs();
            assert!(
                source_offset <= 1.0,
                "fixture expectation invalid: source should be centered for this contract on {from} -> {to}, offset={source_offset}, path={:?}",
                edge.path
            );
            assert!(
                source_support_is_normal_to_attached_rect_face(source_rect, start, next),
                "fixture {fixture} edge {from} -> {to} should leave source face on its normal axis in TD (avoid bottom-border sliding): start={start:?}, next={next:?}, source_rect={source_rect:?}, path={:?}",
                edge.path
            );
            let bends = bend_count(&edge.path);
            assert!(
                bends <= 2,
                "fixture {fixture} edge {from} -> {to} should stay compact after source-support preservation: bends={bends}, path={:?}",
                edge.path
            );
        }
    }
}

// -----------------------------------------------------------------------
// Task 0.1: TD/BT backward entry-face parity RED regressions
// -----------------------------------------------------------------------

#[test]
fn orthogonal_route_decision_backward_debug_to_start_supports_td_top_bottom_parity() {
    let fixture = "decision.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::TopDown,
        "fixture {fixture} should be TD for backward entry-face parity"
    );

    let source_rect = geom
        .nodes
        .get("D")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain source node D"))
        .rect;
    let target_rect = geom
        .nodes
        .get("A")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain target node A"))
        .rect;

    let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let full_edge = full
        .edges
        .iter()
        .find(|edge| edge.from == "D" && edge.to == "A")
        .expect("fixture should contain backward edge D -> A in polyline mode");
    assert!(
        full_edge.is_backward,
        "fixture contract invalid: D -> A should be backward in polyline mode"
    );
    let full_start = full_edge
        .path
        .first()
        .copied()
        .expect("polyline backward edge should have source endpoint");
    let full_end = full_edge
        .path
        .last()
        .copied()
        .expect("polyline backward edge should have target endpoint");
    let full_source_face = point_on_target_face(source_rect, full_start);
    let full_target_face = point_on_target_face(target_rect, full_end);
    // Face-based backward routing builds all backward paths through the
    // canonical face (right for TD), matching orthogonal routing.
    assert_eq!(
        full_source_face, "right",
        "fixture contract changed unexpectedly: polyline D -> A should depart from source right face; start={full_start:?}, path={:?}",
        full_edge.path
    );
    assert_eq!(
        full_target_face, "right",
        "fixture contract changed unexpectedly: polyline D -> A should enter target right face; end={full_end:?}, path={:?}",
        full_edge.path
    );

    let orthogonal_edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "D" && edge.to == "A")
        .expect("fixture should contain backward edge D -> A in orthogonal mode");
    assert!(
        orthogonal_edge.is_backward,
        "fixture contract invalid: D -> A should be backward in orthogonal mode"
    );
    let orthogonal_start = orthogonal_edge
        .path
        .first()
        .copied()
        .expect("orthogonal backward edge should have source endpoint");
    let orthogonal_end = orthogonal_edge
        .path
        .last()
        .copied()
        .expect("orthogonal backward edge should have target endpoint");
    let orthogonal_source_face = point_on_target_face(source_rect, orthogonal_start);
    let orthogonal_target_face = point_on_target_face(target_rect, orthogonal_end);

    // D is to the right of A, so the crossing-avoidance heuristic bypasses
    // TD top/bottom parity and falls back to right-face side-channel routing
    // to avoid crossing the forward A->D edge.
    assert_eq!(
        orthogonal_source_face, "right",
        "orthogonal D -> A source face: D is right of A, parity bypassed; full={full_source_face}, orthogonal={orthogonal_source_face}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path, orthogonal_edge.path
    );
    assert_eq!(
        orthogonal_target_face, "right",
        "orthogonal D -> A target face: D is right of A, parity bypassed; full={full_target_face}, orthogonal={orthogonal_target_face}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path, orthogonal_edge.path
    );
}

#[test]
fn orthogonal_route_decision_backward_debug_to_start_keeps_vertical_source_stem_before_elbow() {
    let fixture = "decision.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::TopDown,
        "fixture {fixture} should be TD for source-stem normalization checks"
    );

    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "D" && edge.to == "A")
        .expect("fixture should contain backward edge D -> A in orthogonal mode");
    assert!(
        edge.is_backward,
        "fixture contract invalid: D -> A should be backward in orthogonal mode"
    );
    assert!(
        edge.path.len() >= 3,
        "D -> A should expose source stem and elbow points for this contract: path={:?}",
        edge.path
    );

    let start = edge.path[0];
    let source_stem = edge.path[1];
    // D is to the right of A; parity override bypassed to avoid crossing the
    // forward edge. The exit is from D's right face, so the initial source stem
    // is horizontal (y constant, x increasing toward the channel).
    assert!(
        approx_eq(start.y, source_stem.y),
        "orthogonal D -> A should keep a horizontal source stem (right-face departure, crossing avoided): start={start:?}, source_stem={source_stem:?}, path={:?}",
        edge.path
    );
    assert!(
        source_stem.x > start.x,
        "TD backward source stem should move rightward from D's right face: start={start:?}, source_stem={source_stem:?}, path={:?}",
        edge.path
    );
}

#[test]
fn orthogonal_route_backward_in_subgraph_uses_compact_inline_terminal_return() {
    let fixture = "backward_in_subgraph.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::TopDown,
        "fixture {fixture} should be TD for compact backward return-shape checks"
    );

    let edge = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute)
        .edges
        .into_iter()
        .find(|edge| edge.from == "B" && edge.to == "A")
        .expect("fixture should contain backward edge B -> A in orthogonal mode");
    assert!(
        edge.is_backward,
        "fixture contract invalid: B -> A should be backward in orthogonal mode"
    );
    assert_eq!(
        edge.path.len(),
        4,
        "backward_in_subgraph B -> A should be a 3-segment V-H-V return (4 points): path={:?}",
        edge.path
    );
    assert_eq!(
        bend_count(&edge.path),
        2,
        "backward_in_subgraph B -> A should have exactly 2 bends: path={:?}",
        edge.path
    );
    assert!(
        edge.path
            .windows(2)
            .all(|window| segment_is_axis_aligned(window[0], window[1])),
        "backward_in_subgraph B -> A should remain orthogonal: path={:?}",
        edge.path
    );

    let p0 = edge.path[0];
    let p1 = edge.path[1];
    let p2 = edge.path[2];
    let p3 = edge.path[3];

    assert!(
        approx_eq(p0.x, p1.x) && p1.y < p0.y,
        "segment 1 should be a vertical upward source stem: p0={p0:?}, p1={p1:?}, path={:?}",
        edge.path
    );
    assert!(
        approx_eq(p1.y, p2.y) && p2.x > p1.x,
        "segment 2 should be a horizontal jog toward the right-side lane: p1={p1:?}, p2={p2:?}, path={:?}",
        edge.path
    );
    assert!(
        approx_eq(p2.x, p3.x) && p3.y < p2.y,
        "segment 3 should be a vertical upward terminal stem into target bottom face: p2={p2:?}, p3={p3:?}, path={:?}",
        edge.path
    );

    let target_rect = geom
        .nodes
        .get("A")
        .expect("fixture should contain target node A")
        .rect;
    let target_face = point_on_target_face(target_rect, p3);
    assert_eq!(
        target_face, "bottom",
        "backward_in_subgraph B -> A should still enter Node on bottom face: end={p3:?}, target_rect={target_rect:?}, path={:?}",
        edge.path
    );
}

#[test]
fn orthogonal_route_complex_backward_more_data_to_input_supports_td_entry_parity() {
    let fixture = "complex.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::TopDown,
        "fixture {fixture} should be TD for backward target-entry parity"
    );

    let target_rect = geom
        .nodes
        .get("A")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain target node A"))
        .rect;

    let full = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute);
    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let full_edge = full
        .edges
        .iter()
        .find(|edge| edge.from == "E" && edge.to == "A")
        .expect("fixture should contain backward edge E -> A in polyline mode");
    assert!(
        full_edge.is_backward,
        "fixture contract invalid: E -> A should be backward in polyline mode"
    );
    let full_end = full_edge
        .path
        .last()
        .copied()
        .expect("polyline backward edge should have target endpoint");
    let full_target_face = point_on_target_face(target_rect, full_end);
    // Face-based backward routing builds all backward edges through the
    // canonical face (right for TD), matching orthogonal routing behavior.
    assert_eq!(
        full_target_face, "right",
        "fixture contract changed unexpectedly: polyline E -> A should enter target right face; end={full_end:?}, path={:?}",
        full_edge.path
    );

    let orthogonal_edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "E" && edge.to == "A")
        .expect("fixture should contain backward edge E -> A in orthogonal mode");
    assert!(
        orthogonal_edge.is_backward,
        "fixture contract invalid: E -> A should be backward in orthogonal mode"
    );
    let orthogonal_end = orthogonal_edge
        .path
        .last()
        .copied()
        .expect("orthogonal backward edge should have target endpoint");
    let orthogonal_target_face = point_on_target_face(target_rect, orthogonal_end);

    // Long backward edges (rank_span >= 6) use side-face channel routing in
    // orthogonal routing (R-BACK-7 Heuristic 4), so the target face is "right"
    // instead of polyline routing's "bottom".
    assert_eq!(
        orthogonal_target_face, "right",
        "orthogonal E -> A should use right-side channel routing for long backward edge (R-BACK-7 H4): full={full_target_face}, orthogonal={orthogonal_target_face}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path, orthogonal_edge.path
    );
}

#[test]
fn orthogonal_route_complex_backward_more_data_to_input_avoids_tiny_terminal_staircase_elbow() {
    const MIN_TERMINAL_LATERAL_RUN: f64 = 6.0;

    let fixture = "complex.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::TopDown,
        "fixture {fixture} should be TD for terminal staircase checks"
    );

    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "E" && edge.to == "A")
        .expect("fixture should contain backward edge E -> A in orthogonal mode");
    assert!(
        edge.is_backward,
        "fixture contract invalid: E -> A should be backward in orthogonal mode"
    );
    assert!(
        edge.path.len() >= 3,
        "E -> A should have at least three points for terminal staircase checks: path={:?}",
        edge.path
    );

    let n = edge.path.len();
    let a = edge.path[n - 3];
    let b = edge.path[n - 2];
    let c = edge.path[n - 1];
    let ab_is_horizontal = approx_eq(a.y, b.y) && !approx_eq(a.x, b.x);
    let bc_is_vertical = approx_eq(b.x, c.x) && !approx_eq(b.y, c.y);
    if ab_is_horizontal && bc_is_vertical {
        let lateral_run = (b.x - a.x).abs();
        assert!(
            lateral_run >= MIN_TERMINAL_LATERAL_RUN,
            "orthogonal E -> A should avoid tiny terminal staircase elbows that create acute kinks near the target (min lateral run {MIN_TERMINAL_LATERAL_RUN}): lateral_run={lateral_run}, path={:?}",
            edge.path
        );
    }
}

#[test]
fn orthogonal_route_td_backward_followup_edges_use_canonical_face_in_polyline() {
    // Face-based backward routing: polyline/basis/straight always use the
    // canonical right face for TD backward edges. Orthogonal routing may use
    // top/bottom parity for short backward edges, so faces don't always match.
    let cases = [("simple_cycle.mmd", "C", "A")];

    for (fixture, from, to) in cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        assert_eq!(
            geom.direction,
            mmdflux::Direction::TopDown,
            "fixture {fixture} should be TD for backward-entry checks"
        );

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

        let full_edge = full
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| {
                panic!(
                    "fixture {fixture} should contain backward edge {from} -> {to} in polyline mode"
                )
            });

        assert!(
            full_edge.is_backward,
            "fixture {fixture} contract invalid: {from} -> {to} should be backward"
        );

        let full_start = full_edge
            .path
            .first()
            .copied()
            .expect("polyline backward edge should have source endpoint");
        let full_end = full_edge
            .path
            .last()
            .copied()
            .expect("polyline backward edge should have target endpoint");

        let full_source_face = point_on_target_face(source_rect, full_start);
        let full_target_face = point_on_target_face(target_rect, full_end);

        // Polyline backward edges consistently use the canonical right face for TD.
        assert_eq!(
            full_source_face, "right",
            "polyline {from}->{to} should depart from right face for fixture {fixture}: path={:?}",
            full_edge.path
        );
        assert_eq!(
            full_target_face, "right",
            "polyline {from}->{to} should enter target right face for fixture {fixture}: path={:?}",
            full_edge.path
        );
    }
}

#[test]
fn orthogonal_route_simple_cycle_backward_terminal_port_respects_minimum_corner_inset() {
    const MIN_CORNER_INSET: f64 = 8.0;
    let (diagram, geom) = layout_fixture_svg("simple_cycle.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "C" && edge.to == "A")
        .expect("simple_cycle should contain backward edge C -> A");
    assert!(
        edge.is_backward,
        "simple_cycle contract invalid: C -> A should be backward in orthogonal routing"
    );

    let target_rect = geom
        .nodes
        .get("A")
        .expect("simple_cycle should contain target node A")
        .rect;
    let end = *edge
        .path
        .last()
        .expect("backward edge should have terminal endpoint");
    let margin = face_corner_inset_margin(target_rect, end).unwrap_or_else(|| {
        panic!(
            "simple_cycle C->A endpoint should lie on a target face: end={end:?}, target_rect={target_rect:?}, path={:?}",
            edge.path
        )
    });

    assert!(
        margin >= MIN_CORNER_INSET,
        "simple_cycle C->A backward terminal port should keep minimum corner inset to preserve visible terminal stem: margin={margin:.2}, min={MIN_CORNER_INSET:.2}, end={end:?}, target_rect={target_rect:?}, path={:?}",
        edge.path
    );
}

// -----------------------------------------------------------------------
// Task 0.2: LR/RL backward clearance parity RED regressions
// -----------------------------------------------------------------------

#[test]
fn orthogonal_route_git_workflow_backward_remote_to_working_preserves_min_lr_channel_spacing() {
    const MIN_CHANNEL_CLEARANCE: f64 = 12.0;
    const MAX_BEND_INCREASE_FROM_FULL: usize = 1;

    let fixture = "git_workflow.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::LeftRight,
        "fixture {fixture} should be LR for channel-spacing parity checks"
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
        .expect("fixture should contain backward edge Remote -> Working in polyline mode");
    let orthogonal_edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "Remote" && edge.to == "Working")
        .expect("fixture should contain backward edge Remote -> Working in orthogonal mode");

    assert!(
        full_edge.is_backward && orthogonal_edge.is_backward,
        "fixture contract invalid: Remote -> Working should be backward in both edge routings"
    );

    let orthogonal_start = orthogonal_edge
        .path
        .first()
        .copied()
        .expect("orthogonal backward edge should have source endpoint");
    let orthogonal_end = orthogonal_edge
        .path
        .last()
        .copied()
        .expect("orthogonal backward edge should have target endpoint");

    let orthogonal_source_face = point_on_target_face(source_rect, orthogonal_start);
    let orthogonal_target_face = point_on_target_face(target_rect, orthogonal_end);

    assert_eq!(
        orthogonal_source_face, "bottom",
        "orthogonal Remote -> Working should preserve canonical bottom source face: start={orthogonal_start:?}, path={:?}",
        orthogonal_edge.path
    );
    assert_eq!(
        orthogonal_target_face, "bottom",
        "orthogonal Remote -> Working should preserve canonical bottom target face: end={orthogonal_end:?}, path={:?}",
        orthogonal_edge.path
    );

    // R-BACK-8: channel lane must have minimum clearance from node envelope.
    let node_envelope_bottom =
        (source_rect.y + source_rect.height).max(target_rect.y + target_rect.height);
    let orthogonal_lane_y = orthogonal_edge
        .path
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);

    assert!(
        orthogonal_lane_y >= node_envelope_bottom + MIN_CHANNEL_CLEARANCE - 0.001,
        "orthogonal Remote -> Working channel lane should have >= {MIN_CHANNEL_CLEARANCE}px clearance from node envelope (R-BACK-8): node_envelope_bottom={node_envelope_bottom}, orthogonal_lane_y={orthogonal_lane_y}, clearance={}, path={:?}",
        orthogonal_lane_y - node_envelope_bottom,
        orthogonal_edge.path
    );

    let full_bends = bend_count(&full_edge.path);
    let orthogonal_bends = bend_count(&orthogonal_edge.path);
    assert!(
        orthogonal_bends <= full_bends + MAX_BEND_INCREASE_FROM_FULL,
        "orthogonal Remote -> Working should avoid extra loop compaction bends relative to polyline baseline: full_bends={full_bends}, orthogonal_bends={orthogonal_bends}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path,
        orthogonal_edge.path
    );
}

#[test]
fn orthogonal_route_git_workflow_backward_no_target_node_intrusion() {
    const INTRUSION_MARGIN: f64 = 1.0;
    const MAX_BACKWARD_POINT_COUNT: usize = 5;

    let fixture = "git_workflow.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    assert_eq!(
        geom.direction,
        mmdflux::Direction::LeftRight,
        "fixture {fixture} should be LR for target-node intrusion checks"
    );

    let target_rect = geom
        .nodes
        .get("Working")
        .unwrap_or_else(|| panic!("fixture {fixture} should contain target node Working"))
        .rect;
    let left = target_rect.x + INTRUSION_MARGIN;
    let right = target_rect.x + target_rect.width - INTRUSION_MARGIN;
    let top = target_rect.y + INTRUSION_MARGIN;
    let bottom = target_rect.y + target_rect.height - INTRUSION_MARGIN;

    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "Remote" && edge.to == "Working")
        .expect("fixture should contain backward edge Remote -> Working in orthogonal mode");
    assert!(
        edge.is_backward,
        "fixture contract invalid: Remote -> Working should be backward in orthogonal mode"
    );
    assert!(
        edge.path.len() <= MAX_BACKWARD_POINT_COUNT,
        "orthogonal Remote -> Working should stay compact after removing target-body intrusions (max points={MAX_BACKWARD_POINT_COUNT}): path={:?}",
        edge.path
    );

    if edge.path.len() >= 3 && left < right && top < bottom {
        for idx in 1..(edge.path.len() - 1) {
            let point = edge.path[idx];
            let inside = point.x > left && point.x < right && point.y > top && point.y < bottom;
            assert!(
                !inside,
                "orthogonal Remote -> Working should not route interior support points through Working node body: idx={idx}, point={point:?}, target_rect={target_rect:?}, path={:?}",
                edge.path
            );
        }
    }
}

#[test]
fn orthogonal_route_http_request_backward_response_to_client_preserves_min_right_clearance() {
    const MAX_RIGHT_CLEARANCE_SHRINK_FROM_FULL: f64 = 8.0;

    let fixture = "http_request.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
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
        .expect("fixture should contain backward edge Response -> Client in polyline mode");
    let orthogonal_edge = orthogonal
        .edges
        .iter()
        .find(|edge| edge.from == "Response" && edge.to == "Client")
        .expect("fixture should contain backward edge Response -> Client in orthogonal mode");

    assert!(
        full_edge.is_backward && orthogonal_edge.is_backward,
        "fixture contract invalid: Response -> Client should be backward in both edge routings"
    );

    let full_start = full_edge
        .path
        .first()
        .copied()
        .expect("polyline backward edge should have source endpoint");
    let full_end = full_edge
        .path
        .last()
        .copied()
        .expect("polyline backward edge should have target endpoint");
    let orthogonal_start = orthogonal_edge
        .path
        .first()
        .copied()
        .expect("orthogonal backward edge should have source endpoint");
    let orthogonal_end = orthogonal_edge
        .path
        .last()
        .copied()
        .expect("orthogonal backward edge should have target endpoint");

    let _full_source_face = point_on_target_face(source_rect, full_start);
    let _full_target_face = point_on_target_face(target_rect, full_end);
    let orthogonal_source_face = point_on_target_face(source_rect, orthogonal_start);
    let orthogonal_target_face = point_on_target_face(target_rect, orthogonal_end);

    assert_eq!(
        orthogonal_source_face, "right",
        "orthogonal Response -> Client should preserve canonical right source face while normalizing right-side clearance: start={orthogonal_start:?}, path={:?}",
        orthogonal_edge.path
    );
    assert_eq!(
        orthogonal_target_face, "right",
        "orthogonal Response -> Client should preserve canonical right target face while normalizing right-side clearance: end={orthogonal_end:?}, path={:?}",
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
        orthogonal_right_lane_x + MAX_RIGHT_CLEARANCE_SHRINK_FROM_FULL >= full_right_lane_x,
        "orthogonal Response -> Client should preserve minimum right-side backward clearance close to polyline baseline (allowed shrink <= {MAX_RIGHT_CLEARANCE_SHRINK_FROM_FULL}): full_right_lane_x={full_right_lane_x}, orthogonal_right_lane_x={orthogonal_right_lane_x}, full_path={:?}, orthogonal_path={:?}",
        full_edge.path,
        orthogonal_edge.path
    );
}

// -----------------------------------------------------------------------
// Task 0.2: Fan-in overflow policy spec contracts
// -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum FanInSpecDirection {
    TopDown,
    BottomTop,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverflowSide {
    LeftOrTop,
    RightOrBottom,
}

const FAN_IN_PRIMARY_FACE_CAPACITY_TD_BT: usize = 4;
const FAN_IN_PRIMARY_FACE_CAPACITY_LR_RL: usize = 2;
const FAN_IN_MIN_PRIMARY_SLOT_SPACING: f64 = 16.0;
const FAN_IN_MIN_CORNER_INSET_FORWARD: f64 = 8.0;

fn fan_in_primary_face_capacity(direction: FanInSpecDirection, face_span: f64) -> usize {
    let _baseline = match direction {
        FanInSpecDirection::TopDown | FanInSpecDirection::BottomTop => {
            FAN_IN_PRIMARY_FACE_CAPACITY_TD_BT
        }
        FanInSpecDirection::LeftRight | FanInSpecDirection::RightLeft => {
            FAN_IN_PRIMARY_FACE_CAPACITY_LR_RL
        }
    };
    let usable = (face_span - 2.0 * FAN_IN_MIN_CORNER_INSET_FORWARD).max(0.0);
    if usable <= f64::EPSILON {
        1
    } else {
        (usable / FAN_IN_MIN_PRIMARY_SLOT_SPACING).floor() as usize + 1
    }
}

fn fan_in_overflow_activates(
    direction: FanInSpecDirection,
    incoming_degree: usize,
    face_span: f64,
) -> bool {
    incoming_degree > fan_in_primary_face_capacity(direction, face_span)
}

fn fan_in_overflow_distribution_order(
    _direction: FanInSpecDirection,
    overflow_count: usize,
) -> Vec<OverflowSide> {
    let mut order = Vec::with_capacity(overflow_count);
    for index in 0..overflow_count {
        if index % 2 == 0 {
            order.push(OverflowSide::LeftOrTop);
        } else {
            order.push(OverflowSide::RightOrBottom);
        }
    }
    order
}

#[test]
fn fan_in_overflow_policy_spec_defines_when_overflow_must_activate() {
    let cases = [
        (
            "stacked_fan_in.mmd",
            FanInSpecDirection::TopDown,
            "C",
            2,
            false,
        ),
        ("fan_in.mmd", FanInSpecDirection::TopDown, "D", 3, false),
        (
            "five_fan_in.mmd",
            FanInSpecDirection::TopDown,
            "F",
            5,
            false,
        ),
        (
            "fan_in_lr.mmd",
            FanInSpecDirection::LeftRight,
            "D",
            3,
            false,
        ),
        (
            "fan_in_backward_channel_conflict.mmd",
            FanInSpecDirection::TopDown,
            "B",
            6,
            false,
        ),
    ];

    for (fixture, direction, target, incoming_degree, expected_overflow) in cases {
        let (_, geom) = layout_fixture_svg(fixture);
        let target_rect = geom
            .nodes
            .get(target)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target node {target}"))
            .rect;
        let face_span = match direction {
            FanInSpecDirection::TopDown | FanInSpecDirection::BottomTop => target_rect.width,
            FanInSpecDirection::LeftRight | FanInSpecDirection::RightLeft => target_rect.height,
        };
        let actual = fan_in_overflow_activates(direction, incoming_degree, face_span);
        assert_eq!(
            actual, expected_overflow,
            "Fan-in overflow activation contract mismatch for fixture {fixture}: direction={direction:?}, incoming_degree={incoming_degree}, face_span={face_span}"
        );
    }

    assert!(
        fan_in_overflow_activates(FanInSpecDirection::TopDown, 8, 106.4),
        "adaptive fan-in capacity should still overflow when inbound degree exceeds available primary-face slots"
    );
}

#[test]
fn fan_in_overflow_policy_spec_defines_spill_distribution_order() {
    let td_order = fan_in_overflow_distribution_order(FanInSpecDirection::TopDown, 4);
    assert_eq!(
        td_order,
        vec![
            OverflowSide::LeftOrTop,
            OverflowSide::RightOrBottom,
            OverflowSide::LeftOrTop,
            OverflowSide::RightOrBottom,
        ],
        "TD/BT overflow slots should alternate side lanes for deterministic spread"
    );

    let lr_order = fan_in_overflow_distribution_order(FanInSpecDirection::LeftRight, 3);
    assert_eq!(
        lr_order,
        vec![
            OverflowSide::LeftOrTop,
            OverflowSide::RightOrBottom,
            OverflowSide::LeftOrTop,
        ],
        "LR/RL overflow slots should alternate side lanes for deterministic spread"
    );

    let bt_order = fan_in_overflow_distribution_order(FanInSpecDirection::BottomTop, 2);
    assert_eq!(
        bt_order,
        vec![OverflowSide::LeftOrTop, OverflowSide::RightOrBottom],
        "BT overflow slots should mirror TD side-lane alternation"
    );

    let rl_order = fan_in_overflow_distribution_order(FanInSpecDirection::RightLeft, 2);
    assert_eq!(
        rl_order,
        vec![OverflowSide::LeftOrTop, OverflowSide::RightOrBottom],
        "RL overflow slots should mirror LR side-lane alternation"
    );
}

#[test]
fn fan_in_backward_channel_conflict_resolution_is_deterministic_and_documented() {
    let fixture = "fan_in_backward_channel_conflict.mmd";
    let (diagram, geom) = layout_fixture_svg(fixture);
    let first = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let second = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    assert_eq!(
        first.edges.len(),
        second.edges.len(),
        "routed edge count should be deterministic"
    );

    let target_rect = geom
        .nodes
        .get("B")
        .expect("fan_in_backward_channel_conflict fixture should contain node B")
        .rect;
    let conflict = first
        .edges
        .iter()
        .find(|edge| edge.from == "Loop" && edge.to == "B")
        .expect("fixture should contain Loop -> B");

    assert!(
        conflict.is_backward,
        "Loop -> B must be backward in orthogonal routing layout for this fixture"
    );

    let source_rect = geom
        .nodes
        .get("Loop")
        .expect("fan_in_backward_channel_conflict fixture should contain node Loop")
        .rect;
    let conflict_start = conflict
        .path
        .first()
        .copied()
        .expect("backward edge should have source endpoint");
    let conflict_start_face = point_on_target_face(source_rect, conflict_start);
    // With corrected reversed-chain-edge tracking, the variable spacing
    // feature no longer inflates gaps for this fixture, so the backward
    // edge departs from right face instead of top.
    assert_eq!(
        conflict_start_face, "right",
        "Loop -> B should depart from the right face (corrected spacing): start={conflict_start:?}, path={:?}",
        conflict.path
    );
    let source_face_margin = match conflict_start_face {
        "top" | "bottom" => {
            let source_left = source_rect.x;
            let source_right = source_rect.x + source_rect.width;
            (conflict_start.x - source_left).min(source_right - conflict_start.x)
        }
        "left" | "right" => {
            let source_top = source_rect.y;
            let source_bottom = source_rect.y + source_rect.height;
            (conflict_start.y - source_top).min(source_bottom - conflict_start.y)
        }
        _ => 0.0,
    };
    assert!(
        source_face_margin >= 5.0,
        "Loop -> B source departure should stay away from source face borders (closer to center) to avoid cramped hooks: margin={source_face_margin}, source_rect={source_rect:?}, start={conflict_start:?}, path={:?}",
        conflict.path
    );
    let conflict_next = conflict
        .path
        .get(1)
        .copied()
        .expect("backward edge should have source support point");
    assert!(
        source_support_is_normal_to_attached_rect_face(source_rect, conflict_start, conflict_next),
        "Loop -> B should leave the canonical source face on its outward normal axis: start={conflict_start:?}, next={conflict_next:?}, path={:?}",
        conflict.path
    );

    let conflict_end = *conflict
        .path
        .last()
        .expect("backward edge should have path endpoint");
    let conflict_face = point_on_target_face(target_rect, conflict_end);
    assert_eq!(
        conflict_face,
        "right",
        "Loop -> B should enter B on the right face (corrected spacing): end={conflict_end:?}, path={path:?}",
        conflict_end = conflict_end,
        path = conflict.path
    );
    let conflict_prev = conflict
        .path
        .get(conflict.path.len().saturating_sub(2))
        .copied()
        .expect("backward edge should have terminal support point");
    assert!(
        terminal_support_is_normal_to_attached_rect_face(target_rect, conflict_prev, conflict_end),
        "Loop -> B should approach the canonical right face with a face-normal terminal segment: prev={conflict_prev:?}, end={conflict_end:?}, path={:?}",
        conflict.path
    );

    let incoming_to_b: Vec<_> = first.edges.iter().filter(|edge| edge.to == "B").collect();
    if std::env::var("MMDFLUX_DEBUG_FAN_IN").is_ok_and(|v| v == "1") {
        for edge in &incoming_to_b {
            let end = edge
                .path
                .last()
                .copied()
                .expect("inbound edge should have endpoint");
            let end_face = point_on_target_face(target_rect, end);
            eprintln!(
                "edge {}->{} index={} backward={} end={:?} face={}",
                edge.from, edge.to, edge.index, edge.is_backward, end, end_face
            );
        }
    }
    assert_eq!(
        incoming_to_b.len(),
        6,
        "fan_in_backward_channel_conflict should create exactly six inbound edges to B"
    );

    let right_face_count = incoming_to_b
        .iter()
        .filter(|edge| {
            let end = edge
                .path
                .last()
                .copied()
                .expect("inbound edge should have endpoint");
            point_on_target_face(target_rect, end) == "right"
        })
        .count();

    // With corrected reversed-chain-edge tracking, the backward Loop->B edge
    // now routes via the right face (reduced spacing from the fix), so exactly
    // 1 inbound edge arrives on B's right face.
    assert_eq!(
        right_face_count, 1,
        "Loop->B backward edge should arrive on B's right face with corrected spacing: right_face_count={right_face_count}"
    );
}

#[test]
fn fan_in_backward_channel_interaction_fixture_matrix_matches_documented_face_policies() {
    let fan_in_cases = [
        ("stacked_fan_in.mmd", "C", 0usize),
        ("fan_in.mmd", "D", 0usize),
        ("five_fan_in.mmd", "F", 0usize),
    ];

    for (fixture, target, min_side_faces) in fan_in_cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
        let target_rect = geom
            .nodes
            .get(target)
            .unwrap_or_else(|| panic!("fixture {fixture} should contain target node {target}"))
            .rect;

        let inbound: Vec<_> = routed
            .edges
            .iter()
            .filter(|edge| edge.to == target && !edge.is_backward)
            .collect();
        assert!(
            !inbound.is_empty(),
            "fixture {fixture} should have inbound edges to {target}"
        );

        let interior_count = inbound
            .iter()
            .filter(|edge| {
                let end = edge
                    .path
                    .last()
                    .copied()
                    .expect("inbound edge should have endpoint");
                point_inside_rect(target_rect, end)
            })
            .count();
        assert_eq!(
            interior_count,
            0,
            "fixture {fixture} should not place inbound endpoints inside target interior under fan-in overflow policy (target={target}, routed={:?})",
            inbound
                .iter()
                .map(|edge| (edge.from.as_str(), edge.path.clone()))
                .collect::<Vec<_>>()
        );

        let side_face_count = inbound
            .iter()
            .filter(|edge| {
                let end = edge
                    .path
                    .last()
                    .copied()
                    .expect("inbound edge should have endpoint");
                matches!(point_on_target_face(target_rect, end), "left" | "right")
            })
            .count();

        if min_side_faces == 0 {
            assert_eq!(
                side_face_count, 0,
                "fixture {fixture} should stay on primary TD incoming face without overflow (target={target})"
            );
        } else {
            assert!(
                side_face_count >= min_side_faces,
                "fixture {fixture} should spill overflow arrivals to side faces under fan-in overflow policy: expected >= {min_side_faces}, actual={side_face_count}, target={target}"
            );
        }
    }

    let backward_channel_cases = [
        // Corridor-obstructed backward edges route via the canonical backward
        // face (right in TD) for both source and target, matching the
        // non-orthogonal channel path approach.
        ("simple_cycle.mmd", "C", "A", "right", "right"),
        ("multiple_cycles.mmd", "C", "A", "right", "right"),
        (
            "fan_in_backward_channel_conflict.mmd",
            "Loop",
            "B",
            "right",
            "right",
        ),
        ("http_request.mmd", "Response", "Client", "right", "right"),
        ("git_workflow.mmd", "Remote", "Working", "bottom", "bottom"),
    ];

    for (fixture, from, to, expected_target_face, expected_source_face) in backward_channel_cases {
        let (diagram, geom) = layout_fixture_svg(fixture);
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

        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        let edge = routed
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
            .unwrap_or_else(|| panic!("fixture {fixture} missing edge {from} -> {to}"));
        let start = edge
            .path
            .first()
            .copied()
            .expect("backward edge should have source endpoint");
        let start_face = point_on_target_face(source_rect, start);
        assert_eq!(
            start_face, expected_source_face,
            "fixture {fixture} edge {from}->{to} should keep canonical backward source face {expected_source_face}; start={start:?}, path={:?}",
            edge.path
        );
        let end = edge
            .path
            .last()
            .copied()
            .expect("backward edge should have endpoint");
        let end_face = point_on_target_face(target_rect, end);
        assert_eq!(
            end_face, expected_target_face,
            "fixture {fixture} edge {from}->{to} should keep canonical backward target face {expected_target_face}; end={end:?}, path={:?}",
            edge.path
        );
    }
}

#[test]
fn five_fan_in_lr_overflow_spills_to_cross_faces_and_spreads_target_ports() {
    let (diagram, geom) = layout_fixture_svg("five_fan_in_lr.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::LeftRight);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let target_rect = geom
        .nodes
        .get("F")
        .expect("five_fan_in_lr should contain target F")
        .rect;
    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "F" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        5,
        "five_fan_in_lr should produce five inbound forward edges to F"
    );

    let mut left_face_count = 0usize;
    let mut right_face_count = 0usize;
    let mut top_or_bottom_count = 0usize;
    let mut overflow_paths: Vec<(String, Vec<FPoint>)> = Vec::new();
    let mut unique_endpoints: Vec<FPoint> = Vec::new();
    for edge in &inbound {
        let end = edge
            .path
            .last()
            .copied()
            .expect("inbound edge should have endpoint");
        let prev = edge
            .path
            .get(edge.path.len().saturating_sub(2))
            .copied()
            .expect("inbound edge should have terminal support point");

        assert!(
            !point_inside_rect(target_rect, end),
            "five_fan_in_lr endpoint should stay on or outside target border: {} -> F end={end:?}, target_rect={target_rect:?}, path={:?}",
            edge.from,
            edge.path
        );
        assert!(
            terminal_support_is_normal_to_attached_rect_face(target_rect, prev, end),
            "five_fan_in_lr terminal segment should remain face-normal: {} -> F prev={prev:?}, end={end:?}, target_rect={target_rect:?}, path={:?}",
            edge.from,
            edge.path
        );

        match point_on_target_face(target_rect, end) {
            "left" => left_face_count += 1,
            "right" => right_face_count += 1,
            "top" | "bottom" => {
                top_or_bottom_count += 1;
                overflow_paths.push((edge.from.clone(), edge.path.clone()));
            }
            _ => {}
        }

        if unique_endpoints
            .iter()
            .all(|point| (point.x - end.x).abs() > 0.5 || (point.y - end.y).abs() > 0.5)
        {
            unique_endpoints.push(end);
        }
    }

    assert_eq!(
        right_face_count, 0,
        "five_fan_in_lr should not attach forward fan-in overflow to RL-facing face of target"
    );
    assert!(
        left_face_count <= 3,
        "five_fan_in_lr should cap primary left-face fan-in slots before spill: left_face_count={left_face_count}"
    );
    assert!(
        top_or_bottom_count >= 1,
        "five_fan_in_lr should spill at least one inbound edge to top/bottom faces when primary face is full"
    );
    assert!(
        unique_endpoints.len() >= 4,
        "five_fan_in_lr should spread target ports instead of collapsing to a small set: unique_endpoints={unique_endpoints:?}"
    );
    for i in 0..overflow_paths.len() {
        for j in (i + 1)..overflow_paths.len() {
            assert!(
                !has_coincident_vertical_overlap(&overflow_paths[i].1, &overflow_paths[j].1),
                "five_fan_in_lr should avoid coincident vertical overflow channel overlap between {} -> F and {} -> F when routing to cross-faces: left={:?} right={:?}",
                overflow_paths[i].0,
                overflow_paths[j].0,
                overflow_paths[i].1,
                overflow_paths[j].1
            );
        }
    }
}

#[test]
fn five_fan_in_rl_overflow_spills_to_cross_faces_and_spreads_target_ports() {
    let input = r#"
graph RL
    A[A] --> F[Target]
    B[B] --> F
    C[C] --> F
    D[D] --> F
    E[E] --> F
"#;
    let (diagram, geom) = layout_test_svg(input);
    assert_eq!(geom.direction, mmdflux::Direction::RightLeft);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let target_rect = geom
        .nodes
        .get("F")
        .expect("five_fan_in_rl should contain target F")
        .rect;
    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "F" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        5,
        "five_fan_in_rl should produce five inbound forward edges to F"
    );

    let mut right_face_count = 0usize;
    let mut left_face_count = 0usize;
    let mut top_or_bottom_count = 0usize;
    let mut overflow_paths: Vec<(String, Vec<FPoint>)> = Vec::new();
    let mut unique_endpoints: Vec<FPoint> = Vec::new();
    for edge in &inbound {
        let end = edge
            .path
            .last()
            .copied()
            .expect("inbound edge should have endpoint");
        let prev = edge
            .path
            .get(edge.path.len().saturating_sub(2))
            .copied()
            .expect("inbound edge should have terminal support point");

        assert!(
            !point_inside_rect(target_rect, end),
            "five_fan_in_rl endpoint should stay on or outside target border: {} -> F end={end:?}, target_rect={target_rect:?}, path={:?}",
            edge.from,
            edge.path
        );
        assert!(
            terminal_support_is_normal_to_attached_rect_face(target_rect, prev, end),
            "five_fan_in_rl terminal segment should remain face-normal: {} -> F prev={prev:?}, end={end:?}, target_rect={target_rect:?}, path={:?}",
            edge.from,
            edge.path
        );

        match point_on_target_face(target_rect, end) {
            "right" => right_face_count += 1,
            "left" => left_face_count += 1,
            "top" | "bottom" => {
                top_or_bottom_count += 1;
                overflow_paths.push((edge.from.clone(), edge.path.clone()));
            }
            _ => {}
        }

        if unique_endpoints
            .iter()
            .all(|point| (point.x - end.x).abs() > 0.5 || (point.y - end.y).abs() > 0.5)
        {
            unique_endpoints.push(end);
        }
    }

    assert_eq!(
        left_face_count, 0,
        "five_fan_in_rl should not attach forward fan-in overflow to LR-facing face of target"
    );
    assert!(
        right_face_count <= 3,
        "five_fan_in_rl should cap primary right-face fan-in slots before spill: right_face_count={right_face_count}"
    );
    assert!(
        top_or_bottom_count >= 1,
        "five_fan_in_rl should spill at least one inbound edge to top/bottom faces when primary face is full"
    );
    assert!(
        unique_endpoints.len() >= 4,
        "five_fan_in_rl should spread target ports instead of collapsing to a small set: unique_endpoints={unique_endpoints:?}"
    );
    for i in 0..overflow_paths.len() {
        for j in (i + 1)..overflow_paths.len() {
            assert!(
                !has_coincident_vertical_overlap(&overflow_paths[i].1, &overflow_paths[j].1),
                "five_fan_in_rl should avoid coincident vertical overflow channel overlap between {} -> F and {} -> F when routing to cross-faces: left={:?} right={:?}",
                overflow_paths[i].0,
                overflow_paths[j].0,
                overflow_paths[i].1,
                overflow_paths[j].1
            );
        }
    }
}

#[test]
fn fan_in_overflow_arrivals_on_same_side_face_are_spread_not_piled_up() {
    let input = r#"
graph TD
    A --> T[Target]
    B --> T
    C --> T
    D --> T
    E --> T
    F --> T
    G --> T
    H --> T
"#;
    let (diagram, geom) = layout_test_svg(input);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let target_rect = geom
        .nodes
        .get("T")
        .expect("inline fan-in fixture should contain target T")
        .rect;

    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "T" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        8,
        "inline fan-in fixture should produce eight forward inbound edges to T"
    );

    let mut right_face_ys = Vec::new();
    let mut left_face_ys = Vec::new();
    let mut side_face_paths: Vec<(String, Vec<FPoint>)> = Vec::new();
    for edge in &inbound {
        let end = edge
            .path
            .last()
            .copied()
            .expect("inbound edge should have endpoint");
        let face = point_on_target_face(target_rect, end);
        if matches!(face, "left" | "right") {
            let prev = edge.path[edge.path.len() - 2];
            assert!(
                terminal_support_is_normal_to_attached_rect_face(target_rect, prev, end),
                "overflow fan-in side-face terminal should approach along face-normal axis for {} -> {}: face={face}, prev={prev:?}, end={end:?}, path={:?}",
                edge.from,
                edge.to,
                edge.path
            );
        }
        match face {
            "right" => {
                right_face_ys.push(end.y);
                side_face_paths.push((edge.from.clone(), edge.path.clone()));
            }
            "left" => {
                left_face_ys.push(end.y);
                side_face_paths.push((edge.from.clone(), edge.path.clone()));
            }
            _ => {}
        }
    }

    let side_face_count = right_face_ys.len() + left_face_ys.len();
    assert!(
        side_face_count >= 2,
        "overflow fan-in should place at least two inbound endpoints on side faces: right={right_face_ys:?}, left={left_face_ys:?}"
    );

    let mut observed_multi_slot_side = false;
    for (face, mut coords) in [("right", right_face_ys), ("left", left_face_ys)] {
        if coords.len() <= 1 {
            continue;
        }
        observed_multi_slot_side = true;
        coords.sort_by(|a, b| a.total_cmp(b));
        let mut unique_count = 0usize;
        let mut last: Option<f64> = None;
        for value in &coords {
            let is_new = match last {
                Some(prev) => (*value - prev).abs() > 0.5,
                None => true,
            };
            if is_new {
                unique_count += 1;
                last = Some(*value);
            }
        }
        assert_eq!(
            unique_count,
            coords.len(),
            "side-face endpoints should use distinct attachment slots on {face}: coords={coords:?}"
        );
    }
    assert!(
        observed_multi_slot_side,
        "expected at least one side face with multiple overflow arrivals to validate slot spreading"
    );

    for i in 0..side_face_paths.len() {
        for j in (i + 1)..side_face_paths.len() {
            assert!(
                !has_coincident_horizontal_overlap(&side_face_paths[i].1, &side_face_paths[j].1),
                "overflow fan-in side-face channels should avoid coincident horizontal overlap between {} -> T and {} -> T: left={:?} right={:?}",
                side_face_paths[i].0,
                side_face_paths[j].0,
                side_face_paths[i].1,
                side_face_paths[j].1
            );
        }
    }
}

#[test]
fn very_narrow_fan_in_primary_face_ports_do_not_collapse_to_single_anchor() {
    let (diagram, geom) = layout_fixture_svg("very_narrow_fan_in.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let target_rect = geom
        .nodes
        .get("E")
        .expect("very_narrow_fan_in should contain target E")
        .rect;

    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "E" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        4,
        "very_narrow_fan_in should produce four inbound forward edges to E"
    );

    let mut top_xs = Vec::new();
    let mut side_count = 0usize;
    for edge in &inbound {
        let end = edge
            .path
            .last()
            .copied()
            .expect("inbound edge should have endpoint");
        match point_on_target_face(target_rect, end) {
            "top" => top_xs.push(end.x),
            "left" | "right" => side_count += 1,
            _ => {}
        }
    }

    assert_eq!(
        side_count, 0,
        "very_narrow_fan_in should stay on the primary TD target face when span can host all inbound slots"
    );
    assert_eq!(
        top_xs.len(),
        4,
        "very_narrow_fan_in should attach all inbound edges on top face"
    );

    top_xs.sort_by(|a, b| a.total_cmp(b));
    let mut unique_count = 0usize;
    let mut last: Option<f64> = None;
    for value in &top_xs {
        let is_new = match last {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_count += 1;
            last = Some(*value);
        }
    }

    assert_eq!(
        unique_count, 4,
        "very_narrow_fan_in top-face target ports should occupy distinct anchors instead of collapsing: top_xs={top_xs:?}"
    );
}

#[test]
fn five_fan_in_primary_face_channels_are_staggered_without_overlap() {
    let (diagram, geom) = layout_fixture_svg("five_fan_in.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let target_rect = geom
        .nodes
        .get("F")
        .expect("five_fan_in should contain target F")
        .rect;

    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "F" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        5,
        "five_fan_in should produce five inbound forward edges to F"
    );

    for edge in &inbound {
        let end = edge
            .path
            .last()
            .copied()
            .expect("inbound edge should have endpoint");
        assert_eq!(
            point_on_target_face(target_rect, end),
            "top",
            "five_fan_in should keep all inbound endpoints on F's top face under adaptive capacity: {} -> F path={:?}",
            edge.from,
            edge.path
        );
    }

    for i in 0..inbound.len() {
        for j in (i + 1)..inbound.len() {
            assert!(
                !has_coincident_horizontal_overlap(&inbound[i].path, &inbound[j].path),
                "five_fan_in should avoid coincident horizontal channel overlap between {} -> F and {} -> F: left={:?} right={:?}",
                inbound[i].from,
                inbound[j].from,
                inbound[i].path,
                inbound[j].path
            );
        }
    }

    let mut lanes_by_source: HashMap<String, f64> = HashMap::new();
    for edge in &inbound {
        if let Some(y) = td_bt_middle_horizontal_lane(&edge.path) {
            lanes_by_source.insert(edge.from.clone(), y);
        }
    }
    for source in ["A", "B", "D", "E"] {
        assert!(
            lanes_by_source.contains_key(source),
            "five_fan_in should route {source} -> F with a V-H-V fan-in channel path"
        );
    }

    let a_lane = *lanes_by_source
        .get("A")
        .expect("lane for A -> F should be present");
    let b_lane = *lanes_by_source
        .get("B")
        .expect("lane for B -> F should be present");
    let d_lane = *lanes_by_source
        .get("D")
        .expect("lane for D -> F should be present");
    let e_lane = *lanes_by_source
        .get("E")
        .expect("lane for E -> F should be present");
    assert!(
        b_lane + 0.5 < a_lane,
        "five_fan_in should place inner-left B -> F shallower than outer-left A -> F to reduce crossing pressure: A={a_lane}, B={b_lane}"
    );
    assert!(
        d_lane + 0.5 < e_lane,
        "five_fan_in should place inner-right D -> F shallower than outer-right E -> F to reduce crossing pressure: E={e_lane}, D={d_lane}"
    );
    assert!(
        (b_lane - d_lane).abs() <= 0.5,
        "five_fan_in mirrored inner pair should share a symmetric lane depth: B={b_lane}, D={d_lane}"
    );
    assert!(
        (a_lane - e_lane).abs() <= 0.5,
        "five_fan_in mirrored outer pair should share a symmetric lane depth: A={a_lane}, E={e_lane}"
    );
}

#[test]
fn five_fan_in_diamond_target_ports_use_distinct_primary_slots() {
    let (diagram, geom) = layout_fixture_svg("five_fan_in_diamond.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "F" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        5,
        "five_fan_in_diamond should produce five inbound forward edges to F"
    );

    let mut target_xs: Vec<f64> = inbound
        .iter()
        .map(|edge| {
            edge.path
                .last()
                .copied()
                .expect("edge should have endpoint")
                .x
        })
        .collect();
    target_xs.sort_by(|a, b| a.total_cmp(b));
    let mut unique_target_count = 0usize;
    let mut last_target: Option<f64> = None;
    for value in &target_xs {
        let is_new = match last_target {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_target_count += 1;
            last_target = Some(*value);
        }
    }
    assert_eq!(
        unique_target_count, 5,
        "five_fan_in_diamond target ports should occupy five distinct primary-face slots instead of collapsing: target_xs={target_xs:?}"
    );
}

#[test]
fn five_fan_out_primary_face_channels_are_staggered_without_overlap() {
    let (diagram, geom) = layout_fixture_svg("five_fan_out.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let outbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.from == "A" && !edge.is_backward)
        .collect();
    assert_eq!(
        outbound.len(),
        5,
        "five_fan_out should produce five outbound forward edges from A"
    );

    let mut source_xs: Vec<f64> = outbound
        .iter()
        .map(|edge| {
            edge.path
                .first()
                .copied()
                .expect("edge should have start")
                .x
        })
        .collect();
    source_xs.sort_by(|a, b| a.total_cmp(b));
    let mut unique_source_count = 0usize;
    let mut last_source: Option<f64> = None;
    for value in &source_xs {
        let is_new = match last_source {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_source_count += 1;
            last_source = Some(*value);
        }
    }
    assert_eq!(
        unique_source_count, 5,
        "five_fan_out source ports should occupy distinct attachment anchors: source_xs={source_xs:?}"
    );

    for i in 0..outbound.len() {
        for j in (i + 1)..outbound.len() {
            assert!(
                !has_coincident_horizontal_overlap(&outbound[i].path, &outbound[j].path),
                "five_fan_out should avoid coincident horizontal channel overlap between A -> {} and A -> {}: left={:?} right={:?}",
                outbound[i].to,
                outbound[j].to,
                outbound[i].path,
                outbound[j].path
            );
        }
    }

    let mut lanes_by_target: HashMap<String, f64> = HashMap::new();
    for edge in &outbound {
        if let Some(y) = td_bt_middle_horizontal_lane(&edge.path) {
            lanes_by_target.insert(edge.to.clone(), y);
        }
    }
    for target in ["B", "C", "E", "F"] {
        assert!(
            lanes_by_target.contains_key(target),
            "five_fan_out should route A -> {target} with a V-H-V fan-out channel path"
        );
    }

    let b_lane = *lanes_by_target
        .get("B")
        .expect("lane for A -> B should be present");
    let c_lane = *lanes_by_target
        .get("C")
        .expect("lane for A -> C should be present");
    let e_lane = *lanes_by_target
        .get("E")
        .expect("lane for A -> E should be present");
    let f_lane = *lanes_by_target
        .get("F")
        .expect("lane for A -> F should be present");
    assert!(
        b_lane + 0.5 < c_lane,
        "five_fan_out should place outer-left A -> B shallower than inner-left A -> C for source-centric fan-out opening: B={b_lane}, C={c_lane}"
    );
    assert!(
        f_lane + 0.5 < e_lane,
        "five_fan_out should place outer-right A -> F shallower than inner-right A -> E for source-centric fan-out opening: F={f_lane}, E={e_lane}"
    );
    assert!(
        (c_lane - e_lane).abs() <= 0.5,
        "five_fan_out mirrored inner pair should share a symmetric lane depth: C={c_lane}, E={e_lane}"
    );
    assert!(
        (b_lane - f_lane).abs() <= 0.5,
        "five_fan_out mirrored outer pair should share a symmetric lane depth: B={b_lane}, F={f_lane}"
    );
}

#[test]
fn criss_cross_forward_pair_uses_distinct_orthogonal_channels() {
    let (diagram, geom) = layout_fixture("criss_cross.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let c_to_d = routed
        .edges
        .iter()
        .find(|edge| edge.from == "C" && edge.to == "D")
        .expect("criss_cross should route C -> D");
    let b_to_e = routed
        .edges
        .iter()
        .find(|edge| edge.from == "B" && edge.to == "E")
        .expect("criss_cross should route B -> E");

    assert!(
        !has_coincident_horizontal_overlap(&c_to_d.path, &b_to_e.path),
        "criss_cross should avoid coincident horizontal overlap between C -> D and B -> E: C->D={:?}, B->E={:?}",
        c_to_d.path,
        b_to_e.path
    );
    assert!(
        !has_coincident_vertical_overlap(&c_to_d.path, &b_to_e.path),
        "criss_cross should avoid coincident vertical overlap between C -> D and B -> E: C->D={:?}, B->E={:?}",
        c_to_d.path,
        b_to_e.path
    );

    let path_with_corridor = if c_to_d.path.len() >= b_to_e.path.len() {
        &c_to_d.path
    } else {
        &b_to_e.path
    };
    assert!(
        path_with_corridor.len() >= 5,
        "criss_cross should preserve a multi-bend detour for one diagonal edge so the orthogonal routes stay legible: C->D={:?}, B->E={:?}",
        c_to_d.path,
        b_to_e.path
    );

    let corridor_x = path_with_corridor.iter().map(|point| point.x).sum::<f64>()
        / path_with_corridor.len() as f64;
    let diagram_center_x = geom
        .nodes
        .get("A")
        .expect("criss_cross should contain A")
        .rect
        .center_x();
    assert!(
        (corridor_x - diagram_center_x).abs() <= 24.0,
        "criss_cross detour should run through the center corridor instead of collapsing back onto the mirrored edge lane: corridor_x={corridor_x}, center_x={diagram_center_x}, path={path_with_corridor:?}"
    );

    let lane_clearance = minimum_parallel_clearance(&c_to_d.path, &b_to_e.path);
    assert!(
        lane_clearance >= 5.0,
        "criss_cross should keep a visible clearance band between the mirrored diagonals so the orthogonal routes remain legible in SVG and text: clearance={lane_clearance}, C->D={:?}, B->E={:?}",
        c_to_d.path,
        b_to_e.path
    );
}

fn minimum_parallel_clearance(path_a: &[FPoint], path_b: &[FPoint]) -> f64 {
    const EPS: f64 = 0.5;
    let mut best = f64::INFINITY;

    for seg_a in path_a.windows(2) {
        let a0 = seg_a[0];
        let a1 = seg_a[1];
        let a_min_x = a0.x.min(a1.x);
        let a_max_x = a0.x.max(a1.x);
        let a_min_y = a0.y.min(a1.y);
        let a_max_y = a0.y.max(a1.y);
        let a_is_horizontal = (a0.y - a1.y).abs() <= EPS && (a0.x - a1.x).abs() > EPS;
        let a_is_vertical = (a0.x - a1.x).abs() <= EPS && (a0.y - a1.y).abs() > EPS;

        for seg_b in path_b.windows(2) {
            let b0 = seg_b[0];
            let b1 = seg_b[1];
            let b_min_x = b0.x.min(b1.x);
            let b_max_x = b0.x.max(b1.x);
            let b_min_y = b0.y.min(b1.y);
            let b_max_y = b0.y.max(b1.y);
            let b_is_horizontal = (b0.y - b1.y).abs() <= EPS && (b0.x - b1.x).abs() > EPS;
            let b_is_vertical = (b0.x - b1.x).abs() <= EPS && (b0.y - b1.y).abs() > EPS;

            let clearance = if a_is_horizontal && b_is_horizontal {
                if a_max_x.min(b_max_x) - a_min_x.max(b_min_x) > EPS {
                    Some((a0.y - b0.y).abs())
                } else {
                    None
                }
            } else if a_is_vertical && b_is_vertical {
                if a_max_y.min(b_max_y) - a_min_y.max(b_min_y) > EPS {
                    Some((a0.x - b0.x).abs())
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(clearance) = clearance.filter(|value| *value > EPS) {
                best = best.min(clearance);
            }
        }
    }

    best
}

#[test]
fn five_fan_out_lr_primary_face_channels_are_staggered_without_overlap() {
    let (diagram, geom) = layout_fixture_svg("five_fan_out_lr.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::LeftRight);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let outbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.from == "A" && !edge.is_backward)
        .collect();
    assert_eq!(
        outbound.len(),
        5,
        "five_fan_out_lr should produce five outbound forward edges from A"
    );

    let mut source_ys: Vec<f64> = outbound
        .iter()
        .map(|edge| {
            edge.path
                .first()
                .copied()
                .expect("edge should have start")
                .y
        })
        .collect();
    source_ys.sort_by(|a, b| a.total_cmp(b));
    let mut unique_source_count = 0usize;
    let mut last_source: Option<f64> = None;
    for value in &source_ys {
        let is_new = match last_source {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_source_count += 1;
            last_source = Some(*value);
        }
    }
    assert_eq!(
        unique_source_count, 5,
        "five_fan_out_lr source ports should occupy distinct attachment anchors: source_ys={source_ys:?}"
    );

    for i in 0..outbound.len() {
        for j in (i + 1)..outbound.len() {
            assert!(
                !has_coincident_vertical_overlap(&outbound[i].path, &outbound[j].path),
                "five_fan_out_lr should avoid coincident vertical channel overlap between A -> {} and A -> {}: left={:?} right={:?}",
                outbound[i].to,
                outbound[j].to,
                outbound[i].path,
                outbound[j].path
            );
        }
    }

    let source_rect = geom
        .nodes
        .get("A")
        .expect("five_fan_out_lr should contain source A")
        .rect;
    let source_center_y = source_rect.y + source_rect.height / 2.0;
    for edge in &outbound {
        if !matches!(edge.to.as_str(), "B" | "C" | "E" | "F") {
            continue;
        }
        assert!(
            edge.path.len() >= 4,
            "five_fan_out_lr edge A -> {} should route with H-V-H fan-out channel path: {:?}",
            edge.to,
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];
        let third = edge.path[2];
        let source_offset = start.y - source_center_y;
        assert!(
            source_offset.abs() >= 1.0,
            "fixture expectation invalid: A -> {} source should be off-center, offset={source_offset}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (next.y - start.y).abs() <= ROUTE_EPS && (next.x - start.x).abs() > ROUTE_EPS,
            "five_fan_out_lr edge A -> {} should keep a short primary-axis source stem before sweeping: start={start:?}, next={next:?}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (third.x - next.x).abs() <= ROUTE_EPS && (third.y - next.y).abs() > ROUTE_EPS,
            "five_fan_out_lr edge A -> {} should sweep vertically after the source stem: next={next:?}, third={third:?}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (third.y - next.y).signum() == source_offset.signum(),
            "five_fan_out_lr edge A -> {} should sweep outward from source center: source_offset={source_offset}, second_dy={}, path={:?}",
            edge.to,
            third.y - next.y,
            edge.path
        );
        assert!(
            !path_has_primary_axis_reversal(&edge.path, geom.direction),
            "five_fan_out_lr edge A -> {} should not reverse along the LR primary axis: path={:?}",
            edge.to,
            edge.path
        );
    }

    let mut lanes_by_target: HashMap<String, f64> = HashMap::new();
    for edge in &outbound {
        if let Some(x) = lr_rl_middle_vertical_lane(&edge.path) {
            lanes_by_target.insert(edge.to.clone(), x);
        }
    }
    for target in ["B", "C", "E", "F"] {
        assert!(
            lanes_by_target.contains_key(target),
            "five_fan_out_lr should route A -> {target} with a H-V-H fan-out channel path"
        );
    }

    let b_lane = *lanes_by_target
        .get("B")
        .expect("lane for A -> B should be present");
    let c_lane = *lanes_by_target
        .get("C")
        .expect("lane for A -> C should be present");
    let e_lane = *lanes_by_target
        .get("E")
        .expect("lane for A -> E should be present");
    let f_lane = *lanes_by_target
        .get("F")
        .expect("lane for A -> F should be present");
    assert!(
        b_lane + 0.5 < c_lane,
        "five_fan_out_lr should place upper-outer A -> B shallower than upper-inner A -> C for source-centric fan-out opening: B={b_lane}, C={c_lane}"
    );
    assert!(
        f_lane + 0.5 < e_lane,
        "five_fan_out_lr should place lower-outer A -> F shallower than lower-inner A -> E for source-centric fan-out opening: F={f_lane}, E={e_lane}"
    );
    assert!(
        (c_lane - e_lane).abs() <= 0.5,
        "five_fan_out_lr mirrored inner pair should share a symmetric lane depth: C={c_lane}, E={e_lane}"
    );
    assert!(
        (b_lane - f_lane).abs() <= 0.5,
        "five_fan_out_lr mirrored outer pair should share a symmetric lane depth: B={b_lane}, F={f_lane}"
    );
}

#[test]
fn five_fan_out_rl_primary_face_channels_are_staggered_without_overlap() {
    let input = r#"
graph RL
      A[Source] --> B[Target A]
      A --> C[Target B]
      A --> D[Target C]
      A --> E[Target D]
      A --> F[Target E]
"#;
    let (diagram, geom) = layout_test_svg(input);
    assert_eq!(geom.direction, mmdflux::Direction::RightLeft);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let outbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.from == "A" && !edge.is_backward)
        .collect();
    assert_eq!(
        outbound.len(),
        5,
        "five_fan_out_rl should produce five outbound forward edges from A"
    );

    for i in 0..outbound.len() {
        for j in (i + 1)..outbound.len() {
            assert!(
                !has_coincident_vertical_overlap(&outbound[i].path, &outbound[j].path),
                "five_fan_out_rl should avoid coincident vertical channel overlap between A -> {} and A -> {}: left={:?} right={:?}",
                outbound[i].to,
                outbound[j].to,
                outbound[i].path,
                outbound[j].path
            );
        }
    }

    let source_rect = geom
        .nodes
        .get("A")
        .expect("five_fan_out_rl should contain source A")
        .rect;
    let source_center_y = source_rect.y + source_rect.height / 2.0;
    for edge in &outbound {
        if !matches!(edge.to.as_str(), "B" | "C" | "E" | "F") {
            continue;
        }
        assert!(
            edge.path.len() >= 4,
            "five_fan_out_rl edge A -> {} should route with H-V-H fan-out channel path: {:?}",
            edge.to,
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];
        let third = edge.path[2];
        let source_offset = start.y - source_center_y;
        assert!(
            source_offset.abs() >= 1.0,
            "fixture expectation invalid: A -> {} source should be off-center, offset={source_offset}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (next.y - start.y).abs() <= ROUTE_EPS && (next.x - start.x).abs() > ROUTE_EPS,
            "five_fan_out_rl edge A -> {} should keep a short primary-axis source stem before sweeping: start={start:?}, next={next:?}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            next.x < start.x - ROUTE_EPS,
            "five_fan_out_rl edge A -> {} primary-axis stem should move toward RL target side: start={start:?}, next={next:?}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (third.x - next.x).abs() <= ROUTE_EPS && (third.y - next.y).abs() > ROUTE_EPS,
            "five_fan_out_rl edge A -> {} should sweep vertically after the source stem: next={next:?}, third={third:?}, path={:?}",
            edge.to,
            edge.path
        );
        assert!(
            (third.y - next.y).signum() == source_offset.signum(),
            "five_fan_out_rl edge A -> {} should sweep outward from source center: source_offset={source_offset}, second_dy={}, path={:?}",
            edge.to,
            third.y - next.y,
            edge.path
        );
        assert!(
            !path_has_primary_axis_reversal(&edge.path, geom.direction),
            "five_fan_out_rl edge A -> {} should not reverse along the RL primary axis: path={:?}",
            edge.to,
            edge.path
        );
    }

    let mut lanes_by_target: HashMap<String, f64> = HashMap::new();
    for edge in &outbound {
        if let Some(x) = lr_rl_middle_vertical_lane(&edge.path) {
            lanes_by_target.insert(edge.to.clone(), x);
        }
    }
    for target in ["B", "C", "E", "F"] {
        assert!(
            lanes_by_target.contains_key(target),
            "five_fan_out_rl should route A -> {target} with a H-V-H fan-out channel path"
        );
    }

    let b_lane = *lanes_by_target
        .get("B")
        .expect("lane for A -> B should be present");
    let c_lane = *lanes_by_target
        .get("C")
        .expect("lane for A -> C should be present");
    let e_lane = *lanes_by_target
        .get("E")
        .expect("lane for A -> E should be present");
    let f_lane = *lanes_by_target
        .get("F")
        .expect("lane for A -> F should be present");
    assert!(
        c_lane + 0.5 < b_lane,
        "five_fan_out_rl should place upper-outer A -> B shallower than upper-inner A -> C for source-centric fan-out opening: B={b_lane}, C={c_lane}"
    );
    assert!(
        e_lane + 0.5 < f_lane,
        "five_fan_out_rl should place lower-outer A -> F shallower than lower-inner A -> E for source-centric fan-out opening: F={f_lane}, E={e_lane}"
    );
    assert!(
        (c_lane - e_lane).abs() <= 0.5,
        "five_fan_out_rl mirrored inner pair should share a symmetric lane depth: C={c_lane}, E={e_lane}"
    );
    assert!(
        (b_lane - f_lane).abs() <= 0.5,
        "five_fan_out_rl mirrored outer pair should share a symmetric lane depth: B={b_lane}, F={f_lane}"
    );
}

#[test]
fn five_fan_out_lr_diamond_source_ports_use_distinct_primary_slots() {
    let input = r#"
graph LR
      A{Source} --> B[Target A]
      A --> C[Target B]
      A --> D[Target C]
      A --> E[Target D]
      A --> F[Target E]
"#;
    let (diagram, geom) = layout_test_svg(input);
    assert_eq!(geom.direction, mmdflux::Direction::LeftRight);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let outbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.from == "A" && !edge.is_backward)
        .collect();
    assert_eq!(
        outbound.len(),
        5,
        "five_fan_out_lr_diamond should produce five outbound forward edges from A"
    );

    let mut source_ys: Vec<f64> = outbound
        .iter()
        .map(|edge| {
            edge.path
                .first()
                .copied()
                .expect("edge should have start")
                .y
        })
        .collect();
    source_ys.sort_by(|a, b| a.total_cmp(b));
    let mut unique_source_count = 0usize;
    let mut last_source: Option<f64> = None;
    for value in &source_ys {
        let is_new = match last_source {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_source_count += 1;
            last_source = Some(*value);
        }
    }
    assert_eq!(
        unique_source_count, 5,
        "five_fan_out_lr_diamond source ports should occupy five distinct primary-face slots instead of collapsing: source_ys={source_ys:?}"
    );
}

#[test]
fn five_fan_out_diamond_primary_face_channels_are_staggered_without_overlap() {
    let (diagram, geom) = layout_fixture_svg("five_fan_out_diamond.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let outbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.from == "A" && !edge.is_backward)
        .collect();
    assert_eq!(
        outbound.len(),
        5,
        "five_fan_out_diamond should produce five outbound forward edges from A"
    );

    let mut source_xs: Vec<f64> = outbound
        .iter()
        .map(|edge| {
            edge.path
                .first()
                .copied()
                .expect("edge should have start")
                .x
        })
        .collect();
    source_xs.sort_by(|a, b| a.total_cmp(b));
    let mut unique_source_count = 0usize;
    let mut last_source: Option<f64> = None;
    for value in &source_xs {
        let is_new = match last_source {
            Some(prev) => (*value - prev).abs() > 0.5,
            None => true,
        };
        if is_new {
            unique_source_count += 1;
            last_source = Some(*value);
        }
    }
    assert_eq!(
        unique_source_count, 5,
        "five_fan_out_diamond source ports should occupy distinct attachment anchors: source_xs={source_xs:?}"
    );

    for i in 0..outbound.len() {
        for j in (i + 1)..outbound.len() {
            assert!(
                !has_coincident_horizontal_overlap(&outbound[i].path, &outbound[j].path),
                "five_fan_out_diamond should avoid coincident horizontal channel overlap between A -> {} and A -> {}: left={:?} right={:?}",
                outbound[i].to,
                outbound[j].to,
                outbound[i].path,
                outbound[j].path
            );
        }
    }

    let mut lanes_by_target: HashMap<String, f64> = HashMap::new();
    for edge in &outbound {
        if let Some(y) = td_bt_primary_horizontal_lane(&edge.path) {
            lanes_by_target.insert(edge.to.clone(), y);
        }
    }
    for target in ["B", "C", "E", "F"] {
        assert!(
            lanes_by_target.contains_key(target),
            "five_fan_out_diamond should route A -> {target} with a horizontal fan-out channel segment (H-V or V-H-V)"
        );
    }

    let b_lane = *lanes_by_target
        .get("B")
        .expect("lane for A -> B should be present");
    let c_lane = *lanes_by_target
        .get("C")
        .expect("lane for A -> C should be present");
    let e_lane = *lanes_by_target
        .get("E")
        .expect("lane for A -> E should be present");
    let f_lane = *lanes_by_target
        .get("F")
        .expect("lane for A -> F should be present");
    assert!(
        b_lane + 0.5 < c_lane,
        "five_fan_out_diamond should place outer-left A -> B shallower than inner-left A -> C for source-centric fan-out opening: B={b_lane}, C={c_lane}"
    );
    assert!(
        f_lane + 0.5 < e_lane,
        "five_fan_out_diamond should place outer-right A -> F shallower than inner-right A -> E for source-centric fan-out opening: F={f_lane}, E={e_lane}"
    );
    assert!(
        (c_lane - e_lane).abs() <= 0.5,
        "five_fan_out_diamond mirrored inner pair should share a symmetric lane depth: C={c_lane}, E={e_lane}"
    );
    assert!(
        (b_lane - f_lane).abs() <= 0.5,
        "five_fan_out_diamond mirrored outer pair should share a symmetric lane depth: B={b_lane}, F={f_lane}"
    );
}

#[test]
fn very_narrow_fan_in_channels_are_staggered_without_overlap() {
    let (diagram, geom) = layout_fixture_svg("very_narrow_fan_in.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let inbound: Vec<_> = routed
        .edges
        .iter()
        .filter(|edge| edge.to == "E" && !edge.is_backward)
        .collect();
    assert_eq!(
        inbound.len(),
        4,
        "very_narrow_fan_in should produce four inbound forward edges to E"
    );

    for i in 0..inbound.len() {
        for j in (i + 1)..inbound.len() {
            assert!(
                !has_coincident_horizontal_overlap(&inbound[i].path, &inbound[j].path),
                "very_narrow_fan_in should avoid coincident horizontal overlap between {} -> E and {} -> E: left={:?} right={:?}",
                inbound[i].from,
                inbound[j].from,
                inbound[i].path,
                inbound[j].path
            );
        }
    }

    let mut lanes_by_source: HashMap<String, f64> = HashMap::new();
    for edge in &inbound {
        let lane = td_bt_middle_horizontal_lane(&edge.path).unwrap_or_else(|| {
            panic!(
                "very_narrow_fan_in {} -> E should route as compact V-H-V to expose staggered channel depth: path={:?}",
                edge.from, edge.path
            )
        });
        lanes_by_source.insert(edge.from.clone(), lane);
    }

    let a_lane = *lanes_by_source
        .get("A")
        .expect("lane for A -> E should be present");
    let b_lane = *lanes_by_source
        .get("B")
        .expect("lane for B -> E should be present");
    let c_lane = *lanes_by_source
        .get("C")
        .expect("lane for C -> E should be present");
    let d_lane = *lanes_by_source
        .get("D")
        .expect("lane for D -> E should be present");
    assert!(
        b_lane + 0.5 < a_lane,
        "very_narrow_fan_in should place inner-left B -> E shallower than outer-left A -> E: A={a_lane}, B={b_lane}"
    );
    assert!(
        c_lane + 0.5 < d_lane,
        "very_narrow_fan_in should place inner-right C -> E shallower than outer-right D -> E: D={d_lane}, C={c_lane}"
    );
    assert!(
        (b_lane - c_lane).abs() <= 0.5,
        "very_narrow_fan_in mirrored inner pair should share a symmetric lane depth: B={b_lane}, C={c_lane}"
    );
    assert!(
        (a_lane - d_lane).abs() <= 0.5,
        "very_narrow_fan_in mirrored outer pair should share a symmetric lane depth: A={a_lane}, D={d_lane}"
    );
    assert!(
        (a_lane - b_lane).abs() > 0.5,
        "very_narrow_fan_in inner and outer bands should stay visibly distinct: outer={a_lane}, inner={b_lane}"
    );
}

#[test]
fn style_segment_monitor_reports_actionable_summary_for_routed_geometry() {
    let report = style_segment_monitor_report_for_routed_geometry(
        &["edge_styles.mmd", "inline_edge_labels.mmd"],
        12.0,
    );
    assert!(
        report.scanned_styled_edges > 0,
        "style monitor should scan at least one styled edge; report={report:?}"
    );
    assert!(
        !report.summary_line.is_empty(),
        "style monitor should emit a stable summary line for CI parsing"
    );
    assert!(
        report.violations.is_empty(),
        "style monitor detected styled-segment violations: {:#?}",
        report
    );
}

#[test]
fn orthogonal_route_diamond_source_endpoints_on_boundary() {
    let (diagram, geom) = layout_fixture_svg("decision.mmd");
    let orthogonal = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // B is a diamond node; B->C and B->D are forward edges from B
    for (from, to) in [("B", "C"), ("B", "D")] {
        let edge = orthogonal
            .edges
            .iter()
            .find(|e| e.from == from && e.to == to)
            .unwrap_or_else(|| panic!("missing edge {from}->{to}"));
        let start = edge.path.first().unwrap();
        let b_node = geom.nodes.get("B").unwrap();
        let b_rect = b_node.rect;
        let cx = b_rect.x + b_rect.width / 2.0;
        let cy = b_rect.y + b_rect.height / 2.0;
        let w = b_rect.width / 2.0;
        let h = b_rect.height / 2.0;
        let boundary = (start.x - cx).abs() / w + (start.y - cy).abs() / h;
        assert!(
            (boundary - 1.0).abs() < 0.05,
            "diamond source endpoint for {from}->{to} should be on diamond boundary, \
             got {boundary}: {start:?}"
        );
    }
}

#[test]
fn diamond_fan_out_source_endpoints_on_boundary() {
    let (diagram, geom) = layout_fixture_svg("diamond_fan_out.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let a_rect = geom.nodes.get("A").unwrap().rect;
    let cx = a_rect.x + a_rect.width / 2.0;
    let cy = a_rect.y + a_rect.height / 2.0;
    let w = a_rect.width / 2.0;
    let h = a_rect.height / 2.0;

    for to in ["B", "C", "D"] {
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == to)
            .unwrap_or_else(|| panic!("missing edge A->{to}"));
        let start = edge.path.first().unwrap();
        let boundary = (start.x - cx).abs() / w + (start.y - cy).abs() / h;
        assert!(
            (boundary - 1.0).abs() < 0.05,
            "A->{to} source should be on diamond boundary, got {boundary}: {start:?}"
        );
    }
}

#[test]
fn diamond_fan_out_source_endpoints_spread() {
    let (diagram, geom) = layout_fixture_svg("diamond_fan_out.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    let mut source_xs: Vec<(String, f64)> = Vec::new();
    for to in ["B", "C", "D"] {
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == to)
            .unwrap_or_else(|| panic!("missing edge A->{to}"));
        source_xs.push((to.to_string(), edge.path[0].x));
    }

    // Not all at the same x (would mean collapsed to vertex)
    let all_same = source_xs.windows(2).all(|w| (w[0].1 - w[1].1).abs() < 0.5);
    assert!(
        !all_same,
        "diamond fan-out source endpoints should spread, got: {source_xs:?}"
    );
}

#[test]
fn diamond_fan_out_td_lateral_edges_depart_horizontally_first() {
    let (diagram, geom) = layout_fixture_svg("diamond_fan_out.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::TopDown);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for to in ["B", "D"] {
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == to)
            .unwrap_or_else(|| panic!("missing edge A->{to}"));
        assert!(
            edge.path.len() >= 2,
            "A->{to} should expose at least two routed points: {:?}",
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];
        assert!(
            approx_eq(start.y, next.y) && (next.x - start.x).abs() > ROUTE_EPS,
            "A->{to} in TD should depart diamond laterally (horizontal-first): start={start:?}, next={next:?}, path={:?}",
            edge.path
        );
    }

    let center = routed
        .edges
        .iter()
        .find(|e| e.from == "A" && e.to == "C")
        .expect("missing edge A->C");
    assert!(
        center.path.len() >= 2,
        "A->C should expose at least two routed points: {:?}",
        center.path
    );
    let start = center.path[0];
    let next = center.path[1];
    assert!(
        approx_eq(start.x, next.x) && (next.y - start.y).abs() > ROUTE_EPS,
        "A->C in TD should remain primary-axis departure (vertical-first): start={start:?}, next={next:?}, path={:?}",
        center.path
    );
}

#[test]
fn ci_pipeline_lr_diamond_exits_depart_vertically_first() {
    let (diagram, geom) = layout_fixture_svg("ci_pipeline.mmd");
    assert_eq!(geom.direction, mmdflux::Direction::LeftRight);
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    for to in ["Staging", "Prod"] {
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "Deploy" && e.to == to)
            .unwrap_or_else(|| panic!("missing edge Deploy->{to}"));
        assert!(
            edge.path.len() >= 2,
            "Deploy->{to} should expose at least two routed points: {:?}",
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];
        assert!(
            approx_eq(start.x, next.x) && (next.y - start.y).abs() > ROUTE_EPS,
            "Deploy->{to} in LR should depart diamond on secondary axis first (vertical-first): start={start:?}, next={next:?}, path={:?}",
            edge.path
        );
    }
}

#[test]
fn hexagon_flow_target_lands_on_flat_top_edge() {
    let (diagram, geom) = layout_fixture_svg("hexagon_flow.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let a_rect = geom.nodes.get("A").unwrap().rect;
    let indent = a_rect.width * 0.2;

    // C->A: target endpoint should be on hexagon's flat top edge
    let edge = routed
        .edges
        .iter()
        .find(|e| e.from == "C" && e.to == "A")
        .unwrap_or_else(|| panic!("missing edge C->A"));
    let end = *edge.path.last().unwrap();

    // Flat top edge: y = a_rect.y, x in [a_rect.x + indent, a_rect.x + width - indent]
    assert!(
        (end.y - a_rect.y).abs() < 1.0,
        "target should land on flat top edge, got y={}, expected y={}",
        end.y,
        a_rect.y
    );
    assert!(
        end.x >= a_rect.x + indent - 1.0 && end.x <= a_rect.x + a_rect.width - indent + 1.0,
        "target x should be within flat top edge [{}, {}], got x={}",
        a_rect.x + indent,
        a_rect.x + a_rect.width - indent,
        end.x
    );
}

#[test]
fn hexagon_flow_sources_use_inset_side_departure_for_lateral_branches() {
    let (diagram, geom) = layout_fixture_svg("hexagon_flow.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let a_rect = geom.nodes.get("A").unwrap().rect;
    let center_x = a_rect.x + a_rect.width / 2.0;
    let center_y = a_rect.y + a_rect.height / 2.0;
    let bottom_y = a_rect.y + a_rect.height;

    // A->B and A->D: lateral branches should depart from inset side attachment,
    // not run along the flat bottom border.
    let mut source_xs = Vec::new();
    for to in ["B", "D"] {
        let edge = routed
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == to)
            .unwrap_or_else(|| panic!("missing edge A->{to}"));
        assert!(
            edge.path.len() >= 3,
            "A->{to} should retain an H-V branch shape from hexagon source: {:?}",
            edge.path
        );
        let start = edge.path[0];
        let next = edge.path[1];

        assert!(
            start.y <= bottom_y - 2.0,
            "A->{to} source should be inset above flat bottom border, got y={}, bottom_y={}",
            start.y,
            bottom_y
        );
        assert!(
            start.y >= center_y + 2.0,
            "A->{to} source should stay in lower half of hexagon when exiting laterally, got start={start:?}, center_y={center_y}"
        );
        assert!(
            (next.y - start.y).abs() < 1.0 && (next.x - start.x).abs() > 1.0,
            "A->{to} first segment should depart laterally (horizontal) from inset side source: start={start:?}, next={next:?}, path={:?}",
            edge.path
        );
        let source_offset = start.x - center_x;
        let expected_sign = if to == "B" { -1.0 } else { 1.0 };
        assert!(
            source_offset.signum() == expected_sign,
            "A->{to} source should be on the expected side of hexagon center: center_x={center_x}, start={start:?}, source_offset={source_offset}, path={:?}",
            edge.path
        );
        source_xs.push((to.to_string(), start.x));
    }

    // Sources should not all collapse to the same x
    if source_xs.len() >= 2 {
        let all_same = source_xs.windows(2).all(|w| (w[0].1 - w[1].1).abs() < 0.5);
        assert!(
            !all_same,
            "hexagon fan-out source endpoints should spread, got: {source_xs:?}"
        );
    }
}

#[test]
fn diamond_backward_target_on_boundary() {
    let (diagram, geom) = layout_fixture_svg("diamond_backward.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // C->B backward edge: target endpoint on diamond B's boundary
    let edge = routed
        .edges
        .iter()
        .find(|e| e.from == "C" && e.to == "B")
        .unwrap_or_else(|| panic!("missing edge C->B"));
    assert!(edge.is_backward, "C->B should be a backward edge");
    let end = *edge.path.last().unwrap();
    let b_rect = geom.nodes.get("B").unwrap().rect;
    let cx = b_rect.x + b_rect.width / 2.0;
    let cy = b_rect.y + b_rect.height / 2.0;
    let w = b_rect.width / 2.0;
    let h = b_rect.height / 2.0;
    let boundary = (end.x - cx).abs() / w + (end.y - cy).abs() / h;
    // Backward edge post-processing (lane clearance, normalization) may push the
    // endpoint slightly beyond the diamond boundary. The key contract is that the
    // endpoint is not deep inside the diamond (boundary >> 0.5 would indicate the
    // router collapsed it to center).
    assert!(
        boundary >= 0.8,
        "backward target on diamond B should be near or outside boundary, got {boundary}: {end:?}"
    );
}

#[test]
fn mixed_shape_chain_diamond_to_hexagon_endpoints() {
    let (diagram, geom) = layout_fixture_svg("mixed_shape_chain.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // B->C: diamond source, hexagon target
    let edge = routed
        .edges
        .iter()
        .find(|e| e.from == "B" && e.to == "C")
        .unwrap_or_else(|| panic!("missing edge B->C"));

    // Source on diamond boundary
    let start = edge.path[0];
    let b_rect = geom.nodes.get("B").unwrap().rect;
    let cx = b_rect.x + b_rect.width / 2.0;
    let cy = b_rect.y + b_rect.height / 2.0;
    let w = b_rect.width / 2.0;
    let h = b_rect.height / 2.0;
    let boundary = (start.x - cx).abs() / w + (start.y - cy).abs() / h;
    assert!(
        (boundary - 1.0).abs() < 0.05,
        "diamond source on boundary: {boundary}: {start:?}"
    );

    // Target on hexagon boundary (flat top edge for TD vertical approach)
    let end = *edge.path.last().unwrap();
    let c_rect = geom.nodes.get("C").unwrap().rect;
    assert!(
        (end.y - c_rect.y).abs() < 2.0,
        "hexagon target should be near top edge, got y={}, expected y={}",
        end.y,
        c_rect.y
    );
}

#[test]
fn mixed_shape_chain_no_staircase_artifacts() {
    let (diagram, geom) = layout_fixture_svg("mixed_shape_chain.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // No edge should have excessive bends (staircase from shape mismatch)
    for edge in &routed.edges {
        // Count direction changes (bends)
        let mut bends = 0;
        for window in edge.path.windows(3) {
            let dx1 = window[1].x - window[0].x;
            let dy1 = window[1].y - window[0].y;
            let dx2 = window[2].x - window[1].x;
            let dy2 = window[2].y - window[1].y;
            // A bend is when direction changes on either axis
            let horizontal_change = (dx1.abs() > 0.1) != (dx2.abs() > 0.1);
            let vertical_change = (dy1.abs() > 0.1) != (dy2.abs() > 0.1);
            if horizontal_change || vertical_change {
                bends += 1;
            }
        }
        assert!(
            bends <= 4,
            "edge {}->{} has {} bends (staircase?), path: {:?}",
            edge.from,
            edge.to,
            bends,
            edge.path
        );
    }
}

// -----------------------------------------------------------------------
// R-BACK-3: Backward edges must not cross intermediate node bodies
// -----------------------------------------------------------------------

#[test]
fn orthogonal_route_complex_backward_edge_clears_intermediate_nodes() {
    let (diagram, geom) = layout_fixture_svg("complex.mmd");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // Find backward edge E→A ("More Data?" → "Input")
    let backward_edge = routed
        .edges
        .iter()
        .find(|e| e.from == "E" && e.to == "A")
        .expect("complex.mmd should have backward edge E→A");
    assert!(backward_edge.is_backward, "E→A should be backward");

    // Node C ("Process") is the intermediate node the edge must avoid
    let process_rect = geom
        .nodes
        .get("C")
        .expect("complex.mmd should have node C (Process)")
        .rect;

    // All interior path points must be outside Process's bounding rect
    for (i, point) in backward_edge.path.iter().enumerate() {
        assert!(
            !point_inside_rect(process_rect, *point),
            "backward edge E→A path point {i} at ({:.1}, {:.1}) is inside Process node rect {:?}",
            point.x,
            point.y,
            process_rect,
        );
    }

    // No axis-aligned path segment should cross Process's interior
    for seg in backward_edge.path.windows(2) {
        assert!(
            !axis_aligned_segment_crosses_rect_interior(seg[0], seg[1], process_rect),
            "backward edge E→A segment ({:.1},{:.1})→({:.1},{:.1}) crosses Process node rect {:?}",
            seg[0].x,
            seg[0].y,
            seg[1].x,
            seg[1].y,
            process_rect,
        );
    }
}

#[test]
fn orthogonal_route_backward_edges_clear_all_intermediate_node_bodies() {
    let fixtures = ["complex.mmd", "multiple_cycles.mmd", "simple_cycle.mmd"];
    let mut failures = Vec::new();

    for fixture in &fixtures {
        let (diagram, geom) = layout_fixture_svg(fixture);
        let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

        for edge in routed.edges.iter().filter(|e| e.is_backward) {
            for seg in edge.path.windows(2) {
                for node in geom.nodes.values() {
                    // Skip the edge's own endpoints
                    if node.id == edge.from || node.id == edge.to {
                        continue;
                    }
                    if axis_aligned_segment_crosses_rect_interior(seg[0], seg[1], node.rect) {
                        failures.push(format!(
                            "{fixture} backward {}->{} seg ({:.1},{:.1})→({:.1},{:.1}) crosses node {} rect {:?}",
                            edge.from, edge.to,
                            seg[0].x, seg[0].y, seg[1].x, seg[1].y,
                            node.id, node.rect,
                        ));
                    }
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "R-BACK-3 violations: backward edges crossing intermediate node bodies:\n{}",
        failures.join("\n"),
    );
}
