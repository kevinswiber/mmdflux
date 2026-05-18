//! Post-gate regression pins for the label-bend bow's endpoint preservation.
//!
//! These tests exercise `LabelBendPolicy::ApplyIfLanePacked` actually
//! applying the bow at `stage.rs:265–278`. Pins from #240's regression
//! catalog: (1) endpoints must not be re-projected by SVG clipping,
//! (2) the corner-rounding pass must never emit Q control points
//! within `STUB_LEN_MAX` of an endpoint.

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::run_layered_layout;
use crate::engines::graph::contracts::MeasurementMode;
use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::{EdgeRouting, LabelBendPolicy, route_graph_geometry_with_policy};
use crate::graph::space::FPoint;
use crate::mermaid::parse_flowchart;

const STUB_LEN_MAX: f64 = 8.0;

fn proportional_layout(input: &str) -> (crate::graph::Graph, GraphGeometry) {
    let fc = parse_flowchart(input).expect("fixture parses");
    let diagram = compile_to_graph(&fc);
    let metrics = default_proportional_text_metrics();
    let config = EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig {
        variable_rank_spacing: true,
        ..crate::engines::graph::algorithms::layered::LayoutConfig::default()
    });
    let geom = run_layered_layout(&MeasurementMode::Proportional(&metrics), &diagram, &config)
        .expect("layered layout succeeds");
    (diagram, geom)
}

fn route_with_policy(
    diagram: &crate::graph::Graph,
    geometry: &GraphGeometry,
    policy: LabelBendPolicy,
) -> RoutedGraphGeometry {
    let metrics = default_proportional_text_metrics();
    route_graph_geometry_with_policy(
        diagram,
        geometry,
        EdgeRouting::PolylineRoute,
        &metrics,
        policy,
    )
}

const LANE_PACKED_FIXTURE: &str = r#"graph TD
    A -->|this is a deliberately long label| B
    B -->|another deliberately long reply label| A
"#;

#[test]
fn apply_if_lane_packed_changes_displaced_edge_path() {
    let (diagram, geometry) = proportional_layout(LANE_PACKED_FIXTURE);
    let skipped = route_with_policy(&diagram, &geometry, LabelBendPolicy::Skip);
    let applied = route_with_policy(&diagram, &geometry, LabelBendPolicy::ApplyIfLanePacked);

    let displaced_idx = applied
        .edges
        .iter()
        .position(|e| e.label_geometry.as_ref().is_some_and(|g| g.track != 0))
        .expect("at least one edge must land on a non-zero track");

    assert_ne!(
        skipped.edges[displaced_idx].path, applied.edges[displaced_idx].path,
        "ApplyIfLanePacked should change the path of the displaced edge; \
         skipped={:?} applied={:?}",
        skipped.edges[displaced_idx].path, applied.edges[displaced_idx].path
    );
}

#[test]
fn lane_bowed_edge_endpoints_stay_on_node_faces() {
    let (diagram, geometry) = proportional_layout(LANE_PACKED_FIXTURE);
    let skipped = route_with_policy(&diagram, &geometry, LabelBendPolicy::Skip);
    let applied = route_with_policy(&diagram, &geometry, LabelBendPolicy::ApplyIfLanePacked);

    let displaced_idx = applied
        .edges
        .iter()
        .position(|e| e.label_geometry.as_ref().is_some_and(|g| g.track != 0))
        .expect("at least one edge must land on a non-zero track");

    let bowed_start = applied.edges[displaced_idx].path.first().copied().unwrap();
    let bowed_end = applied.edges[displaced_idx].path.last().copied().unwrap();
    let baseline_start = skipped.edges[displaced_idx].path.first().copied().unwrap();
    let baseline_end = skipped.edges[displaced_idx].path.last().copied().unwrap();

    assert!(
        (bowed_start.x - baseline_start.x).abs() < 1e-6
            && (bowed_start.y - baseline_start.y).abs() < 1e-6,
        "bowed edge start must equal the unbowed start (no SVG endpoint re-projection): \
         bowed={bowed_start:?} baseline={baseline_start:?}"
    );
    assert!(
        (bowed_end.x - baseline_end.x).abs() < 1e-6 && (bowed_end.y - baseline_end.y).abs() < 1e-6,
        "bowed edge end must equal the unbowed end: \
         bowed={bowed_end:?} baseline={baseline_end:?}"
    );
}

#[test]
fn apply_if_lane_packed_displaces_symmetrized_track_zero_member() {
    // Reciprocal edges land on tracks `[0, -1]` (or similar) which the
    // lane pass symmetrizes to centered offsets `[+0.5, -0.5]`. Both
    // members carry real displacement, so both routed paths must
    // change under `ApplyIfLanePacked` — even the one stored with raw
    // `track == 0`. Regression guard against keying the gate off
    // `outcome.track != 0`.
    let (diagram, geometry) = proportional_layout(LANE_PACKED_FIXTURE);
    let skipped = route_with_policy(&diagram, &geometry, LabelBendPolicy::Skip);
    let applied = route_with_policy(&diagram, &geometry, LabelBendPolicy::ApplyIfLanePacked);

    // Confirm at least one edge has raw `track == 0` AND a non-zero
    // centered offset (i.e. the regression scenario actually exists in
    // this fixture).
    let mut found_symmetrized_zero = false;
    for edge in &applied.edges {
        if let Some(g) = edge.label_geometry.as_ref()
            && g.track == 0
            && g.compartment_size > 1
        {
            found_symmetrized_zero = true;
            break;
        }
    }
    assert!(
        found_symmetrized_zero,
        "fixture should contain a track==0 member in a multi-member compartment; \
         label_geoms={:?}",
        applied
            .edges
            .iter()
            .map(|e| e
                .label_geometry
                .as_ref()
                .map(|g| (g.track, g.compartment_size)))
            .collect::<Vec<_>>()
    );

    // Every multi-member-compartment edge must have its path shifted.
    for (i, (s, a)) in skipped.edges.iter().zip(applied.edges.iter()).enumerate() {
        let in_multi_member = a
            .label_geometry
            .as_ref()
            .is_some_and(|g| g.compartment_size > 1);
        if in_multi_member {
            assert_ne!(
                s.path, a.path,
                "edge {i}: multi-member-compartment path must be shifted under ApplyIfLanePacked; \
                 skipped={:?} applied={:?}",
                s.path, a.path
            );
        }
    }
}

#[test]
fn lane_bowed_edge_no_acute_joins_near_endpoints() {
    let (diagram, geometry) = proportional_layout(LANE_PACKED_FIXTURE);
    let applied = route_with_policy(&diagram, &geometry, LabelBendPolicy::ApplyIfLanePacked);

    let displaced_idx = applied
        .edges
        .iter()
        .position(|e| e.label_geometry.as_ref().is_some_and(|g| g.track != 0))
        .expect("at least one edge must land on a non-zero track");

    let path = &applied.edges[displaced_idx].path;
    assert!(path.len() >= 3, "bowed path must have interior joins");

    let start = path[0];
    let end = *path.last().unwrap();
    let near_end_threshold = STUB_LEN_MAX + 1e-3;

    for window in path.windows(3) {
        let a = window[0];
        let b = window[1];
        let c = window[2];

        if !is_near(b, start, near_end_threshold) && !is_near(b, end, near_end_threshold) {
            continue;
        }

        let (ux, uy) = unit_diff(a, b);
        let (vx, vy) = unit_diff(b, c);
        let dot = ux * vx + uy * vy;
        let cross = ux * vy - uy * vx;

        assert!(
            cross.abs() > 0.5 || dot.abs() < 0.5,
            "acute or reflex join detected near endpoint at vertex {b:?}; \
             dot={dot}, cross={cross}, full path={path:?}"
        );
    }
}

fn is_near(p: FPoint, anchor: FPoint, threshold: f64) -> bool {
    let dx = p.x - anchor.x;
    let dy = p.y - anchor.y;
    (dx * dx + dy * dy).sqrt() < threshold
}

fn unit_diff(a: FPoint, b: FPoint) -> (f64, f64) {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < f64::EPSILON {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}
