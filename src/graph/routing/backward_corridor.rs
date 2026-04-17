//! Shared backward edge corridor policy and lane-slot assignment.
//!
//! This module owns the shared backward-corridor concept used by both the
//! direct/polyline routing path and the orthogonal routing path. It decides
//! whether an edge participates in corridor routing, computes scope-aware base
//! lanes, groups overlapping corridors into shared compartments, and assigns
//! stable per-edge lane slots.

use std::collections::HashMap;

use crate::graph::Direction;
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::FRect;

/// Shared lane spacing between adjacent backward edge corridors.
pub(in crate::graph::routing) const LANE_SPACING: f64 = 8.0;

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
pub(in crate::graph::routing) struct BackwardCorridorSlot {
    /// Shared base lane for the compartment (max of all member base lanes).
    pub(crate) base_lane: f64,
    /// Slot index within the compartment (0, 1, 2, ...).
    pub(crate) slot: usize,
}

/// Per-edge corridor slot assignments for all backward edges that participate
/// in corridor deconfliction (obstructed edges and same-target edges).
#[derive(Debug, Default)]
pub(in crate::graph::routing) struct BackwardCorridorContext {
    slots: HashMap<usize, BackwardCorridorSlot>,
}

impl BackwardCorridorContext {
    /// Look up the corridor slot for a given edge index.
    pub(crate) fn slot_for(&self, edge_index: usize) -> Option<&BackwardCorridorSlot> {
        self.slots.get(&edge_index)
    }
}

/// Compute shared corridor context for the direct/polyline routing path.
pub(in crate::graph::routing) fn compute_direct_backward_corridor_context(
    geometry: &GraphGeometry,
    direction: Direction,
) -> BackwardCorridorContext {
    compute_backward_corridor_context(geometry, direction, has_direct_corridor_obstructions)
}

/// Compute shared corridor context for the orthogonal routing path.
pub(in crate::graph::routing) fn compute_orthogonal_backward_corridor_context(
    geometry: &GraphGeometry,
    direction: Direction,
) -> BackwardCorridorContext {
    compute_backward_corridor_context(geometry, direction, has_orthogonal_corridor_obstructions)
}

fn compute_backward_corridor_context(
    geometry: &GraphGeometry,
    direction: Direction,
    has_obstructions: fn(&LayoutEdge, &GraphGeometry, Direction) -> bool,
) -> BackwardCorridorContext {
    if !geometry.enhanced_backward_routing {
        return BackwardCorridorContext::default();
    }

    let descriptors = build_descriptors(geometry, direction, has_obstructions);
    if descriptors.is_empty() {
        return BackwardCorridorContext::default();
    }

    let compartments = group_into_compartments(descriptors);
    build_context_from_compartments(&compartments)
}

/// If both edge endpoints share the same parent subgraph, return that
/// subgraph's rect. Used to constrain backward edge routing channels to the
/// interior of the containing subgraph.
pub(in crate::graph::routing) fn shared_parent_subgraph_rect(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
) -> Option<FRect> {
    let from_parent = geometry.nodes.get(&edge.from)?.parent.as_deref()?;
    let to_parent = geometry.nodes.get(&edge.to)?.parent.as_deref()?;
    if from_parent != to_parent {
        return None;
    }
    geometry.subgraphs.get(from_parent).map(|sg| sg.rect)
}

/// Check if a node belongs to the given parent subgraph (or has no parent when
/// `parent_id` is None).
pub(in crate::graph::routing) fn node_in_scope(
    node_id: &str,
    parent_id: Option<&str>,
    geometry: &GraphGeometry,
) -> bool {
    let node_parent = geometry
        .nodes
        .get(node_id)
        .and_then(|n| n.parent.as_deref());
    node_parent == parent_id
}

/// Direct/polyline corridor obstruction detection preserves the existing
/// non-scoped behavior from `stage.rs`.
pub(in crate::graph::routing) fn has_direct_corridor_obstructions(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> bool {
    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return false;
    };

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let corridor_left = sr.x.min(tr.x);
            let corridor_right = (sr.x + sr.width).max(tr.x + tr.width);
            let (min_y, max_y) = source_target_rank_range_y(from_rect, to_rect);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                cy > min_y
                    && cy < max_y
                    && node.rect.x < corridor_right
                    && node_right > corridor_left
            })
        }
        Direction::LeftRight | Direction::RightLeft => {
            let corridor_top = sr.y.min(tr.y);
            let corridor_bottom = (sr.y + sr.height).max(tr.y + tr.height);
            let (min_x, max_x) = source_target_rank_range_x(from_rect, to_rect);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                let cx = node.rect.center_x();
                let node_bottom = node.rect.y + node.rect.height;
                cx > min_x
                    && cx < max_x
                    && node.rect.y < corridor_bottom
                    && node_bottom > corridor_top
            })
        }
    }
}

/// Orthogonal corridor obstruction detection preserves the existing scoped
/// behavior from `orthogonal/backward.rs`.
pub(in crate::graph::routing) fn has_orthogonal_corridor_obstructions(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    direction: Direction,
) -> bool {
    let from_rect = geometry.nodes.get(&edge.from).map(|n| n.rect);
    let to_rect = geometry.nodes.get(&edge.to).map(|n| n.rect);

    let (Some(sr), Some(tr)) = (from_rect, to_rect) else {
        return false;
    };

    let scope_parent = geometry
        .nodes
        .get(&edge.from)
        .and_then(|n| n.parent.as_deref());

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let corridor_left = sr.x.min(tr.x);
            let corridor_right = (sr.x + sr.width).max(tr.x + tr.width);
            let min_y = sr.y.min(tr.y);
            let max_y = (sr.y + sr.height).max(tr.y + tr.height);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                if !node_in_scope(&node.id, scope_parent, geometry) {
                    return false;
                }
                let cy = node.rect.center_y();
                let node_right = node.rect.x + node.rect.width;
                cy > min_y
                    && cy < max_y
                    && node.rect.x < corridor_right
                    && node_right > corridor_left
            })
        }
        Direction::LeftRight | Direction::RightLeft => {
            let corridor_top = sr.y.min(tr.y);
            let corridor_bottom = (sr.y + sr.height).max(tr.y + tr.height);
            let min_x = sr.x.min(tr.x);
            let max_x = (sr.x + sr.width).max(tr.x + tr.width);
            geometry.nodes.values().any(|node| {
                if node.id == edge.from || node.id == edge.to {
                    return false;
                }
                if !node_in_scope(&node.id, scope_parent, geometry) {
                    return false;
                }
                let cx = node.rect.center_x();
                let node_bottom = node.rect.y + node.rect.height;
                cx > min_x
                    && cx < max_x
                    && node.rect.y < corridor_bottom
                    && node_bottom > corridor_top
            })
        }
    }
}

/// Minimum clearance between the target face envelope and the innermost
/// corridor lane (slot 0). Must be at least as large as the SVG marker
/// offset's minimum endpoint support (arrowhead pullback 4 px + minimum
/// visible stem 12 px = 16 px) so the SVG renderer does not push slot 0
/// outward and break the uniform lane spacing.
const CHANNEL_CLEARANCE: f64 = 16.0;

fn build_descriptors(
    geometry: &GraphGeometry,
    direction: Direction,
    has_obstructions: fn(&LayoutEdge, &GraphGeometry, Direction) -> bool,
) -> Vec<CorridorDescriptor> {
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
                && (has_obstructions(edge, geometry, direction)
                    || backward_target_counts
                        .get(edge.to.as_str())
                        .copied()
                        .unwrap_or(0)
                        >= 2)
        })
        .filter_map(|edge| build_one_descriptor(edge, geometry, direction))
        .collect()
}

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

/// Group corridor descriptors into compartments.
///
/// Two edges are in the same compartment when they share the same
/// `scope_parent` and their primary-axis spans overlap, or when they share the
/// same target node regardless of span overlap.
fn group_into_compartments(
    mut descriptors: Vec<CorridorDescriptor>,
) -> Vec<Vec<CorridorDescriptor>> {
    if descriptors.is_empty() {
        return Vec::new();
    }

    descriptors.sort_by(|a, b| {
        a.scope_parent
            .cmp(&b.scope_parent)
            .then(a.span_min.partial_cmp(&b.span_min).unwrap())
    });

    let mut compartments: Vec<Vec<CorridorDescriptor>> = Vec::new();
    let mut current_group: Vec<CorridorDescriptor> = vec![descriptors[0].clone()];
    let mut current_span_max = descriptors[0].span_max;

    for desc in descriptors.into_iter().skip(1) {
        let same_scope = current_group
            .last()
            .map(|current| current.scope_parent == desc.scope_parent)
            .unwrap_or(false);
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

    merge_compartments_by_shared_target(&mut compartments);
    compartments
}

fn merge_compartments_by_shared_target(compartments: &mut Vec<Vec<CorridorDescriptor>>) {
    loop {
        let mut target_to_compartment: HashMap<&str, usize> = HashMap::new();
        let mut merge_pair: Option<(usize, usize)> = None;

        for (index, compartment) in compartments.iter().enumerate() {
            for descriptor in compartment {
                if let Some(&earlier) = target_to_compartment.get(descriptor.target.as_str()) {
                    if earlier != index {
                        merge_pair = Some((earlier, index));
                        break;
                    }
                } else {
                    target_to_compartment.insert(&descriptor.target, index);
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

fn build_context_from_compartments(
    compartments: &[Vec<CorridorDescriptor>],
) -> BackwardCorridorContext {
    let mut slots = HashMap::new();

    for compartment in compartments {
        if compartment.len() < 2 {
            continue;
        }

        let base_lane = compartment
            .iter()
            .map(|descriptor| descriptor.base_lane)
            .fold(f64::NEG_INFINITY, f64::max);

        let mut sorted: Vec<(usize, f64, usize)> = compartment
            .iter()
            .map(|descriptor| {
                (
                    descriptor.edge_index,
                    descriptor.span_max - descriptor.span_min,
                    descriptor.edge_index,
                )
            })
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.2.cmp(&b.2)));

        for (slot, (edge_index, _, _)) in sorted.iter().enumerate() {
            slots.insert(*edge_index, BackwardCorridorSlot { base_lane, slot });
        }
    }

    BackwardCorridorContext { slots }
}

fn source_target_rank_range_y(from_rect: Option<FRect>, to_rect: Option<FRect>) -> (f64, f64) {
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for rect in [from_rect, to_rect].iter().flatten() {
        min_y = min_y.min(rect.y);
        max_y = max_y.max(rect.y + rect.height);
    }
    (min_y, max_y)
}

fn source_target_rank_range_x(from_rect: Option<FRect>, to_rect: Option<FRect>) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for rect in [from_rect, to_rect].iter().flatten() {
        min_x = min_x.min(rect.x);
        max_x = max_x.max(rect.x + rect.width);
    }
    (min_x, max_x)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::graph::Shape;
    use crate::graph::geometry::{LayoutEdge, PositionedNode, SubgraphGeometry};
    use crate::graph::space::FRect;

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

    fn positioned_node(id: &str, rect: FRect, parent: Option<&str>) -> PositionedNode {
        PositionedNode {
            id: id.to_string(),
            rect,
            shape: Shape::Rectangle,
            label: id.to_string(),
            parent: parent.map(str::to_string),
        }
    }

    fn scoped_geometry() -> GraphGeometry {
        let mut nodes = HashMap::new();
        nodes.insert(
            "A".into(),
            positioned_node("A", FRect::new(0.0, 0.0, 40.0, 20.0), Some("sg")),
        );
        nodes.insert(
            "B".into(),
            positioned_node("B", FRect::new(60.0, 0.0, 40.0, 20.0), Some("sg")),
        );
        nodes.insert(
            "Outside".into(),
            positioned_node("Outside", FRect::new(120.0, 0.0, 40.0, 20.0), None),
        );

        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "sg".into(),
            SubgraphGeometry {
                id: "sg".into(),
                rect: FRect::new(-10.0, -10.0, 140.0, 80.0),
                title: "Group".into(),
                depth: 0,
            },
        );

        GraphGeometry {
            nodes,
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
                label_geometry: None,
                effective_wrapped_lines: None,
            }],
            subgraphs,
            self_edges: vec![],
            direction: Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(-10.0, -10.0, 200.0, 120.0),
            reversed_edges: vec![0],
            engine_hints: None,
            grid_projection: None,
            rerouted_edges: HashSet::new(),
            enhanced_backward_routing: true,
        }
    }

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
        let descriptors = vec![
            desc(0, None, 10.0, 60.0, 200.0),
            desc(1, None, 50.0, 110.0, 200.0),
            desc(2, None, 100.0, 160.0, 200.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].len(), 3);
    }

    #[test]
    fn shorter_span_gets_inner_slot() {
        let compartments = vec![vec![
            desc(5, None, 10.0, 100.0, 200.0),
            desc(3, None, 20.0, 90.0, 195.0),
        ]];
        let ctx = build_context_from_compartments(&compartments);

        let slot3 = ctx.slot_for(3).unwrap();
        let slot5 = ctx.slot_for(5).unwrap();
        assert_eq!(slot3.slot, 0);
        assert_eq!(slot5.slot, 1);
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
        let compartments = vec![vec![desc(7, None, 10.0, 100.0, 200.0)]];
        let ctx = build_context_from_compartments(&compartments);
        assert!(ctx.slot_for(7).is_none());
    }

    #[test]
    fn edges_without_entry_return_none() {
        let ctx = BackwardCorridorContext::default();
        assert!(ctx.slot_for(42).is_none());
    }

    #[test]
    fn same_target_non_overlapping_spans_merge_into_one_compartment() {
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
        let descriptors = vec![
            desc_with_target(0, "Alpha", None, 10.0, 50.0, 200.0),
            desc_with_target(1, "Bravo", None, 100.0, 200.0, 210.0),
        ];
        let compartments = group_into_compartments(descriptors);
        assert_eq!(compartments.len(), 2);
    }

    #[test]
    fn same_target_merges_across_span_groups() {
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
    fn shared_parent_subgraph_rect_requires_same_parent() {
        let geometry = scoped_geometry();
        let rect = shared_parent_subgraph_rect(&geometry.edges[0], &geometry).unwrap();
        assert_eq!(rect, FRect::new(-10.0, -10.0, 140.0, 80.0));
    }

    #[test]
    fn node_in_scope_matches_exact_parent() {
        let geometry = scoped_geometry();
        assert!(node_in_scope("A", Some("sg"), &geometry));
        assert!(!node_in_scope("Outside", Some("sg"), &geometry));
        assert!(node_in_scope("Outside", None, &geometry));
    }

    #[test]
    fn orthogonal_context_assigns_distinct_slots_for_same_target_edges() {
        let mut geometry = scoped_geometry();
        geometry.nodes.insert(
            "C".into(),
            positioned_node("C", FRect::new(30.0, 100.0, 40.0, 20.0), Some("sg")),
        );
        geometry.edges = vec![
            LayoutEdge {
                index: 0,
                from: "C".into(),
                to: "A".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
                label_geometry: None,
                effective_wrapped_lines: None,
            },
            LayoutEdge {
                index: 1,
                from: "C".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
                label_geometry: None,
                effective_wrapped_lines: None,
            },
        ];
        geometry.reversed_edges = vec![0, 1];
        geometry.nodes.insert(
            "Mid".into(),
            positioned_node("Mid", FRect::new(20.0, 45.0, 40.0, 20.0), Some("sg")),
        );

        let ctx = compute_orthogonal_backward_corridor_context(&geometry, Direction::TopDown);
        let slot0 = ctx.slot_for(0).expect("edge 0 should have a corridor slot");
        let slot1 = ctx.slot_for(1).expect("edge 1 should have a corridor slot");

        assert_eq!(slot0.base_lane, slot1.base_lane);
        assert_ne!(slot0.slot, slot1.slot);
    }
}
