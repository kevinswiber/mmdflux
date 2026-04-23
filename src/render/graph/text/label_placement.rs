//! Render-time corridor-aware label placer (Plan 0153).
//!
//! The wrapper lives on the render side so it can reach for render-owned
//! helpers (`effective_edge_label`, `label_block`, `clamp_label_x`,
//! `label_top_for_center`, `calc_label_position`) without inverting the
//! `render → graph` dependency. Graph owns only the primitive footprint
//! operations (`segments_to_footprint`, `choose_corridor_aware_anchor`,
//! seeding helpers, cell-claim) — see `src/graph/grid/label_placement.rs`.
//!
//! PR #A activates `RenderTimePlacementScope::AuthoritativeOnly` from the
//! authoritative branch at `edge::render_all_edges_with_labels` (task 1.6);
//! PR #B flips every body-label site to `AllBodyLabels` and collapses the
//! multi-branch body.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

use super::label_util::{
    calc_label_position, clamp_label_x, effective_edge_label, label_block, label_top_for_center,
};
use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide, RoutedGraphGeometry};
use crate::graph::grid::label_placement::{
    PathFootprint, choose_corridor_aware_anchor, claim_label_cells_into, extend_segments_into,
    label_rect_overlaps_nodes, seed_node_cells_into, seed_subgraph_borders_into,
};
use crate::graph::grid::{GridLayout, RoutedEdge, Segment};
use crate::graph::{Edge, Stroke};

const OFF_CORRIDOR_DRIFT_THRESHOLD: f64 = 3.0;

/// Which edges the render-time placer is allowed to own.
///
/// Used to stage the migration from the derive-time placer to the render-time
/// placer. PR #A uses `AuthoritativeOnly`; PR #B flips to `AllBodyLabels`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderTimePlacementScope {
    /// Only edges the label-lane pass coordinated (track != 0 or
    /// compartment_size > 1). Matches Plan 0152 Phase 3's
    /// `authoritative_label_positions` subset.
    AuthoritativeOnly,
    /// Every body-label edge. Phase 4 head/tail labels are still excluded.
    AllBodyLabels,
}

/// A finalized render-time label placement for a single edge.
///
/// `center` is in grid coordinates. `label_dims` reflects the resolved
/// `label_block(&effective_label)` dimensions, so callers can compute the
/// top-left draw cell via `(center.0 - label_dims.0 / 2,
/// label_top_for_center(center.1, label_dims.1))`.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // fields read in task 1.6 when placer is wired into edge.rs
pub(crate) struct RenderTimePlacement {
    pub center: (usize, usize),
    pub side: EdgeLabelSide,
    pub is_backward: bool,
    pub label_dims: (usize, usize),
}

/// Tracks a label that has already been placed in this pass. Used by the
/// sibling-shift search to steer subsequent labels off already-claimed cells.
#[derive(Debug, Clone, Copy)]
struct ClaimedLabel {
    top_left_x: usize,
    top_left_y: usize,
    width: usize,
    height: usize,
}

impl ClaimedLabel {
    fn from_center(center: (usize, usize), dims: (usize, usize)) -> Self {
        let (w, h) = (dims.0.max(1), dims.1.max(1));
        Self {
            top_left_x: center.0.saturating_sub(w / 2),
            top_left_y: center.1.saturating_sub(h / 2),
            width: w,
            height: h,
        }
    }

    fn overlaps(&self, top_left: (usize, usize), dims: (usize, usize)) -> bool {
        let (w, h) = (dims.0.max(1), dims.1.max(1));
        let self_end_x = self.top_left_x.saturating_add(self.width).saturating_add(1);
        let self_start_x = self.top_left_x.saturating_sub(1);
        let self_end_y = self.top_left_y.saturating_add(self.height);
        let other_end_x = top_left.0.saturating_add(w).saturating_add(1);
        let other_start_x = top_left.0.saturating_sub(1);
        let other_end_y = top_left.1.saturating_add(h);
        other_start_x < self_end_x
            && self_start_x < other_end_x
            && top_left.1 < self_end_y
            && self.top_left_y < other_end_y
    }
}

/// Compute label placements for every routed edge in `scope` that has a body
/// label, using the Pass-3 segment footprint as the source of truth.
///
/// Final placement order per edge:
///
/// 1. Build the candidate cell: project `EdgeLabelGeometry.center` via
///    `layout.project_layout_point` when available; fall back to
///    `calc_label_position(&segments)` Pass-3 midpoint otherwise.
/// 2. Steer with `choose_corridor_aware_anchor` against the global footprint.
/// 3. If the resulting anchor still overlaps a node, retry from the Pass-3
///    midpoint (never from the stale projected candidate — Q4's lesson).
/// 4. Sibling-label shift against previously claimed labels.
/// 5. Containment clamp via `clamp_label_x` using `edge_containment`.
/// 6. Claim the final cells back into the footprint so the next edge's
///    placer sees them as Terminal obstacles.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_label_placements(
    routed_edges: &[RoutedEdge],
    routed_geometry: Option<&RoutedGraphGeometry>,
    layout: &GridLayout,
    edge_containment: &HashMap<usize, (usize, usize)>,
    canvas_width: usize,
    canvas_height: usize,
    scope: RenderTimePlacementScope,
) -> HashMap<usize, RenderTimePlacement> {
    let mut placements: HashMap<usize, RenderTimePlacement> = HashMap::new();
    let mut claimed: Vec<ClaimedLabel> = Vec::new();

    let mut footprint = PathFootprint::default();
    seed_subgraph_borders_into(&mut footprint, layout);
    seed_node_cells_into(&mut footprint, layout);
    for routed in routed_edges {
        if routed.edge.stroke == Stroke::Invisible {
            continue;
        }
        extend_segments_into(&routed.segments, &mut footprint);
    }

    for routed in routed_edges {
        if routed.edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(effective) = effective_edge_label(&routed.edge) else {
            continue;
        };
        let label = effective.as_ref();
        let block = label_block(label);
        if block.width == 0 || block.height == 0 {
            continue;
        }
        let label_dims = (block.width, block.height);

        let geometry = routed_geometry.and_then(|rg| edge_label_geometry(rg, &routed.edge));
        let is_authoritative = geometry.is_some_and(is_authoritative_geometry);
        if matches!(scope, RenderTimePlacementScope::AuthoritativeOnly) && !is_authoritative {
            continue;
        }

        let midpoint = calc_label_position(&routed.segments).map(|p| (p.x, p.y));
        let projected = geometry.map(|g| layout.project_layout_point(g.center.x, g.center.y));
        // Some forward edges carry a projected `EdgeLabelGeometry.center`
        // shifted well away from the Pass-3 corridor. If that shifted cell is
        // safe, `choose_corridor_aware_anchor` returns it unchanged and never
        // searches back toward the drawn path. Bypass projection only when the
        // projected cell is materially off-corridor and the Pass-3 midpoint is
        // on-corridor. This keeps ordinary lane-coordinated labels stable while
        // covering the singleton F2 cases and observed two-label forward
        // cases that share the same off-corridor projection shape. See
        // research 0069 Q3 addendum + Q4 F2.
        let projected =
            if should_prefer_midpoint_for_forward_edge(routed, geometry, projected, midpoint) {
                None
            } else {
                projected
            };
        let Some(candidate_center) = projected.or(midpoint) else {
            continue;
        };

        let side = geometry.map(|g| g.side).unwrap_or(EdgeLabelSide::Center);

        let corridor_anchor = choose_corridor_aware_anchor(
            candidate_center,
            side,
            &footprint,
            canvas_width,
            canvas_height,
            label_dims.0,
            label_dims.1,
        );

        // Node-overlap recovery: retry from Pass-3 midpoint, not the stale
        // projected candidate. Q4's `complex.mmd` idx=5 case is the trace this
        // guards against — projecting `label_geometry.center` can land inside
        // a node even when the Pass-3 midpoint is safely on the drawn path.
        //
        // `choose_corridor_aware_anchor` returns the original candidate when
        // it cannot find a safe shift; a retry can therefore still yield a
        // node-overlapping cell. Re-check the retry result and skip the edge
        // outright if even the Pass-3-midpoint retry is unsafe. Emitting a
        // label that overlaps a node glyph is strictly worse than letting the
        // legacy fallback branch handle the edge.
        let node_safe_center =
            if label_rect_overlaps_nodes(corridor_anchor, label_dims, &layout.node_bounds) {
                let Some(m) = midpoint else { continue };
                let retry = choose_corridor_aware_anchor(
                    m,
                    side,
                    &footprint,
                    canvas_width,
                    canvas_height,
                    label_dims.0,
                    label_dims.1,
                );
                if label_rect_overlaps_nodes(retry, label_dims, &layout.node_bounds) {
                    continue;
                }
                retry
            } else {
                corridor_anchor
            };

        let shifted_center = shift_against_claimed_labels(
            node_safe_center,
            label_dims,
            &claimed,
            canvas_width,
            canvas_height,
        );

        let bounds = edge_containment.get(&routed.edge.index).copied();
        let base_x = shifted_center.0.saturating_sub(label_dims.0 / 2);
        let base_y = label_top_for_center(shifted_center.1, label_dims.1);
        let final_x = clamp_label_x(base_x, label_dims.0, bounds);
        let final_center = (
            final_x.saturating_add(label_dims.0 / 2),
            label_center_from_top(base_y, label_dims.1),
        );

        claim_label_cells_into(final_center, label_dims, &mut footprint);
        claimed.push(ClaimedLabel::from_center(final_center, label_dims));

        placements.insert(
            routed.edge.index,
            RenderTimePlacement {
                center: final_center,
                side,
                is_backward: routed.is_backward,
                label_dims,
            },
        );
    }

    placements
}

/// Resolve the `EdgeLabelGeometry` for a given edge from
/// `RoutedGraphGeometry`. `RoutedEdgeGeometry.index` carries the canonical
/// `Graph::edges` index, so match on that — `(from, to)` would alias same-
/// endpoint parallel edges onto the first match and break fixtures like
/// `three_parallel_labels.mmd`.
fn edge_label_geometry<'rg>(
    routed: &'rg RoutedGraphGeometry,
    edge: &Edge,
) -> Option<&'rg EdgeLabelGeometry> {
    routed
        .edges
        .iter()
        .find(|e| e.index == edge.index)
        .and_then(|e| e.label_geometry.as_ref())
}

/// Mirrors Plan 0152 Phase 3's authoritative gate: an edge is authoritative
/// when the lane pass coordinated its label track or the compartment size is
/// greater than 1.
fn is_authoritative_geometry(geometry: &EdgeLabelGeometry) -> bool {
    geometry.track != 0 || geometry.compartment_size > 1
}

fn should_prefer_midpoint_for_forward_edge(
    routed: &RoutedEdge,
    geometry: Option<&EdgeLabelGeometry>,
    projected: Option<(usize, usize)>,
    midpoint: Option<(usize, usize)>,
) -> bool {
    if routed.is_backward {
        return false;
    }
    let Some(geometry) = geometry else {
        return false;
    };

    let (Some(projected), Some(midpoint)) = (projected, midpoint) else {
        return false;
    };
    // Research 0069 Q2 used >3 cells as the bucket-(a) off-corridor
    // boundary. Keeping that same boundary makes this a measured bad-
    // projection repair, not a general rewrite of coordinated label policy.
    if distance_to_segments(projected, &routed.segments) <= OFF_CORRIDOR_DRIFT_THRESHOLD
        || distance_to_segments(midpoint, &routed.segments) > OFF_CORRIDOR_DRIFT_THRESHOLD
    {
        return false;
    }

    !is_authoritative_geometry(geometry) || (geometry.compartment_size == 2 && geometry.track != 0)
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

fn label_center_from_top(top_y: usize, height: usize) -> usize {
    top_y + height / 2
}

fn shift_against_claimed_labels(
    center: (usize, usize),
    dims: (usize, usize),
    claimed: &[ClaimedLabel],
    canvas_width: usize,
    canvas_height: usize,
) -> (usize, usize) {
    let base_x = center.0.saturating_sub(dims.0 / 2);
    let base_y = center.1.saturating_sub(dims.1 / 2);
    if !claimed_overlaps(claimed, (base_x, base_y), dims) {
        return center;
    }
    const SHIFTS: &[(isize, isize)] = &[
        (0, -1),
        (0, 1),
        (0, -2),
        (0, 2),
        (-1, 0),
        (1, 0),
        (-2, 0),
        (2, 0),
        (0, -3),
        (0, 3),
        (-3, 0),
        (3, 0),
    ];
    for (dx, dy) in SHIFTS {
        let Some(new_x) = offset_clamped(center.0, *dx, canvas_width) else {
            continue;
        };
        let Some(new_y) = offset_clamped(center.1, *dy, canvas_height) else {
            continue;
        };
        let nbx = new_x.saturating_sub(dims.0 / 2);
        let nby = new_y.saturating_sub(dims.1 / 2);
        if !claimed_overlaps(claimed, (nbx, nby), dims) {
            return (new_x, new_y);
        }
    }
    center
}

fn claimed_overlaps(
    claimed: &[ClaimedLabel],
    top_left: (usize, usize),
    dims: (usize, usize),
) -> bool {
    claimed.iter().any(|c| c.overlaps(top_left, dims))
}

fn offset_clamped(value: usize, delta: isize, max_exclusive: usize) -> Option<usize> {
    let signed = value as isize + delta;
    if signed < 0 {
        return None;
    }
    let unsigned = signed as usize;
    if unsigned >= max_exclusive {
        return None;
    }
    Some(unsigned)
}
