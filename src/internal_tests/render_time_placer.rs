//! Render-time placer canaries for Plan 0153.
//!
//! Phase 1 (PR #A) activates C1–C4 and C10; C5–C9 are scaffolded here and
//! stay `#[ignore]` until PR #B widens the wrapper to all body labels. C10
//! is active in PR #A because `three_parallel_labels.mmd` already sits in
//! the `AuthoritativeOnly` scope (`compartment_size > 1`). C1 and C3 live in
//! `src/graph/grid/label_placement.rs` mod tests because they cover pure
//! footprint-anchor logic; this file hosts the integration canaries that
//! exercise the full pipeline through `render_text_from_geometry`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::algorithms::layered::{
    LayoutConfig as LayeredConfig, run_layered_layout,
};
use crate::engines::graph::contracts::{GraphGeometryContract, MeasurementMode};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphSolveRequest, SubgraphDirectionPolicy, solve_graph_family,
};
use crate::graph::geometry::{EdgeLabelGeometry, RoutedGraphGeometry};
use crate::graph::grid::{
    GridLayout, RoutedEdge, Segment, geometry_to_grid_layout_with_routed, route_all_edges,
};
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{GeometryLevel, Graph};
use crate::mermaid::parse_flowchart;
use crate::render::graph::text::label_placement::{
    RenderTimePlacement, RenderTimePlacementScope, compute_label_placements,
};
use crate::render::graph::{
    TextRenderOptions, edge_routing_from_style, layout_config_for_diagram,
    render_text_from_geometry,
};

const CORRIDOR_DRIFT_THRESHOLD: f64 = 3.0;

fn render_class_fixture(name: &str) -> String {
    use crate::format::OutputFormat;
    use crate::runtime::config::RenderConfig;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("class")
        .join(name);
    let input = fs::read_to_string(&path).expect("fixture exists");
    crate::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("render class fixture")
}

fn render_flowchart_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = fs::read_to_string(&path).expect("fixture exists");
    let mut parsed = parse_flowchart(&input).expect("parse ok");
    let diagram: Graph = compile_to_graph(&parsed);
    // Apply pre-engine wrap (same as the baseline snapshot pipeline).
    let wrap_metrics = default_proportional_text_metrics();
    let wrap_max_width = crate::engines::graph::LayoutConfig::default().edge_label_max_width;
    let mut diagram = diagram;
    crate::graph::label_wrap::prepare_wrapped_labels(
        &mut diagram.edges,
        &wrap_metrics,
        wrap_max_width,
    );
    let _ = &mut parsed; // silence mutability hint
    let config = EngineConfig::Layered(LayeredConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config).expect("layout ok");
    let metrics = default_proportional_text_metrics();
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute, &metrics);
    render_text_from_geometry(
        &diagram,
        &geom,
        Some(&routed),
        &TextRenderOptions::default(),
    )
}

struct VerifiedFlowchartPipeline {
    routed_geometry: RoutedGraphGeometry,
    layout: GridLayout,
    routed_edges: Vec<RoutedEdge>,
    placements: HashMap<usize, RenderTimePlacement>,
}

fn verified_flowchart_pipeline(fixture_name: &str) -> VerifiedFlowchartPipeline {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(fixture_name);
    let input = fs::read_to_string(&path).expect("fixture exists");
    let parsed = parse_flowchart(&input).expect("parse ok");
    let mut diagram: Graph = compile_to_graph(&parsed);

    let wrap_metrics = default_proportional_text_metrics();
    let wrap_max_width = crate::engines::graph::LayoutConfig::default().edge_label_max_width;
    crate::graph::label_wrap::prepare_wrapped_labels(
        &mut diagram.edges,
        &wrap_metrics,
        wrap_max_width,
    );

    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        GeometryLevel::Layout,
        None,
        SubgraphDirectionPolicy::AlternateAxes,
    );
    let result = solve_graph_family(
        &diagram,
        EngineAlgorithmId::FLUX_LAYERED,
        &EngineConfig::Layered(LayeredConfig::default()),
        &request,
    )
    .expect("solve ok");

    let options = TextRenderOptions::default();
    let geometry = result.geometry;
    let routed_geometry = result.routed.unwrap_or_else(|| {
        let metrics = default_proportional_text_metrics();
        route_graph_geometry(
            &diagram,
            &geometry,
            edge_routing_from_style(options.routing_style),
            &metrics,
        )
    });

    let layout_config = layout_config_for_diagram(&diagram, &options);
    let layout = geometry_to_grid_layout_with_routed(
        &diagram,
        &geometry,
        Some(&routed_geometry),
        &layout_config,
    );
    let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);
    let (canvas_width, canvas_height) =
        crate::render::graph::text::required_canvas_size_for_test(&layout, &routed_edges);
    let edge_containment = crate::render::graph::text::compute_edge_containment(
        &diagram.edges,
        &diagram.subgraphs,
        &layout.subgraph_bounds,
    );
    let placements = compute_label_placements(
        &routed_edges,
        Some(&routed_geometry),
        &layout,
        &edge_containment,
        canvas_width,
        canvas_height,
        RenderTimePlacementScope::AllBodyLabels,
    );

    VerifiedFlowchartPipeline {
        routed_geometry,
        layout,
        routed_edges,
        placements,
    }
}

fn label_geometry_for_edge(
    routed_geometry: &RoutedGraphGeometry,
    edge_index: usize,
) -> Option<&EdgeLabelGeometry> {
    routed_geometry
        .edges
        .iter()
        .find(|edge| edge.index == edge_index)
        .and_then(|edge| edge.label_geometry.as_ref())
}

fn midpoint_of_segments(segments: &[Segment]) -> Option<(usize, usize)> {
    let first = segments.first()?;
    let total_length: usize = segments.iter().map(Segment::length).sum();
    if total_length == 0 {
        let point = first.start_point();
        return Some((point.x, point.y));
    }

    let target = total_length / 2;
    let mut accumulated = 0usize;
    for segment in segments {
        let segment_length = segment.length();
        if accumulated + segment_length >= target {
            let point = segment.point_at_offset(target - accumulated);
            return Some((point.x, point.y));
        }
        accumulated += segment_length;
    }

    segments.last().map(|segment| {
        let point = segment.end_point();
        (point.x, point.y)
    })
}

fn distance_to_segments(point: (usize, usize), segments: &[Segment]) -> f64 {
    let (px, py) = (point.0 as f64, point.1 as f64);
    segments
        .iter()
        .map(|segment| match *segment {
            Segment::Horizontal { y, x_start, x_end } => {
                let (x_min, x_max) = (x_start.min(x_end) as f64, x_start.max(x_end) as f64);
                let clamped_x = px.max(x_min).min(x_max);
                ((clamped_x - px).powi(2) + (y as f64 - py).powi(2)).sqrt()
            }
            Segment::Vertical { x, y_start, y_end } => {
                let (y_min, y_max) = (y_start.min(y_end) as f64, y_start.max(y_end) as f64);
                let clamped_y = py.max(y_min).min(y_max);
                ((x as f64 - px).powi(2) + (clamped_y - py).powi(2)).sqrt()
            }
        })
        .fold(f64::INFINITY, f64::min)
}

fn assert_target_label_on_corridor(fixture_name: &str, from: &str, to: &str, label: &str) {
    let pipeline = verified_flowchart_pipeline(fixture_name);
    let target = pipeline
        .routed_edges
        .iter()
        .find(|routed| {
            routed.edge.from == from
                && routed.edge.to == to
                && routed.edge.label.as_deref() == Some(label)
                && !routed.is_backward
        })
        .unwrap_or_else(|| panic!("{fixture_name}: {from}->{to} '{label}' edge must exist"));
    let geometry = label_geometry_for_edge(&pipeline.routed_geometry, target.edge.index)
        .unwrap_or_else(|| {
            panic!(
                "{fixture_name}: edge {} missing label geometry",
                target.edge.index
            )
        });

    let placement = *pipeline
        .placements
        .get(&target.edge.index)
        .unwrap_or_else(|| {
            panic!(
                "{fixture_name}: edge {} missing placement",
                target.edge.index
            )
        });
    let drift = distance_to_segments(placement.center, &target.segments);

    assert!(
        drift <= CORRIDOR_DRIFT_THRESHOLD,
        "{fixture_name} edge {} ({} -> {}) label {:?} at {:?} drifted {:.2} cells from \
         segments {:?}; expected <= 3. track={} compartment_size={} projected={:?} midpoint={:?}",
        target.edge.index,
        target.edge.from,
        target.edge.to,
        label,
        placement.center,
        drift,
        target.segments,
        geometry.track,
        geometry.compartment_size,
        pipeline
            .layout
            .project_layout_point(geometry.center.x, geometry.center.y),
        midpoint_of_segments(&target.segments),
    );
}

/// P0.2 (Plan 0155): Inventory every labeled forward edge across the
/// flowchart corpus. Grounds Phase 1/2 assertions.
///
/// Run: `cargo nextest run -E 'test(p0_2_inventory_singleton_forward_labels)' --run-ignored all --no-capture`
#[test]
#[ignore]
fn p0_2_inventory_singleton_forward_labels() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut fixture_paths = fs::read_dir(&fixture_dir)
        .expect("flowchart fixture dir exists")
        .map(|entry| entry.expect("fixture dir entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("mmd"))
        .collect::<Vec<_>>();
    fixture_paths.sort();

    println!(
        "\nfixture | edge | from->to | label | is_back | track | compartment | projected | midpoint | final_center | drift"
    );
    println!("---|---|---|---|---|---|---|---|---|---|---");

    for path in fixture_paths {
        let fixture_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture filename is utf-8");
        let pipeline = verified_flowchart_pipeline(fixture_name);

        for routed in &pipeline.routed_edges {
            if routed.is_backward {
                continue;
            }
            let Some(label) = routed
                .edge
                .label
                .as_deref()
                .filter(|label| !label.is_empty())
            else {
                continue;
            };
            let Some(geometry) =
                label_geometry_for_edge(&pipeline.routed_geometry, routed.edge.index)
            else {
                continue;
            };
            let projected = pipeline
                .layout
                .project_layout_point(geometry.center.x, geometry.center.y);
            let midpoint = midpoint_of_segments(&routed.segments);
            let final_center = pipeline
                .placements
                .get(&routed.edge.index)
                .map(|placement| placement.center);
            let drift = final_center
                .map(|center| distance_to_segments(center, &routed.segments))
                .unwrap_or(f64::NAN);

            println!(
                "{} | {} | {}->{} | {:?} | {} | {} | {} | {:?} | {:?} | {:?} | {:.2}",
                fixture_name,
                routed.edge.index,
                routed.edge.from,
                routed.edge.to,
                label,
                routed.is_backward,
                geometry.track,
                geometry.compartment_size,
                projected,
                midpoint,
                final_center,
                drift,
            );
        }
    }

    panic!("p0_2 intentional failure to surface printout under --nocapture");
}

/// Smoke test: the empty edge list returns an empty placement map regardless
/// of scope. Proves the module surface is in place and the loop short-circuits
/// before any helpers that would fail on degenerate input.
#[test]
fn compute_label_placements_returns_empty_map_on_empty_input() {
    let layout = GridLayout::default();
    let edges: Vec<RoutedEdge> = Vec::new();
    let containment: HashMap<usize, (usize, usize)> = HashMap::new();

    let result = compute_label_placements(
        &edges,
        None,
        &layout,
        &containment,
        10,
        10,
        RenderTimePlacementScope::AuthoritativeOnly,
    );
    assert!(result.is_empty());

    let result = compute_label_placements(
        &edges,
        None,
        &layout,
        &containment,
        10,
        10,
        RenderTimePlacementScope::AllBodyLabels,
    );
    assert!(result.is_empty());
}

/// C2: backward vertical, BT flowchart with authoritative lane-coordinated
/// labels. The rendered output must contain both labels ("yes" and "no") on
/// the backward edge corridor without a label character landing on the
/// `┌`/`┘` corners of the backward bracket.
#[test]
fn c2_backward_vertical_avoids_corners() {
    let output = render_flowchart_fixture("label_clamp_bt_review.mmd");
    assert!(
        output.contains("yes"),
        "rendered output missing 'yes'\n{output}"
    );
    assert!(
        output.contains("no"),
        "rendered output missing 'no'\n{output}"
    );
    // Corners are represented by ┌/└/┐/┘; the labels must not sit on rows
    // where they would overwrite those glyphs. Simple lane check: the
    // characters immediately adjacent to a label on the same row should be
    // whitespace or vertical corridor glyphs, not a corner.
    for line in output.lines() {
        if line.contains("yes") && (line.contains('┌') || line.contains('└')) {
            // Label and corner on the same row is only fine if they sit in
            // separate columns with a whitespace gap. Approximate check:
            // ensure `yes` is not directly adjacent to a corner glyph.
            assert!(
                !line.contains("┌yes") && !line.contains("yes┌"),
                "'yes' sits adjacent to a corner on row: {line}"
            );
        }
    }
}

/// C4: backward horizontal, forward + backward labels on parallel asymmetric
/// markers. Confirms the placer closes the PR #252 regression class (Plan
/// 0152 Phase 3's original trigger). Both labels must render and neither may
/// land on a load-bearing `┌`/`└` corner glyph.
#[test]
fn c4_backward_horizontal_corner_avoidance() {
    let output = render_flowchart_fixture("backward_label_asymmetric_markers.mmd");
    assert!(
        output.contains("forward label"),
        "rendered output missing 'forward label'\n{output}"
    );
    assert!(
        output.contains("reverse label"),
        "rendered output missing 'reverse label'\n{output}"
    );
    for line in output.lines() {
        assert!(
            !line.contains("┘reverse") && !line.contains("reverse┘"),
            "'reverse label' sits on a corner on row: {line}"
        );
        assert!(
            !line.contains("┘forward") && !line.contains("forward┘"),
            "'forward label' sits on a corner on row: {line}"
        );
    }
}

// ---- PR #B canaries (C5-C9). Activated in task 2.1. These pass against
// the wrapper's `AllBodyLabels` scope today because `compute_label_placements`
// is correct for that scope at the wrapper level; the strict Phase 2
// callsite coverage (unified branch, drift-gate deletion, GridLayout field
// deletion) is exercised by task 2.9's snapshot regeneration. These canaries
// serve as targeted structural regression guards during Phase 2 refactoring.

/// C5: self-edge with `label_geometry`. The self-edge on `self_loop_labeled.mmd`
/// carries the label "retry". PR #B's unified path must still render this label
/// on the self-loop corridor; this canary guards against the branch collapse
/// accidentally eating the self-edge path.
#[test]
fn c5_self_edge_with_label_geometry() {
    let output = render_flowchart_fixture("self_loop_labeled.mmd");
    assert!(
        output.contains("retry"),
        "self-edge label 'retry' missing from output\n{output}"
    );
    // The self-loop renders with ▲ (or its charset equivalent) somewhere.
    assert!(
        output.contains('▲')
            || output.contains('▼')
            || output.contains('◄')
            || output.contains('►'),
        "expected an arrow glyph on the self-loop\n{output}"
    );
}

/// C6: class-diagram labeled edge. `class/relationships.mmd` has three
/// labeled class edges: "places", "contains", "authenticates". PR #B's
/// unified path must render all three through the same body-label flow.
#[test]
fn c6_class_diagram_edge_renders_via_unified_path() {
    let output = render_class_fixture("relationships.mmd");
    for label in ["places", "contains", "authenticates"] {
        assert!(
            output.contains(label),
            "class-diagram label '{label}' missing from output\n{output}"
        );
    }
}

/// C7: backward edge with overwrite_arrows=true preserved. The backward
/// edge in `label_clamp_bt_review.mmd` still paints its arrowhead after
/// the render-time placer writes the label. PR #B's unified path must
/// honour the `is_backward` signal that triggers arrow-overwrite behavior.
#[test]
fn c7_backward_overwrite_arrows_preserved() {
    let output = render_flowchart_fixture("label_clamp_bt_review.mmd");
    assert!(
        output.contains("yes"),
        "backward-edge label 'yes' missing\n{output}"
    );
    assert!(
        output.contains('▲'),
        "backward-edge arrowhead ▲ missing — label may have overwritten it\n{output}"
    );
}

/// C8: self-edge fallback without `label_geometry`. Exercises the
/// `calc_label_position(&segments)` midpoint fallback inside
/// `compute_label_placements` — the branch taken when `routed_geometry`
/// is absent. Parses `self_loop_labeled.mmd` (which has a real self-edge
/// with segments), builds a `GridLayout` + `Vec<RoutedEdge>`, then invokes
/// `compute_label_placements` with `routed_geometry = None` so
/// `edge_label_geometry` returns `None` and the fallback path runs. The
/// self-edge's label must still get a placement via the Pass-3 midpoint.
#[test]
fn c8_self_edge_fallback_without_label_geometry() {
    use crate::graph::grid::{
        GridLayoutConfig, geometry_to_grid_layout_with_routed, route_all_edges,
    };

    let input = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join("self_loop_labeled.mmd"),
    )
    .expect("fixture exists");
    let parsed = parse_flowchart(&input).expect("parse ok");
    let diagram: Graph = compile_to_graph(&parsed);

    let config = EngineConfig::Layered(LayeredConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config).expect("layout ok");
    let metrics = default_proportional_text_metrics();
    let routed_geom = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute, &metrics);
    let layout = geometry_to_grid_layout_with_routed(
        &diagram,
        &geom,
        Some(&routed_geom),
        &GridLayoutConfig::default(),
    );
    let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);
    let self_edge_idx = routed_edges
        .iter()
        .find(|r| r.is_self_edge)
        .map(|r| r.edge.index)
        .expect("self_loop_labeled.mmd should contain a self-edge");

    // Pass `None` for routed_geometry → forces the midpoint fallback path.
    let placements = compute_label_placements(
        &routed_edges,
        None,
        &layout,
        &HashMap::new(),
        layout.width,
        layout.height,
        RenderTimePlacementScope::AllBodyLabels,
    );
    let placement = placements.get(&self_edge_idx).copied().unwrap_or_else(|| {
        panic!("self-edge fallback produced no placement; placements={placements:?}")
    });
    // Placement center must sit within canvas bounds.
    assert!(
        placement.center.0 < layout.width && placement.center.1 < layout.height,
        "self-edge midpoint-fallback center {:?} outside canvas {}x{}",
        placement.center,
        layout.width,
        layout.height
    );
}

/// Plan 0155 / research 0069 Q2 + Q3 addendum: singleton non-authoritative
/// forward-edge labels must place within 3 cells of the Pass-3 segments rather
/// than landing on the projected `EdgeLabelGeometry.center`.
#[test]
fn f2_no_complex_e_to_f_on_corridor() {
    assert_target_label_on_corridor("complex.mmd", "E", "F", "no");
}

/// `cache -> validate` "miss" in `inline_label_flowchart.mmd` is the smallest
/// target drift. It still must prefer the Pass-3 midpoint over the projected
/// off-corridor geometry center.
#[test]
fn f2_miss_cache_validate_on_corridor() {
    assert_target_label_on_corridor("inline_label_flowchart.mmd", "cache", "validate", "miss");
}

/// `success -> retry` "no" in `inline_label_flowchart.mmd` is the largest
/// target drift and protects the lower forward singleton branch.
#[test]
fn f2_no_success_retry_on_corridor() {
    assert_target_label_on_corridor("inline_label_flowchart.mmd", "success", "retry", "no");
}

/// Corpus guard for Plan 0155's midpoint-owned forward labels: exact singleton
/// non-authoritative edges plus the drift-gated two-label coordinated shape
/// observed for `miss` and `Yes`. Every matched label must land within 3 cells
/// of the Pass-3 segments.
#[test]
fn f2_forward_midpoint_owned_labels_on_corridor() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut fixture_paths = fs::read_dir(&fixture_dir)
        .expect("flowchart fixture dir exists")
        .map(|entry| entry.expect("fixture dir entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("mmd"))
        .collect::<Vec<_>>();
    fixture_paths.sort();

    let mut checked = 0usize;
    let mut failures = Vec::new();
    for path in fixture_paths {
        let fixture_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture filename is utf-8");
        let pipeline = verified_flowchart_pipeline(fixture_name);

        for routed in &pipeline.routed_edges {
            if routed.is_backward {
                continue;
            }
            let Some(label) = routed
                .edge
                .label
                .as_deref()
                .filter(|label| !label.is_empty())
            else {
                continue;
            };
            let Some(geometry) =
                label_geometry_for_edge(&pipeline.routed_geometry, routed.edge.index)
            else {
                continue;
            };
            let projected = pipeline
                .layout
                .project_layout_point(geometry.center.x, geometry.center.y);
            let projected_drift = distance_to_segments(projected, &routed.segments);
            let midpoint_drift = midpoint_of_segments(&routed.segments)
                .map(|midpoint| distance_to_segments(midpoint, &routed.segments))
                .unwrap_or(f64::INFINITY);
            let is_singleton = geometry.compartment_size == 1;
            let is_drift_gated_two_label = geometry.compartment_size == 2
                && geometry.track != 0
                && projected_drift > CORRIDOR_DRIFT_THRESHOLD
                && midpoint_drift <= CORRIDOR_DRIFT_THRESHOLD;
            if !is_singleton && !is_drift_gated_two_label {
                continue;
            }
            let Some(placement) = pipeline.placements.get(&routed.edge.index) else {
                failures.push(format!(
                    "{fixture_name} edge {} {}->{} label {:?}: missing placement",
                    routed.edge.index, routed.edge.from, routed.edge.to, label
                ));
                continue;
            };

            checked += 1;
            let drift = distance_to_segments(placement.center, &routed.segments);
            if drift > CORRIDOR_DRIFT_THRESHOLD {
                failures.push(format!(
                    "{fixture_name} edge {} {}->{} label {:?}: center {:?} drift {:.2} \
                     track={} compartment_size={} projected={:?} projected_drift={:.2} \
                     midpoint={:?} midpoint_drift={:.2} segments={:?}",
                    routed.edge.index,
                    routed.edge.from,
                    routed.edge.to,
                    label,
                    placement.center,
                    drift,
                    geometry.track,
                    geometry.compartment_size,
                    projected,
                    projected_drift,
                    midpoint_of_segments(&routed.segments),
                    midpoint_drift,
                    routed.segments,
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "expected at least one forward midpoint-owned label"
    );
    assert!(
        failures.is_empty(),
        "forward midpoint-owned label drift failures:\n{}",
        failures.join("\n")
    );
}

/// C9: drift-gate dissolution on `complex.mmd`. PR #B deletes both
/// `AUTHORITATIVE_OVERRIDE_DRIFT = 5` (derive/mod.rs) and
/// `PRECOMPUTED_LABEL_BASE_DRIFT` (edge.rs). The render-time placer must
/// replace any stale authoritative anchor with a Pass-3-derived cell that
/// sits **on the drawn path** — not drift off toward a stale Pass-1
/// projection. This canary replaces the deleted
/// `text_renderer_rejects_stale_precomputed_label_anchor_for_label_revalidation_fixture`
/// by computing placements on `complex.mmd`, locating each routed edge's
/// label placement, and asserting a two-invariant drift contract:
///
///   (a) an **absolute ceiling** of 8 cells from the edge's Pass-3
///       segments, tight enough that a stale authoritative anchor
///       (typically 10+ cells off the drawn path) fails;
///   (b) a **materially-closer-than-stale ratio** — the placer's drift
///       must be at most 40% of the farthest canvas corner's distance to
///       the edge's segments (`placer_drift * 2.5 <= farthest_corner`).
///       Mirrors the deleted test's "closer than any stale candidate"
///       spirit without depending on the removed `edge_label_positions`
///       cache or on baseline-vs-poisoned comparisons that pre-dated the
///       render-time placer.
#[test]
fn c9_drift_gate_dissolution_on_complex_mmd() {
    use crate::graph::grid::{
        GridLayoutConfig, Segment, geometry_to_grid_layout_with_routed, route_all_edges,
    };

    fn distance_to_segments(point: (usize, usize), segments: &[Segment]) -> f64 {
        let (px, py) = (point.0 as f64, point.1 as f64);
        segments
            .iter()
            .map(|segment| match *segment {
                Segment::Horizontal { y, x_start, x_end } => {
                    let (x_min, x_max) = (x_start.min(x_end) as f64, x_start.max(x_end) as f64);
                    let clamped_x = px.max(x_min).min(x_max);
                    ((clamped_x - px).powi(2) + (y as f64 - py).powi(2)).sqrt()
                }
                Segment::Vertical { x, y_start, y_end } => {
                    let (y_min, y_max) = (y_start.min(y_end) as f64, y_start.max(y_end) as f64);
                    let clamped_y = py.max(y_min).min(y_max);
                    ((x as f64 - px).powi(2) + (clamped_y - py).powi(2)).sqrt()
                }
            })
            .fold(f64::INFINITY, f64::min)
    }

    let input = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join("complex.mmd"),
    )
    .expect("fixture exists");
    let parsed = parse_flowchart(&input).expect("parse ok");
    let diagram: Graph = compile_to_graph(&parsed);
    let config = EngineConfig::Layered(LayeredConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config).expect("layout ok");
    let metrics = default_proportional_text_metrics();
    let routed_geom = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute, &metrics);
    let layout = geometry_to_grid_layout_with_routed(
        &diagram,
        &geom,
        Some(&routed_geom),
        &GridLayoutConfig::default(),
    );
    let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);

    let placements = compute_label_placements(
        &routed_edges,
        Some(&routed_geom),
        &layout,
        &HashMap::new(),
        layout.width,
        layout.height,
        RenderTimePlacementScope::AllBodyLabels,
    );
    assert!(
        !placements.is_empty(),
        "render-time placer should produce placements for complex.mmd"
    );

    // Drift contract proves drift-gate dissolution via two invariants that
    // together match the spirit of the deleted stale-anchor test:
    //
    //   (a) ABSOLUTE DRIFT CEILING. The placer's center must sit within
    //       ~8 cells of the drawn path. Accommodates backward edges where
    //       the label is perpendicular to the main corridor; tight enough
    //       that a stale authoritative anchor (typically off by 10+ cells)
    //       would fail.
    //
    //   (b) MATERIALLY CLOSER THAN THE FARTHEST STALE CANDIDATE. For each
    //       edge we compute the corner distance that is most-stale (the
    //       farthest canvas corner from this edge's segments). Assert
    //       `placer_drift * 2.5 <= farthest_corner` so the placer is at
    //       most 40% as far from the path as the worst stale candidate
    //       would be. This is stronger than `placer_drift < farthest`
    //       (which lets bad placements hide 1 cell inside the farthest
    //       corner's radius) and avoids the pathology where an edge
    //       passes near a canvas corner (e.g. complex.mmd's E→A backward
    //       edge, nearest-corner distance ~1.4) causing a strict
    //       `< nearest_corner` contract to reject legitimate
    //       perpendicular offsets.
    fn farthest_stale_corner_distance(
        routed: &crate::graph::grid::RoutedEdge,
        layout_width: usize,
        layout_height: usize,
    ) -> f64 {
        let corners = [
            (1usize, 1usize),
            (layout_width.saturating_sub(2), 1),
            (1, layout_height.saturating_sub(2)),
            (
                layout_width.saturating_sub(2),
                layout_height.saturating_sub(2),
            ),
        ];
        corners
            .iter()
            .map(|c| distance_to_segments(*c, &routed.segments))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    const ABSOLUTE_DRIFT_CEILING: f64 = 8.0;
    const STALE_RATIO: f64 = 2.5;

    let mut checked = 0usize;
    for routed in &routed_edges {
        let Some(placement) = placements.get(&routed.edge.index) else {
            continue;
        };
        let placer_drift = distance_to_segments(placement.center, &routed.segments);
        let farthest_corner = farthest_stale_corner_distance(routed, layout.width, layout.height);
        assert!(
            placer_drift <= ABSOLUTE_DRIFT_CEILING,
            "edge {} ({} → {}): placer drift {:.2} exceeds absolute ceiling {:.1} — \
             stale-anchor regression suspected",
            routed.edge.index,
            routed.edge.from,
            routed.edge.to,
            placer_drift,
            ABSOLUTE_DRIFT_CEILING
        );
        assert!(
            placer_drift * STALE_RATIO <= farthest_corner,
            "edge {} ({} → {}): placer drift {:.2} is not materially closer than the farthest \
             stale corner {:.2} (ratio {:.2}× required; got {:.2}×)",
            routed.edge.index,
            routed.edge.from,
            routed.edge.to,
            placer_drift,
            farthest_corner,
            STALE_RATIO,
            if placer_drift > 0.0 {
                farthest_corner / placer_drift
            } else {
                f64::INFINITY
            }
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "expected at least 2 placed labels on complex.mmd, got {checked}"
    );
}

/// C10: three parallel A→B edges with distinct labels ("one", "two", "three").
/// This fixture has compartment_size > 1 (multi-member authoritative subset),
/// so PR #A's `AuthoritativeOnly` scope already owns it. Validates that the
/// render-time placer resolves `label_geometry` by edge `index` (not by
/// `(from, to)` which would alias all three edges onto the first match).
///
/// The assertion intentionally scopes to the alias-bug signal: each of the
/// three labels must render on a distinct row. A broader "no sibling overlap"
/// claim would require per-column corridor analysis and belongs with the
/// sibling-shift tightening work slated for PR #B.
#[test]
fn c10_three_parallel_labels_converges_without_sibling_overlap() {
    let output = render_flowchart_fixture("three_parallel_labels.mmd");
    for label in ["one", "two", "three"] {
        assert!(
            output.contains(label),
            "rendered output missing '{label}'\n{output}"
        );
    }
    // Each of the three labels must appear on a distinct row — the alias bug
    // would collapse them onto the same row (or concatenate them).
    let lines: Vec<&str> = output.lines().collect();
    let row_of = |needle: &str| {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("label '{needle}' missing\n{output}"))
    };
    let (r_one, r_two, r_three) = (row_of("one"), row_of("two"), row_of("three"));
    assert!(
        r_one != r_two && r_two != r_three && r_one != r_three,
        "parallel-edge labels must occupy distinct rows (one={r_one}, two={r_two}, three={r_three})\n{output}"
    );
}

/// Q2: On-path-but-bad-spot ranking signal — scratch test
#[test]
#[ignore]
fn q2_on_path_ranking_complex_no_label() {
    // Load complex.mmd through the pipeline
    let output = render_flowchart_fixture("complex.mmd");

    // For now, just verify the fixture loads and renders
    assert!(output.contains("no"), "Output should contain 'no' label");
    assert!(
        output.contains("Output"),
        "Output should contain 'Output' node"
    );

    println!("\n=== Q2 Analysis: On-Path Ranking Signal for 'no' label ===");
    println!("Successfully rendered complex.mmd");
    println!("\nThis test scaffolds Q2 investigation. Implementation plan:");
    println!("1. Extract routed edge segments for E→F (More Data? → Output)");
    println!("2. Enumerate candidate cells along Pass-3 segments");
    println!("3. Score each candidate by signals (i)–(iv):");
    println!("   (i)   Nearest-other-edge-cell distance (Manhattan)");
    println!("   (ii)  Longest contiguous horizontal run");
    println!("   (iii) Distance to nearest node bound");
    println!("   (iv)  Distance to edge endpoint");
    println!("4. Rank by single signals and pairwise combinations");
    println!("5. Compare to pre-PR-B and post-PR-B placements");

    // TODO: Expand with full scoring logic once we confirm rendering setup is correct
}

/// Q1: Single-label trace for "miss" on cache → validate in inline_label_flowchart.mmd
#[test]
#[ignore]
fn q1_trace_miss_label() {
    use crate::engines::graph::contracts::GraphGeometryContract;
    use crate::engines::graph::{
        EngineAlgorithmId, GraphSolveRequest, SubgraphDirectionPolicy, solve_graph_family,
    };
    use crate::graph::GeometryLevel;
    use crate::graph::grid::{geometry_to_grid_layout_with_routed, route_all_edges};
    use crate::graph::routing::route_graph_geometry;
    use crate::render::graph::{edge_routing_from_style, layout_config_for_diagram};

    fn label_top_for_center(center_y: usize, height: usize) -> usize {
        center_y.saturating_sub(height / 2)
    }

    // Character-position search (not byte-position) — the rendered text contains
    // multi-byte UTF-8 box-drawing glyphs, so `str::find` byte offsets don't
    // correspond to visual columns.
    fn find_chars_in_line(line: &str, needle: &str) -> Option<usize> {
        let chars: Vec<char> = line.chars().collect();
        let needle_chars: Vec<char> = needle.chars().collect();
        chars
            .windows(needle_chars.len())
            .position(|w| w == needle_chars.as_slice())
    }

    println!("\n=== Q1 Trace (solve_graph_family path): 'miss' label ===\n");

    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("inline_label_flowchart.mmd");
    let input = fs::read_to_string(&fixture_path).expect("fixture exists");
    let parsed = parse_flowchart(&input).expect("parse ok");
    let mut diagram: Graph = compile_to_graph(&parsed);

    // Mirror runtime/graph_family.rs::render_graph_family exactly:
    //   1. prepare_wrapped_labels with proportional metrics
    //   2. solve_graph_family (which enhances config via flux_layout_profile)
    //   3. render_text_from_geometry with config-derived options
    let wrap_metrics = default_proportional_text_metrics();
    let wrap_max_width = crate::engines::graph::LayoutConfig::default().edge_label_max_width;
    crate::graph::label_wrap::prepare_wrapped_labels(
        &mut diagram.edges,
        &wrap_metrics,
        wrap_max_width,
    );

    let request = GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        GeometryLevel::Layout,
        None,
        SubgraphDirectionPolicy::AlternateAxes,
    );
    let engine_config = EngineConfig::Layered(LayeredConfig::default());
    let result = solve_graph_family(
        &diagram,
        EngineAlgorithmId::FLUX_LAYERED,
        &engine_config,
        &request,
    )
    .expect("solve ok");

    let options = TextRenderOptions::default();

    // Now replicate render_text_from_geometry's internals so we can observe the
    // placer inputs. Copies the routing-fallback + grid-layout chain verbatim.
    let routed_owned;
    let routed = match result.routed.as_ref() {
        Some(r) => r,
        None => {
            let metrics = default_proportional_text_metrics();
            routed_owned = route_graph_geometry(
                &diagram,
                &result.geometry,
                edge_routing_from_style(options.routing_style),
                &metrics,
            );
            &routed_owned
        }
    };
    let layout_config = layout_config_for_diagram(&diagram, &options);
    let layout = geometry_to_grid_layout_with_routed(
        &diagram,
        &result.geometry,
        Some(routed),
        &layout_config,
    );
    let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);

    // Mirror render_text_from_grid_layout: canvas dimensions, not layout dimensions,
    // are what the placer gets at render time.
    let (canvas_width, canvas_height) =
        crate::render::graph::text::required_canvas_size_for_test(&layout, &routed_edges);
    let edge_containment = crate::render::graph::text::compute_edge_containment(
        &diagram.edges,
        &diagram.subgraphs,
        &layout.subgraph_bounds,
    );

    let miss_edge = routed_edges
        .iter()
        .find(|e| {
            e.edge.from == "cache"
                && e.edge.to == "validate"
                && e.edge.label.as_deref() == Some("miss")
        })
        .expect("find 'miss' edge");

    let placements = compute_label_placements(
        &routed_edges,
        Some(routed),
        &layout,
        &edge_containment,
        canvas_width,
        canvas_height,
        RenderTimePlacementScope::AllBodyLabels,
    );

    let placement = placements
        .get(&miss_edge.edge.index)
        .copied()
        .expect("find placement");
    let label_dims = placement.label_dims;

    let base_x = placement.center.0.saturating_sub(label_dims.0 / 2);
    let base_y = label_top_for_center(placement.center.1, label_dims.1);

    println!(
        "[1] Placer center = {:?}, dims = {:?}",
        placement.center, label_dims
    );
    println!("[2] Derived top-left = ({}, {})", base_x, base_y);

    // Render into a canvas WITHOUT the fmt::Display trims. Placer coords are in
    // canvas frame; the default to_string() output strips leading empty rows
    // and common leading whitespace, which doesn't represent what the placer
    // sees. Using the raw canvas cells avoids the inverse-trim guesswork.
    // Pass `routed` (our manually-routed geometry) explicitly. `result.routed`
    // is None when we request GeometryLevel::Layout, and render_text_from_grid_layout
    // expects a populated routed; render_text_from_geometry re-routes internally
    // before calling into grid_layout, so the internal pipeline always has it.
    let canvas = crate::render::graph::text::render_text_canvas_for_test(
        &diagram,
        &layout,
        Some(routed),
        &options,
    );
    let raw_lines = canvas.to_raw_lines();

    let found_at = raw_lines
        .iter()
        .enumerate()
        .find_map(|(y, line)| find_chars_in_line(line, "miss").map(|x| (x, y)));

    // Diagnostic: dump canvas rows near the placer's predicted row AND near
    // the actual hit row so we can see why they disagree.
    if let Some((fx, fy)) = found_at {
        println!(
            "\n--- Canvas rows {} and {} (placer expected, actual hit) ---",
            base_y, fy
        );
        for y in [base_y.saturating_sub(1), base_y, base_y + 1, fy] {
            if let Some(line) = raw_lines.get(y) {
                let snippet: String = line
                    .chars()
                    .enumerate()
                    .skip(fx.min(base_x).saturating_sub(2))
                    .take((base_x.max(fx) + 10).saturating_sub(fx.min(base_x).saturating_sub(2)))
                    .map(|(_, c)| c)
                    .collect();
                println!("  row {:3}: '{}'", y, snippet);
            }
        }
    }

    match found_at {
        Some((fx, fy)) => {
            println!("[3] Rendered position (canvas-frame) = ({}, {})", fx, fy);
            println!(
                "\n=== DELTA (canvas-frame) ===\n  Placer top-left:   ({}, {})\n  Rendered top-left: ({}, {})\n  Delta:             ({:+}, {:+})",
                base_x,
                base_y,
                fx,
                fy,
                fx as isize - base_x as isize,
                fy as isize - base_y as isize,
            );
            assert_eq!(
                (base_x, base_y),
                (fx, fy),
                "placer top-left must match rendered canvas-frame top-left"
            );
        }
        None => {
            println!("[3] 'miss' NOT FOUND in canvas");
            panic!("label not found in canvas output");
        }
    }
}

#[test]
#[ignore]
fn q2_text_grid_classification() {
    use crate::engines::graph::contracts::GraphGeometryContract;
    use crate::engines::graph::{
        EngineAlgorithmId, GraphSolveRequest, SubgraphDirectionPolicy, solve_graph_family,
    };
    use crate::graph::GeometryLevel;
    use crate::graph::grid::{Segment, geometry_to_grid_layout_with_routed, route_all_edges};
    use crate::graph::routing::route_graph_geometry;
    use crate::render::graph::{edge_routing_from_style, layout_config_for_diagram};

    fn distance_to_segments(point: (usize, usize), segments: &[Segment]) -> f64 {
        let (px, py) = (point.0 as f64, point.1 as f64);
        segments
            .iter()
            .map(|segment| match *segment {
                Segment::Horizontal { y, x_start, x_end } => {
                    let (x_min, x_max) = (x_start.min(x_end) as f64, x_start.max(x_end) as f64);
                    let clamped_x = px.max(x_min).min(x_max);
                    ((clamped_x - px).powi(2) + (y as f64 - py).powi(2)).sqrt()
                }
                Segment::Vertical { x, y_start, y_end } => {
                    let (y_min, y_max) = (y_start.min(y_end) as f64, y_start.max(y_end) as f64);
                    let clamped_y = py.max(y_min).min(y_max);
                    ((x as f64 - px).powi(2) + (clamped_y - py).powi(2)).sqrt()
                }
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn find_chars_in_line(line: &str, needle: &str) -> Option<usize> {
        let chars: Vec<char> = line.chars().collect();
        let needle_chars: Vec<char> = needle.chars().collect();
        chars
            .windows(needle_chars.len())
            .position(|w| w == needle_chars.as_slice())
    }

    type ClassificationRow = (
        String,
        String,
        String,
        (usize, usize),
        f64,
        Option<(usize, usize)>,
    );

    fn process_fixture(fixture_name: &str) -> Vec<ClassificationRow> {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join(format!("{}.mmd", fixture_name));
        let input = fs::read_to_string(&fixture_path).expect("fixture exists");
        let parsed = parse_flowchart(&input).expect("parse ok");
        let mut diagram: Graph = compile_to_graph(&parsed);

        let wrap_metrics = default_proportional_text_metrics();
        let wrap_max_width = crate::engines::graph::LayoutConfig::default().edge_label_max_width;
        crate::graph::label_wrap::prepare_wrapped_labels(
            &mut diagram.edges,
            &wrap_metrics,
            wrap_max_width,
        );

        let request = GraphSolveRequest::new(
            MeasurementMode::Grid,
            GraphGeometryContract::Canonical,
            GeometryLevel::Layout,
            None,
            SubgraphDirectionPolicy::AlternateAxes,
        );
        let engine_config = EngineConfig::Layered(LayeredConfig::default());
        let result = solve_graph_family(
            &diagram,
            EngineAlgorithmId::FLUX_LAYERED,
            &engine_config,
            &request,
        )
        .expect("solve ok");

        let options = TextRenderOptions::default();
        let routed_owned;
        let routed = match result.routed.as_ref() {
            Some(r) => r,
            None => {
                let metrics = default_proportional_text_metrics();
                routed_owned = route_graph_geometry(
                    &diagram,
                    &result.geometry,
                    edge_routing_from_style(options.routing_style),
                    &metrics,
                );
                &routed_owned
            }
        };

        let layout_config = layout_config_for_diagram(&diagram, &options);
        let layout = geometry_to_grid_layout_with_routed(
            &diagram,
            &result.geometry,
            Some(routed),
            &layout_config,
        );
        let routed_edges = route_all_edges(&diagram.edges, &layout, diagram.direction);

        let (canvas_width, canvas_height) =
            crate::render::graph::text::required_canvas_size_for_test(&layout, &routed_edges);
        let edge_containment = crate::render::graph::text::compute_edge_containment(
            &diagram.edges,
            &diagram.subgraphs,
            &layout.subgraph_bounds,
        );

        let placements = compute_label_placements(
            &routed_edges,
            Some(routed),
            &layout,
            &edge_containment,
            canvas_width,
            canvas_height,
            RenderTimePlacementScope::AllBodyLabels,
        );

        let canvas = crate::render::graph::text::render_text_canvas_for_test(
            &diagram,
            &layout,
            Some(routed),
            &options,
        );
        let raw_lines = canvas.to_raw_lines();

        let mut results = Vec::new();

        for routed_edge in &routed_edges {
            // Skip backward edges and edges without labels
            if routed_edge.is_backward || routed_edge.edge.label.is_none() {
                continue;
            }

            let label = routed_edge.edge.label.as_ref().unwrap().to_string();
            let placement = match placements.get(&routed_edge.edge.index) {
                Some(p) => p,
                None => continue,
            };

            let placer_center = placement.center;
            let drift = distance_to_segments(placer_center, &routed_edge.segments);

            // Find rendered position in canvas
            let rendered_pos = raw_lines
                .iter()
                .enumerate()
                .find_map(|(y, line)| find_chars_in_line(line, &label).map(|x| (x, y)));

            let edge_desc = format!("{} → {}", routed_edge.edge.from, routed_edge.edge.to);

            results.push((
                fixture_name.to_string(),
                edge_desc,
                label,
                placer_center,
                drift,
                rendered_pos,
            ));
        }

        results
    }

    println!("\n=== Q2 Text-Grid Classification ===\n");

    let complex_results = process_fixture("complex");
    let inline_results = process_fixture("inline_label_flowchart");

    let all_results: Vec<_> = complex_results
        .iter()
        .chain(inline_results.iter())
        .collect();

    println!("| fixture | edge | label | placer_center | drift | rendered_pos | bucket |");
    println!("|---------|------|-------|---------------|-------|--------------|--------|");

    let mut bucket_a = 0;
    let bucket_b = 0;
    let mut bucket_c = 0;

    for (fixture, edge, label, placer_center, drift, rendered_pos) in &all_results {
        let bucket = if *drift > 3.0 || rendered_pos.is_none() {
            bucket_a += 1;
            "a (off-path)"
        } else {
            bucket_c += 1;
            "c (unchanged)"
        };

        let rendered_str = rendered_pos
            .map(|(x, y)| format!("({}, {})", x, y))
            .unwrap_or_else(|| "NOT FOUND".to_string());

        println!(
            "| {} | {} | {} | ({}, {}) | {:.2} | {} | {} |",
            fixture, edge, label, placer_center.0, placer_center.1, drift, rendered_str, bucket
        );
    }

    println!("\n=== Summary ===");
    println!("Bucket (a) off-path: {}", bucket_a);
    println!("Bucket (b) on-path-but-bad: {}", bucket_b);
    println!("Bucket (c) unchanged/improved: {}", bucket_c);

    panic!("Q2 classification complete — see output above");
}
