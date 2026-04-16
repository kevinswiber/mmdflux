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
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::space::{FPoint, FRect};
#[allow(unused_imports)]
use crate::graph::Graph;

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

/// Build label descriptors from diagram geometry and routed edge paths.
///
/// Stubbed for now — will be populated in task 3.4.
pub(super) fn build_label_descriptors(
    _diagram: &Graph,
    _geometry: &GraphGeometry,
    _paths: &HashMap<usize, Vec<FPoint>>,
    _metrics: &ProportionalTextMetrics,
) -> Vec<LabelDescriptor> {
    // Populated in task 3.4
    vec![]
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
}
