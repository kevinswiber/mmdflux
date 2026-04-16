//! Plan 0146 Task 2.1: edge-label boundary clamp.
//!
//! Walks every routed edge with `label_geometry`, computes the available
//! gap between visual source/target node faces (plus marker avoidance)
//! along the edge-parallel axis, and slides the label rect into that gap.
//! When the label cannot fit (`span > gap`), records an `UnfitOverlap`
//! entry on the routed geometry's public field so downstream consumers
//! (MMDS diagnostics + CLI stderr per Task 2.3) can surface the
//! diagnostic. The label rect is left at its pre-clamp position in
//! that case so the visual fallback is the same as pre-fix.

use std::collections::HashMap;

use crate::graph::edge_marker::marker_avoidance_distance;
use crate::graph::geometry::{
    EdgeLabelSide, FRect, PositionedNode, RoutedEdgeGeometry, UnfitOverlap,
};
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::label_gap::Axis;
use crate::graph::{Direction, Graph};

/// Edge-parallel clamp: keep label rects beyond source/target node faces
/// (plus marker avoidance) along the axis the edge runs.
///
/// Records unfit cases (label too large for the gap) on the supplied
/// `unfit_overlaps` vec — populated unconditionally in **all builds**
/// per plan 0146 Rev 2 (no `cfg(test)`, no env-gating, no thread-local).
pub(crate) fn clamp_label_geometry_to_node_bounds(
    edges: &mut [RoutedEdgeGeometry],
    nodes: &HashMap<String, PositionedNode>,
    diagram: &Graph,
    direction: Direction,
    metrics: &ProportionalTextMetrics,
    unfit_overlaps: &mut Vec<UnfitOverlap>,
) {
    let spacing = metrics.label_padding_y;

    for edge in edges.iter_mut() {
        // Skip early when there's no label geometry to clamp.
        if edge.label_geometry.is_none() {
            continue;
        }
        let edge_index = edge.index;
        let Some(diagram_edge) = diagram.edges.get(edge_index) else {
            continue;
        };

        // The clamp uses authored `from`/`to` and the markers anchored at
        // each path endpoint (`arrow_start` lives at path[0] = `from`,
        // `arrow_end` at path[end] = `to`). `edge_parallel_bounds` is
        // direction-aware via rect positions, so no visual src/tgt swap is
        // needed here.
        let Some(from_node) = nodes.get(edge.from.as_str()) else {
            continue;
        };
        let Some(to_node) = nodes.get(edge.to.as_str()) else {
            continue;
        };

        let from_avoid = marker_avoidance_distance(diagram_edge.arrow_start);
        let to_avoid = marker_avoidance_distance(diagram_edge.arrow_end);

        let (lo, hi, axis) = edge_parallel_bounds(
            from_node.rect,
            to_node.rect,
            direction,
            from_avoid,
            to_avoid,
            spacing,
        );

        // Now take the mutable borrow.
        let geom = edge.label_geometry.as_mut().expect("checked is_some above");
        match try_clamp(geom.rect, lo, hi, axis, geom.side) {
            ClampResult::Ok(new_rect) => {
                if new_rect != geom.rect {
                    geom.rect = new_rect;
                    geom.center = rect_center(new_rect);
                    edge.label_position = Some(rect_center(new_rect));
                }
            }
            ClampResult::Unfit {
                gap,
                span,
                attempted,
            } => {
                let label = diagram_edge.label.clone().unwrap_or_default();
                unfit_overlaps.push(UnfitOverlap {
                    edge_index,
                    label,
                    gap_pixels: gap,
                    label_span_pixels: span,
                    attempted_side: attempted,
                });
                // Visual fallback: leave geom unchanged. Diagnostic data
                // attached to RoutedGraphGeometry::unfit_label_overlaps for
                // downstream surfacing.
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ClampResult {
    Ok(FRect),
    Unfit {
        gap: f64,
        span: f64,
        attempted: EdgeLabelSide,
    },
}

fn try_clamp(
    rect: FRect,
    lo: f64,
    hi: f64,
    axis: Axis,
    current_side: EdgeLabelSide,
) -> ClampResult {
    let (start, span) = match axis {
        Axis::Y => (rect.y, rect.height),
        Axis::X => (rect.x, rect.width),
    };
    let gap = hi - lo;

    if span > gap {
        // Label genuinely too large for the available space. Side-flip is
        // perpendicular to the gap axis so it cannot resolve a sizing
        // failure. Record as unfit; visual fallback degrades to the
        // pre-fix position.
        return ClampResult::Unfit {
            gap,
            span,
            attempted: current_side,
        };
    }

    // Slide the rect into [lo, hi - span]. If already inside, no-op.
    let new_start = start.max(lo).min(hi - span);
    if (new_start - start).abs() < 1e-9 {
        return ClampResult::Ok(rect);
    }
    ClampResult::Ok(rect_with_axis_start(rect, axis, new_start))
}

fn rect_with_axis_start(rect: FRect, axis: Axis, new_start: f64) -> FRect {
    match axis {
        Axis::Y => FRect::new(rect.x, new_start, rect.width, rect.height),
        Axis::X => FRect::new(new_start, rect.y, rect.width, rect.height),
    }
}

fn rect_center(rect: FRect) -> crate::graph::geometry::FPoint {
    crate::graph::geometry::FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Compute the edge-parallel `[lo, hi]` interval inside which the label
/// rect must sit, given source/target rects and marker avoidance distances.
///
/// Determines upper/lower (or left/right) **from the actual rect positions**,
/// not from `(authored_from, authored_to)`. This matters for `BT` and `RL`
/// diagrams, where the layered engine puts the authored target above/right
/// of the authored source — using authored direction here would produce a
/// negative `gap` and false `UnfitOverlap` reports for healthy edges.
///
/// `source_avoid` is the marker avoidance distance for the marker at
/// `path[0]` (= `arrow_start`); `target_avoid` is for `path[end]`
/// (= `arrow_end`). Which face each marker lives on is implied by the
/// resolved upper/lower (or left/right) — see comments below.
fn edge_parallel_bounds(
    source_rect: FRect,
    target_rect: FRect,
    direction: Direction,
    source_avoid: f64,
    target_avoid: f64,
    spacing: f64,
) -> (f64, f64, Axis) {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            // Upper node = whichever rect has smaller y; lower = larger y.
            // For TD forward (source above target) and BT backward, source is upper.
            // For BT forward and TD backward, target is upper.
            let (upper, upper_avoid, lower, lower_avoid) = if source_rect.y <= target_rect.y {
                (source_rect, source_avoid, target_rect, target_avoid)
            } else {
                (target_rect, target_avoid, source_rect, source_avoid)
            };
            let lo = upper.y + upper.height + upper_avoid + spacing;
            let hi = lower.y - lower_avoid - spacing;
            (lo, hi, Axis::Y)
        }
        Direction::LeftRight | Direction::RightLeft => {
            // Left node = whichever rect has smaller x; right = larger x.
            let (left, left_avoid, right, right_avoid) = if source_rect.x <= target_rect.x {
                (source_rect, source_avoid, target_rect, target_avoid)
            } else {
                (target_rect, target_avoid, source_rect, source_avoid)
            };
            let lo = left.x + left.width + left_avoid + spacing;
            let hi = right.x - right_avoid - spacing;
            (lo, hi, Axis::X)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide, FPoint, RoutedGraphGeometry};
    use crate::graph::measure::default_proportional_text_metrics;
    use crate::graph::{Edge, Graph, Node, Shape};

    /// Build a hand-stuffed `RoutedGraphGeometry` for clamp tests. Returns
    /// the matching `Graph` too because the clamp pulls arrow info from it.
    fn synthetic(
        direction: Direction,
        source_rect: FRect,
        target_rect: FRect,
        label_rect: FRect,
        is_backward: bool,
    ) -> (Graph, RoutedGraphGeometry) {
        let mut diagram = Graph::new(direction);
        diagram.add_node(Node::new("S"));
        diagram.add_node(Node::new("T"));
        diagram.add_edge(Edge::new("S", "T").with_label("synthetic"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "S".into(),
            PositionedNode {
                id: "S".into(),
                rect: source_rect,
                shape: Shape::Rectangle,
                label: "S".into(),
                parent: None,
            },
        );
        nodes.insert(
            "T".into(),
            PositionedNode {
                id: "T".into(),
                rect: target_rect,
                shape: Shape::Rectangle,
                label: "T".into(),
                parent: None,
            },
        );

        // Path endpoints on the source bottom face / target top face for TD,
        // similar for LR.
        let path = match direction {
            Direction::TopDown | Direction::BottomTop => vec![
                FPoint::new(
                    source_rect.x + source_rect.width / 2.0,
                    source_rect.y + source_rect.height,
                ),
                FPoint::new(target_rect.x + target_rect.width / 2.0, target_rect.y),
            ],
            Direction::LeftRight | Direction::RightLeft => vec![
                FPoint::new(
                    source_rect.x + source_rect.width,
                    source_rect.y + source_rect.height / 2.0,
                ),
                FPoint::new(target_rect.x, target_rect.y + target_rect.height / 2.0),
            ],
        };

        let edge = RoutedEdgeGeometry {
            index: 0,
            from: "S".into(),
            to: "T".into(),
            path,
            label_position: Some(rect_center(label_rect)),
            label_side: Some(EdgeLabelSide::Above),
            head_label_position: None,
            tail_label_position: None,
            is_backward,
            from_subgraph: None,
            to_subgraph: None,
            source_port: None,
            target_port: None,
            preserve_orthogonal_topology: false,
            label_geometry: Some(EdgeLabelGeometry {
                center: rect_center(label_rect),
                rect: label_rect,
                padding: (4.0, 2.0),
                side: EdgeLabelSide::Above,
                track: 0,
            }),
        };

        let routed = RoutedGraphGeometry {
            nodes,
            edges: vec![edge],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction,
            bounds: FRect::new(0.0, 0.0, 200.0, 200.0),
            unfit_label_overlaps: Vec::new(),
        };

        (diagram, routed)
    }

    #[test]
    fn clamp_lr_label_too_wide_for_gap_records_unfit() {
        // LR layout: source 0..50, target 70..120, gap = 20.
        // Label width = 40 (way too wide for the gap).
        let (diagram, mut routed) = synthetic(
            Direction::LeftRight,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(70.0, 0.0, 50.0, 30.0),
            FRect::new(40.0, 5.0, 40.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::LeftRight,
            &metrics,
            &mut unfits,
        );
        assert_eq!(unfits.len(), 1, "expected one unfit entry, got {unfits:?}");
        let u = &unfits[0];
        assert_eq!(u.edge_index, 0);
        assert_eq!(u.label, "synthetic");
        assert!(u.gap_pixels < u.label_span_pixels);
        // Rect stays at pre-clamp position (graceful fallback).
        assert_eq!(routed.edges[0].label_geometry.unwrap().rect.width, 40.0);
        assert_eq!(routed.edges[0].label_geometry.unwrap().rect.x, 40.0);
    }

    #[test]
    fn clamp_lr_label_fits_with_slide_in() {
        // Wide gap (60), narrow label (15). Pre-clamp rect overlaps source
        // node interior; clamp slides it past source.right + avoidance.
        let (diagram, mut routed) = synthetic(
            Direction::LeftRight,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(110.0, 0.0, 50.0, 30.0),
            FRect::new(40.0, 5.0, 15.0, 20.0), // x=40 starts inside source
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::LeftRight,
            &metrics,
            &mut unfits,
        );
        assert!(unfits.is_empty(), "expected no unfit, got {unfits:?}");
        let new_rect = routed.edges[0].label_geometry.unwrap().rect;
        // Must sit beyond source.right (50) + Normal target_avoid (8) + spacing (2) = 60
        // Wait — source has Arrow::None (Edge default for source). target has Arrow::Normal.
        // source.x + source.width + source_avoid + spacing = 50 + 0 + 2 = 52 (lo)
        // target.x - target_avoid - spacing = 110 - 8 - 2 = 100 (hi)
        // Label width 15, so available range [52, 100 - 15] = [52, 85]
        // Pre-clamp x=40 < 52, so clamp slides to x=52.
        assert!(
            (new_rect.x - 52.0).abs() < 1e-6,
            "expected x=52 after slide-in, got {}",
            new_rect.x
        );
    }

    #[test]
    fn clamp_rl_uses_x_axis() {
        // RL: same axis as LR (resolve_visual_endpoints + edge_parallel_bounds
        // share the formula since visual src is always to the left of visual tgt
        // in the rect coordinates after engine direction-flip).
        // Use a configuration that fits, then verify clamp ran on x.
        let (diagram, mut routed) = synthetic(
            Direction::RightLeft,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(110.0, 0.0, 50.0, 30.0),
            FRect::new(40.0, 5.0, 15.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::RightLeft,
            &metrics,
            &mut unfits,
        );
        assert!(unfits.is_empty(), "expected no unfit, got {unfits:?}");
        // Same lo/hi math as LR — the clamp formula is direction-symmetric
        // because resolve_visual_endpoints handles is_backward and the engine
        // handles the visual flip via reversed_edges.
        assert!(routed.edges[0].label_geometry.unwrap().rect.x >= 50.0);
    }

    #[test]
    fn clamp_td_label_already_fits_unchanged() {
        // Pre-clamp rect already in the valid range — clamp is a no-op.
        let (diagram, mut routed) = synthetic(
            Direction::TopDown,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 60.0, 30.0, 20.0), // y=60 well inside [32, 92]
            false,
        );
        let original_rect = routed.edges[0].label_geometry.unwrap().rect;
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::TopDown,
            &metrics,
            &mut unfits,
        );
        assert!(unfits.is_empty());
        assert_eq!(routed.edges[0].label_geometry.unwrap().rect, original_rect);
    }

    #[test]
    fn clamp_td_label_above_source_bottom_slides_down() {
        // TD: pre-clamp rect intrudes into source node bottom (y=20 < source.bottom=30).
        // Source has no marker (Edge::new default arrow_start=None), target has Normal (8 px).
        // lo = source.y + source.height + 0 + 2 = 32
        // hi = target.y - 8 - 2 = 90
        // Label height 20, so range [32, 70]. Pre-clamp y=20 < 32 → slide to 32.
        let (diagram, mut routed) = synthetic(
            Direction::TopDown,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 20.0, 30.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::TopDown,
            &metrics,
            &mut unfits,
        );
        assert!(unfits.is_empty());
        let new_rect = routed.edges[0].label_geometry.unwrap().rect;
        assert!(
            (new_rect.y - 32.0).abs() < 1e-6,
            "expected y=32 after slide-in, got {}",
            new_rect.y
        );
    }

    /// Regression for the GPT-5.4 review finding: BT diagrams put the authored
    /// `target` *above* the authored `source` in render space. The clamp must
    /// derive upper/lower from rect positions, not from authored direction —
    /// otherwise the gap goes negative and a healthy edge gets reported as
    /// `UnfitOverlap`.
    #[test]
    fn clamp_bt_forward_uses_rect_positions_not_authored_direction() {
        // BT layout: authored `from` (S) is BELOW authored `to` (T).
        // S at y=100, T at y=0. Label sits in the gap between (e.g., y=50).
        let (diagram, mut routed) = synthetic(
            Direction::BottomTop,
            FRect::new(0.0, 100.0, 50.0, 30.0), // source (from) at bottom
            FRect::new(0.0, 0.0, 50.0, 30.0),   // target (to) at top
            FRect::new(10.0, 50.0, 30.0, 20.0), // label safely between
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::BottomTop,
            &metrics,
            &mut unfits,
        );
        assert!(
            unfits.is_empty(),
            "BT forward edge with authored source below target must not \
             produce a bogus UnfitOverlap; got {unfits:?}"
        );
        // Label should be unchanged (already inside the valid range).
        let new_rect = routed.edges[0].label_geometry.unwrap().rect;
        assert!(
            (new_rect.y - 50.0).abs() < 1e-6,
            "BT label that already fits should not be moved; got y={}",
            new_rect.y
        );
    }

    /// Same test, RL direction: authored `from` is to the *right* of `to`,
    /// and the gap should still be computed correctly along the x-axis.
    #[test]
    fn clamp_rl_forward_uses_rect_positions_not_authored_direction() {
        let (diagram, mut routed) = synthetic(
            Direction::RightLeft,
            FRect::new(110.0, 0.0, 50.0, 30.0), // source (from) on the right
            FRect::new(0.0, 0.0, 50.0, 30.0),   // target (to) on the left
            FRect::new(70.0, 5.0, 25.0, 20.0),  // label in the gap
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::RightLeft,
            &metrics,
            &mut unfits,
        );
        assert!(
            unfits.is_empty(),
            "RL forward edge with authored source to the right of target \
             must not produce a bogus UnfitOverlap; got {unfits:?}"
        );
    }

    /// BT clamp must still slide labels that intrude into a node face.
    /// Pre-clamp label y=85 sits inside the *upper-of-the-pair* node S
    /// (which is at y=100..130 in BT layout). Wait — re-read: S is the
    /// authored `from` and sits *below* T. Pre-clamp rect at y=10 intrudes
    /// into T (the upper node, at y=0..30). Slide should push it down past
    /// T.bottom + spacing.
    #[test]
    fn clamp_bt_label_intruding_upper_node_slides_down() {
        let (diagram, mut routed) = synthetic(
            Direction::BottomTop,
            FRect::new(0.0, 100.0, 50.0, 30.0), // S (from) at bottom
            FRect::new(0.0, 0.0, 50.0, 30.0),   // T (to) at top
            FRect::new(10.0, 10.0, 30.0, 20.0), // intrudes into T
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::BottomTop,
            &metrics,
            &mut unfits,
        );
        assert!(unfits.is_empty(), "got unfits: {unfits:?}");
        let new_rect = routed.edges[0].label_geometry.unwrap().rect;
        // Upper = T (y=0..30), avoidance = arrow_end (Normal) = 8, spacing = 2.
        // Lower = S (y=100..130), avoidance = arrow_start (None) = 0, spacing = 2.
        // lo = upper.bottom + upper_avoid + spacing = 30 + 8 + 2 = 40.
        // hi = lower.top - lower_avoid - spacing = 100 - 0 - 2 = 98.
        // Label height 20 → fits in [40, 78]. Pre-clamp y=10 < 40 → slide to 40.
        assert!(
            (new_rect.y - 40.0).abs() < 1e-6,
            "expected y=40 after slide-in, got {}",
            new_rect.y
        );
    }

    #[test]
    fn unfit_collector_observable_in_all_builds() {
        // Smoke test: the field is just a Vec<UnfitOverlap> — verify it
        // accumulates across multiple unfit edges.
        let (diagram, mut routed) = synthetic(
            Direction::LeftRight,
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(60.0, 0.0, 50.0, 30.0),
            FRect::new(45.0, 5.0, 30.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let mut unfits = Vec::new();
        clamp_label_geometry_to_node_bounds(
            &mut routed.edges,
            &routed.nodes,
            &diagram,
            Direction::LeftRight,
            &metrics,
            &mut unfits,
        );
        assert_eq!(unfits.len(), 1);
        // The collector is just a Vec — no thread-local, no env gating.
        // This is the AC-3 invariant: observable in all builds.
    }
}
