//! Label-lane packing types and compartment grouping for Algorithm C.
//!
//! This module introduces the core data structures for interval-track lane
//! packing of edge labels. `LabelDescriptor` captures the axis/cross bands,
//! direction sign, and geometry of each labeled edge. `LabelCompartment`
//! groups descriptors that share a scope parent and have overlapping
//! cross-bands. `group_label_compartments` partitions descriptors into
//! compartments using an iterative merge with `LANE_GAP` slack.

use std::collections::{BTreeMap, HashMap};

use crate::graph::Direction;
use crate::graph::Graph;
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::labels::arc_length_midpoint;
use crate::graph::space::{FPoint, FRect};

/// Gap between adjacent label lanes within a compartment.
pub(super) const LANE_GAP: f64 = 4.0;

/// Minimum step size for label lane assignment.
pub(super) const MIN_LABEL_LANE_STEP: f64 = 16.0;

/// Ratio of path lane to label lane width allocation.
pub(super) const PATH_LANE_RATIO: f64 = 0.5;

/// Describes one labeled edge's axis/cross bands, direction sign, and geometry.
#[derive(Debug, Clone)]
pub(super) struct LabelDescriptor {
    /// Index into the diagram's edge list.
    pub edge_index: usize,
    /// Shared parent subgraph of both endpoints (None = top level).
    pub scope_parent: Option<String>,
    /// Primary-axis lower bound of the label region.
    pub axis_min: f64,
    /// Primary-axis upper bound of the label region.
    pub axis_max: f64,
    /// Cross-axis lower bound of the label region.
    pub cross_min: f64,
    /// Cross-axis upper bound of the label region.
    pub cross_max: f64,
    /// Direction sign: +1 for forward, -1 for backward.
    pub direction_sign: i32,
    /// Midpoint of the label placement.
    pub midpoint: FPoint,
    /// Bounding rectangle of the label text.
    pub label_rect: FRect,
}

/// Groups descriptors that share a scope parent and have overlapping cross-bands.
#[derive(Debug)]
pub(super) struct LabelCompartment {
    /// Member descriptors within this compartment.
    pub members: Vec<LabelDescriptor>,
    /// Layout direction for this compartment.
    pub direction: Direction,
}

/// Partition label descriptors into compartments by scope parent and
/// overlapping cross-bands (with `LANE_GAP` slack).
pub(super) fn group_label_compartments(
    descriptors: Vec<LabelDescriptor>,
    direction: Direction,
) -> Vec<LabelCompartment> {
    if descriptors.is_empty() {
        return vec![];
    }

    // Group by scope_parent first.
    let mut by_scope: HashMap<Option<String>, Vec<LabelDescriptor>> = HashMap::new();
    for desc in descriptors {
        by_scope
            .entry(desc.scope_parent.clone())
            .or_default()
            .push(desc);
    }

    let mut compartments = Vec::new();
    for (_scope, mut descs) in by_scope {
        // Sort by cross_min for merge pass.
        descs.sort_by(|a, b| {
            a.cross_min
                .partial_cmp(&b.cross_min)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Iterative merge: two descriptors merge if cross_bands overlap
        // (with LANE_GAP slack).
        let mut groups: Vec<Vec<LabelDescriptor>> = Vec::new();
        for desc in descs {
            let merged = groups.iter_mut().find(|g| {
                let group_max = g
                    .iter()
                    .map(|d| d.cross_max)
                    .fold(f64::NEG_INFINITY, f64::max);
                desc.cross_min <= group_max + LANE_GAP
            });
            if let Some(group) = merged {
                group.push(desc);
            } else {
                groups.push(vec![desc]);
            }
        }

        for members in groups {
            compartments.push(LabelCompartment { members, direction });
        }
    }

    compartments
}

/// Pack label descriptors within a compartment onto signed integer tracks.
///
/// Single-member compartments emit track 0 (no displacement needed).
/// Multi-member compartments skip track 0 so reciprocal pairs land on
/// opposite-sign tracks (`+1`/`-1`). Sweep-line packing iterates members
/// in `(axis_min, edge_index)` order; a track is reused when the next
/// descriptor's `axis_min` exceeds the previous occupant's `axis_max`
/// plus `LANE_GAP`.
pub(super) fn pack_signed_tracks(compartment: &LabelCompartment) -> HashMap<usize, i32> {
    // Single-member compartments: no displacement needed — emit track 0.
    if compartment.members.len() == 1 {
        let only = &compartment.members[0];
        return std::iter::once((only.edge_index, 0)).collect();
    }

    // Multi-member: skip track 0 so reciprocal pairs land on +1/-1.
    let mut sorted = compartment.members.clone();
    sorted.sort_by(|a, b| {
        a.axis_min
            .partial_cmp(&b.axis_min)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.edge_index.cmp(&b.edge_index))
    });

    let mut last_end: BTreeMap<i32, f64> = BTreeMap::new();
    let mut out: HashMap<usize, i32> = HashMap::new();

    for desc in &sorted {
        let track = find_or_open_track_skipping_zero(&last_end, desc);
        last_end.insert(track, desc.axis_max);
        out.insert(desc.edge_index, track);
    }
    out
}

fn find_or_open_track_skipping_zero(last_end: &BTreeMap<i32, f64>, desc: &LabelDescriptor) -> i32 {
    for k in candidate_track_order_nonzero(desc.direction_sign).take(64) {
        let fits = last_end
            .get(&k)
            .map_or(true, |&end| end + LANE_GAP <= desc.axis_min);
        if fits {
            return k;
        }
    }
    panic!("label lane packer exhausted 64 candidate tracks — this should not happen in practice");
}

fn candidate_track_order_nonzero(sign: i32) -> impl Iterator<Item = i32> {
    // Yields: sign*1, -sign*1, sign*2, -sign*2, sign*3, ...
    // Track 0 explicitly omitted for multi-member compartments.
    (1..).flat_map(move |k| [sign * k, -sign * k])
}

/// Per-edge outcome of the lane assignment pass.
#[derive(Debug, Clone)]
pub(super) struct LabelTrackOutcome {
    pub label_center: FPoint,
    pub label_rect: FRect,
    pub track: i32,
    pub adjusted_path: Vec<FPoint>,
}

/// Assign labels to signed tracks, shift label centers and middle path
/// segments by the resulting offsets, and emit one outcome per edge.
///
/// `backward_flags` is a side-input from the routing pipeline that maps
/// edge index → is_backward. Diagram-level `Edge` does not carry this
/// (it is set during layout), so the orchestrator's caller — the routing
/// pipeline in `route_graph_geometry` — populates it from the routed
/// edges. In unit tests where only the diagram defines edges, pass an
/// empty map (defaults to forward / +1).
pub(super) fn assign_label_tracks(
    diagram: &Graph,
    geometry: &GraphGeometry,
    paths: &HashMap<usize, Vec<FPoint>>,
    backward_flags: &HashMap<usize, bool>,
    metrics: &ProportionalTextMetrics,
    direction: Direction,
) -> HashMap<usize, LabelTrackOutcome> {
    let descriptors = build_label_descriptors(diagram, geometry, paths, backward_flags, metrics);
    let compartments = group_label_compartments(descriptors, direction);
    let mut outcomes: HashMap<usize, LabelTrackOutcome> = HashMap::new();

    for compartment in compartments {
        let tracks = pack_signed_tracks(&compartment);

        // Per-compartment LABEL_LANE_STEP from the max cross-band height.
        let max_cross = compartment
            .members
            .iter()
            .map(|m| m.cross_max - m.cross_min)
            .fold(0.0_f64, f64::max);
        let label_step = (max_cross + LANE_GAP).max(MIN_LABEL_LANE_STEP);
        let path_step = label_step * PATH_LANE_RATIO;

        for desc in &compartment.members {
            let track = tracks[&desc.edge_index];
            let label_offset = track as f64 * label_step;
            let path_offset = track as f64 * path_step;
            let (new_center, new_rect) = shift_label(desc, label_offset, direction);
            let new_path = paths
                .get(&desc.edge_index)
                .map(|p| shift_middle_segment(p, path_offset, direction))
                .unwrap_or_default();
            outcomes.insert(
                desc.edge_index,
                LabelTrackOutcome {
                    label_center: new_center,
                    label_rect: new_rect,
                    track,
                    adjusted_path: new_path,
                },
            );
        }
    }

    outcomes
}

/// Build label descriptors from diagram geometry and routed edge paths.
///
/// Iterates `diagram.edges` to source the canonical edge index, skipping
/// edges without a label or without a routed path. Each descriptor projects
/// the label rectangle onto the primary (axis) and cross axes based on
/// `diagram.direction`, and records `direction_sign` from `backward_flags`
/// (forward = +1, backward = -1). The routing pipeline populates
/// `backward_flags` from the routed edge list; unit tests may pass an
/// empty map (defaults to forward).
pub(super) fn build_label_descriptors(
    diagram: &Graph,
    geometry: &GraphGeometry,
    paths: &HashMap<usize, Vec<FPoint>>,
    backward_flags: &HashMap<usize, bool>,
    metrics: &ProportionalTextMetrics,
) -> Vec<LabelDescriptor> {
    let direction = diagram.direction;
    let mut out = Vec::new();

    for (edge_index, edge) in diagram.edges.iter().enumerate() {
        let Some(label_text) = edge.label.as_deref() else {
            continue;
        };
        if label_text.is_empty() {
            continue;
        }
        let Some(path) = paths.get(&edge_index) else {
            continue;
        };
        if path.len() < 2 {
            continue;
        }

        let midpoint = arc_length_midpoint(path).unwrap_or_else(|| path[path.len() / 2]);
        let (w, h) = metrics.edge_label_dimensions(label_text);

        let direction_sign = if *backward_flags.get(&edge_index).unwrap_or(&false) {
            -1
        } else {
            1
        };

        let scope_parent = compute_shared_parent(diagram, geometry, &edge.from, &edge.to);

        // Project label onto axis/cross bands based on flow direction.
        // For TD/BU: axis = Y (height), cross = X (width).
        // For LR/RL: axis = X (width), cross = Y (height).
        let (axis_dim, cross_dim, axis_center, cross_center) = match direction {
            Direction::TopDown | Direction::BottomTop => (h, w, midpoint.y, midpoint.x),
            Direction::LeftRight | Direction::RightLeft => (w, h, midpoint.x, midpoint.y),
        };

        let label_rect = FRect::new(midpoint.x - w / 2.0, midpoint.y - h / 2.0, w, h);

        out.push(LabelDescriptor {
            edge_index,
            scope_parent,
            axis_min: axis_center - axis_dim / 2.0,
            axis_max: axis_center + axis_dim / 2.0,
            cross_min: cross_center - cross_dim / 2.0,
            cross_max: cross_center + cross_dim / 2.0,
            direction_sign,
            midpoint,
            label_rect,
        });
    }

    out
}

/// Compute the shared parent subgraph ID for both endpoints, if any.
///
/// Prefers `geometry.nodes[id].parent` (already resolved during layout).
/// Falls back to scanning `diagram.subgraphs` membership when the geometry
/// node is missing. Returns `None` when the endpoints differ in parent or
/// when either endpoint has no parent.
fn compute_shared_parent(
    diagram: &Graph,
    geometry: &GraphGeometry,
    from: &str,
    to: &str,
) -> Option<String> {
    let from_parent = parent_of(diagram, geometry, from)?;
    let to_parent = parent_of(diagram, geometry, to)?;
    if from_parent == to_parent {
        Some(from_parent)
    } else {
        None
    }
}

fn parent_of(diagram: &Graph, geometry: &GraphGeometry, node_id: &str) -> Option<String> {
    if let Some(parent) = geometry
        .nodes
        .get(node_id)
        .and_then(|n| n.parent.as_deref())
    {
        return Some(parent.to_string());
    }
    diagram
        .subgraphs
        .values()
        .find(|sg| sg.nodes.iter().any(|n| n == node_id))
        .map(|sg| sg.id.clone())
}

/// Shift a label center by `offset` along the primary axis.
/// For TD/BU, offset applies to y. For LR/RL, offset applies to x.
fn shift_label(desc: &LabelDescriptor, offset: f64, direction: Direction) -> (FPoint, FRect) {
    let new_center = match direction {
        Direction::TopDown | Direction::BottomTop => {
            FPoint::new(desc.midpoint.x, desc.midpoint.y + offset)
        }
        Direction::LeftRight | Direction::RightLeft => {
            FPoint::new(desc.midpoint.x + offset, desc.midpoint.y)
        }
    };
    let new_rect = FRect::new(
        new_center.x - desc.label_rect.width / 2.0,
        new_center.y - desc.label_rect.height / 2.0,
        desc.label_rect.width,
        desc.label_rect.height,
    );
    (new_center, new_rect)
}

/// Shift the middle segment of an orthogonal path on the cross axis.
/// For TD/BU, cross axis is X. For LR/RL, cross axis is Y.
///
/// Three shapes are handled:
/// - Two-point direct paths: no interior to bend (label-only shift legal
///   for reciprocal pairs).
/// - Three-point collinear path: synthesize a bend by inserting two
///   interior points at the 25% / 75% marks and offsetting them on the
///   cross axis. Endpoints preserved.
/// - Multi-segment orthogonal path: shift every interior point on the
///   cross axis. Endpoints preserved.
fn shift_middle_segment(path: &[FPoint], offset: f64, direction: Direction) -> Vec<FPoint> {
    if path.len() < 2 || offset == 0.0 {
        return path.to_vec();
    }
    if path.len() == 2 {
        return path.to_vec();
    }
    if path.len() == 3 && are_collinear(path) {
        let start = path[0];
        let end = path[2];
        let lerp = |t: f64, a: FPoint, b: FPoint| {
            FPoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        };
        let mut p1 = lerp(0.25, start, end);
        let mut p2 = lerp(0.75, start, end);
        offset_on_cross_axis(&mut p1, offset, direction);
        offset_on_cross_axis(&mut p2, offset, direction);
        return vec![start, p1, p2, end];
    }
    let mut new_path = path.to_vec();
    let last = new_path.len() - 1;
    for point in new_path.iter_mut().take(last).skip(1) {
        offset_on_cross_axis(point, offset, direction);
    }
    new_path
}

fn offset_on_cross_axis(p: &mut FPoint, offset: f64, direction: Direction) {
    match direction {
        Direction::TopDown | Direction::BottomTop => p.x += offset,
        Direction::LeftRight | Direction::RightLeft => p.y += offset,
    }
}

fn are_collinear(path: &[FPoint]) -> bool {
    if path.len() < 3 {
        return true;
    }
    let (a, b, c) = (path[0], path[1], path[2]);
    ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() < 1e-6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_descriptor(
        edge_index: usize,
        scope_parent: Option<&str>,
        axis: (f64, f64),
        cross: (f64, f64),
        sign: i32,
    ) -> LabelDescriptor {
        LabelDescriptor {
            edge_index,
            scope_parent: scope_parent.map(|s| s.to_string()),
            axis_min: axis.0,
            axis_max: axis.1,
            cross_min: cross.0,
            cross_max: cross.1,
            direction_sign: sign,
            midpoint: FPoint::new(
                (axis.0 + axis.1) / 2.0,
                (cross.0 + cross.1) / 2.0,
            ),
            label_rect: FRect::new(
                axis.0,
                cross.0,
                axis.1 - axis.0,
                cross.1 - cross.0,
            ),
        }
    }

    #[test]
    fn label_descriptor_constructs_with_all_fields() {
        let d = make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1);
        assert_eq!(d.axis_max - d.axis_min, 40.0);
        assert_eq!(d.cross_max - d.cross_min, 20.0);
        assert_eq!(d.direction_sign, 1);
    }

    #[test]
    fn group_label_compartments_partitions_by_scope_parent() {
        let a = make_descriptor(0, Some("A"), (10.0, 50.0), (100.0, 120.0), 1);
        let b = make_descriptor(1, Some("B"), (10.0, 50.0), (100.0, 120.0), 1);
        let compartments = group_label_compartments(vec![a, b], Direction::TopDown);
        assert_eq!(
            compartments.len(),
            2,
            "different scope_parent -> separate compartments"
        );
    }

    #[test]
    fn group_label_compartments_merges_overlapping_cross_bands_within_same_scope() {
        let a = make_descriptor(0, Some("A"), (10.0, 50.0), (100.0, 120.0), 1);
        let b = make_descriptor(1, Some("A"), (15.0, 55.0), (110.0, 130.0), -1);
        let compartments = group_label_compartments(vec![a, b], Direction::TopDown);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].members.len(), 2);
    }

    #[test]
    fn group_label_compartments_separates_non_overlapping_cross_bands_same_scope() {
        let a = make_descriptor(0, Some("A"), (10.0, 50.0), (100.0, 120.0), 1);
        let b = make_descriptor(1, Some("A"), (15.0, 55.0), (200.0, 220.0), -1);
        let compartments = group_label_compartments(vec![a, b], Direction::TopDown);
        assert_eq!(
            compartments.len(),
            2,
            "non-overlapping cross bands -> separate compartments"
        );
    }

    #[test]
    fn group_label_compartments_merges_cross_bands_within_lane_gap() {
        let a = make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1);
        // b's cross_min is within LANE_GAP of a's cross_max
        let b = make_descriptor(1, None, (10.0, 50.0), (123.0, 140.0), -1);
        let compartments = group_label_compartments(vec![a, b], Direction::TopDown);
        assert_eq!(compartments.len(), 1, "within LANE_GAP slack -> merge");
    }

    #[test]
    fn group_label_compartments_empty_input() {
        let compartments = group_label_compartments(vec![], Direction::TopDown);
        assert!(compartments.is_empty());
    }

    #[test]
    fn pack_signed_tracks_assigns_zero_track_to_singleton_compartment() {
        let compartment = LabelCompartment {
            members: vec![make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1)],
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks[&0], 0);
    }

    #[test]
    fn pack_signed_tracks_assigns_opposite_signs_to_reciprocal_pair() {
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1),
                make_descriptor(1, None, (10.0, 50.0), (100.0, 120.0), -1),
            ],
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks[&0], 1);
        assert_eq!(tracks[&1], -1);
    }

    #[test]
    fn pack_signed_tracks_packs_three_same_direction_skipping_zero() {
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1),
                make_descriptor(1, None, (20.0, 60.0), (100.0, 120.0), 1),
                make_descriptor(2, None, (30.0, 70.0), (100.0, 120.0), 1),
            ],
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        // Candidate order for sign=+1 is [1, -1, 2, -2, ...], so three
        // overlapping same-sign descriptors land on tracks [1, -1, 2].
        let mut values: Vec<_> = tracks.values().copied().collect();
        values.sort();
        assert_eq!(values, vec![-1, 1, 2]);
        assert_eq!(tracks[&0], 1); // lowest axis_min gets |k|=1
        // None of them land on track 0 (multi-member skips zero).
        assert!(tracks.values().all(|&t| t != 0));
    }

    #[test]
    fn pack_signed_tracks_breaks_ties_by_edge_index() {
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(1, None, (10.0, 50.0), (100.0, 120.0), 1),
                make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1),
            ],
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        // Tie-break on (axis_min, edge_index): edge 0 sorted first, gets
        // track 1; edge 1 gets the next candidate (-1) since axis ranges
        // overlap and candidate order is [1, -1, 2, -2, ...].
        assert_eq!(tracks[&0], 1);
        assert_eq!(tracks[&1], -1);
    }

    #[test]
    fn pack_signed_tracks_non_overlapping_axis_can_reuse_track() {
        // Two descriptors with non-overlapping axis ranges and same sign
        // The second can reuse track 1 because axis ranges don't overlap.
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(0, None, (10.0, 30.0), (100.0, 120.0), 1),
                make_descriptor(1, None, (40.0, 60.0), (100.0, 120.0), 1), // axis_min > prev axis_max + LANE_GAP
            ],
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks[&0], 1);
        assert_eq!(tracks[&1], 1, "non-overlapping axis ranges should reuse same track");
    }

    #[test]
    fn pack_signed_tracks_handles_ten_same_sign_without_panic() {
        let members: Vec<_> = (0..10)
            .map(|i| make_descriptor(i, None, (i as f64 * 5.0, i as f64 * 5.0 + 40.0), (100.0, 120.0), 1))
            .collect();
        let compartment = LabelCompartment {
            members,
            direction: Direction::TopDown,
        };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks.len(), 10);
        // All tracks should be non-zero (multi-member)
        assert!(tracks.values().all(|&t| t != 0));
    }

    #[test]
    fn shift_label_topdown_offsets_y_axis() {
        let desc = make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1);
        let (new_center, new_rect) = shift_label(&desc, 16.0, Direction::TopDown);
        // For TD, axis is Y — label.y shifts by 16
        assert!((new_center.y - desc.midpoint.y - 16.0).abs() < 1e-6);
        assert!((new_center.x - desc.midpoint.x).abs() < 1e-6, "x unchanged");
        assert!((new_rect.y - desc.label_rect.y - 16.0).abs() < 1e-6);
    }

    #[test]
    fn shift_label_leftright_offsets_x_axis() {
        let desc = make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1);
        let (new_center, new_rect) = shift_label(&desc, 16.0, Direction::LeftRight);
        // For LR, axis is X — label.x shifts by 16
        assert!((new_center.x - desc.midpoint.x - 16.0).abs() < 1e-6);
        assert!((new_center.y - desc.midpoint.y).abs() < 1e-6, "y unchanged");
        let _ = new_rect;
    }

    #[test]
    fn shift_middle_segment_two_point_path_unchanged() {
        let path = vec![FPoint::new(0.0, 0.0), FPoint::new(10.0, 0.0)];
        let new_path = shift_middle_segment(&path, 5.0, Direction::TopDown);
        assert_eq!(new_path, path);
    }

    #[test]
    fn shift_middle_segment_three_collinear_path_bends_on_cross_axis() {
        // Vertical path, TD direction — bend on X (cross axis)
        let path = vec![
            FPoint::new(0.0, 0.0),
            FPoint::new(0.0, 5.0),
            FPoint::new(0.0, 10.0),
        ];
        let new_path = shift_middle_segment(&path, 5.0, Direction::TopDown);
        // Endpoints preserved
        assert_eq!(new_path.first(), path.first());
        assert_eq!(new_path.last(), path.last());
        // Bend produced — should have more points (or interior x shifted)
        assert!(new_path.len() >= 3);
        // At least one interior point should differ in x from the original
        let has_x_shift = new_path[1..new_path.len() - 1]
            .iter()
            .any(|p| (p.x - 0.0).abs() > 1e-6);
        assert!(has_x_shift, "interior should shift on cross axis (x for TD)");
    }

    #[test]
    fn shift_middle_segment_endpoints_preserved() {
        let path = vec![
            FPoint::new(0.0, 0.0),
            FPoint::new(5.0, 5.0),
            FPoint::new(10.0, 0.0),
            FPoint::new(15.0, 5.0),
        ];
        let new_path = shift_middle_segment(&path, 8.0, Direction::TopDown);
        assert_eq!(new_path.first(), path.first());
        assert_eq!(new_path.last(), path.last());
    }
}
