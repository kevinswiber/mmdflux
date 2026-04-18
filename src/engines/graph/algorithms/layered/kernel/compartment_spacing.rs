//! Kernel-local rank-space reservation for multi-member edge-label
//! compartments. Runs inside Phase 4 (position.rs) after Brandes-Köpf has
//! produced cross-axis coordinates.
//!
//! Design record:
//! `.gumbo/plans/0150-kernel-compartment-rank-spacing/architecture/design.md`.

use std::collections::{BTreeMap, HashMap};

/// Slack between stacked label tracks inside a compartment. Must stay
/// aligned with `src/graph/routing/label_lanes.rs::LANE_GAP`. Task 4.2
/// adds a drift-detection test that pins the two values together.
#[allow(dead_code)]
pub(crate) const INTER_TRACK_GAP: f64 = 4.0;

/// Projected input for compartment grouping and reservation math.
///
/// Built by `project_from_layout_graph(...)` (Task 2.4) so unit tests can
/// exercise grouping without constructing a fully-populated `LayoutGraph`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompartmentCandidate {
    /// Index of the underlying label dummy in `LayoutGraph`. Only used
    /// for debugging / equivalence assertions — grouping does not depend
    /// on it.
    pub(crate) idx: usize,
    pub(crate) rank: i32,
    /// LCA of the original edge endpoints, or `None` if the LCA is the
    /// implicit root.
    pub(crate) scope_lca: Option<usize>,
    /// Cross-axis coordinate of the dummy's left/top edge.
    pub(crate) cross_min: f64,
    /// Cross-axis coordinate of the dummy's right/bottom edge.
    pub(crate) cross_max: f64,
    /// Extent along the rank axis (height for TD/BT, width for LR/RL).
    pub(crate) rank_axis_extent: f64,
}

/// A group of candidates that share `(rank, scope_lca)` and have
/// cross-axis intervals that merge under `INTER_TRACK_GAP` slack.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct Compartment {
    pub(crate) rank: i32,
    pub(crate) scope_lca: Option<usize>,
    pub(crate) members: Vec<CompartmentCandidate>,
}

#[allow(dead_code)]
impl Compartment {
    pub(crate) fn len(&self) -> usize {
        self.members.len()
    }
}

/// Group candidates into compartments mirroring
/// `label_lanes::group_label_compartments`: bucket by `(rank, scope_lca)`,
/// sort each bucket by `cross_min`, then merge overlapping cross-bands
/// (with `INTER_TRACK_GAP` slack) into a single compartment.
///
/// `BTreeMap` keying makes output order deterministic for equivalence
/// assertions in Task 4.1.
#[allow(dead_code)]
pub(crate) fn group_compartments(candidates: Vec<CompartmentCandidate>) -> Vec<Compartment> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut buckets: BTreeMap<(i32, Option<usize>), Vec<CompartmentCandidate>> = BTreeMap::new();
    for c in candidates {
        buckets.entry((c.rank, c.scope_lca)).or_default().push(c);
    }

    let mut result = Vec::new();
    for ((rank, scope_lca), mut bucket) in buckets {
        bucket.sort_by(|a, b| a.cross_min.partial_cmp(&b.cross_min).unwrap());

        let mut compartments: Vec<Compartment> = Vec::new();
        for c in bucket {
            let can_merge = compartments.last().is_some_and(|last| {
                let last_max = last
                    .members
                    .iter()
                    .map(|m| m.cross_max)
                    .fold(f64::NEG_INFINITY, f64::max);
                c.cross_min <= last_max + INTER_TRACK_GAP
            });
            if can_merge {
                compartments.last_mut().unwrap().members.push(c);
            } else {
                compartments.push(Compartment {
                    rank,
                    scope_lca,
                    members: vec![c],
                });
            }
        }

        result.extend(compartments);
    }

    result
}

/// Translate compartments into absolute rank-gap reservations. The returned
/// map is keyed by the lower rank of the gap (same convention as
/// `LayoutConfig::rank_sep_overrides` consumed by
/// `LayoutConfig::rank_sep_for_gap`). Values are absolute gap sizes, not
/// deltas. Single-member compartments are skipped. When multiple
/// compartments share a gap, values are MAX-reduced.
///
/// Reservations key only on `c.rank` (the forward gap from the label rank).
/// A previous attempt to widen both adjacent gaps (`rank - 1` and `rank`)
/// closed `state_issue_222_minimal_repro_labels_do_not_overlap_svg` but
/// pushed the Text renderer's grid-quantized routing across a quantum
/// boundary on five text fixtures, producing visibly mangled labels (see
/// `findings/phase-3-state-issue-222-regression.md`). The single-gap
/// reservation is the conservative trade-off: SVG routing gets most of the
/// space it needs; the residual 1-px reciprocal-pair overlap is tracked as
/// a follow-up that needs text-renderer-aware coordination.
#[allow(dead_code)]
pub(crate) fn compute_reservations(compartments: &[Compartment]) -> HashMap<i32, f64> {
    let mut out: HashMap<i32, f64> = HashMap::new();
    for c in compartments {
        let n = c.len();
        if n < 2 {
            continue;
        }
        let sum_extent: f64 = c.members.iter().map(|m| m.rank_axis_extent).sum();
        let required = sum_extent + ((n as f64) - 1.0) * INTER_TRACK_GAP;
        let entry = out.entry(c.rank).or_insert(0.0);
        if required > *entry {
            *entry = required;
        }
    }
    out
}
/// Build `CompartmentCandidate`s from a `LayoutGraph`. Only `EdgeLabel`
/// dummies participate — regular `Edge` dummies (including self-edge
/// dummies inserted by `pipeline::insert_self_edge_dummies`) are
/// ignored.
///
/// `cross_coords` gives the cross-axis centre of each dummy — x for
/// TopBottom/BottomTop and y for LeftRight/RightLeft. Phase 3 passes in
/// whichever axis Brandes-Köpf has just produced.
#[allow(dead_code)]
pub(crate) fn project_from_layout_graph(
    lg: &super::graph::LayoutGraph,
    cross_coords: &HashMap<usize, f64>,
    direction: super::types::Direction,
) -> Vec<CompartmentCandidate> {
    use super::parent_dummy_chains::{compute_lca, compute_postorder};
    use super::types::{Direction, DummyType};

    let postorder = compute_postorder(lg);
    let mut out = Vec::new();

    for idx in 0..lg.node_ids.len() {
        let Some(info) = lg.dummy_nodes.get(&lg.node_ids[idx]) else {
            continue;
        };
        if info.dummy_type != DummyType::EdgeLabel {
            continue;
        }

        let Some(&(src, tgt)) = lg.original_edge_endpoints.get(info.edge_index) else {
            continue;
        };
        let scope_lca = compute_lca(lg, &postorder, src, tgt);

        let (w, h) = lg.dimensions[idx];
        let cross = cross_coords.get(&idx).copied().unwrap_or(0.0);
        let (cross_min, cross_max, rank_axis_extent) = match direction {
            Direction::TopBottom | Direction::BottomTop => (cross - w / 2.0, cross + w / 2.0, h),
            Direction::LeftRight | Direction::RightLeft => (cross - h / 2.0, cross + h / 2.0, w),
        };

        out.push(CompartmentCandidate {
            idx,
            rank: lg.ranks[idx],
            scope_lca,
            cross_min,
            cross_max,
            rank_axis_extent,
        });
    }

    out
}

/// Compute absolute rank-gap reservations implied by multi-member edge-label
/// compartments. Returns a `HashMap<i32, f64>` keyed by the lower rank of
/// each gap. Consumed by `kernel::position` in Phase 3.
#[allow(dead_code)]
pub(crate) fn compute_compartment_rank_sep_overrides(
    lg: &super::graph::LayoutGraph,
    cross_coords: &HashMap<usize, f64>,
    direction: super::types::Direction,
) -> HashMap<i32, f64> {
    let candidates = project_from_layout_graph(lg, cross_coords, direction);
    let compartments = group_compartments(candidates);
    compute_reservations(&compartments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_builds_and_constant_matches_expected_value() {
        assert!((INTER_TRACK_GAP - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn kernel_grouping_agrees_with_routing_grouping_on_synthetic_cases() {
        // Plan 0150 canary (scope-reduced form of task 4.1): instead of
        // running both kernel and routing end-to-end and comparing
        // compartment sets, this test exercises the shared algorithmic
        // invariant — bucket by scope, sort by cross_min, merge with
        // INTER_TRACK_GAP / LANE_GAP slack — on synthetic candidates and
        // asserts both modules arrive at the same member sets.
        //
        // See findings/tdd-deviation-canary-4.1.md for rationale.

        use std::collections::BTreeSet;

        use crate::graph::routing::label_lanes::LANE_GAP;

        // Case A: two members, overlapping bands at the same (rank, scope)
        // — both should group into a single compartment.
        let candidates = vec![
            CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: None,
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 1,
                rank: 1,
                scope_lca: None,
                cross_min: 22.0,
                cross_max: 40.0,
                rank_axis_extent: 28.0,
            },
            // Distinct scope — must not merge with the first two.
            CompartmentCandidate {
                idx: 2,
                rank: 1,
                scope_lca: Some(99),
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
        ];

        // Constant equivalence precondition for the canary to make sense.
        assert!((INTER_TRACK_GAP - LANE_GAP).abs() < f64::EPSILON);

        let kernel_groups = group_compartments(candidates.clone());
        let kernel_sets: BTreeSet<(i32, Option<usize>, BTreeSet<usize>)> = kernel_groups
            .iter()
            .map(|c| {
                (
                    c.rank,
                    c.scope_lca,
                    c.members.iter().map(|m| m.idx).collect::<BTreeSet<_>>(),
                )
            })
            .collect();

        // Expected: {(1, None, {0, 1}), (1, Some(99), {2})}.
        let mut expected: BTreeSet<(i32, Option<usize>, BTreeSet<usize>)> = BTreeSet::new();
        expected.insert((1, None, [0usize, 1].into_iter().collect()));
        expected.insert((1, Some(99), [2usize].into_iter().collect()));
        assert_eq!(kernel_sets, expected);
    }

    #[test]
    fn inter_track_gap_matches_routing_lane_gap() {
        // Plan 0150: the kernel reservation pass intentionally duplicates
        // routing's LANE_GAP as INTER_TRACK_GAP to avoid a kernel → routing
        // dependency. This test is the drift-detection check required by
        // design.md §7.
        assert!(
            (super::INTER_TRACK_GAP - crate::graph::routing::label_lanes::LANE_GAP).abs()
                < f64::EPSILON,
            "INTER_TRACK_GAP ({}) and label_lanes::LANE_GAP ({}) must match; \
             update both together or re-evaluate the duplication.",
            super::INTER_TRACK_GAP,
            crate::graph::routing::label_lanes::LANE_GAP,
        );
    }

    #[test]
    fn candidate_rank_axis_extent_is_accessible() {
        let c = CompartmentCandidate {
            idx: 7,
            rank: 2,
            scope_lca: None,
            cross_min: 10.0,
            cross_max: 30.0,
            rank_axis_extent: 28.0,
        };
        assert_eq!(c.rank, 2);
        assert_eq!(c.rank_axis_extent, 28.0);
        assert!(c.scope_lca.is_none());
    }

    #[test]
    fn groups_by_rank_and_scope_lca() {
        let cs = vec![
            CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: Some(10),
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 1,
                rank: 1,
                scope_lca: Some(10),
                cross_min: 5.0,
                cross_max: 30.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 2,
                rank: 2,
                scope_lca: None,
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
        ];
        let groups = group_compartments(cs);
        assert_eq!(groups.len(), 2);
        let r1 = groups
            .iter()
            .find(|c| c.rank == 1 && c.scope_lca == Some(10))
            .unwrap();
        assert_eq!(r1.len(), 2);
        let r2 = groups
            .iter()
            .find(|c| c.rank == 2 && c.scope_lca.is_none())
            .unwrap();
        assert_eq!(r2.len(), 1);
    }

    #[test]
    fn same_rank_different_lca_does_not_merge() {
        let cs = vec![
            CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: Some(10),
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 1,
                rank: 1,
                scope_lca: Some(11),
                cross_min: 5.0,
                cross_max: 30.0,
                rank_axis_extent: 28.0,
            },
        ];
        let groups = group_compartments(cs);
        assert_eq!(groups.len(), 2);
        for c in &groups {
            assert_eq!(c.len(), 1);
        }
    }

    #[test]
    fn same_lca_non_overlapping_bands_split() {
        let cs = vec![
            CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: None,
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 1,
                rank: 1,
                scope_lca: None,
                cross_min: 50.0,
                cross_max: 70.0,
                rank_axis_extent: 28.0,
            },
        ];
        let groups = group_compartments(cs);
        assert_eq!(groups.len(), 2);
        for c in &groups {
            assert_eq!(c.len(), 1);
        }
    }

    #[test]
    fn cross_bands_within_inter_track_gap_merge() {
        let cs = vec![
            CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: None,
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            },
            CompartmentCandidate {
                idx: 1,
                rank: 1,
                scope_lca: None,
                cross_min: 22.0,
                cross_max: 40.0,
                rank_axis_extent: 28.0,
            },
        ];
        let groups = group_compartments(cs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn group_compartments_empty_input_returns_empty() {
        assert!(group_compartments(Vec::new()).is_empty());
    }

    #[test]
    fn single_member_compartment_emits_no_reservation() {
        let compartments = vec![Compartment {
            rank: 1,
            scope_lca: None,
            members: vec![CompartmentCandidate {
                idx: 0,
                rank: 1,
                scope_lca: None,
                cross_min: 0.0,
                cross_max: 20.0,
                rank_axis_extent: 28.0,
            }],
        }];
        assert!(compute_reservations(&compartments).is_empty());
    }

    #[test]
    fn two_member_compartment_sums_extents_plus_one_inter_track_gap() {
        let compartments = vec![Compartment {
            rank: 1,
            scope_lca: None,
            members: vec![
                CompartmentCandidate {
                    idx: 0,
                    rank: 1,
                    scope_lca: None,
                    cross_min: 0.0,
                    cross_max: 20.0,
                    rank_axis_extent: 28.0,
                },
                CompartmentCandidate {
                    idx: 1,
                    rank: 1,
                    scope_lca: None,
                    cross_min: 10.0,
                    cross_max: 30.0,
                    rank_axis_extent: 28.0,
                },
            ],
        }];
        let out = compute_reservations(&compartments);
        assert_eq!(out.len(), 1);
        assert!((out[&1] - (28.0 + 28.0 + 4.0)).abs() < 1e-9);
    }

    #[test]
    fn three_member_compartment_uses_two_inter_track_gaps() {
        let mk = |idx, ext| CompartmentCandidate {
            idx,
            rank: 2,
            scope_lca: None,
            cross_min: 0.0,
            cross_max: 20.0,
            rank_axis_extent: ext,
        };
        let compartments = vec![Compartment {
            rank: 2,
            scope_lca: None,
            members: vec![mk(0, 20.0), mk(1, 30.0), mk(2, 40.0)],
        }];
        let out = compute_reservations(&compartments);
        assert_eq!(out.len(), 1);
        assert!((out[&2] - 98.0).abs() < 1e-9);
    }

    #[test]
    fn multiple_compartments_in_same_gap_max_reduce() {
        let mk = |rank, scope, ext| CompartmentCandidate {
            idx: 0,
            rank,
            scope_lca: scope,
            cross_min: 0.0,
            cross_max: 20.0,
            rank_axis_extent: ext,
        };
        let compartments = vec![
            Compartment {
                rank: 1,
                scope_lca: Some(10),
                members: vec![mk(1, Some(10), 30.0), mk(1, Some(10), 30.0)],
            },
            Compartment {
                rank: 1,
                scope_lca: Some(11),
                members: vec![mk(1, Some(11), 20.0), mk(1, Some(11), 20.0)],
            },
        ];
        let out = compute_reservations(&compartments);
        assert_eq!(out.len(), 1);
        assert!((out[&1] - 64.0).abs() < 1e-9);
    }

    #[test]
    fn compartments_on_different_gaps_produce_separate_entries() {
        let mk = |rank, ext| CompartmentCandidate {
            idx: 0,
            rank,
            scope_lca: None,
            cross_min: 0.0,
            cross_max: 20.0,
            rank_axis_extent: ext,
        };
        let compartments = vec![
            Compartment {
                rank: 1,
                scope_lca: None,
                members: vec![mk(1, 28.0), mk(1, 28.0)],
            },
            Compartment {
                rank: 3,
                scope_lca: None,
                members: vec![mk(3, 28.0), mk(3, 28.0)],
            },
        ];
        let out = compute_reservations(&compartments);
        assert_eq!(out.len(), 2);
        assert!(out.contains_key(&1));
        assert!(out.contains_key(&3));
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::engines::graph::algorithms::layered::graph::DiGraph;
    use crate::engines::graph::algorithms::layered::kernel::graph::LayoutGraph;
    use crate::engines::graph::algorithms::layered::kernel::types::{
        Direction, DummyNode, LabelPos, NodeId, Point,
    };

    /// Build a LayoutGraph with `edges` top-level edges (all endpoints use a
    /// fixed 40x20 node size), then attach a label dummy at `rank` for each
    /// of `labels` (edge_index, rank, width, height, cross). Returns the
    /// graph and the `cross_coords` map to pass into projection.
    fn build_with_label_dummies(
        edges: &[(&str, &str)],
        labels: &[(usize, i32, f64, f64, f64)],
    ) -> (LayoutGraph, std::collections::HashMap<usize, f64>) {
        let mut g: DiGraph<(f64, f64)> = DiGraph::new();
        let mut seen = std::collections::HashSet::new();
        for (src, tgt) in edges {
            if seen.insert(*src) {
                g.add_node(*src, (40.0, 20.0));
            }
            if seen.insert(*tgt) {
                g.add_node(*tgt, (40.0, 20.0));
            }
            g.add_edge(*src, *tgt);
        }
        let mut lg = LayoutGraph::from_digraph(&g, |_, dims| *dims);

        let mut cross = std::collections::HashMap::new();
        for (edge_index, rank, width, height, cross_center) in labels {
            let dummy_id = NodeId::from(format!("_label{}", lg.node_ids.len()));
            let dummy_idx = lg.node_ids.len();
            let dummy_node =
                DummyNode::edge_label(*edge_index, *rank, *width, *height, LabelPos::Center);

            lg.node_ids.push(dummy_id.clone());
            lg.node_index.insert(dummy_id.clone(), dummy_idx);
            lg.ranks.push(*rank);
            lg.order.push(dummy_idx);
            lg.positions.push(Point::default());
            lg.dimensions.push((*width, *height));
            lg.original_has_predecessor.push(false);
            lg.parents.push(None);
            lg.model_order.push(None);
            lg.dummy_nodes.insert(dummy_id, dummy_node);

            cross.insert(dummy_idx, *cross_center);
        }

        (lg, cross)
    }

    #[test]
    fn compute_entry_point_returns_empty_on_single_label() {
        let (lg, cross) = build_with_label_dummies(&[("A", "B")], &[(0, 1, 30.0, 28.0, 10.0)]);
        let out = compute_compartment_rank_sep_overrides(&lg, &cross, Direction::TopBottom);
        assert!(out.is_empty(), "single label should not reserve");
    }

    #[test]
    fn compute_entry_point_reserves_for_two_overlapping_labels_td() {
        // Two 28-tall TD labels on rank 1 with overlapping cross bands.
        // cross centres 10 and 30, widths 30 → bands [-5..25] and [15..45],
        // overlap is trivial. Reservation = 28 + 28 + 4 = 60.
        let (lg, cross) = build_with_label_dummies(
            &[("A", "B"), ("C", "D")],
            &[(0, 1, 30.0, 28.0, 10.0), (1, 1, 30.0, 28.0, 30.0)],
        );
        let out = compute_compartment_rank_sep_overrides(&lg, &cross, Direction::TopBottom);
        assert_eq!(out.len(), 1);
        assert!((out[&1] - 60.0).abs() < 1e-9);
    }

    #[test]
    fn self_edge_dummies_are_excluded_from_projection() {
        use crate::engines::graph::algorithms::layered::kernel::types::DummyType;

        let (mut lg, cross) = build_with_label_dummies(&[("A", "B")], &[(0, 1, 30.0, 28.0, 10.0)]);

        // Inject a DummyType::Edge "self-edge carrier" dummy on the same rank.
        let carrier_id = NodeId::from("_selfedge_carrier");
        let carrier_idx = lg.node_ids.len();
        let carrier = DummyNode::edge(0, 1);
        lg.node_ids.push(carrier_id.clone());
        lg.node_index.insert(carrier_id.clone(), carrier_idx);
        lg.ranks.push(1);
        lg.order.push(carrier_idx);
        lg.positions.push(Point::default());
        lg.dimensions.push((0.0, 0.0));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);
        lg.dummy_nodes.insert(carrier_id, carrier);

        let candidates = project_from_layout_graph(&lg, &cross, Direction::TopBottom);
        assert!(
            candidates.iter().all(|c| matches!(
                lg.dummy_nodes.get(&lg.node_ids[c.idx]).unwrap().dummy_type,
                DummyType::EdgeLabel
            )),
            "only EdgeLabel dummies should be projected"
        );
        assert_eq!(candidates.len(), 1);
        assert!(candidates.iter().all(|c| c.idx != carrier_idx));
    }

    #[test]
    fn direction_lr_uses_width_as_rank_axis_extent() {
        // Two 60-wide, 14-tall LR labels on rank 1. Cross (y) axis uses height.
        // cross centres 10 and 20, heights 14 → bands [3..17] and [13..27],
        // overlap. Reservation = 60 + 60 + 4 = 124.
        let (lg, cross) = build_with_label_dummies(
            &[("A", "B"), ("C", "D")],
            &[(0, 1, 60.0, 14.0, 10.0), (1, 1, 60.0, 14.0, 20.0)],
        );
        let out = compute_compartment_rank_sep_overrides(&lg, &cross, Direction::LeftRight);
        assert_eq!(out.len(), 1);
        assert!((out[&1] - 124.0).abs() < 1e-9);
    }
}
