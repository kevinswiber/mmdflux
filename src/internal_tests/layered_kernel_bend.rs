//! Plan 0147 Task 2.1 Red: TD labeled edge emits two waypoints under
//! `LabelDummyRouting::Bend`.
//!
//! This file references `LabelDummyPlacement` / `LabelDummyRouting` which
//! are introduced in Task 2.3. Until 2.3 ships the enum split, the whole
//! crate test binary will not compile — this is the documented Red state
//! per `plan-0147/tasks/2.1-red-two-waypoint-td.md`.
//!
//! Once 2.3 adds the enums the test will compile and fail at runtime
//! (path length 3, not 4). Task 2.4 emits the two waypoints and turns the
//! test green.

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::algorithms::layered::{
    LabelDummyPlacement, LabelDummyRouting, LabelSideStrategy, LayoutConfig,
    build_float_layout_with_flags,
};
use crate::graph::grid::GridLayoutConfig;
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::mermaid::parse_flowchart;

#[test]
fn labeled_edge_emits_two_waypoints_under_bend_routing_td() {
    let flowchart = parse_flowchart("graph TD\n    A -->|hi| B\n").expect("fixture parses");
    let mut diagram = compile_to_graph(&flowchart);
    // Skip the wrap pass — the label "hi" is short enough to stay one line
    // regardless; the test targets waypoint emission, not wrap.
    let _ = &mut diagram;

    let grid_config = GridLayoutConfig::default();
    let engine_flags = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        ..Default::default()
    };
    let metrics = default_proportional_text_metrics();

    let geom = build_float_layout_with_flags(
        &diagram,
        &grid_config,
        &metrics,
        EdgeRouting::PolylineRoute,
        true,
        Some(&engine_flags),
    );

    let edge = geom
        .edges
        .iter()
        .find(|e| e.index == 0)
        .expect("the single labeled edge must be present");
    let path = edge
        .layout_path_hint
        .as_ref()
        .expect("edge layout_path_hint must be populated for TD labeled edges");

    // Path: [src, pre_label, post_label, tgt] → 4 points minimum.
    assert!(
        path.len() >= 4,
        "expected ≥4 waypoints (two around the label), got {path:?}"
    );

    // Pre-label and post-label waypoints align on x (TD layout is vertical)
    // and the pre point sits above the post point.
    let pre = path[1];
    let post = path[2];
    assert!(
        (pre.x - post.x).abs() < 0.01,
        "pre/post x must align in TD: {pre:?} vs {post:?}"
    );
    assert!(
        pre.y < post.y,
        "pre must be above post in TD: {pre:?} vs {post:?}"
    );
}

// Plan 0147 Task 2.2: user RL repro — the two peer labeled edges'
// routed paths must not cross into each other's label rects under
// `LabelDummyRouting::Bend`.
#[test]
fn user_rl_repro_edges_do_not_cross_peer_label_rects() {
    let src = "graph RL\n    A -->|x| B\n    A -->|yes<br>no| B\n";
    let flowchart = parse_flowchart(src).expect("fixture parses");
    let diagram = compile_to_graph(&flowchart);

    let grid_config = GridLayoutConfig::default();
    let engine_flags = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        label_side_strategy: LabelSideStrategy::DirectionDown,
        ..Default::default()
    };
    let metrics = default_proportional_text_metrics();

    let geom = build_float_layout_with_flags(
        &diagram,
        &grid_config,
        &metrics,
        EdgeRouting::PolylineRoute,
        true,
        Some(&engine_flags),
    );
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::PolylineRoute, &metrics);

    fn rect_contains(
        rect: &crate::graph::space::FRect,
        p: &crate::graph::geometry::FPoint,
    ) -> bool {
        p.x >= rect.x && p.x <= rect.x + rect.width && p.y >= rect.y && p.y <= rect.y + rect.height
    }

    for (i, e_i) in routed.edges.iter().enumerate() {
        let Some(lg_i) = e_i.label_geometry.as_ref() else {
            continue;
        };
        for (j, e_j) in routed.edges.iter().enumerate() {
            if i == j {
                continue;
            }
            for p in &e_j.path {
                assert!(
                    !rect_contains(&lg_i.rect, p),
                    "edge {j}'s path point {p:?} passes through edge {i}'s label rect {:?}",
                    lg_i.rect
                );
            }
        }
    }
}

// Plan 0147 Task 2.8: reversed-edge two-waypoint bend flips symmetrically
// under `pipeline.rs::reversePointsForReversedEdges`.
#[test]
fn bent_label_dummy_waypoints_flip_symmetrically_on_reversed_edges() {
    let src = "graph TD\n    A --> B\n    B -->|loop| A\n";
    let flowchart = parse_flowchart(src).expect("fixture parses");
    let diagram = compile_to_graph(&flowchart);

    let grid_config = GridLayoutConfig::default();
    let engine_flags = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        acyclic: true,
        ..Default::default()
    };
    let metrics = default_proportional_text_metrics();
    let geom = build_float_layout_with_flags(
        &diagram,
        &grid_config,
        &metrics,
        EdgeRouting::PolylineRoute,
        true,
        Some(&engine_flags),
    );

    let loop_edge = geom
        .edges
        .iter()
        .find(|e| diagram.edges[e.index].label.as_deref() == Some("loop"))
        .expect("loop edge present");
    let path = loop_edge
        .layout_path_hint
        .as_ref()
        .expect("loop edge has layout_path_hint");
    assert!(
        path.len() >= 4,
        "expected 2 label waypoints + 2 endpoints, got {path:?}"
    );

    // Forward emission order puts pre.y < post.y in TD. After
    // `reversePointsForReversedEdges` flips the chain in-place, pre sits
    // BELOW post (pre.y > post.y), preserving the bend shape relative to
    // the reversed edge's source/target.
    let (pre, post) = (path[1], path[2]);
    assert!(
        pre.y > post.y,
        "reversed edge's label waypoints must flip: pre.y={} post.y={}",
        pre.y,
        post.y
    );
}

// Plan 0147 Task 2.5: label dummy height is padded by
// `edge_label_spacing + thickness` to match ELK `LabelDummyInserter`.
//
// Uses an override value far from the 2.0 default so that a regression
// which silently drops `flags.edge_label_spacing` (the bug GPT-5.4 flagged
// on 2026-04-16) fails this test instead of passing trivially at default.
#[test]
fn edge_label_info_padding_includes_edge_label_spacing_and_thickness() {
    let flowchart = parse_flowchart("graph TD\n    A -->|label| B\n").expect("fixture parses");
    let diagram = compile_to_graph(&flowchart);

    let grid_config = GridLayoutConfig::default();
    let engine_flags = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        edge_label_spacing: 40.0,
        ..Default::default()
    };
    let metrics = default_proportional_text_metrics();
    let (_unpadded_w, unpadded_h) = metrics.edge_label_dimensions("label");

    let geom = build_float_layout_with_flags(
        &diagram,
        &grid_config,
        &metrics,
        EdgeRouting::PolylineRoute,
        true,
        Some(&engine_flags),
    );
    let edge = geom.edges.iter().find(|e| e.index == 0).unwrap();
    let path = edge.layout_path_hint.as_ref().unwrap();
    let (pre, post) = (path[1], path[2]);
    let effective_h = (post.y - pre.y).abs();

    let default_thickness = 1.0_f64;
    let expected_min = unpadded_h + engine_flags.edge_label_spacing + default_thickness - 0.01;
    assert!(
        effective_h >= expected_min,
        "label dummy height {effective_h} not padded to ≥{expected_min} (ELK formula: {unpadded_h} + spacing {} + thickness {default_thickness}); the caller override was likely dropped before reaching the kernel",
        engine_flags.edge_label_spacing
    );
}

// Plan 0148 (#238): Text rendering must respond to `edge_label_spacing`.
// Grid-mode measurement pads label-dummy dims via
// `pad_edge_label_dims_grid`, widening rank gaps in proportion to the
// knob. Increasing spacing from the 2.0 default adds rows between
// labelled ranks — the public-API anchor matching the kernel-level
// `grid_text_bounds_grow_with_edge_label_spacing` test.
#[test]
fn text_render_honors_edge_label_spacing() {
    use crate::engines::graph::LayoutConfig as PublicLayoutConfig;
    use crate::{OutputFormat, RenderConfig};

    let src = "graph TD\n    A -->|label| B\n";
    let mk = |spacing: f64| RenderConfig {
        layout: PublicLayoutConfig {
            edge_label_spacing: spacing,
            ..PublicLayoutConfig::default()
        },
        ..RenderConfig::default()
    };

    let text_small = crate::render_diagram(src, OutputFormat::Text, &mk(2.0)).unwrap();
    let text_big = crate::render_diagram(src, OutputFormat::Text, &mk(40.0)).unwrap();

    assert_ne!(
        text_small, text_big,
        "edge_label_spacing must visibly affect Text output"
    );

    let small_rows = text_small.lines().count();
    let big_rows = text_big.lines().count();
    assert!(
        big_rows > small_rows + 1,
        "increasing edge_label_spacing must add rows between labelled ranks: small={small_rows}, big={big_rows}"
    );
}

// Plan 0147: the canonical-proportional solve path (`run_layered_layout`
// with `MeasurementMode::Proportional`) must also honour
// `edge_label_spacing`. MMDS routed output and any other Canonical
// consumer reads proportional geometry through this code path; a value
// of 40 vs the 2.0 default must visibly change layout bounds / node
// positions, otherwise the override is a no-op on the public config
// surface (GPT-5.4 review, 2026-04-16).
#[test]
fn canonical_proportional_solve_honors_edge_label_spacing_override() {
    use crate::engines::graph::algorithms::layered::run_layered_layout;
    use crate::engines::graph::contracts::{EngineConfig, MeasurementMode};

    let flowchart = parse_flowchart("graph TD\n    A -->|label| B\n").expect("fixture parses");
    let diagram = compile_to_graph(&flowchart);
    let metrics = default_proportional_text_metrics();
    let mode = MeasurementMode::Proportional(metrics);

    let baseline = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        edge_label_spacing: 2.0,
        ..Default::default()
    };
    let widened = LayoutConfig {
        edge_label_spacing: 40.0,
        ..baseline.clone()
    };

    let baseline_geom =
        run_layered_layout(&mode, &diagram, &EngineConfig::Layered(baseline)).unwrap();
    let widened_geom =
        run_layered_layout(&mode, &diagram, &EngineConfig::Layered(widened)).unwrap();

    let dy = widened_geom.bounds.height - baseline_geom.bounds.height;
    assert!(
        dy >= 37.0,
        "widening edge_label_spacing 2→40 must grow TD bounds by ≥ (40 − 2) px; got ΔH={dy} (bounds baseline={:?}, widened={:?})",
        baseline_geom.bounds,
        widened_geom.bounds,
    );
}

// Plan 0148 Task 1.1 spike (#238): Grid-mode `run_layered_layout` must
// respond to `edge_label_spacing`. The Text renderer consumes Grid-mode
// geometry, so if bounds don't grow with the knob, Text cannot honour
// it. This is the kernel anchor for the public-API contract in
// `text_render_honors_edge_label_spacing`. Red today (Grid arm ignores
// the knob); Task 2.3 wires `pad_edge_label_dims_grid` and turns it
// green. If the test is still red after 2.3, pivot to Option 2
// (`waypoints.rs::transform_label_positions_direct`).
#[test]
fn grid_text_bounds_grow_with_edge_label_spacing() {
    use crate::engines::graph::algorithms::layered::run_layered_layout;
    use crate::engines::graph::contracts::{EngineConfig, MeasurementMode};

    let flowchart = parse_flowchart("graph TD\n    A -->|label| B\n").expect("fixture parses");
    let diagram = compile_to_graph(&flowchart);
    let mode = MeasurementMode::Grid;

    let small = LayoutConfig {
        label_dummy_placement: LabelDummyPlacement::WidestLayer,
        label_dummy_routing: LabelDummyRouting::Bend,
        label_side_selection: true,
        edge_label_spacing: 2.0,
        ..Default::default()
    };
    let big = LayoutConfig {
        edge_label_spacing: 20.0,
        ..small.clone()
    };

    let small_geom = run_layered_layout(&mode, &diagram, &EngineConfig::Layered(small)).unwrap();
    let big_geom = run_layered_layout(&mode, &diagram, &EngineConfig::Layered(big)).unwrap();

    let dy = big_geom.bounds.height - small_geom.bounds.height;
    assert!(
        dy >= 1.0,
        "Grid bounds must grow with edge_label_spacing 2→20; got ΔH={dy} (small={:?}, big={:?})",
        small_geom.bounds,
        big_geom.bounds,
    );
}
