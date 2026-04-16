//! Plan 0146: corpus-wide assertion that no edge label rect overlaps the
//! source/target node rect or the source/target marker bounding box.
//!
//! Walks every `.mmd` flowchart fixture under `tests/fixtures/flowchart/`,
//! routes each through the same pipeline used by `render_diagram` for SVG
//! output, and aggregates every overlap finding into a single panic message
//! so the full picture is visible on test failure.
//!
//! Marker bboxes are computed as **oriented** rectangles along the actual
//! endpoint segment vector, then converted to their conservative
//! axis-aligned bounding boxes for the overlap test. This catches diagonal
//! terminal segments that the polyline route fallback can produce.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::diagrams::flowchart::compile_to_graph;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::run_layered_layout;
use crate::engines::graph::contracts::MeasurementMode;
use crate::graph::edge_marker::{MarkerEnvelope, marker_envelope};
use crate::graph::geometry::{FPoint, FRect, RoutedEdgeGeometry, RoutedGraphGeometry};
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{Direction, Graph};
use crate::mermaid::parse_flowchart;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerSide {
    Source,
    Target,
}

#[derive(Debug)]
struct OverlapFinding {
    fixture: String,
    edge_index: usize,
    label: String,
    kind: &'static str, // "source-node" | "target-node" | "source-marker" | "target-marker"
    overlap_dx: f64,
    overlap_dy: f64,
}

fn rect_overlap(a: FRect, b: FRect) -> Option<(f64, f64)> {
    let dx = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let dy = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if dx > 0.0 && dy > 0.0 {
        Some((dx, dy))
    } else {
        None
    }
}

/// Compute the AABB of the marker's oriented bounding box at the given path
/// endpoint. Uses the actual endpoint tangent vector — not just an
/// axis-aligned approximation — so diagonal terminal segments are modeled.
fn marker_aabb(path: &[FPoint], side: MarkerSide, envelope: MarkerEnvelope) -> Option<FRect> {
    if path.len() < 2 {
        return None;
    }

    // The marker bbox extends `length` AWAY from the anchor's node (into
    // the gap). For each side, compute the "away from anchor node" unit
    // vector at the path endpoint:
    //
    // - **Source side** (marker-start, orient="auto-start-reverse"): the
    //   marker is rotated 180° from path direction, so the marker's "forward"
    //   axis points OPPOSITE to the path direction. But the "away from
    //   source node" direction is also opposite the path's "into source"
    //   direction. Source node is "behind" path[0]; "away" = path direction
    //   from path[0] toward path[1].
    //
    // - **Target side** (marker-end, orient="auto"): marker forward = path
    //   direction. Away from target node = OPPOSITE to path direction at
    //   the end (since target node is "in front of" path[end]).
    let (anchor, tangent_raw) = match side {
        MarkerSide::Source => {
            // Away direction = from path[0] toward path[1] (into gap).
            let a = path[0];
            let next = path[1];
            (a, FPoint::new(next.x - a.x, next.y - a.y))
        }
        MarkerSide::Target => {
            // Away direction = from path[end] toward path[end-1] (into gap).
            let n = path.len();
            let a = path[n - 1];
            let prev = path[n - 2];
            (a, FPoint::new(prev.x - a.x, prev.y - a.y))
        }
    };

    let mag = (tangent_raw.x * tangent_raw.x + tangent_raw.y * tangent_raw.y).sqrt();
    if mag < 1e-9 {
        return None;
    }
    let along = FPoint::new(tangent_raw.x / mag, tangent_raw.y / mag);
    let perp = FPoint::new(-along.y, along.x);
    let half_w = envelope.width / 2.0;
    let length = envelope.length;

    // OBB corners.
    let c0 = FPoint::new(anchor.x + perp.x * half_w, anchor.y + perp.y * half_w);
    let c1 = FPoint::new(anchor.x - perp.x * half_w, anchor.y - perp.y * half_w);
    let c2 = FPoint::new(
        anchor.x + along.x * length + perp.x * half_w,
        anchor.y + along.y * length + perp.y * half_w,
    );
    let c3 = FPoint::new(
        anchor.x + along.x * length - perp.x * half_w,
        anchor.y + along.y * length - perp.y * half_w,
    );

    let xs = [c0.x, c1.x, c2.x, c3.x];
    let ys = [c0.y, c1.y, c2.y, c3.y];
    let min_x = xs.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_x = xs.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let min_y = ys.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_y = ys.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    Some(FRect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

fn marker_bbox_for_edge(
    edge: &RoutedEdgeGeometry,
    diagram: &Graph,
    side: MarkerSide,
) -> Option<FRect> {
    let diagram_edge = diagram.edges.get(edge.index)?;
    // Path-anchored mapping: arrow_start lives at path[0] (= authored
    // `from`), arrow_end at path[end] (= authored `to`). Holds for forward
    // and backward edges in every diagram direction. The two `MarkerSide`
    // variants here are *path-positional*, picking which endpoint of the
    // routed path the marker bbox is anchored to — independent of which
    // node sits where in the gap.
    let arrow = match side {
        MarkerSide::Source => diagram_edge.arrow_start,
        MarkerSide::Target => diagram_edge.arrow_end,
    };
    let envelope = marker_envelope(arrow)?;
    if envelope.length == 0.0 && envelope.width == 0.0 {
        return None;
    }
    marker_aabb(&edge.path, side, envelope)
}

fn flowchart_routed_for_fixture(path: &Path) -> (Graph, RoutedGraphGeometry) {
    let input = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let fc = parse_flowchart(&input)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    let diagram = compile_to_graph(&fc);
    let metrics = default_proportional_text_metrics();
    let mode = MeasurementMode::Proportional(metrics.clone());
    let config = EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig {
        greedy_switch: true,
        model_order_tiebreak: true,
        variable_rank_spacing: true,
        track_reversed_chains: true,
        ..crate::engines::graph::algorithms::layered::LayoutConfig::default()
    });
    let geom = run_layered_layout(&mode, &diagram, &config)
        .unwrap_or_else(|e| panic!("layout failed for {}: {e}", path.display()));
    // Match the default SVG render path: orthogonal routing.
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute, &metrics);
    (diagram, routed)
}

fn walk_flowchart_fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut out = Vec::new();
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("mmd") {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn check_fixture_for_overlaps(path: &Path) -> Vec<OverlapFinding> {
    let fixture_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_string();
    let (diagram, routed) = flowchart_routed_for_fixture(path);

    let mut findings = Vec::new();

    for edge in &routed.edges {
        let Some(geom) = edge.label_geometry.as_ref() else {
            continue;
        };
        let Some(diagram_edge) = diagram.edges.get(edge.index) else {
            continue;
        };
        let label = diagram_edge
            .label
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let Some(label_text) = label else {
            continue;
        };

        let label_rect = geom.rect;

        let push = |findings: &mut Vec<OverlapFinding>, kind: &'static str, other: FRect| {
            if let Some((dx, dy)) = rect_overlap(label_rect, other) {
                findings.push(OverlapFinding {
                    fixture: fixture_name.clone(),
                    edge_index: edge.index,
                    label: label_text.clone(),
                    kind,
                    overlap_dx: dx,
                    overlap_dy: dy,
                });
            }
        };

        // Use authored from/to since the test only cares whether the label
        // rect overlaps either endpoint node — direction/backward swaps are
        // gap-direction concerns, not label-vs-node-rect concerns.
        if let Some(node) = routed.nodes.get(edge.from.as_str()) {
            push(&mut findings, "source-node", node.rect);
        }
        if let Some(node) = routed.nodes.get(edge.to.as_str()) {
            push(&mut findings, "target-node", node.rect);
        }
        let _ = diagram_edge;
        if let Some(bbox) = marker_bbox_for_edge(edge, &diagram, MarkerSide::Source) {
            push(&mut findings, "source-marker", bbox);
        }
        if let Some(bbox) = marker_bbox_for_edge(edge, &diagram, MarkerSide::Target) {
            push(&mut findings, "target-marker", bbox);
        }
    }

    findings
}

#[test]
fn label_rect_does_not_overlap_node_or_marker_for_any_flowchart_fixture() {
    let mut all_findings: Vec<OverlapFinding> = Vec::new();
    for path in walk_flowchart_fixtures() {
        all_findings.extend(check_fixture_for_overlaps(&path));
    }

    if !all_findings.is_empty() {
        let mut by_fixture: HashMap<String, Vec<&OverlapFinding>> = HashMap::new();
        for f in &all_findings {
            by_fixture.entry(f.fixture.clone()).or_default().push(f);
        }
        let mut report = format!(
            "{} label-overlap finding(s) across {} fixture(s):\n",
            all_findings.len(),
            by_fixture.len()
        );
        let mut keys: Vec<&String> = by_fixture.keys().collect();
        keys.sort();
        for fixture in keys {
            let entries = &by_fixture[fixture];
            report.push_str(&format!("\n  {}: {} overlap(s)\n", fixture, entries.len()));
            for e in entries {
                report.push_str(&format!(
                    "    edge[{}] label={:?} kind={} dx={:.2} dy={:.2}\n",
                    e.edge_index, e.label, e.kind, e.overlap_dx, e.overlap_dy
                ));
            }
        }
        panic!("{}", report);
    }
}

/// Sanity-check that the corpus walker actually visits the known-failing
/// fixture from #229. If the walker silently misses fixtures, the headline
/// assertion would pass without proving anything.
#[test]
fn corpus_walker_includes_known_failing_fixture() {
    let names: Vec<String> = walk_flowchart_fixtures()
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(String::from))
        .collect();
    assert!(
        names.iter().any(|n| n == "br_line_breaks.mmd"),
        "expected br_line_breaks.mmd in corpus walk; got: {names:?}"
    );
}

#[allow(dead_code)]
fn _suppress_unused_direction(_d: Direction) {}
