//! Label-lane packing types and compartment grouping for Algorithm C.
//!
//! This module introduces the core data structures for interval-track lane
//! packing of edge labels. `LabelDescriptor` captures the axis/cross bands,
//! direction sign, and geometry of each labeled edge. `LabelCompartment`
//! groups descriptors that share a scope parent and have overlapping
//! cross-bands. `group_label_compartments` partitions descriptors into
//! compartments using an iterative merge with `LANE_GAP` slack.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::labels::arc_length_midpoint;
use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Graph};

/// Gap between adjacent label lanes within a compartment.
// Exposed `pub(crate)` for Plan 0150 kernel drift-detection test in
// `kernel::compartment_spacing`. The kernel intentionally duplicates this
// value as `INTER_TRACK_GAP` to avoid a kernel → routing dependency; the
// drift-detection test pins the two together.
pub(crate) const LANE_GAP: f64 = 4.0;

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
}

/// Partition label descriptors into compartments by scope parent and
/// overlapping cross-bands (with `LANE_GAP` slack).
pub(super) fn group_label_compartments(descriptors: Vec<LabelDescriptor>) -> Vec<LabelCompartment> {
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
            compartments.push(LabelCompartment { members });
        }
    }

    compartments
}

/// Pack label descriptors within a compartment onto signed integer tracks.
///
/// Sweep-line packing iterates members in `(axis_min, edge_index)` order.
/// Each member tries track 0 first, then `[sign·1, -sign·1, sign·2, ...]`
/// so reciprocal pairs and same-direction siblings get opposite-sign
/// fallbacks when track 0 is unavailable.
///
/// A track is "available" for a descriptor when no previous occupant of
/// that track has `axis_max + LANE_GAP > desc.axis_min` (i.e., the axis
/// bands don't overlap). Single-member compartments and multi-member
/// compartments with non-overlapping axis bands all land on track 0
/// (no displacement) — only members whose axis bands actually conflict
/// get pushed onto non-zero tracks.
pub(super) fn pack_signed_tracks(compartment: &LabelCompartment) -> HashMap<usize, i32> {
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
        let track = find_or_open_track(&last_end, desc);
        last_end.insert(track, desc.axis_max);
        out.insert(desc.edge_index, track);
    }
    out
}

fn find_or_open_track(last_end: &BTreeMap<i32, f64>, desc: &LabelDescriptor) -> i32 {
    // Candidate iterator yields [0, sign*1, -sign*1, sign*2, ...] —
    // track 0 first, then alternating non-zero tracks. The iterator is
    // unbounded by construction, so the packer scales to any
    // compartment size that fits in i32.
    candidate_track_order(desc.direction_sign)
        .find(|k| {
            last_end
                .get(k)
                .is_none_or(|&end| end + LANE_GAP <= desc.axis_min)
        })
        .expect("candidate_track_order is unbounded")
}

fn candidate_track_order(sign: i32) -> impl Iterator<Item = i32> {
    // Yields: 0, sign*1, -sign*1, sign*2, -sign*2, sign*3, ...
    // Track 0 is preferred — labels stay on their original path when
    // possible. Non-zero tracks are reached only when track 0 is
    // already occupied by an axis-band-overlapping descriptor, so the
    // packer never displaces a label that doesn't need to be displaced.
    std::iter::once(0).chain((1..).flat_map(move |k| [sign * k, -sign * k]))
}

/// Per-edge outcome of the lane assignment pass.
///
/// Plan 0149 extended this with `label_step`, `compartment_id`, and
/// `midpoint` so the post-lane re-wrap pass in
/// `crate::graph::routing::label_rewrap` can run its bounded fixed-point
/// without re-entering descriptor building. If `label_rewrap` is refactored
/// out or the fixed-point is removed, those three fields can be dropped.
#[derive(Debug, Clone)]
pub(super) struct LabelTrackOutcome {
    pub label_center: FPoint,
    pub label_rect: FRect,
    pub track: i32,
    pub adjusted_path: Vec<FPoint>,
    /// Number of members in the **axis-conflict sub-cluster** this edge
    /// belongs to. `1` means singleton sub-cluster (nothing axis-conflicts
    /// with it, so the lane pass picked track 0 with no displacement);
    /// `>= 2` means the lane pass placed it as part of a coordinated
    /// stack. Forwarded onto `EdgeLabelGeometry.compartment_size` so the
    /// SVG renderer can distinguish singleton track-0 (revalidate
    /// against the edge path) from coordinated multi-member track-0
    /// (trust the shared-anchor placement).
    pub compartment_size: usize,
    /// Number of members in the **cross-band compartment** this edge
    /// belongs to, before the axis-conflict split. Used by the wire-up
    /// in `routing::stage` to decide whether to adopt the lane pass's
    /// `label_center` (which for singleton sub-clusters equals the edge's
    /// arc-length midpoint) or fall through to the engine's
    /// `label_position`. Pre-split code conflated the two sizes; keeping
    /// the cross-band size preserves wire-up semantics for singleton
    /// sub-clusters that share a compartment with other labelled edges.
    pub full_compartment_size: usize,
    /// Per-compartment lane step (max axis-projected extent + `LANE_GAP`,
    /// floored to `MIN_LABEL_LANE_STEP`). Shared by every member of the
    /// compartment — identical across all outcomes with the same
    /// `compartment_id`.
    ///
    /// Read by `label_rewrap::re_wrap_labels_for_lane_fit` as the overflow
    /// budget for each edge's label width. The rewrap module may mutate
    /// this field in-place across fixed-point iterations when re-wrap
    /// grows the axis-projected extent (TD re-wrap → more lines → taller
    /// label → larger `label_step`).
    pub label_step: f64,
    /// Midpoint of the occupied track range for this compartment,
    /// subtracted from `track` before scaling by `label_step` so the
    /// sub-cluster sits symmetric around the shared anchor. Identical
    /// across all outcomes with the same `compartment_id`.
    ///
    /// For a 2-member same-direction sub-cluster with tracks `[0, +1]`
    /// this is `0.5`, producing centered tracks `[-0.5, +0.5]`. For a
    /// reciprocal `[0, -1]` it is `-0.5`, producing `[+0.5, -0.5]`.
    /// `[0, +1, -1]` stays `0.0` / `[0, +1, -1]` (unchanged). Singleton
    /// sub-clusters always store `0.0`.
    ///
    /// `label_rewrap::apply_new_label_steps` must use this when
    /// recomputing `label_center` after `label_step` mutates so the
    /// re-wrap pass preserves the symmetrized layout.
    pub track_center: f64,
    /// Dense per-run compartment key assigned in compartment-iteration
    /// order. Edges that share an ID share a compartment, which means they
    /// share `label_step` and must be recentered together when the fixed-
    /// point pass recomputes that budget. Not stable across renders — do
    /// not serialize or compare across invocations.
    pub compartment_id: usize,
    /// Pre-lane-shift anchor used by `label_rewrap` to recompute
    /// `label_center = midpoint + track_vector(direction) * label_step`
    /// from scratch after `label_step` mutates.
    ///
    /// For singleton compartments this is the edge path's
    /// `arc_length_midpoint` as built by `build_label_descriptors`. For
    /// multi-member compartments `recenter_compartment_to_shared_anchor`
    /// overwrites the primary-axis coordinate with the mean across members
    /// so all members share a common anchor; the cross-axis coordinate
    /// stays per-edge. Re-
    /// centering off the previous `label_center` accumulates floating-
    /// point error across iterations, and the rewrap fixed-point can run
    /// up to 3 rounds.
    pub midpoint: FPoint,
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
    let compartments = group_label_compartments(descriptors);
    let mut outcomes: HashMap<usize, LabelTrackOutcome> = HashMap::new();

    // Dense id counter assigned in iteration order. Each axis-conflict
    // sub-cluster within a compartment gets its own id so `label_step` is
    // sized per sub-cluster and the rewrap fixed-point groups the right
    // members together. Not stable across renders.
    let mut next_id: usize = 0;
    for compartment in compartments {
        let full_compartment_size = compartment.members.len();
        for mut subcluster in split_into_axis_conflict_subclusters(compartment.members) {
            let compartment_id = next_id;
            next_id += 1;

            recenter_members_to_shared_anchor(&mut subcluster, direction);
            let tracks = pack_signed_tracks_for_members(&subcluster);

            // Per-sub-cluster LABEL_LANE_STEP from the max axis-band
            // extent. Labels stack along the primary axis (Y for TD/BU,
            // X for LR/RL), so the step needs to clear the axis-projected
            // dimension of the largest label plus a small gap.
            let max_axis = subcluster
                .iter()
                .map(|m| m.axis_max - m.axis_min)
                .fold(0.0_f64, f64::max);
            let label_step = (max_axis + LANE_GAP).max(MIN_LABEL_LANE_STEP);
            let path_step = label_step * PATH_LANE_RATIO;

            let compartment_size = subcluster.len();
            // Symmetrize offsets around the midpoint of the occupied
            // track range so the sub-cluster sits symmetric around the
            // shared anchor. Without this, a 2-member same-direction
            // sub-cluster packs to tracks [0, +1] — extent `label_step`
            // above the anchor, zero below — and `label_clamp` can yank
            // the +1 member back into the available gap, re-overlapping
            // the other member. With the midpoint subtraction,
            // [0, +1] becomes [-0.5, +0.5] and the sub-cluster stays
            // centred. [-1, 0, +1] stays [-1, 0, +1] (unchanged);
            // reciprocal [0, -1] becomes [+0.5, -0.5].
            let track_center = if compartment_size > 1 {
                let (min_track, max_track) = tracks
                    .values()
                    .fold((i32::MAX, i32::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
                (min_track as f64 + max_track as f64) / 2.0
            } else {
                0.0
            };
            for desc in &subcluster {
                let track = tracks[&desc.edge_index];
                let centered_track = track as f64 - track_center;
                let label_offset = centered_track * label_step;
                let path_offset = centered_track * path_step;
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
                        compartment_size,
                        full_compartment_size,
                        label_step,
                        track_center,
                        compartment_id,
                        midpoint: desc.midpoint,
                    },
                );
            }
        }
    }

    outcomes
}

/// Split a compartment into axis-conflict sub-clusters via a sort-then-
/// merge sweep along the primary axis. Two members conflict when their
/// axis bands overlap with `LANE_GAP` slack; pack_signed_tracks would
/// push them onto different tracks. Members that don't axis-conflict
/// with anyone remain singletons even when the compartment has multiple
/// members (their labels sit along the same cross-band but at distinct
/// axis positions, so no lane coordination is needed).
fn split_into_axis_conflict_subclusters(
    mut members: Vec<LabelDescriptor>,
) -> Vec<Vec<LabelDescriptor>> {
    members.sort_by(|a, b| {
        a.axis_min
            .partial_cmp(&b.axis_min)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut clusters: Vec<Vec<LabelDescriptor>> = Vec::new();
    let mut current: Vec<LabelDescriptor> = Vec::new();
    let mut current_max: f64 = f64::NEG_INFINITY;
    for desc in members {
        if current.is_empty() || desc.axis_min <= current_max + LANE_GAP {
            current_max = current_max.max(desc.axis_max);
            current.push(desc);
        } else {
            clusters.push(std::mem::take(&mut current));
            current_max = desc.axis_max;
            current.push(desc);
        }
    }
    if !current.is_empty() {
        clusters.push(current);
    }
    clusters
}

fn pack_signed_tracks_for_members(members: &[LabelDescriptor]) -> HashMap<usize, i32> {
    pack_signed_tracks(&LabelCompartment {
        members: members.to_vec(),
    })
}

fn recenter_members_to_shared_anchor(members: &mut [LabelDescriptor], direction: Direction) {
    if members.len() < 2 {
        return;
    }
    let n = members.len() as f64;
    let anchor = members
        .iter()
        .map(|m| match direction {
            Direction::TopDown | Direction::BottomTop => m.midpoint.y,
            Direction::LeftRight | Direction::RightLeft => m.midpoint.x,
        })
        .sum::<f64>()
        / n;

    for member in members.iter_mut() {
        let delta = anchor
            - match direction {
                Direction::TopDown | Direction::BottomTop => member.midpoint.y,
                Direction::LeftRight | Direction::RightLeft => member.midpoint.x,
            };
        if delta == 0.0 {
            continue;
        }
        match direction {
            Direction::TopDown | Direction::BottomTop => {
                member.midpoint = FPoint::new(member.midpoint.x, anchor);
                member.label_rect = FRect::new(
                    member.label_rect.x,
                    member.label_rect.y + delta,
                    member.label_rect.width,
                    member.label_rect.height,
                );
                member.axis_min += delta;
                member.axis_max += delta;
            }
            Direction::LeftRight | Direction::RightLeft => {
                member.midpoint = FPoint::new(anchor, member.midpoint.y);
                member.label_rect = FRect::new(
                    member.label_rect.x + delta,
                    member.label_rect.y,
                    member.label_rect.width,
                    member.label_rect.height,
                );
                member.axis_min += delta;
                member.axis_max += delta;
            }
        }
    }
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
        // Plan 0149 (#237): descriptor must reflect the same dims the SVG
        // renderer will emit, otherwise the packer sizes compartments
        // against unwrapped single-line heights while the actual rects
        // stack multi-line rendered labels. Pre-fix, `label_step` was
        // 32 (unwrapped h=28 + LANE_GAP) but the rendered rect was 52
        // tall — tracks at ±32 from midpoint let 52-tall rects overlap
        // the track-0 band, which is exactly the `long_reciprocal_labels`
        // failure. Prefer `wrapped_label_lines` when present; fall back
        // to single-line measurement for edges that opted out of the
        // pre-engine wrap (dagre-parity mode).
        let (w, h) = match edge.wrapped_label_lines.as_deref() {
            Some(lines) => metrics.edge_label_dimensions_wrapped(lines),
            None => metrics.edge_label_dimensions(label_text),
        };

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
/// Returns the **lowest common ancestor** of the two nodes in the
/// subgraph hierarchy — not just the immediate parent. An edge from
/// `A` in `subgraph(LEFT)` to `B` in `subgraph(RIGHT)` where `LEFT`
/// and `RIGHT` are both children of `OUTER` returns `Some("OUTER")`.
/// Returns `None` only when the two endpoints share no common ancestor
/// subgraph (e.g., they live under entirely separate top-level
/// subgraphs, or one is at top-level).
///
/// Without LCA-aware scoping, edges that are visually isolated inside
/// different outer subgraphs would all collapse to the top-level `None`
/// scope and could be packed into the same compartment — producing
/// label shifts on edges that should never interact.
fn compute_shared_parent(
    diagram: &Graph,
    geometry: &GraphGeometry,
    from: &str,
    to: &str,
) -> Option<String> {
    let from_chain = subgraph_ancestor_chain(diagram, geometry, from);
    if from_chain.is_empty() {
        return None;
    }
    let from_set: HashSet<&str> = from_chain.iter().map(String::as_str).collect();
    // Walk up `to`'s chain (deepest → shallowest); return the first
    // ancestor that's also in `from`'s chain. This is the LCA.
    subgraph_ancestor_chain(diagram, geometry, to)
        .into_iter()
        .find(|sg| from_set.contains(sg.as_str()))
}

/// Build the chain of subgraph ancestors for a node, from deepest
/// (immediate parent) to shallowest (top-level subgraph). Returns an
/// empty vector if the node is at top-level (no parent subgraph).
fn subgraph_ancestor_chain(
    diagram: &Graph,
    geometry: &GraphGeometry,
    node_id: &str,
) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = parent_of(diagram, geometry, node_id);
    while let Some(sg_id) = current {
        let next = diagram
            .subgraphs
            .get(&sg_id)
            .and_then(|sg| sg.parent.clone());
        chain.push(sg_id);
        current = next;
    }
    chain
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
            midpoint: FPoint::new((axis.0 + axis.1) / 2.0, (cross.0 + cross.1) / 2.0),
            label_rect: FRect::new(axis.0, cross.0, axis.1 - axis.0, cross.1 - cross.0),
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
        let compartments = group_label_compartments(vec![a, b]);
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
        let compartments = group_label_compartments(vec![a, b]);
        assert_eq!(compartments.len(), 1);
        assert_eq!(compartments[0].members.len(), 2);
    }

    #[test]
    fn group_label_compartments_separates_non_overlapping_cross_bands_same_scope() {
        let a = make_descriptor(0, Some("A"), (10.0, 50.0), (100.0, 120.0), 1);
        let b = make_descriptor(1, Some("A"), (15.0, 55.0), (200.0, 220.0), -1);
        let compartments = group_label_compartments(vec![a, b]);
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
        let compartments = group_label_compartments(vec![a, b]);
        assert_eq!(compartments.len(), 1, "within LANE_GAP slack -> merge");
    }

    #[test]
    fn group_label_compartments_empty_input() {
        let compartments = group_label_compartments(vec![]);
        assert!(compartments.is_empty());
    }

    #[test]
    fn pack_signed_tracks_assigns_zero_track_to_singleton_compartment() {
        let compartment = LabelCompartment {
            members: vec![make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1)],
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
        };
        let tracks = pack_signed_tracks(&compartment);
        // Candidate order is [0, sign*1, -sign*1, ...]. Forward (sign=+1)
        // sorts first by edge_index tie-break, lands on track 0. Reverse
        // (sign=-1) tries 0 (occupied with overlapping axis), then -1
        // (empty) — opposite-side displacement. The pair is still visually
        // separated even though only one shifts.
        assert_eq!(tracks[&0], 0);
        assert_eq!(tracks[&1], -1);
    }

    #[test]
    fn pack_signed_tracks_packs_three_same_direction_overlapping_axis() {
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1),
                make_descriptor(1, None, (20.0, 60.0), (100.0, 120.0), 1),
                make_descriptor(2, None, (30.0, 70.0), (100.0, 120.0), 1),
            ],
        };
        let tracks = pack_signed_tracks(&compartment);
        // Candidate order for sign=+1 is [0, 1, -1, 2, ...]. Three
        // overlapping same-sign descriptors land on tracks [0, 1, -1].
        let mut values: Vec<_> = tracks.values().copied().collect();
        values.sort();
        assert_eq!(values, vec![-1, 0, 1]);
        assert_eq!(tracks[&0], 0); // lowest axis_min stays on track 0
        assert_eq!(tracks[&1], 1);
        assert_eq!(tracks[&2], -1);
    }

    #[test]
    fn pack_signed_tracks_breaks_ties_by_edge_index() {
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(1, None, (10.0, 50.0), (100.0, 120.0), 1),
                make_descriptor(0, None, (10.0, 50.0), (100.0, 120.0), 1),
            ],
        };
        let tracks = pack_signed_tracks(&compartment);
        // Tie-break on (axis_min, edge_index): edge 0 sorted first,
        // lands on track 0. Edge 1 tries 0 (occupied), gets +1.
        assert_eq!(tracks[&0], 0);
        assert_eq!(tracks[&1], 1);
    }

    #[test]
    fn pack_signed_tracks_non_overlapping_axis_can_reuse_track() {
        // Two descriptors with non-overlapping axis ranges and same sign.
        // Both fit on track 0 because their axis bands don't overlap —
        // no displacement needed for either one.
        let compartment = LabelCompartment {
            members: vec![
                make_descriptor(0, None, (10.0, 30.0), (100.0, 120.0), 1),
                make_descriptor(1, None, (40.0, 60.0), (100.0, 120.0), 1),
            ],
        };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks[&0], 0);
        assert_eq!(
            tracks[&1], 0,
            "non-overlapping axis ranges should both stay on track 0"
        );
    }

    #[test]
    fn pack_signed_tracks_handles_ten_same_sign_without_panic() {
        let members: Vec<_> = (0..10)
            .map(|i| {
                make_descriptor(
                    i,
                    None,
                    (i as f64 * 5.0, i as f64 * 5.0 + 40.0),
                    (100.0, 120.0),
                    1,
                )
            })
            .collect();
        let compartment = LabelCompartment { members };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks.len(), 10);
        // First member lands on track 0; subsequent overlapping members
        // get pushed to non-zero tracks. The last member's axis_min
        // (45) clears the first member's axis_max (40) + LANE_GAP (4),
        // so it can wrap back onto track 0.
        assert_eq!(tracks[&0], 0);
        assert!(tracks.values().filter(|&&t| t != 0).count() >= 1);
    }

    #[test]
    fn pack_signed_tracks_handles_dense_compartment_above_64_members() {
        // Regression: an earlier `.take(64)` cap in the candidate iterator
        // panicked on valid dense inputs (e.g., 65+ parallel labeled
        // edges from the same node pair, all with overlapping axis bands).
        // The packer must scale to any compartment size that fits in i32.
        const N: usize = 65;
        let members: Vec<_> = (0..N)
            .map(|i| make_descriptor(i, None, (10.0, 50.0), (100.0, 120.0), 1))
            .collect();
        let compartment = LabelCompartment { members };
        let tracks = pack_signed_tracks(&compartment);
        assert_eq!(tracks.len(), N);
        // First member lands on track 0; other 64 get distinct non-zero tracks.
        assert_eq!(tracks[&0], 0);
        // All tracks must be unique (axis bands fully overlap, so the
        // packer cannot reuse any track within this compartment).
        let mut sorted: Vec<i32> = tracks.values().copied().collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            N,
            "expected {N} distinct tracks for fully-overlapping members"
        );
    }

    #[test]
    fn group_compartments_isolates_edges_in_different_outer_subgraphs_via_lca() {
        // Regression: previously `compute_shared_parent` only compared
        // immediate parents. An edge from `A1 in LEFT1` to `B1 in RIGHT1`
        // would get scope `None` because LEFT1 != RIGHT1, even though
        // both nodes share OUTER1 as a common ancestor. The same for an
        // unrelated edge under OUTER2. Both edges then collapsed to the
        // top-level None scope and got packed together, producing label
        // shifts on edges that should never interact.
        //
        // With LCA-aware scoping, edge 0 lives under OUTER1 and edge 1
        // lives under OUTER2, so they end up in two singleton
        // compartments and neither shifts.
        let edge_under_outer1 = make_descriptor(0, Some("OUTER1"), (10.0, 50.0), (100.0, 120.0), 1);
        let edge_under_outer2 = make_descriptor(1, Some("OUTER2"), (10.0, 50.0), (100.0, 120.0), 1);
        let compartments = group_label_compartments(vec![edge_under_outer1, edge_under_outer2]);
        assert_eq!(
            compartments.len(),
            2,
            "edges under different outer subgraphs must NOT share a compartment"
        );
        for c in &compartments {
            assert_eq!(c.members.len(), 1, "each compartment must be a singleton");
            let tracks = pack_signed_tracks(c);
            assert_eq!(
                tracks.values().copied().collect::<Vec<_>>(),
                vec![0],
                "singleton must produce track 0 (no shift)"
            );
        }
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
        assert!(
            has_x_shift,
            "interior should shift on cross axis (x for TD)"
        );
    }

    fn make_descriptor_midpoint(
        edge_index: usize,
        midpoint: FPoint,
        label_w: f64,
        label_h: f64,
        sign: i32,
        direction: Direction,
    ) -> LabelDescriptor {
        let (axis_dim, cross_dim, axis_center, cross_center) = match direction {
            Direction::TopDown | Direction::BottomTop => (label_h, label_w, midpoint.y, midpoint.x),
            Direction::LeftRight | Direction::RightLeft => {
                (label_w, label_h, midpoint.x, midpoint.y)
            }
        };
        LabelDescriptor {
            edge_index,
            scope_parent: None,
            axis_min: axis_center - axis_dim / 2.0,
            axis_max: axis_center + axis_dim / 2.0,
            cross_min: cross_center - cross_dim / 2.0,
            cross_max: cross_center + cross_dim / 2.0,
            direction_sign: sign,
            midpoint,
            label_rect: FRect::new(
                midpoint.x - label_w / 2.0,
                midpoint.y - label_h / 2.0,
                label_w,
                label_h,
            ),
        }
    }

    #[test]
    fn recenter_members_to_shared_anchor_is_noop_for_singleton() {
        let d = make_descriptor_midpoint(
            0,
            FPoint::new(50.0, 100.0),
            40.0,
            20.0,
            1,
            Direction::TopDown,
        );
        let mut members = vec![d.clone()];
        recenter_members_to_shared_anchor(&mut members, Direction::TopDown);
        assert_eq!(members[0].midpoint, d.midpoint);
        assert_eq!(members[0].axis_min, d.axis_min);
        assert_eq!(members[0].axis_max, d.axis_max);
    }

    #[test]
    fn recenter_members_to_shared_anchor_td_shifts_y_only() {
        let a = make_descriptor_midpoint(
            0,
            FPoint::new(20.0, 100.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let b = make_descriptor_midpoint(
            1,
            FPoint::new(80.0, 110.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let mut members = vec![a.clone(), b.clone()];
        recenter_members_to_shared_anchor(&mut members, Direction::TopDown);
        assert_eq!(members[0].midpoint.y, 105.0, "anchor = mean(100, 110)");
        assert_eq!(members[1].midpoint.y, 105.0);
        assert_eq!(members[0].midpoint.x, 20.0, "x preserved per-edge");
        assert_eq!(members[1].midpoint.x, 80.0, "x preserved per-edge");
        // axis bands shift by the y delta
        assert_eq!(members[0].axis_min, a.axis_min + 5.0);
        assert_eq!(members[0].axis_max, a.axis_max + 5.0);
        assert_eq!(members[1].axis_min, b.axis_min - 5.0);
        assert_eq!(members[1].axis_max, b.axis_max - 5.0);
    }

    #[test]
    fn recenter_members_to_shared_anchor_lr_shifts_x_only() {
        let a = make_descriptor_midpoint(
            0,
            FPoint::new(100.0, 20.0),
            28.0,
            40.0,
            1,
            Direction::LeftRight,
        );
        let b = make_descriptor_midpoint(
            1,
            FPoint::new(110.0, 80.0),
            28.0,
            40.0,
            1,
            Direction::LeftRight,
        );
        let mut members = vec![a, b];
        recenter_members_to_shared_anchor(&mut members, Direction::LeftRight);
        assert_eq!(members[0].midpoint.x, 105.0);
        assert_eq!(members[1].midpoint.x, 105.0);
        assert_eq!(members[0].midpoint.y, 20.0);
        assert_eq!(members[1].midpoint.y, 80.0);
    }

    #[test]
    fn split_into_axis_conflict_subclusters_isolates_non_conflicting_members() {
        // Two members in same compartment by cross-band but with Y bands
        // 100 px apart — no axis conflict. Each becomes its own cluster.
        let a = make_descriptor_midpoint(
            0,
            FPoint::new(50.0, 100.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let b = make_descriptor_midpoint(
            1,
            FPoint::new(60.0, 250.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let clusters = split_into_axis_conflict_subclusters(vec![a, b]);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 1);
        assert_eq!(clusters[1].len(), 1);
    }

    #[test]
    fn split_into_axis_conflict_subclusters_merges_overlapping_axis_bands() {
        let a = make_descriptor_midpoint(
            0,
            FPoint::new(50.0, 100.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let b = make_descriptor_midpoint(
            1,
            FPoint::new(60.0, 110.0),
            40.0,
            28.0,
            1,
            Direction::TopDown,
        );
        let clusters = split_into_axis_conflict_subclusters(vec![a, b]);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 2);
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
