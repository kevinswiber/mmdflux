//! Edge-parallel gap measurement and visual source/target resolution for
//! the routing label clamp pass and its tests.
//!
//! `compute_label_gap_and_span` returns the available gap between a routed
//! edge's source and target node faces (along the edge-parallel axis) and
//! the label rect's span along the same axis. Both the Task 2.1 clamp and
//! Task 1.4's Red-phase test consume this — single source of truth so the
//! two cannot drift.
//!
//! `resolve_visual_endpoints` handles the `is_backward` flip: after the
//! acyclic phase reverses a backward edge, the path's first point sits at
//! the **visual** source (= the authored `to`) and the last at the visual
//! target (= the authored `from`). Marker arrows are swapped to match.

// Consumers land in Task 1.1 (assertion test), Task 1.4 (unfit-gap Red test),
// and Task 2.1 (clamp pass) of plan 0146. Until those land, these helpers
// are exercised only by the unit tests in this module.
#![allow(dead_code)]

use crate::graph::edge::{Arrow, Edge};
use crate::graph::edge_marker::marker_avoidance_distance;
use crate::graph::geometry::{RoutedEdgeGeometry, RoutedGraphGeometry};
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::{Direction, Graph};

/// Edge-parallel axis along which the gap is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Gap is measured along the y-axis (TD/BT diagrams).
    Y,
    /// Gap is measured along the x-axis (LR/RL diagrams).
    X,
}

/// One edge's gap-vs-span measurement along the edge-parallel axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LabelGapMeasurement {
    /// Available gap between the visual source's far face and the visual
    /// target's near face, **after** subtracting source/target marker
    /// avoidance distances and `2 × edge_label_spacing` for breathing room.
    /// `gap < 0` means the markers and spacing already exceed the inter-node
    /// distance — extreme density.
    pub gap: f64,
    /// Label rect dimension along the edge-parallel axis.
    /// (Height for TD/BT; width for LR/RL.)
    pub span: f64,
    /// Which axis the measurement is taken along.
    pub axis: Axis,
}

/// Resolve the authored endpoints + path-anchored markers of a routed edge.
///
/// `path[0]` always sits at authored `from` (with `arrow_start` as its
/// marker), `path[end]` at authored `to` (with `arrow_end`). This holds
/// for forward and backward edges in every diagram direction.
///
/// Kept as a thin helper so call sites (`compute_label_gap_and_span`,
/// `clamp_label_geometry_to_node_bounds`, the corpus assertion test)
/// share one canonical mapping; gap-direction logic (which node is
/// upper/lower, etc.) is handled separately from rect positions, not via
/// any swap here.
///
/// Returns `(from_id, to_id, arrow_start, arrow_end)`.
pub(crate) fn resolve_visual_endpoints<'a>(
    edge: &'a RoutedEdgeGeometry,
    diagram_edge: &'a Edge,
) -> (&'a str, &'a str, Arrow, Arrow) {
    let _ = edge.is_backward;
    (
        edge.from.as_str(),
        edge.to.as_str(),
        diagram_edge.arrow_start,
        diagram_edge.arrow_end,
    )
}

/// Measure the available label gap along the edge-parallel axis and the
/// label rect's span along the same axis. Returns `None` when the edge has
/// no `label_geometry` or when the source/target node rects can't be
/// resolved.
///
/// The gap formula uses **rect positions** (not authored direction) to pick
/// the upper/lower (TD/BT) or left/right (LR/RL) node, then:
///
/// ```text
/// gap = lower.near_face - upper.far_face
///         - upper_marker_avoidance - lower_marker_avoidance
///         - 2 * edge_label_spacing
/// ```
///
/// Using rect positions means the formula is identical for forward and
/// backward edges, and for TD vs BT (or LR vs RL) — what matters is which
/// node is physically above/left, not which is the authored source. The
/// markers follow path orientation: `arrow_start` lives at `path[0]` (=
/// authored `from`), `arrow_end` at `path[end]` (= authored `to`).
///
/// `gap < span` means the label cannot fit in the available space — the
/// case the clamp records as `UnfitOverlap`. The Task 1.4 Red test calls
/// this directly to prove the engineered fixture is genuinely unfit before
/// any clamp exists.
pub(crate) fn compute_label_gap_and_span(
    edge: &RoutedEdgeGeometry,
    routed: &RoutedGraphGeometry,
    diagram: &Graph,
    direction: Direction,
    metrics: &ProportionalTextMetrics,
) -> Option<LabelGapMeasurement> {
    let geom = edge.label_geometry.as_ref()?;
    let diagram_edge = diagram.edges.get(edge.index)?;

    let from_rect = routed.nodes.get(edge.from.as_str()).map(|n| n.rect)?;
    let to_rect = routed.nodes.get(edge.to.as_str()).map(|n| n.rect)?;

    let s = edge_label_spacing(metrics);
    let from_avoid = marker_avoidance_distance(diagram_edge.arrow_start);
    let to_avoid = marker_avoidance_distance(diagram_edge.arrow_end);

    let (axis, gap, span) = match direction {
        Direction::TopDown | Direction::BottomTop => {
            let (upper, upper_avoid, lower, lower_avoid) = if from_rect.y <= to_rect.y {
                (from_rect, from_avoid, to_rect, to_avoid)
            } else {
                (to_rect, to_avoid, from_rect, from_avoid)
            };
            let gap = lower.y - (upper.y + upper.height) - upper_avoid - lower_avoid - 2.0 * s;
            (Axis::Y, gap, geom.rect.height)
        }
        Direction::LeftRight | Direction::RightLeft => {
            let (left, left_avoid, right, right_avoid) = if from_rect.x <= to_rect.x {
                (from_rect, from_avoid, to_rect, to_avoid)
            } else {
                (to_rect, to_avoid, from_rect, from_avoid)
            };
            let gap = right.x - (left.x + left.width) - left_avoid - right_avoid - 2.0 * s;
            (Axis::X, gap, geom.rect.width)
        }
    };

    Some(LabelGapMeasurement { gap, span, axis })
}

/// Pick the spacing constant used between the label rect and the marker
/// avoidance zone. We reuse `label_padding_y` as a proxy for "margin past
/// the marker bbox" — it's the same scale (~2 px) and is already part of
/// the metrics struct, so we don't need a separate constant.
fn edge_label_spacing(metrics: &ProportionalTextMetrics) -> f64 {
    metrics.label_padding_y
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::graph::geometry::{
        EdgeLabelGeometry, EdgeLabelSide, FPoint, FRect, PositionedNode, RoutedGraphGeometry,
    };
    use crate::graph::measure::default_proportional_text_metrics;
    use crate::graph::{Edge, Graph, Node};

    fn synthetic_routed_td(
        source_rect: FRect,
        target_rect: FRect,
        label_rect: FRect,
        is_backward: bool,
    ) -> (Graph, RoutedGraphGeometry) {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("S"));
        diagram.add_node(Node::new("T"));
        diagram.add_edge(Edge::new("S", "T").with_label("lbl"));

        let mut nodes = HashMap::new();
        nodes.insert(
            "S".into(),
            PositionedNode {
                id: "S".into(),
                rect: source_rect,
                shape: crate::graph::Shape::Rectangle,
                label: "S".into(),
                parent: None,
            },
        );
        nodes.insert(
            "T".into(),
            PositionedNode {
                id: "T".into(),
                rect: target_rect,
                shape: crate::graph::Shape::Rectangle,
                label: "T".into(),
                parent: None,
            },
        );

        let edge = RoutedEdgeGeometry {
            index: 0,
            from: "S".into(),
            to: "T".into(),
            path: vec![
                FPoint::new(source_rect.center_x(), source_rect.y + source_rect.height),
                FPoint::new(target_rect.center_x(), target_rect.y),
            ],
            label_position: Some(FPoint::new(
                label_rect.x + label_rect.width / 2.0,
                label_rect.y + label_rect.height / 2.0,
            )),
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
                center: FPoint::new(
                    label_rect.x + label_rect.width / 2.0,
                    label_rect.y + label_rect.height / 2.0,
                ),
                rect: label_rect,
                padding: (4.0, 2.0),
                side: EdgeLabelSide::Above,
                track: 0,
                compartment_size: 1,
            }),
            effective_wrapped_lines: None,
        };

        let routed = RoutedGraphGeometry {
            nodes,
            edges: vec![edge],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: Direction::TopDown,
            bounds: FRect::new(0.0, 0.0, 200.0, 200.0),
            unfit_label_overlaps: Vec::new(),
        };

        (diagram, routed)
    }

    #[test]
    fn forward_edge_resolves_to_authored_endpoints() {
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 50.0, 30.0, 20.0),
            /* is_backward */ false,
        );
        let (vs, vt, sa, ta) = resolve_visual_endpoints(&routed.edges[0], &diagram.edges[0]);
        assert_eq!(vs, "S");
        assert_eq!(vt, "T");
        assert_eq!(sa, Arrow::None); // Edge::new() default tail
        assert_eq!(ta, Arrow::Normal); // default arrow_end
    }

    #[test]
    fn resolve_visual_endpoints_is_identity_regardless_of_backward_or_direction() {
        // After the GPT-5.4 review fix: gap-direction logic moved into
        // compute_label_gap_and_span (via rect positions), so this resolver
        // is now identity for forward and backward alike. The path itself
        // always runs from authored `from` to authored `to`, with
        // `arrow_start` at path[0] and `arrow_end` at path[end].
        for is_backward in [false, true] {
            let (diagram, routed) = synthetic_routed_td(
                FRect::new(0.0, 0.0, 50.0, 30.0),
                FRect::new(0.0, 100.0, 50.0, 30.0),
                FRect::new(10.0, 50.0, 30.0, 20.0),
                is_backward,
            );
            let (vs, vt, sa, ta) = resolve_visual_endpoints(&routed.edges[0], &diagram.edges[0]);
            assert_eq!(vs, "S", "is_backward={is_backward}: from preserved");
            assert_eq!(vt, "T", "is_backward={is_backward}: to preserved");
            assert_eq!(sa, Arrow::None, "arrow_start preserved");
            assert_eq!(ta, Arrow::Normal, "arrow_end preserved");
        }
    }

    /// GPT-5.4 review regression: BT direction with the authored source
    /// rendered BELOW the authored target must still produce a sensible
    /// (positive) gap. Before the fix, this used `source.y + height` and
    /// `target.y` directly, which gave gap = `target.y - source.bottom` =
    /// negative number (since target is above source in BT layout).
    #[test]
    fn bt_gap_uses_rect_positions_not_authored_direction() {
        // S (authored from) at y=100..130 (bottom). T (authored to) at
        // y=0..30 (top). Label rect at y=50..70 (in the gap).
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(10.0, 50.0, 30.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let m = compute_label_gap_and_span(
            &routed.edges[0],
            &routed,
            &diagram,
            Direction::BottomTop,
            &metrics,
        )
        .expect("measurement should be Some");
        assert_eq!(m.axis, Axis::Y);
        // upper = T (y=0..30), upper_avoid = arrow_end (Normal) = 8
        // lower = S (y=100..130), lower_avoid = arrow_start (None) = 0
        // gap = 100 - 30 - 8 - 0 - 2*2 = 58
        assert!(
            (m.gap - 58.0).abs() < 0.01,
            "BT gap: expected 58, got {}",
            m.gap
        );
        assert!(m.gap > 0.0, "BT gap must be positive for healthy edges");
    }

    /// Same regression, RL direction.
    #[test]
    fn rl_gap_uses_rect_positions_not_authored_direction() {
        // S (authored from) at x=100..150 (right). T (authored to) at
        // x=0..50 (left). Label rect at x=60..85 (in the gap), height
        // doesn't matter for x-axis.
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(100.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(60.0, 5.0, 25.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let m = compute_label_gap_and_span(
            &routed.edges[0],
            &routed,
            &diagram,
            Direction::RightLeft,
            &metrics,
        )
        .expect("measurement should be Some");
        assert_eq!(m.axis, Axis::X);
        // left = T (x=0..50), left_avoid = arrow_end (Normal) = 8
        // right = S (x=100..150), right_avoid = arrow_start (None) = 0
        // gap = 100 - 50 - 8 - 0 - 2*2 = 38
        assert!(
            (m.gap - 38.0).abs() < 0.01,
            "RL gap: expected 38, got {}",
            m.gap
        );
        assert!(m.gap > 0.0, "RL gap must be positive for healthy edges");
    }

    #[test]
    fn td_gap_and_span_with_no_markers() {
        // Source 0..30, target 100..130. Gap = 100 - 30 = 70.
        // No markers (default Arrow::None at start, Arrow::Normal at end).
        // source_avoid = 0, target_avoid = 8 (Normal).
        // spacing = label_padding_y = 2.
        // gap = 70 - 0 - 8 - 2*2 = 58.
        // span = label rect height = 20.
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 50.0, 30.0, 20.0),
            false,
        );
        let metrics = default_proportional_text_metrics();
        let m = compute_label_gap_and_span(
            &routed.edges[0],
            &routed,
            &diagram,
            Direction::TopDown,
            &metrics,
        )
        .expect("measurement should be Some");

        assert_eq!(m.axis, Axis::Y);
        assert!(
            (m.gap - 58.0).abs() < 0.01,
            "expected gap=58, got {}",
            m.gap
        );
        assert_eq!(m.span, 20.0);
    }

    #[test]
    fn unfit_when_label_taller_than_gap() {
        // Tiny gap, tall label.
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 50.0, 50.0, 30.0),  // gap of only 20
            FRect::new(10.0, 30.0, 30.0, 50.0), // 50 px tall label
            false,
        );
        let metrics = default_proportional_text_metrics();
        let m = compute_label_gap_and_span(
            &routed.edges[0],
            &routed,
            &diagram,
            Direction::TopDown,
            &metrics,
        )
        .expect("measurement should be Some");
        // gap = 20 - 0 - 8 - 4 = 8; span = 50; 8 < 50 ⇒ unfit
        assert!(
            m.gap < m.span,
            "expected unfit (gap={} < span={})",
            m.gap,
            m.span
        );
    }

    #[test]
    fn no_label_geometry_returns_none() {
        let (diagram, mut routed) = synthetic_routed_td(
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 50.0, 30.0, 20.0),
            false,
        );
        routed.edges[0].label_geometry = None;
        let metrics = default_proportional_text_metrics();
        assert!(
            compute_label_gap_and_span(
                &routed.edges[0],
                &routed,
                &diagram,
                Direction::TopDown,
                &metrics,
            )
            .is_none()
        );
    }
}
