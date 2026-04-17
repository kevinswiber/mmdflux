//! Plan 0149 (#237): lane-aware edge-label re-wrap.
//!
//! # What this module does
//!
//! Runs AFTER `label_lanes::assign_label_tracks` has packed compartments
//! onto signed tracks. Each edge in a multi-member compartment has been
//! assigned a track, a `label_step` budget, and a compartment id. For any
//! edge whose rendered label is wider than the compartment's `label_step`
//! budget (the distance between adjacent lane centerlines), we re-wrap the
//! label at a narrower target width, recompute the label's rect, and
//! store the re-wrapped lines on `RoutedEdgeGeometry::effective_wrapped_lines`
//! so renderers emit text that matches the narrower rect.
//!
//! Re-wrapping a TD label at a narrower width generally INCREASES its
//! axis-projected height (more lines). That height feeds back into the
//! per-compartment `label_step` (max axis extent + LANE_GAP), which feeds
//! back into every member's `label_center = midpoint + track_vector *
//! label_step`. To absorb that cascade we run a bounded fixed-point:
//!
//! 1. Detect overflowing edges (`rect.width > label_step * FIT_THRESHOLD`).
//! 2. Re-wrap them and update their rect dimensions + effective lines.
//! 3. Per affected compartment, recompute `max_axis`, `label_step`.
//! 4. Recompute every compartment member's `label_center` from its
//!    `midpoint` and the fresh `label_step`.
//! 5. Repeat until no edge overflows or `MAX_REWRAP_ITERATIONS` is hit.
//!
//! # Why we chose Option (b) storage (`effective_wrapped_lines`)
//!
//! We deliberately do NOT mutate `diagram.edges[i].wrapped_label_lines`
//! post-routing. `runtime::graph_family::render_graph_family` runs
//! `prepare_wrapped_labels` BEFORE `solve_graph_family`; the kernel's
//! Grid-mode measurement (used by the Text renderer) consumes
//! `wrapped_label_lines` DURING that solve and is already frozen by the
//! time this callback fires. Mutating the source artifact after the fact
//! would leave Grid-measured layer heights out of sync with the text
//! rendered into them. Putting the re-wrap output on `RoutedEdgeGeometry`
//! keeps the override on a type that Grid measurement never reads.
//!
//! # Pipeline interactions to remember before editing
//!
//! - **Kernel dummy heights are frozen.** If re-wrap grows label height
//!   from 52 px to 100 px, the label dummy reserved only 52 px of layer
//!   gap. The extra 48 px overflows into adjacent layer space. Today we
//!   accept the visual imperfection because the overlap-resolution is
//!   more important than layer-gap purity; if that tradeoff ever flips,
//!   a follow-up would plumb grown heights back to the kernel for a
//!   second-pass relayout.
//! - **`recompute_routed_bounds` runs after this.** At `stage.rs` just
//!   below the callback invocation, the routing orchestrator recomputes
//!   the graph bounds from the current rects. Growing label rects grows
//!   the bounds automatically — no explicit bounds update here.
//! - **Self-edges are built after this.** Self-loop geometry never
//!   interacts with compartment packing, so re-wrap has no effect on it.
//! - **Hard line breaks.** `wrap_lines` treats `'\n'` as a hard break.
//!   The pre-engine wrap is derived from `edge.label` (post-
//!   `normalize_br_tags`), so re-wrapping from `edge.label` again
//!   preserves `<br>` semantics. We therefore re-wrap from `edge.label`,
//!   NOT from the previously wrapped lines joined with spaces.
//! - **Idempotence.** On the second full call the narrowed rects are
//!   already within `FIT_THRESHOLD` of `label_step`, so the overflow
//!   detector fires on zero edges and the fixed-point exits in its
//!   first iteration. See `rewrap_is_noop_when_no_label_overflows_*`.

use std::collections::{HashMap, HashSet};

use crate::graph::geometry::RoutedEdgeGeometry;
use crate::graph::measure::{ProportionalTextMetrics, wrap_lines};
use crate::graph::routing::label_lanes::{LANE_GAP, LabelTrackOutcome, MIN_LABEL_LANE_STEP};
use crate::graph::space::FPoint;
use crate::graph::{Direction, Graph};

/// Labels whose axis-projected extent is within this fraction of `label_step`
/// are considered "fitting" and left alone.
///
/// Set to 1.0 so re-wrap only fires when the rect actually exceeds the
/// compartment budget. Loose thresholds (e.g. 0.9) spuriously re-wrap
/// labels that *technically* pass the geometric no-overlap check —
/// `h0/2 + h1/2 <= label_step` for a reciprocal TD pair means rects do
/// not touch, so re-wrapping just for a few pixels of slack produces
/// harshly narrow output without a correctness win.
const FIT_THRESHOLD: f64 = 1.0;

/// Re-wrap target: `label_step * REWRAP_SAFETY`. Overshooting the budget
/// a bit (80 % instead of 100 %) leaves room for marker-avoidance padding
/// and for the label_step itself to grow during subsequent iterations
/// without re-triggering overflow on this edge.
const REWRAP_SAFETY: f64 = 0.8;

/// Floor on the re-wrap target. Narrower than roughly three short words
/// produces unreadable one-letter-per-line output without improving
/// overlap. When `label_step * REWRAP_SAFETY` falls below this the
/// rewrap target is clamped to `MIN_REWRAP_WIDTH`.
const MIN_REWRAP_WIDTH: f64 = 48.0;

/// Cap on fixed-point iterations. In the common case (TD reciprocal
/// pair) the loop converges in 2 rounds. The cap guards against
/// pathological inputs where re-wrap endlessly toggles label_step.
const MAX_REWRAP_ITERATIONS: usize = 3;

/// Run the lane-aware re-wrap fixed-point.
///
/// Mutates affected edges' `label_geometry` rects and sets
/// `effective_wrapped_lines`. Mutates the supplied `lane_outcomes` map
/// in-place so caller-visible `label_step`, `label_center`, and
/// `label_rect` reflect the post-rewrap state — downstream passes
/// (`label_clamp::clamp_label_geometry_to_node_bounds`,
/// `recompute_routed_bounds`) read directly from the routed edges and do
/// not consult `lane_outcomes` again, so the outcome mutation is for
/// diagnostics / testability rather than re-consumption.
///
/// Returns the total number of edges re-wrapped across all iterations.
/// A non-zero return is purely informational; the caller does not gate
/// anything on it today.
pub(super) fn re_wrap_labels_for_lane_fit(
    diagram: &Graph,
    edges: &mut [RoutedEdgeGeometry],
    lane_outcomes: &mut HashMap<usize, LabelTrackOutcome>,
    metrics: &ProportionalTextMetrics,
    direction: Direction,
) -> usize {
    let mut total_rewraps: usize = 0;
    for _iteration in 0..MAX_REWRAP_ITERATIONS {
        // Step 1: detect which edges overflow their compartment's current
        // `label_step`. Skip singletons on track 0 (no compartment siblings
        // to coordinate with, so no lane budget to enforce). We iterate
        // edges (not outcomes) because the fresh rect dims live on the
        // routed edge; outcomes reflect the initial pack only.
        //
        // Compare the **axis-projected** extent (height for TD/BT, width
        // for LR/RL) against `label_step` — the two are measured in the
        // same axis. Comparing the cross-axis dim would be meaningless
        // because labels in a TD compartment share the same X center by
        // construction and their X widths are unconstrained by the lane
        // packer.
        let overflowing: Vec<usize> = edges
            .iter()
            .filter_map(|edge| {
                let outcome = lane_outcomes.get(&edge.index)?;
                if outcome.compartment_size == 1 && outcome.track == 0 {
                    return None;
                }
                let rect = edge.label_geometry.as_ref()?.rect;
                let axis = axis_projected_dim(rect.width, rect.height, direction);
                if axis > outcome.label_step * FIT_THRESHOLD {
                    Some(edge.index)
                } else {
                    None
                }
            })
            .collect();

        if overflowing.is_empty() {
            break;
        }

        // Step 2: re-wrap overflowing edges in place. We set the new rect
        // dimensions centered on the CURRENT `label_center`; step 4 below
        // will replace that center with a fresh midpoint-based one once
        // label_step is recomputed. Keeping the recenter separate avoids
        // the double-adjustment trap where a width change + a center
        // change interact non-linearly.
        let mut touched_compartments: HashSet<usize> = HashSet::new();
        for edge_idx in &overflowing {
            let outcome = match lane_outcomes.get(edge_idx) {
                Some(o) => o,
                None => continue,
            };
            let Some(source_edge) = diagram.edges.get(*edge_idx) else {
                continue;
            };
            // Re-wrap from the raw label text (post `normalize_br_tags`),
            // NOT from the previously wrapped lines. `wrap_lines` treats
            // `'\n'` as a hard break so `<br>` semantics survive; joining
            // wrapped_label_lines back into a single string would lose
            // line-break fidelity for labels that had explicit breaks.
            let Some(label_text) = source_edge.label.as_deref() else {
                continue;
            };
            if label_text.is_empty() {
                continue;
            }

            let target = (outcome.label_step * REWRAP_SAFETY).max(MIN_REWRAP_WIDTH);
            let new_lines = wrap_lines(metrics, label_text, target);
            let (new_w, new_h) = metrics.edge_label_dimensions_wrapped(&new_lines);

            // Skip edges where the re-wrap would not actually narrow the
            // rect. Can happen with single oversized words where the
            // per-character fallback in `wrap_lines` produces a line as
            // wide as the original (or wider thanks to padding math). Not
            // worth burning an iteration on no-ops.
            let current_width = edges
                .iter()
                .find(|e| e.index == *edge_idx)
                .and_then(|e| e.label_geometry.as_ref().map(|g| g.rect.width));
            if let Some(cur) = current_width
                && new_w >= cur
            {
                continue;
            }

            if let Some(edge) = edges.iter_mut().find(|e| e.index == *edge_idx)
                && let Some(geom) = edge.label_geometry.as_mut()
            {
                // Position update here is temporary; Step 4 overwrites it
                // if the compartment's label_step changed.
                geom.rect.width = new_w;
                geom.rect.height = new_h;
                geom.rect.x = geom.center.x - new_w / 2.0;
                geom.rect.y = geom.center.y - new_h / 2.0;
                edge.effective_wrapped_lines = Some(new_lines);
                total_rewraps += 1;
                touched_compartments.insert(outcome.compartment_id);
            }
        }

        if touched_compartments.is_empty() {
            break;
        }

        // Step 3: recompute per-compartment `label_step`. `max_axis` is
        // the largest axis-projected rect dimension across all members;
        // axis = Y for TD/BT, axis = X for LR/RL. Growing height (TD
        // re-wrap) grows label_step; shrinking width (LR re-wrap) shrinks
        // it. Either way the final value replaces the stale one on every
        // affected member's outcome.
        let new_steps =
            recompute_label_steps(edges, lane_outcomes, &touched_compartments, direction);

        // Step 4: apply fresh label_step back to outcomes AND recompute
        // every member's label_center from the preserved midpoint. Update
        // the routed edge's rect position to match.
        //
        // IMPORTANT: compute from the stored midpoint, not from the old
        // label_center. Re-centering off the old center accumulates
        // floating-point drift across iterations, and on each iteration
        // the FIT_THRESHOLD check uses the updated rect, so an accumulated
        // drift could falsely clear the overflow detector even when the
        // true geometry still overflows.
        apply_new_label_steps(edges, lane_outcomes, &new_steps, direction);
    }

    total_rewraps
}

/// Compute fresh `label_step` values for every compartment touched this
/// iteration. Reads the CURRENT routed-edge rect dimensions (post
/// re-wrap) so the returned steps reflect the new axis extents.
fn recompute_label_steps(
    edges: &[RoutedEdgeGeometry],
    lane_outcomes: &HashMap<usize, LabelTrackOutcome>,
    affected: &HashSet<usize>,
    direction: Direction,
) -> HashMap<usize, f64> {
    let mut max_axis_per_compartment: HashMap<usize, f64> = HashMap::new();
    for edge in edges {
        let Some(outcome) = lane_outcomes.get(&edge.index) else {
            continue;
        };
        if !affected.contains(&outcome.compartment_id) {
            continue;
        }
        let Some(rect) = edge.label_geometry.as_ref().map(|g| g.rect) else {
            continue;
        };
        let axis_extent = axis_projected_dim(rect.width, rect.height, direction);
        max_axis_per_compartment
            .entry(outcome.compartment_id)
            .and_modify(|cur| *cur = cur.max(axis_extent))
            .or_insert(axis_extent);
    }
    max_axis_per_compartment
        .into_iter()
        .map(|(id, max_axis)| (id, (max_axis + LANE_GAP).max(MIN_LABEL_LANE_STEP)))
        .collect()
}

/// For every edge in an affected compartment, write the new `label_step`
/// onto its outcome and recompute `label_center` from the preserved
/// `midpoint`. Propagate the new center to the routed edge's rect
/// position; width/height were already set in Step 2 and stay put.
fn apply_new_label_steps(
    edges: &mut [RoutedEdgeGeometry],
    lane_outcomes: &mut HashMap<usize, LabelTrackOutcome>,
    new_steps: &HashMap<usize, f64>,
    direction: Direction,
) {
    for edge in edges.iter_mut() {
        let Some(outcome) = lane_outcomes.get_mut(&edge.index) else {
            continue;
        };
        let Some(&new_step) = new_steps.get(&outcome.compartment_id) else {
            continue;
        };
        outcome.label_step = new_step;

        // track_vector returns the unit vector along the axis-stacking
        // direction. label_center = midpoint + track_vector * track *
        // label_step.
        let new_center = compute_label_center(outcome.midpoint, outcome.track, new_step, direction);
        outcome.label_center = new_center;

        if let Some(geom) = edge.label_geometry.as_mut() {
            geom.center = new_center;
            geom.rect.x = new_center.x - geom.rect.width / 2.0;
            geom.rect.y = new_center.y - geom.rect.height / 2.0;
            // outcome.label_rect is informational at this point; mirror
            // the rect state back onto it for diagnostics and for any
            // test that inspects outcomes directly.
            outcome.label_rect = geom.rect;
        }
        if let Some(lp) = edge.label_position.as_mut() {
            *lp = new_center;
        }
    }
}

/// Axis-projected dimension for a rect in a given flow direction.
/// TD/BT stack labels in Y, so the axis extent is the rect height;
/// LR/RL stack in X, so it is the rect width.
fn axis_projected_dim(width: f64, height: f64, direction: Direction) -> f64 {
    match direction {
        Direction::TopDown | Direction::BottomTop => height,
        Direction::LeftRight | Direction::RightLeft => width,
    }
}

/// Compute a member's `label_center` from its midpoint, track, step, and
/// flow direction. Mirrors `label_lanes::shift_label` — keep the two in
/// sync if either changes. Extracting a separate helper here avoids
/// coupling this module to the crate-private internals of `label_lanes`.
fn compute_label_center(
    midpoint: FPoint,
    track: i32,
    label_step: f64,
    direction: Direction,
) -> FPoint {
    let offset = track as f64 * label_step;
    match direction {
        Direction::TopDown | Direction::BottomTop => FPoint::new(midpoint.x, midpoint.y + offset),
        Direction::LeftRight | Direction::RightLeft => FPoint::new(midpoint.x + offset, midpoint.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide};
    use crate::graph::measure::default_proportional_text_metrics;
    use crate::graph::space::FRect;
    use crate::graph::{Edge, Node};

    // Test helpers: build a minimal graph + routed edge pair + outcomes
    // that can drive the callback without running the full routing
    // pipeline. Keeping this explicit (rather than going through a
    // fixture) makes it obvious which input shape each assertion relies
    // on.

    fn graph_with_labels(labels: &[(&str, &str, &str)]) -> Graph {
        let mut g = Graph::new(Direction::TopDown);
        let mut seen: HashSet<String> = HashSet::new();
        for (from, to, _label) in labels {
            if seen.insert((*from).to_string()) {
                g.add_node(Node::new(*from));
            }
            if seen.insert((*to).to_string()) {
                g.add_node(Node::new(*to));
            }
        }
        for (from, to, label) in labels {
            g.add_edge(Edge::new(*from, *to).with_label(*label));
        }
        g
    }

    fn routed_edge_with_label(
        index: usize,
        from: &str,
        to: &str,
        rect: FRect,
    ) -> RoutedEdgeGeometry {
        RoutedEdgeGeometry {
            index,
            from: from.into(),
            to: to.into(),
            path: vec![],
            label_position: Some(FPoint::new(
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0,
            )),
            label_side: Some(EdgeLabelSide::Center),
            head_label_position: None,
            tail_label_position: None,
            is_backward: false,
            from_subgraph: None,
            to_subgraph: None,
            source_port: None,
            target_port: None,
            preserve_orthogonal_topology: false,
            label_geometry: Some(EdgeLabelGeometry {
                center: FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                rect,
                padding: (4.0, 2.0),
                side: EdgeLabelSide::Center,
                track: 0,
                // Note: the rect passed in assumes padding already included.
            }),
            effective_wrapped_lines: None,
        }
    }

    fn outcome(
        track: i32,
        label_step: f64,
        compartment_id: usize,
        compartment_size: usize,
        midpoint: FPoint,
        rect: FRect,
    ) -> LabelTrackOutcome {
        LabelTrackOutcome {
            label_center: FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
            label_rect: rect,
            track,
            adjusted_path: vec![],
            compartment_size,
            label_step,
            compartment_id,
            midpoint,
        }
    }

    #[test]
    fn rewrap_noop_for_singleton_track_zero_edges() {
        // A label that is wide but in a 1-member compartment on track 0
        // has no one to pack against, so the rewrap must leave it alone.
        let g = graph_with_labels(&[("A", "B", "this is a wide singleton label")]);
        let rect = FRect::new(0.0, 0.0, 250.0, 28.0);
        let mut edges = vec![routed_edge_with_label(0, "A", "B", rect)];
        let mut outcomes = HashMap::new();
        outcomes.insert(0, outcome(0, 32.0, 0, 1, FPoint::new(125.0, 14.0), rect));
        let metrics = default_proportional_text_metrics();

        let n = re_wrap_labels_for_lane_fit(
            &g,
            &mut edges,
            &mut outcomes,
            &metrics,
            Direction::TopDown,
        );
        assert_eq!(n, 0);
        assert!(edges[0].effective_wrapped_lines.is_none());
        assert_eq!(edges[0].label_geometry.as_ref().unwrap().rect.width, 250.0);
    }

    #[test]
    fn rewrap_noop_when_axis_extent_already_fits_budget() {
        // A label whose axis-projected extent is within `label_step *
        // FIT_THRESHOLD` must not be re-wrapped even if it lives in a
        // multi-member compartment. For TD, axis = height.
        let g = graph_with_labels(&[("A", "B", "short"), ("B", "A", "short")]);
        // rect.height=28 vs step=50 → 28 < 50 * 1.0 = 50 → fits.
        let rect = FRect::new(0.0, 0.0, 40.0, 28.0);
        let mut edges = vec![
            routed_edge_with_label(0, "A", "B", rect),
            routed_edge_with_label(1, "B", "A", rect),
        ];
        let mut outcomes = HashMap::new();
        outcomes.insert(0, outcome(0, 50.0, 0, 2, FPoint::new(20.0, 14.0), rect));
        outcomes.insert(1, outcome(-1, 50.0, 0, 2, FPoint::new(20.0, -36.0), rect));
        let metrics = default_proportional_text_metrics();

        let n = re_wrap_labels_for_lane_fit(
            &g,
            &mut edges,
            &mut outcomes,
            &metrics,
            Direction::TopDown,
        );
        assert_eq!(n, 0);
        assert!(edges[0].effective_wrapped_lines.is_none());
        assert!(edges[1].effective_wrapped_lines.is_none());
    }

    #[test]
    fn rewrap_shrinks_overflowing_label_and_records_effective_lines() {
        // A label whose axis-projected extent exceeds the compartment's
        // `label_step` must be re-wrapped and the narrower rect +
        // effective lines must land on the routed edge. TD reciprocal
        // pair with a tall multi-line rect (h=60) against a small step
        // (32) forces overflow.
        let label = "this is a deliberately long label";
        let g = graph_with_labels(&[("A", "B", label), ("B", "A", label)]);
        let rect = FRect::new(0.0, 0.0, 200.0, 60.0);
        let mut edges = vec![
            routed_edge_with_label(0, "A", "B", rect),
            routed_edge_with_label(1, "B", "A", rect),
        ];
        let mut outcomes = HashMap::new();
        outcomes.insert(0, outcome(0, 32.0, 0, 2, FPoint::new(100.0, 30.0), rect));
        outcomes.insert(1, outcome(-1, 32.0, 0, 2, FPoint::new(100.0, -2.0), rect));
        let metrics = default_proportional_text_metrics();

        let n = re_wrap_labels_for_lane_fit(
            &g,
            &mut edges,
            &mut outcomes,
            &metrics,
            Direction::TopDown,
        );
        assert!(n >= 1, "expected at least one rewrap, got {n}");
        let e0 = &edges[0];
        assert!(e0.effective_wrapped_lines.is_some());
        let new_w = e0.label_geometry.as_ref().unwrap().rect.width;
        assert!(
            new_w < 200.0,
            "rewrapped rect width must shrink below original 200: got {new_w}"
        );
    }

    #[test]
    fn rewrap_preserves_track_and_compartment_identity() {
        // After re-wrap, track and compartment_id on the outcome must
        // remain unchanged — only label_step and label_center should
        // have moved.
        let label = "this is a deliberately long label";
        let g = graph_with_labels(&[("A", "B", label), ("B", "A", label)]);
        let rect = FRect::new(0.0, 0.0, 200.0, 60.0);
        let mut edges = vec![
            routed_edge_with_label(0, "A", "B", rect),
            routed_edge_with_label(1, "B", "A", rect),
        ];
        let mut outcomes = HashMap::new();
        outcomes.insert(0, outcome(0, 32.0, 0, 2, FPoint::new(100.0, 30.0), rect));
        outcomes.insert(1, outcome(-1, 32.0, 0, 2, FPoint::new(100.0, -2.0), rect));
        let metrics = default_proportional_text_metrics();

        re_wrap_labels_for_lane_fit(&g, &mut edges, &mut outcomes, &metrics, Direction::TopDown);
        assert_eq!(outcomes[&0].track, 0);
        assert_eq!(outcomes[&1].track, -1);
        assert_eq!(outcomes[&0].compartment_id, 0);
        assert_eq!(outcomes[&1].compartment_id, 0);
    }

    #[test]
    fn rewrap_fixed_point_grows_label_step_for_td_reciprocal_pair() {
        // The reciprocal-pair TD case that motivated Plan 0149. After the
        // first round of re-wrap the labels become taller (more lines),
        // so `label_step` must grow on iteration 2; otherwise the two
        // members would overlap in Y. Check the final label_step > initial.
        let label = "this is a deliberately long reply label";
        let g = graph_with_labels(&[("A", "B", label), ("B", "A", label)]);
        // Tall rect (h=60) vs small step (32) forces TD axis-extent
        // overflow → triggers re-wrap → rewritten axis dim feeds back
        // into recomputed label_step.
        let rect = FRect::new(0.0, 0.0, 200.0, 60.0);
        let mut edges = vec![
            routed_edge_with_label(0, "A", "B", rect),
            routed_edge_with_label(1, "B", "A", rect),
        ];
        let mut outcomes = HashMap::new();
        let initial_step = 32.0;
        outcomes.insert(
            0,
            outcome(0, initial_step, 0, 2, FPoint::new(100.0, 30.0), rect),
        );
        outcomes.insert(
            1,
            outcome(-1, initial_step, 0, 2, FPoint::new(100.0, -2.0), rect),
        );
        let metrics = default_proportional_text_metrics();

        re_wrap_labels_for_lane_fit(&g, &mut edges, &mut outcomes, &metrics, Direction::TopDown);
        assert!(
            outcomes[&0].label_step > initial_step,
            "label_step must grow to accommodate re-wrap height (was {}, now {})",
            initial_step,
            outcomes[&0].label_step
        );
        // Both members see the same fresh step.
        assert_eq!(outcomes[&0].label_step, outcomes[&1].label_step);
    }

    #[test]
    fn rewrap_is_idempotent_when_already_wrapped() {
        // Running the callback a second time on an already-narrowed label
        // set must be a no-op: the narrowed rect already sits inside the
        // (now larger) label_step budget.
        let label = "this is a deliberately long label";
        let g = graph_with_labels(&[("A", "B", label), ("B", "A", label)]);
        let rect = FRect::new(0.0, 0.0, 200.0, 60.0);
        let mut edges = vec![
            routed_edge_with_label(0, "A", "B", rect),
            routed_edge_with_label(1, "B", "A", rect),
        ];
        let mut outcomes = HashMap::new();
        outcomes.insert(0, outcome(0, 32.0, 0, 2, FPoint::new(100.0, 30.0), rect));
        outcomes.insert(1, outcome(-1, 32.0, 0, 2, FPoint::new(100.0, -2.0), rect));
        let metrics = default_proportional_text_metrics();

        let first = re_wrap_labels_for_lane_fit(
            &g,
            &mut edges,
            &mut outcomes,
            &metrics,
            Direction::TopDown,
        );
        assert!(first >= 1);
        let second = re_wrap_labels_for_lane_fit(
            &g,
            &mut edges,
            &mut outcomes,
            &metrics,
            Direction::TopDown,
        );
        assert_eq!(
            second, 0,
            "running rewrap a second time must be a no-op on already-fitted labels"
        );
    }
}
