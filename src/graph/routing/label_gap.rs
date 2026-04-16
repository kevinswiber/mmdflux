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

/// Resolve the **clamp-side** source/target of a routed edge plus the
/// arrow markers that decorate each side.
///
/// "Visual source" here means the node forming the **upper boundary of
/// the inter-node gap** in TD/LR (the higher-positioned node), and
/// "visual target" the lower boundary. For forward edges this matches
/// authored `from`/`to`. For backward edges the layout puts authored `to`
/// above authored `from` (the layered engine reversed the edge for
/// acyclicity but renders it in the visual `to → from` direction in render
/// space), so the swap reads:
///
/// | is_backward | visual_source | visual_target | source_arrow      | target_arrow      |
/// | ----------- | ------------- | ------------- | ----------------- | ----------------- |
/// | false       | `from`        | `to`          | `arrow_start`     | `arrow_end`       |
/// | true        | `to`          | `from`        | `arrow_end`       | `arrow_start`     |
///
/// The path orientation itself is unchanged (`path[0]` is always at
/// authored `from`, `path[end]` at authored `to`). The swap here is for
/// the **clamp gap calculation** which needs "the upper node + the marker
/// on its bottom face" vs. "the lower node + the marker on its top face".
///
/// Verified against the backward-edge fixture
/// `tests/fixtures/flowchart/backward_label_asymmetric_markers.mmd` and
/// the corpus boundary assertion test.
///
/// Returns `(visual_source_id, visual_target_id, source_arrow, target_arrow)`.
pub(crate) fn resolve_visual_endpoints<'a>(
    edge: &'a RoutedEdgeGeometry,
    diagram_edge: &'a Edge,
) -> (&'a str, &'a str, Arrow, Arrow) {
    if edge.is_backward {
        (
            edge.to.as_str(),
            edge.from.as_str(),
            diagram_edge.arrow_end,
            diagram_edge.arrow_start,
        )
    } else {
        (
            edge.from.as_str(),
            edge.to.as_str(),
            diagram_edge.arrow_start,
            diagram_edge.arrow_end,
        )
    }
}

/// Measure the available label gap along the edge-parallel axis and the
/// label rect's span along the same axis. Returns `None` when the edge has
/// no `label_geometry` or when the source/target node rects can't be
/// resolved.
///
/// The gap formula:
/// ```text
/// gap = visual_target_near_face - visual_source_far_face
///         - source_marker_avoidance - target_marker_avoidance
///         - 2 * edge_label_spacing
/// ```
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

    let (vs_id, vt_id, vs_arrow, vt_arrow) = resolve_visual_endpoints(edge, diagram_edge);
    let vs_rect = routed.nodes.get(vs_id).map(|n| n.rect)?;
    let vt_rect = routed.nodes.get(vt_id).map(|n| n.rect)?;

    let s = edge_label_spacing(metrics);
    let source_avoid = marker_avoidance_distance(vs_arrow);
    let target_avoid = marker_avoidance_distance(vt_arrow);

    let (axis, source_far, target_near, span) = match direction {
        Direction::TopDown => (
            Axis::Y,
            vs_rect.y + vs_rect.height,
            vt_rect.y,
            geom.rect.height,
        ),
        Direction::BottomTop => (
            Axis::Y,
            // Visual source is *above* visual target after the engine swap;
            // resolve_visual_endpoints already handles backward edges, but
            // for BT the visual source is whichever node sits higher in
            // render space, which is the authored `from` for forward edges
            // (BT engine flips y, so source.y is smaller after flip).
            // Same formula as TD because resolve_visual_endpoints + the
            // engine's BT flip together leave us with vs above vt.
            vs_rect.y + vs_rect.height,
            vt_rect.y,
            geom.rect.height,
        ),
        Direction::LeftRight => (
            Axis::X,
            vs_rect.x + vs_rect.width,
            vt_rect.x,
            geom.rect.width,
        ),
        Direction::RightLeft => (
            Axis::X,
            vs_rect.x + vs_rect.width,
            vt_rect.x,
            geom.rect.width,
        ),
    };

    let gap = target_near - source_far - source_avoid - target_avoid - 2.0 * s;

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
            }),
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
    fn backward_edge_swaps_clamp_endpoints_and_arrows() {
        // Per resolve_visual_endpoints docs: backward edges have authored
        // `to` rendered above authored `from`, so the clamp's "upper" node
        // (= clamp source) is `to` and "lower" (= clamp target) is `from`.
        let (diagram, routed) = synthetic_routed_td(
            FRect::new(0.0, 0.0, 50.0, 30.0),
            FRect::new(0.0, 100.0, 50.0, 30.0),
            FRect::new(10.0, 50.0, 30.0, 20.0),
            /* is_backward */ true,
        );
        let (vs, vt, sa, ta) = resolve_visual_endpoints(&routed.edges[0], &diagram.edges[0]);
        assert_eq!(vs, "T", "backward swaps to=upper=clamp-source");
        assert_eq!(vt, "S", "backward swaps from=lower=clamp-target");
        assert_eq!(sa, Arrow::Normal, "arrow_end becomes upper-side marker");
        assert_eq!(ta, Arrow::None, "arrow_start becomes lower-side marker");
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
