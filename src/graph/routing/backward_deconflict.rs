//! Backward edge corridor deconfliction.
//!
//! When multiple backward edges share the same corridor space, their
//! independently computed lane positions overlap.  This module groups
//! backward edges into *corridor compartments* (same scope parent,
//! overlapping primary-axis spans) and assigns distinct lane slots
//! within each compartment so corridors never overlap.

use std::collections::HashMap;

use super::orthogonal::backward::{
    has_backward_corridor_obstructions, node_in_scope, shared_parent_subgraph_rect,
};
use super::stage::has_corridor_obstructions;
use crate::graph::Direction;
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::FRect;

/// Shared lane spacing between adjacent backward edge corridors.
pub(crate) const LANE_SPACING: f64 = 8.0;

/// Describes one backward edge's corridor geometry before slot assignment.
#[derive(Debug, Clone)]
struct CorridorDescriptor {
    /// Index into `geometry.edges`.
    edge_index: usize,
    /// Target node id.
    target: String,
    /// Shared parent subgraph of both endpoints (None = top level).
    scope_parent: Option<String>,
    /// Primary-axis lower bound of the corridor (y for TD/BT, x for LR/RL).
    span_min: f64,
    /// Primary-axis upper bound of the corridor.
    span_max: f64,
    /// Independently computed lane position (the current algorithm's result).
    base_lane: f64,
}

/// Pre-computed corridor slot for a single backward edge.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BackwardCorridorSlot {
    /// Shared base lane for the compartment (max of all member base lanes).
    pub(crate) base_lane: f64,
    /// Slot index within the compartment (0, 1, 2, ...).
    pub(crate) slot: usize,
}

/// Per-edge corridor slot assignments for all backward edges that participate
/// in corridor deconfliction (obstructed edges and same-target edges).
#[derive(Debug, Default)]
pub(crate) struct BackwardCorridorContext {
    slots: HashMap<usize, BackwardCorridorSlot>,
}

impl BackwardCorridorContext {
    /// Look up the corridor slot for a given edge index.
    pub(crate) fn slot_for(&self, edge_index: usize) -> Option<&BackwardCorridorSlot> {
        self.slots.get(&edge_index)
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compute corridor deconfliction context for backward edges.
///
/// This is a pre-pass that should run before the per-edge routing loop.
/// For each backward edge that has corridor obstructions, it computes
/// an independent base lane, groups overlapping corridors into
/// compartments, and assigns distinct slot indices within each group.
pub(crate) fn compute_backward_corridor_context(
    geometry: &GraphGeometry,
    direction: Direction,
) -> BackwardCorridorContext {
    if !geometry.enhanced_backward_routing {
        return BackwardCorridorContext::default();
    }

    let descriptors = build_descriptors(geometry, direction);
    if descriptors.is_empty() {
        return BackwardCorridorContext::default();
    }

    let compartments = group_into_compartments(descriptors);
    build_context_from_compartments(&compartments)
}

/// Variant for the orthogonal routing path which uses a different
/// obstruction check.
pub(crate) fn compute_backward_corridor_context_orthogonal(
    geometry: &GraphGeometry,
    direction: Direction,
) -> BackwardCorridorContext {
    if !geometry.enhanced_backward_routing {
        return BackwardCorridorContext::default();
    }

    let descriptors = build_descriptors_orthogonal(geometry, direction);
    if descriptors.is_empty() {
        return BackwardCorridorContext::default();
    }

    let compartments = group_into_compartments(descriptors);
    build_context_from_compartments(&compartments)
}

// ---------------------------------------------------------------------------
// Descriptor construction
// ---------------------------------------------------------------------------

/// Build corridor descriptors for all backward edges (DirectRoute path).
///
/// Includes edges with corridor obstructions *and* edges that share a target
/// with another backward edge (even without obstructions).
fn build_descriptors(geometry: &GraphGeometry, direction: Direction) -> Vec<CorridorDescriptor> {
    let mut backward_target_counts: HashMap<&str, usize> = HashMap::new();
    for edge in &geometry.edges {
        if geometry.reversed_edges.contains(&edge.index) {
            *backward_target_counts.entry(&edge.to).or_default() += 1;
        }
    }

    geometry
        .edges
        .iter()
        .filter(|edge| {
            geometry.reversed_edges.contains(&edge.index)
                && (has_corridor_obstructions(edge, geometry, direction)
                    || backward_target_counts
                        .get(edge.to.as_str())
                        .copied()
                        .unwrap_or(0)
                        >= 2)
        })
        .filter_map(|edge| build_one_descriptor(edge, geometry, direction))
        .collect()
}

/// Build corridor descriptors for all backward edges (OrthogonalRoute path).
///
/// Includes edges with corridor obstructions *and* edges that share a target
/// with another backward edge (even without obstructions), so that same-target
/// edges receive distinct corridor lane slots.
fn build_descriptors_orthogonal(
    geometry: &GraphGeometry,
    direction: Direction,
) -> Vec<CorridorDescriptor> {
    // Pre-scan: find targets with 2+ backward inbound edges.
    let mut backward_target_counts: HashMap<&str, usize> = HashMap::new();
    for edge in &geometry.edges {
        if geometry.reversed_edges.contains(&edge.index) {
            *backward_target_counts.entry(&edge.to).or_default() += 1;
        }
    }

    geometry
        .edges
        .iter()
        .filter(|edge| {
            geometry.reversed_edges.contains(&edge.index)
                && (has_backward_corridor_obstructions(edge, geometry, direction)
                    || backward_target_counts
                        .get(edge.to.as_str())
                        .copied()
                        .unwrap_or(0)
                        >= 2)
        })
        .filter_map(|edge| build_one_descriptor(edge, geometry, direction))
        .collect()
}

/// Compute a single corridor descriptor for one backward edge.
fn build_one_descriptor(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> Option<CorridorDescriptor> {
    let from_node = geometry.nodes.get(&edge.from)?;
    let to_node = geometry.nodes.get(&edge.to)?;
    let sr = from_node.rect;
    let tr = to_node.rect;

    let scope_parent = from_node.parent.as_deref().and_then(|fp| {
        let tp = to_node.parent.as_deref()?;
        if fp == tp { Some(fp.to_string()) } else { None }
    });
    let sg_rect = shared_parent_subgraph_rect(edge, geometry);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let span_min = sr.y.min(tr.y);
            let span_max = (sr.y + sr.height).max(tr.y + tr.height);
            let base_lane = compute_base_lane_td_bt(edge, geometry, &sr, &tr, sg_rect);
            Some(CorridorDescriptor {
                edge_index: edge.index,
                target: edge.to.clone(),
                scope_parent,
                span_min,
                span_max,
                base_lane,
            })
        }
        Direction::LeftRight | Direction::RightLeft => {
            let span_min = sr.x.min(tr.x);
            let span_max = (sr.x + sr.width).max(tr.x + tr.width);
            let base_lane = compute_base_lane_lr_rl(edge, geometry, &sr, &tr, sg_rect);
            Some(CorridorDescriptor {
                edge_index: edge.index,
                target: edge.to.clone(),
                scope_parent,
                span_min,
                span_max,
                base_lane,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Base lane computation (mirrors existing logic in stage.rs / backward.rs)
// ---------------------------------------------------------------------------

const CHANNEL_CLEARANCE: f64 = 12.0;

fn compute_base_lane_td_bt(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    sr: &FRect,
    tr: &FRect,
    sg_rect: Option<FRect>,
) -> f64 {
    let source_right = sr.x + sr.width;
    let target_right = tr.x + tr.width;
    let face_envelope = source_right.max(target_right);
    let min_y = sr.y.min(tr.y);
    let max_y = (sr.y + sr.height).max(tr.y + tr.height);

    let scope_parent = geometry
        .nodes
        .get(&edge.from)
        .and_then(|n| n.parent.as_deref());

    let mut lane = face_envelope + CHANNEL_CLEARANCE;
    for node in geometry.nodes.values() {
        if node.id == edge.from || node.id == edge.to {
            continue;
        }
        if !node_in_scope(&node.id, scope_parent, geometry) {
            continue;
        }
        let cy = node.rect.center_y();
        let node_right = node.rect.x + node.rect.width;
        if cy >= min_y && cy <= max_y {
            lane = lane.max(node_right + CHANNEL_CLEARANCE);
        }
    }
    if let Some(sg) = sg_rect {
        lane = lane.min(sg.x + sg.width - CHANNEL_CLEARANCE);
    }
    lane
}

fn compute_base_lane_lr_rl(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    sr: &FRect,
    tr: &FRect,
    sg_rect: Option<FRect>,
) -> f64 {
    let source_bottom = sr.y + sr.height;
    let target_bottom = tr.y + tr.height;
    let face_envelope = source_bottom.max(target_bottom);
    let min_x = sr.x.min(tr.x);
    let max_x = (sr.x + sr.width).max(tr.x + tr.width);
    let corridor_top = sr.y.min(tr.y);

    let scope_parent = geometry
        .nodes
        .get(&edge.from)
        .and_then(|n| n.parent.as_deref());

    let mut lane = face_envelope + CHANNEL_CLEARANCE;
    for node in geometry.nodes.values() {
        if node.id == edge.from || node.id == edge.to {
            continue;
        }
        if !node_in_scope(&node.id, scope_parent, geometry) {
            continue;
        }
        let cx = node.rect.center_x();
        let node_bottom = node.rect.y + node.rect.height;
        if cx >= min_x && cx <= max_x && node.rect.y < lane && node_bottom > corridor_top {
            lane = lane.max(node_bottom + CHANNEL_CLEARANCE);
        }
    }
    if let Some(sg) = sg_rect {
        lane = lane.min(sg.y + sg.height - CHANNEL_CLEARANCE);
    }
    lane
}

// ---------------------------------------------------------------------------
// Compartment grouping
// ---------------------------------------------------------------------------

/// Group corridor descriptors into compartments.
///
/// Two edges are in the same compartment when they share the same
/// `scope_parent` and their primary-axis spans overlap, **or** when
/// they share the same target node (regardless of span overlap).
fn group_into_compartments(
    mut descriptors: Vec<CorridorDescriptor>,
) -> Vec<Vec<CorridorDescriptor>> {
    if descriptors.is_empty() {
        return Vec::new();
    }

    // Sort by (scope_parent, span_min) for sweep-line merge.
    descriptors.sort_by(|a, b| {
        a.scope_parent
            .cmp(&b.scope_parent)
            .then(a.span_min.partial_cmp(&b.span_min).unwrap())
    });

    let mut compartments: Vec<Vec<CorridorDescriptor>> = Vec::new();
    let mut current_group: Vec<CorridorDescriptor> = vec![descriptors[0].clone()];
    let mut current_span_max = descriptors[0].span_max;

    for desc in descriptors.into_iter().skip(1) {
        let same_scope = current_group.last().unwrap().scope_parent == desc.scope_parent;
        let overlaps = desc.span_min < current_span_max;

        if same_scope && overlaps {
            current_span_max = current_span_max.max(desc.span_max);
            current_group.push(desc);
        } else {
            compartments.push(std::mem::take(&mut current_group));
            current_span_max = desc.span_max;
            current_group.push(desc);
        }
    }
    compartments.push(current_group);

    // Merge compartments that share a target node.  Edges from widely
    // separated sources produce non-overlapping spans but still need
    // distinct corridor lanes when they converge on the same target.
    merge_compartments_by_shared_target(&mut compartments);

    compartments
}

/// Merge any compartments that share at least one target node.
fn merge_compartments_by_shared_target(compartments: &mut Vec<Vec<CorridorDescriptor>>) {
    // Build target → compartment-index mapping; when a target appears in
    // two compartments, merge the later one into the earlier one.
    loop {
        let mut target_to_comp: HashMap<&str, usize> = HashMap::new();
        let mut merge_pair: Option<(usize, usize)> = None;

        for (ci, comp) in compartments.iter().enumerate() {
            for desc in comp {
                if let Some(&earlier) = target_to_comp.get(desc.target.as_str()) {
                    if earlier != ci {
                        merge_pair = Some((earlier, ci));
                        break;
                    }
                } else {
                    target_to_comp.insert(&desc.target, ci);
                }
            }
            if merge_pair.is_some() {
                break;
            }
        }

        if let Some((keep, remove)) = merge_pair {
            let removed = compartments.remove(remove);
            compartments[keep].extend(removed);
        } else {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Slot assignment
// ---------------------------------------------------------------------------

fn build_context_from_compartments(
    compartments: &[Vec<CorridorDescriptor>],
) -> BackwardCorridorContext {
    let mut slots = HashMap::new();

    for compartment in compartments {
        if compartment.len() < 2 {
            // Single-edge compartments don't need deconfliction.
            // Return None from slot_for() so callers use their own
            // independent corridor computation (which may use a
            // different CHANNEL_CLEARANCE per routing mode).
            continue;
        }

        // Compartment base lane: max of all members ensures every edge
        // clears every obstruction in the shared corridor.
        let base_lane = compartment
            .iter()
            .map(|d| d.base_lane)
            .fold(f64::NEG_INFINITY, f64::max);

        // Sort within compartment so shorter-span edges get inner slots
        // (lower index) and longer-span edges get outer slots.  This
        // ensures nested corridors: the inner edge peels off before
        // reaching the outer edge's destination, avoiding crossings.
        let mut sorted: Vec<(usize, f64, usize)> = compartment
            .iter()
            .map(|d| (d.edge_index, d.span_max - d.span_min, d.edge_index))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.2.cmp(&b.2)));

        for (slot, (edge_index, _, _)) in sorted.iter().enumerate() {
            slots.insert(*edge_index, BackwardCorridorSlot { base_lane, slot });
        }
    }

    BackwardCorridorContext { slots }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(
        edge_index: usize,
        scope_parent: Option<&str>,
        span_min: f64,
        span_max: f64,
        base_lane: f64,
    ) -> CorridorDescriptor {
        desc_with_target(
            edge_index,
            &format!("t{edge_index}"),
            scope_parent,
            span_min,
            span_max,
            base_lane,
        )
    }

    fn desc_with_target(
        edge_index: usize,
        target: &str,
        scope_parent: Option<&str>,
        span_min: f64,
        span_max: f64,
        base_lane: f64,
    ) -> CorridorDescriptor {
        CorridorDescriptor {
            edge_index,
            target: target.to_string(),
            scope_parent: scope_parent.map(String::from),
            span_min,
            span_max,
            base_lane,
        }
    }

    // -- compartment grouping -----------------------------------------------

    #[test]
    fn overlapping_spans_same_scope_form_one_compartment() {
        let descriptors = vec![
            desc(0, None, 10.0, 100.0, 200.0),
            desc(1, None, 50.0, 150.0, 195.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 2);
    }

    #[test]
    fn non_overlapping_spans_form_separate_compartments() {
        let descriptors = vec![
            desc(0, None, 10.0, 50.0, 200.0),
            desc(1, None, 100.0, 200.0, 200.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 2);
    }

    #[test]
    fn different_scope_parents_form_separate_compartments() {
        let descriptors = vec![
            desc(0, Some("sg1"), 10.0, 100.0, 200.0),
            desc(1, Some("sg2"), 50.0, 150.0, 200.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 2);
    }

    #[test]
    fn three_edges_chain_overlap_form_one_compartment() {
        // A overlaps B, B overlaps C, but A does not overlap C directly.
        // The sweep merge should chain them into a single compartment.
        let descriptors = vec![
            desc(0, None, 10.0, 60.0, 200.0),
            desc(1, None, 50.0, 110.0, 200.0),
            desc(2, None, 100.0, 160.0, 200.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 3);
    }

    // -- slot assignment ----------------------------------------------------

    #[test]
    fn shorter_span_gets_inner_slot() {
        // Edge 3 has a shorter span (70) than edge 5 (90).
        // Shorter span → inner slot (0), longer span → outer slot (1).
        let compartments = vec![vec![
            desc(5, None, 10.0, 100.0, 200.0), // span = 90
            desc(3, None, 20.0, 90.0, 195.0),  // span = 70
        ]];
        let ctx = build_context_from_compartments(&compartments);

        let slot3 = ctx.slot_for(3).unwrap();
        let slot5 = ctx.slot_for(5).unwrap();
        assert_eq!(slot3.slot, 0); // shorter span → inner
        assert_eq!(slot5.slot, 1); // longer span → outer
    }

    #[test]
    fn compartment_base_lane_is_max_of_members() {
        let compartments = vec![vec![
            desc(0, None, 10.0, 100.0, 200.0),
            desc(1, None, 50.0, 150.0, 220.0),
        ]];
        let ctx = build_context_from_compartments(&compartments);

        let slot0 = ctx.slot_for(0).unwrap();
        let slot1 = ctx.slot_for(1).unwrap();
        assert!((slot0.base_lane - 220.0).abs() < 0.01);
        assert!((slot1.base_lane - 220.0).abs() < 0.01);
    }

    #[test]
    fn single_edge_compartment_returns_none() {
        // Single-edge compartments are not tracked — callers fall back
        // to their own independent corridor computation.
        let compartments = vec![vec![desc(7, None, 10.0, 100.0, 200.0)]];
        let ctx = build_context_from_compartments(&compartments);
        assert!(ctx.slot_for(7).is_none());
    }

    #[test]
    fn edges_without_entry_return_none() {
        let ctx = BackwardCorridorContext::default();
        assert!(ctx.slot_for(42).is_none());
    }

    // -- same-target compartment merging ---------------------------------------

    #[test]
    fn same_target_non_overlapping_spans_merge_into_one_compartment() {
        // Edges 0 and 1 share target "Alpha" but have non-overlapping spans.
        // The same-target merge pass should combine them.
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Alpha", None, 100.0, 200.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 2);
    }

    #[test]
    fn same_target_three_edges_all_merged() {
        // Three edges targeting "Alpha" from widely separated sources.
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Alpha", None, 80.0, 120.0, 205.0),
            desc_with_target(2, "Alpha", None, 200.0, 300.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 3);
    }

    #[test]
    fn different_targets_stay_separate() {
        // Edges targeting different nodes stay in separate compartments.
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Bravo", None, 100.0, 200.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 2);
    }

    #[test]
    fn same_target_merges_across_span_groups() {
        // Edge 0 targets Alpha (span 10-50), edge 1 targets Bravo (span 40-90),
        // edge 2 targets Alpha (span 200-300).
        // Sweep groups: {0, 1} (overlapping spans) and {2}.
        // Same-target merge should combine {0, 1} and {2} because edges 0
        // and 2 share target Alpha.
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Bravo", None, 40.0, 90.0, 205.0),
            desc_with_target(2, "Alpha", None, 200.0, 300.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 3);
    }

    #[test]
    fn same_target_merged_compartment_gets_slots() {
        // Two same-target edges with non-overlapping spans should both
        // receive distinct slots after merging.
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Alpha", None, 100.0, 200.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        let ctx = build_context_from_compartments(&compartments);

        let slot0 = ctx.slot_for(0).expect("edge 0 should have a slot");
        let slot1 = ctx.slot_for(1).expect("edge 1 should have a slot");
        assert_ne!(slot0.slot, slot1.slot);
        // Base lane should be max of 200 and 210.
        assert!((slot0.base_lane - 210.0).abs() < 0.01);
        assert!((slot1.base_lane - 210.0).abs() < 0.01);
    }
}
