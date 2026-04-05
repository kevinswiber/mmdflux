use std::collections::{HashMap, HashSet};

use super::constants::MIN_PORT_CORNER_INSET_FORWARD;
use super::endpoints::{endpoint_rect, endpoint_rect_and_shape};
use crate::graph::attachment::{
    Face, OverflowSide, fan_in_overflow_face_for_slot, fan_in_primary_face_capacity,
    fan_in_primary_target_face,
};
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::FRect;
use crate::graph::{Direction, Shape};

const MIN_FAN_IN_PRIMARY_SLOT_SPACING: f64 = 16.0;
const FAN_PRIMARY_SIDE_BAND_DEPTH_MARGIN: f64 = 0.1;

#[derive(Default)]
pub(super) struct FanInTargetOverflowContext {
    pub(super) target_face_for_edge: HashMap<usize, Face>,
    pub(super) target_fraction_for_edge: HashMap<usize, f64>,
    pub(super) target_primary_channel_depth_for_edge: HashMap<usize, f64>,
    pub(super) overflow_targeted: HashSet<String>,
    pub(super) targets_with_backward_inbound: HashSet<String>,
}

#[derive(Default)]
pub(super) struct FanOutSourceStaggerContext {
    pub(super) source_primary_channel_depth_for_edge: HashMap<usize, f64>,
    pub(super) source_fraction_for_edge: HashMap<usize, f64>,
}

pub(super) fn fan_in_target_overflow_context(
    geometry: &GraphGeometry,
    direction: Direction,
    visible_edge_count: usize,
) -> FanInTargetOverflowContext {
    let mut incoming_by_target: HashMap<String, Vec<&LayoutEdge>> = HashMap::new();
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| edge.index < visible_edge_count)
    {
        incoming_by_target
            .entry(edge.to.clone())
            .or_default()
            .push(edge);
    }

    let primary_face = fan_in_primary_target_face(direction);
    let mut target_face_for_edge: HashMap<usize, Face> = HashMap::new();
    let mut target_fraction_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut target_primary_channel_depth_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut overflow_targeted: HashSet<String> = HashSet::new();
    let mut targets_with_backward_inbound: HashSet<String> = HashSet::new();
    const CENTER_EPS: f64 = 0.5;

    for (target_id, mut incoming_edges) in incoming_by_target {
        incoming_edges.sort_unstable_by_key(|edge| edge.index);
        let mut forward_edges: Vec<&LayoutEdge> = Vec::new();
        let mut backward_edge_count = 0usize;
        for edge in incoming_edges {
            if geometry.reversed_edges.contains(&edge.index) {
                backward_edge_count += 1;
            } else {
                forward_edges.push(edge);
            }
        }

        if backward_edge_count > 0 {
            targets_with_backward_inbound.insert(target_id.clone());
        }

        if forward_edges.len() <= 1 {
            continue;
        }

        let target_rect_and_shape = forward_edges.first().and_then(|edge| {
            endpoint_rect_and_shape(geometry, &edge.to, edge.to_subgraph.as_deref())
        });
        let target_rect = target_rect_and_shape.map(|(rect, _)| rect);
        let target_is_angular = target_rect_and_shape
            .is_some_and(|(_, shape)| matches!(shape, Shape::Diamond | Shape::Hexagon));
        let capacity = target_rect
            .as_ref()
            .map(|rect| adaptive_fan_in_primary_face_capacity(direction, rect))
            .unwrap_or_else(|| fan_in_primary_face_capacity(direction));

        forward_edges.sort_by(|a, b| {
            let a_cross = fan_in_source_cross_axis(geometry, a, direction);
            let b_cross = fan_in_source_cross_axis(geometry, b, direction);
            a_cross
                .total_cmp(&b_cross)
                .then_with(|| a.index.cmp(&b.index))
        });

        let primary_count = forward_edges.len().min(capacity);
        for edge in &forward_edges[..primary_count] {
            target_face_for_edge.insert(edge.index, primary_face);
        }

        if forward_edges.len() > capacity {
            overflow_targeted.insert(target_id);
            let overflow_edges = &forward_edges[capacity..];
            let target_cross = overflow_edges
                .first()
                .and_then(|edge| endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref()))
                .map(|rect| face_cross_axis(rect, direction))
                .unwrap_or(0.0);
            for (idx, edge) in overflow_edges.iter().enumerate() {
                let source_cross = fan_in_source_cross_axis(geometry, edge, direction);
                let overflow_slot = if source_cross < target_cross - CENTER_EPS {
                    OverflowSide::LeftOrTop
                } else if source_cross > target_cross + CENTER_EPS {
                    OverflowSide::RightOrBottom
                } else if idx % 2 == 0 {
                    OverflowSide::LeftOrTop
                } else {
                    OverflowSide::RightOrBottom
                };
                let face = fan_in_overflow_face_for_slot(direction, overflow_slot);
                target_face_for_edge.insert(edge.index, face);
            }
        }

        let mut edges_by_face: HashMap<Face, Vec<(usize, f64)>> = HashMap::new();
        for edge in &forward_edges {
            let Some(face) = target_face_for_edge.get(&edge.index).copied() else {
                continue;
            };
            let source_cross = fan_in_source_cross_axis(geometry, edge, direction);
            edges_by_face
                .entry(face)
                .or_default()
                .push((edge.index, source_cross));
        }

        for (face, mut face_edges) in edges_by_face {
            face_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            let count = face_edges.len();
            for (idx, (edge_index, _)) in face_edges.iter().enumerate() {
                let base_fraction = if count <= 1 {
                    0.5
                } else {
                    idx as f64 / (count - 1) as f64
                };
                let fraction = if target_is_angular
                    && face == primary_face
                    && matches!(direction, Direction::TopDown | Direction::BottomTop)
                {
                    remap_angular_fan_in_target_fraction(base_fraction, count)
                } else {
                    base_fraction
                };
                target_fraction_for_edge.insert(*edge_index, fraction);
            }
            if count > 1 {
                if face == primary_face {
                    let target_cross = target_rect
                        .as_ref()
                        .map(|rect| face_cross_axis(rect, direction))
                        .unwrap_or_else(|| {
                            if count % 2 == 1 {
                                face_edges[count / 2].1
                            } else {
                                (face_edges[count / 2 - 1].1 + face_edges[count / 2].1) / 2.0
                            }
                        });

                    let mut left_edges: Vec<(usize, f64)> = Vec::new();
                    let mut right_edges: Vec<(usize, f64)> = Vec::new();
                    let mut center_edges: Vec<(usize, f64)> = Vec::new();
                    for (edge_index, source_cross) in &face_edges {
                        if *source_cross < target_cross - CENTER_EPS {
                            left_edges.push((*edge_index, *source_cross));
                        } else if *source_cross > target_cross + CENTER_EPS {
                            right_edges.push((*edge_index, *source_cross));
                        } else {
                            center_edges.push((*edge_index, *source_cross));
                        }
                    }

                    left_edges.sort_by(|a, b| {
                        (target_cross - a.1)
                            .total_cmp(&(target_cross - b.1))
                            .then_with(|| a.0.cmp(&b.0))
                    });
                    right_edges.sort_by(|a, b| {
                        (a.1 - target_cross)
                            .total_cmp(&(b.1 - target_cross))
                            .then_with(|| a.0.cmp(&b.0))
                    });
                    center_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

                    let band_count = left_edges.len().max(right_edges.len());
                    for (band_index, (edge_index, _)) in left_edges.into_iter().enumerate() {
                        target_primary_channel_depth_for_edge.insert(
                            edge_index,
                            symmetric_side_band_depth(band_index, band_count),
                        );
                    }
                    for (band_index, (edge_index, _)) in right_edges.into_iter().enumerate() {
                        target_primary_channel_depth_for_edge.insert(
                            edge_index,
                            symmetric_side_band_depth(band_index, band_count),
                        );
                    }

                    if center_edges.len() == 1 {
                        target_primary_channel_depth_for_edge.insert(center_edges[0].0, 0.5);
                    } else if center_edges.len() > 1 {
                        let denom = center_edges.len() as f64 + 1.0;
                        for (idx, (edge_index, _)) in center_edges.into_iter().enumerate() {
                            target_primary_channel_depth_for_edge
                                .insert(edge_index, (idx as f64 + 1.0) / denom);
                        }
                    }
                } else {
                    for (idx, (edge_index, _)) in face_edges.iter().enumerate() {
                        let depth = idx as f64 / (count - 1) as f64;
                        target_primary_channel_depth_for_edge.insert(*edge_index, depth);
                    }
                }
            }
        }

        if let Some(target_rect) = target_rect {
            apply_near_aligned_primary_face_fraction_override(
                geometry,
                direction,
                primary_face,
                &target_rect,
                &forward_edges,
                &target_face_for_edge,
                &mut target_fraction_for_edge,
            );
        }
    }

    FanInTargetOverflowContext {
        target_face_for_edge,
        target_fraction_for_edge,
        target_primary_channel_depth_for_edge,
        overflow_targeted,
        targets_with_backward_inbound,
    }
}

pub(super) fn fan_out_source_stagger_context(
    geometry: &GraphGeometry,
    direction: Direction,
    visible_edge_count: usize,
) -> FanOutSourceStaggerContext {
    let mut outgoing_by_source: HashMap<String, Vec<&LayoutEdge>> = HashMap::new();
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| edge.index < visible_edge_count)
    {
        outgoing_by_source
            .entry(edge.from.clone())
            .or_default()
            .push(edge);
    }

    let mut source_primary_channel_depth_for_edge: HashMap<usize, f64> = HashMap::new();
    let mut source_fraction_for_edge: HashMap<usize, f64> = HashMap::new();
    const CENTER_EPS: f64 = 0.5;

    for (source_id, mut outgoing_edges) in outgoing_by_source {
        outgoing_edges.sort_unstable_by_key(|edge| edge.index);
        let mut forward_edges: Vec<&LayoutEdge> = Vec::new();
        for edge in outgoing_edges {
            if geometry.reversed_edges.contains(&edge.index) {
                continue;
            }
            forward_edges.push(edge);
        }
        if forward_edges.len() <= 1 {
            continue;
        }

        let source_cross = forward_edges
            .first()
            .and_then(|edge| endpoint_rect(geometry, &source_id, edge.from_subgraph.as_deref()))
            .map(|rect| face_cross_axis(rect, direction))
            .unwrap_or(0.0);

        let mut ordered_for_fraction: Vec<(usize, f64)> = forward_edges
            .iter()
            .map(|edge| {
                (
                    edge.index,
                    fan_out_target_cross_axis(geometry, edge, direction),
                )
            })
            .collect();
        ordered_for_fraction.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let angular_source = forward_edges
            .first()
            .and_then(|edge| {
                endpoint_rect_and_shape(geometry, &source_id, edge.from_subgraph.as_deref())
            })
            .is_some_and(|(_, shape)| matches!(shape, Shape::Diamond | Shape::Hexagon));
        let count = ordered_for_fraction.len();
        for (idx, (edge_index, _)) in ordered_for_fraction.iter().enumerate() {
            let base_fraction = if count <= 1 {
                0.5
            } else {
                idx as f64 / (count - 1) as f64
            };
            let fraction = if angular_source
                && matches!(direction, Direction::TopDown | Direction::BottomTop)
            {
                remap_angular_fan_out_source_fraction(base_fraction, count)
            } else {
                base_fraction
            };
            source_fraction_for_edge.insert(*edge_index, fraction);
        }

        let mut left_edges: Vec<(usize, f64)> = Vec::new();
        let mut right_edges: Vec<(usize, f64)> = Vec::new();
        let mut center_edges: Vec<(usize, f64)> = Vec::new();
        for edge in &forward_edges {
            let target_cross = fan_out_target_cross_axis(geometry, edge, direction);
            if target_cross < source_cross - CENTER_EPS {
                left_edges.push((edge.index, target_cross));
            } else if target_cross > source_cross + CENTER_EPS {
                right_edges.push((edge.index, target_cross));
            } else {
                center_edges.push((edge.index, target_cross));
            }
        }

        left_edges.sort_by(|a, b| {
            (source_cross - b.1)
                .total_cmp(&(source_cross - a.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        right_edges.sort_by(|a, b| {
            (b.1 - source_cross)
                .total_cmp(&(a.1 - source_cross))
                .then_with(|| a.0.cmp(&b.0))
        });
        center_edges.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

        let band_count = left_edges.len().max(right_edges.len());
        for (band_index, (edge_index, _)) in left_edges.into_iter().enumerate() {
            source_primary_channel_depth_for_edge.insert(
                edge_index,
                symmetric_side_band_depth(band_index, band_count),
            );
        }
        for (band_index, (edge_index, _)) in right_edges.into_iter().enumerate() {
            source_primary_channel_depth_for_edge.insert(
                edge_index,
                symmetric_side_band_depth(band_index, band_count),
            );
        }

        if center_edges.len() == 1 {
            source_primary_channel_depth_for_edge.insert(center_edges[0].0, 0.5);
        } else if center_edges.len() > 1 {
            let denom = center_edges.len() as f64 + 1.0;
            for (idx, (edge_index, _)) in center_edges.into_iter().enumerate() {
                source_primary_channel_depth_for_edge
                    .insert(edge_index, (idx as f64 + 1.0) / denom);
            }
        }
    }

    FanOutSourceStaggerContext {
        source_primary_channel_depth_for_edge,
        source_fraction_for_edge,
    }
}

/// Spread backward edge source ports that share the exact same departure
/// point.  Runs as a post-processing step after all paths are routed so the
/// actual departure face is known.  Only adjusts edges that truly overlap —
/// edges already on different faces or at different positions are left alone.
pub(super) fn spread_colocated_backward_source_ports(
    routed: &mut [crate::graph::geometry::RoutedEdgeGeometry],
    geometry: &crate::graph::geometry::GraphGeometry,
) {
    use super::path_utils::points_match;
    use crate::graph::space::FPoint;

    const CORNER_INSET: f64 = 4.0;
    // Minimum spacing between adjacent ports (px).
    const MIN_PORT_SPACING: f64 = 12.0;

    // Group backward edges by (source node, rounded start point).
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, edge) in routed.iter().enumerate() {
        if !edge.is_backward || edge.path.len() < 2 {
            continue;
        }
        groups.entry(edge.from.clone()).or_default().push(idx);
    }

    for edge_indices in groups.values() {
        if edge_indices.len() <= 1 {
            continue;
        }

        // Sub-group by co-located start point (same departure point within epsilon).
        let mut colocated: Vec<Vec<usize>> = Vec::new();
        for &idx in edge_indices {
            let start = routed[idx].path[0];
            let found = colocated.iter_mut().find(|group| {
                let representative = routed[group[0]].path[0];
                points_match(start, representative)
            });
            if let Some(group) = found {
                group.push(idx);
            } else {
                colocated.push(vec![idx]);
            }
        }

        for group in &colocated {
            if group.len() <= 1 {
                continue;
            }

            let representative_idx = group[0];
            let start = routed[representative_idx].path[0];
            let next = routed[representative_idx].path[1];

            // Determine departure axis: horizontal first segment means
            // vertical face (left/right), vertical means horizontal face
            // (top/bottom).  Spread along the face's free axis.
            let dx = (start.x - next.x).abs();
            let dy = (start.y - next.y).abs();
            let spread_along_y = dx > dy; // horizontal departure → spread y
            let source_id = &routed[representative_idx].from;

            let source_rect =
                if let Some(sg_id) = routed[representative_idx].from_subgraph.as_deref() {
                    geometry.subgraphs.get(sg_id).map(|sg| sg.rect)
                } else {
                    geometry.nodes.get(source_id).map(|node| node.rect)
                };
            let Some(sr) = source_rect else {
                continue;
            };

            let (face_min, face_max) = if spread_along_y {
                (sr.y + CORNER_INSET, sr.y + sr.height - CORNER_INSET)
            } else {
                (sr.x + CORNER_INSET, sr.x + sr.width - CORNER_INSET)
            };

            if face_max <= face_min {
                continue;
            }

            // Sort by departure segment length (distance from source face to
            // corridor lane).  Inner corridors (shorter departure) get ports
            // nearer the start of the face (top for TD/BT right-face, left
            // for LR/RL bottom-face) so departure segments nest without
            // crossing: shorter horizontal/vertical segments sit inside
            // longer ones.
            let mut sorted_group: Vec<(usize, f64)> = group
                .iter()
                .map(|&idx| {
                    let departure_len = if routed[idx].path.len() >= 2 {
                        let p0 = routed[idx].path[0];
                        let p1 = routed[idx].path[1];
                        if spread_along_y {
                            (p1.x - p0.x).abs()
                        } else {
                            (p1.y - p0.y).abs()
                        }
                    } else {
                        0.0
                    };
                    (idx, departure_len)
                })
                .collect();
            sorted_group.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

            // Local adjustment (Schulze et al. 2013, §2 Figure 4b): cluster
            // ports in a compact zone near face center rather than spanning
            // the full face.  Combined with departure-length sorting this
            // keeps inner corridors nested inside outer ones.
            let count = sorted_group.len();
            let anchor = (face_min + face_max) / 2.0;
            let half_span = (count as f64 - 1.0) * MIN_PORT_SPACING / 2.0;
            let zone_start = (anchor - half_span).max(face_min);
            let zone_end = (anchor + half_span).min(face_max);

            for (slot, &(idx, _)) in sorted_group.iter().enumerate() {
                let new_coord = if count <= 1 {
                    anchor
                } else {
                    zone_start + (zone_end - zone_start) * slot as f64 / (count - 1) as f64
                };
                let old_start = routed[idx].path[0];

                if spread_along_y {
                    routed[idx].path[0] = FPoint::new(old_start.x, new_coord);
                    // Propagate to aligned adjacent point.
                    if routed[idx].path.len() >= 2
                        && (routed[idx].path[1].y - old_start.y).abs() < 0.01
                    {
                        routed[idx].path[1].y = new_coord;
                    }
                } else {
                    routed[idx].path[0] = FPoint::new(new_coord, old_start.y);
                    if routed[idx].path.len() >= 2
                        && (routed[idx].path[1].x - old_start.x).abs() < 0.01
                    {
                        routed[idx].path[1].x = new_coord;
                    }
                }
            }
        }
    }
}

/// Spread backward edge target ports that share the exact same arrival
/// point.  Mirrors `spread_colocated_backward_source_ports` but operates on
/// the last path point (arrival) rather than the first (departure).
pub(super) fn spread_colocated_backward_target_ports(
    routed: &mut [crate::graph::geometry::RoutedEdgeGeometry],
    geometry: &crate::graph::geometry::GraphGeometry,
) {
    use super::path_utils::points_match;
    use crate::graph::space::FPoint;

    const CORNER_INSET: f64 = 4.0;
    const MIN_PORT_SPACING: f64 = 12.0;

    // Group backward edges by target node.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, edge) in routed.iter().enumerate() {
        if !edge.is_backward || edge.path.len() < 2 {
            continue;
        }
        groups.entry(edge.to.clone()).or_default().push(idx);
    }

    for edge_indices in groups.values() {
        if edge_indices.len() <= 1 {
            continue;
        }

        // Sub-group by co-located arrival point (same last point within
        // epsilon).  Unlike the source side, we only check the final point —
        // edges approaching from different corridors still converge to the
        // same arrowhead position and need spreading.
        let mut colocated: Vec<Vec<usize>> = Vec::new();
        for &idx in edge_indices {
            let n = routed[idx].path.len();
            let end = routed[idx].path[n - 1];
            let found = colocated.iter_mut().find(|group| {
                let rn = routed[group[0]].path.len();
                let rep_end = routed[group[0]].path[rn - 1];
                points_match(end, rep_end)
            });
            if let Some(group) = found {
                group.push(idx);
            } else {
                colocated.push(vec![idx]);
            }
        }

        for group in &colocated {
            if group.len() <= 1 {
                continue;
            }

            let representative_idx = group[0];
            let n = routed[representative_idx].path.len();
            let end = routed[representative_idx].path[n - 1];
            let target_id = &routed[representative_idx].to;

            let target_rect = if let Some(sg_id) = routed[representative_idx].to_subgraph.as_deref()
            {
                geometry.subgraphs.get(sg_id).map(|sg| sg.rect)
            } else {
                geometry.nodes.get(target_id).map(|node| node.rect)
            };
            let Some(tr) = target_rect else {
                continue;
            };

            // Determine which face the arrival point sits on by checking
            // proximity to each edge of the target rect.
            let dx_left = (end.x - tr.x).abs();
            let dx_right = (end.x - (tr.x + tr.width)).abs();
            let dy_top = (end.y - tr.y).abs();
            let dy_bottom = (end.y - (tr.y + tr.height)).abs();
            let min_dist = dx_left.min(dx_right).min(dy_top).min(dy_bottom);
            // On a vertical face (left/right) → spread along y;
            // on a horizontal face (top/bottom) → spread along x.
            let spread_along_y =
                (dx_left - min_dist).abs() < 0.5 || (dx_right - min_dist).abs() < 0.5;

            let (face_min, face_max) = if spread_along_y {
                (tr.y + CORNER_INSET, tr.y + tr.height - CORNER_INSET)
            } else {
                (tr.x + CORNER_INSET, tr.x + tr.width - CORNER_INSET)
            };

            if face_max <= face_min {
                continue;
            }

            // Sort by source distance from target along the primary
            // axis — farther sources get ports nearer the start of the
            // face so approach segments nest without crossing.
            let mut sorted_group: Vec<(usize, f64)> = group
                .iter()
                .map(|&idx| {
                    let source_id = &routed[idx].from;
                    let source_main = geometry
                        .nodes
                        .get(source_id)
                        .map(|n| match geometry.direction {
                            crate::graph::Direction::TopDown
                            | crate::graph::Direction::BottomTop => n.rect.y + n.rect.height / 2.0,
                            crate::graph::Direction::LeftRight
                            | crate::graph::Direction::RightLeft => n.rect.x + n.rect.width / 2.0,
                        })
                        .unwrap_or(0.0);
                    (idx, source_main)
                })
                .collect();
            // Descending for TD/LR (farther = higher value = top port),
            // ascending for BT/RL (farther = lower value = top port).
            let descending = matches!(
                geometry.direction,
                crate::graph::Direction::TopDown | crate::graph::Direction::LeftRight
            );
            sorted_group.sort_by(|a, b| {
                let ord = a.1.total_cmp(&b.1);
                let ord = if descending { ord.reverse() } else { ord };
                ord.then_with(|| a.0.cmp(&b.0))
            });

            let count = sorted_group.len();
            let anchor = (face_min + face_max) / 2.0;
            let half_span = (count as f64 - 1.0) * MIN_PORT_SPACING / 2.0;
            let zone_start = (anchor - half_span).max(face_min);
            let zone_end = (anchor + half_span).min(face_max);

            // Minimum offset from the face for a proper approach stub.
            const APPROACH_STUB: f64 = 8.0;

            for (slot, &(idx, _)) in sorted_group.iter().enumerate() {
                let new_coord = if count <= 1 {
                    anchor
                } else {
                    zone_start + (zone_end - zone_start) * slot as f64 / (count - 1) as f64
                };
                let pn = routed[idx].path.len();
                let old_end = routed[idx].path[pn - 1];

                if spread_along_y {
                    routed[idx].path[pn - 1] = FPoint::new(old_end.x, new_coord);
                    // Propagate to penultimate point if axis-aligned
                    // (horizontal segment on vertical face — already normal).
                    if pn >= 2 && (routed[idx].path[pn - 2].y - old_end.y).abs() < 0.01 {
                        routed[idx].path[pn - 2].y = new_coord;
                    } else if pn >= 2 {
                        // Penultimate has a different y: the last segment is
                        // vertical (parallel to the face, not normal).  Push the
                        // corridor outward and insert a horizontal approach stub.
                        let pen = routed[idx].path[pn - 2];
                        let outward_x = if pen.x >= old_end.x {
                            pen.x.max(old_end.x + APPROACH_STUB)
                        } else {
                            pen.x.min(old_end.x - APPROACH_STUB)
                        };
                        routed[idx].path[pn - 2].x = outward_x;
                        routed[idx]
                            .path
                            .insert(pn - 1, FPoint::new(outward_x, new_coord));
                    }
                } else {
                    routed[idx].path[pn - 1] = FPoint::new(new_coord, old_end.y);
                    if pn >= 2 && (routed[idx].path[pn - 2].x - old_end.x).abs() < 0.01 {
                        routed[idx].path[pn - 2].x = new_coord;
                    } else if pn >= 2 {
                        let pen = routed[idx].path[pn - 2];
                        let outward_y = if pen.y >= old_end.y {
                            pen.y.max(old_end.y + APPROACH_STUB)
                        } else {
                            pen.y.min(old_end.y - APPROACH_STUB)
                        };
                        routed[idx].path[pn - 2].y = outward_y;
                        routed[idx]
                            .path
                            .insert(pn - 1, FPoint::new(new_coord, outward_y));
                    }
                }
            }
        }
    }
}

fn adaptive_fan_in_primary_face_capacity(direction: Direction, target_rect: &FRect) -> usize {
    let baseline_capacity = fan_in_primary_face_capacity(direction);
    let face_span = match direction {
        Direction::TopDown | Direction::BottomTop => target_rect.width.abs(),
        Direction::LeftRight | Direction::RightLeft => target_rect.height.abs(),
    };
    let usable_span = (face_span - 2.0 * MIN_PORT_CORNER_INSET_FORWARD).max(0.0);
    let dynamic_capacity = if usable_span <= f64::EPSILON {
        1
    } else {
        (usable_span / MIN_FAN_IN_PRIMARY_SLOT_SPACING).floor() as usize + 1
    };
    dynamic_capacity.max(baseline_capacity).max(1)
}

pub(crate) fn symmetric_side_band_depth(band_index: usize, band_count: usize) -> f64 {
    let margin = FAN_PRIMARY_SIDE_BAND_DEPTH_MARGIN.clamp(0.0, 0.49);
    if band_count <= 1 {
        margin
    } else {
        let raw = band_index as f64 / (band_count - 1) as f64;
        margin + (1.0 - 2.0 * margin) * raw
    }
}

fn remap_angular_fan_out_source_fraction(base_fraction: f64, edge_count: usize) -> f64 {
    if edge_count <= 3 {
        return base_fraction.clamp(0.0, 1.0);
    }

    let exponent = (1.0 + (edge_count as f64 - 3.0)).clamp(1.0, 4.0);
    let centered = (base_fraction.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let remapped = centered.signum() * centered.abs().powf(exponent);
    ((remapped + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn remap_angular_fan_in_target_fraction(base_fraction: f64, edge_count: usize) -> f64 {
    if edge_count <= 2 {
        return base_fraction.clamp(0.0, 1.0);
    }
    // Map base fractions into the inset-safe range so the outermost
    // edges land right at the corner-inset boundary (≈8% of face width)
    // instead of getting clamped.  This uses the full usable face width
    // and gives uniform gaps between all entry points.
    let margin = 0.08;
    let usable = 1.0 - 2.0 * margin;
    (margin + usable * base_fraction).clamp(0.0, 1.0)
}

fn fan_in_source_cross_axis(
    geometry: &GraphGeometry,
    edge: &LayoutEdge,
    direction: Direction,
) -> f64 {
    let Some(rect) = endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref()) else {
        return edge.index as f64;
    };
    face_cross_axis(rect, direction)
}

fn fan_out_target_cross_axis(
    geometry: &GraphGeometry,
    edge: &LayoutEdge,
    direction: Direction,
) -> f64 {
    let Some(rect) = endpoint_rect(geometry, &edge.to, edge.to_subgraph.as_deref()) else {
        return edge.index as f64;
    };
    face_cross_axis(rect, direction)
}

fn apply_near_aligned_primary_face_fraction_override(
    geometry: &GraphGeometry,
    direction: Direction,
    primary_face: Face,
    target_rect: &FRect,
    forward_edges: &[&LayoutEdge],
    target_face_for_edge: &HashMap<usize, Face>,
    target_fraction_for_edge: &mut HashMap<usize, f64>,
) {
    let target_cross = face_cross_axis(target_rect, direction);
    let mut best: Option<(usize, f64, f64)> = None;

    for edge in forward_edges {
        if target_face_for_edge.get(&edge.index).copied() != Some(primary_face) {
            continue;
        }
        let Some(source_rect) = endpoint_rect(geometry, &edge.from, edge.from_subgraph.as_deref())
        else {
            continue;
        };
        let source_cross = face_cross_axis(source_rect, direction);
        let delta = (source_cross - target_cross).abs();
        if delta > near_alignment_threshold(source_rect, target_rect, direction) {
            continue;
        }

        match best {
            Some((best_index, _, best_delta))
                if delta > best_delta
                    || ((delta - best_delta).abs() <= f64::EPSILON && edge.index >= best_index) => {
            }
            _ => {
                best = Some((edge.index, source_cross, delta));
            }
        }
    }

    if let Some((edge_index, source_cross, _)) = best {
        let aligned_fraction = cross_axis_to_face_fraction(source_cross, target_rect, direction);
        let aligned_slot_occupied = forward_edges.iter().any(|edge| {
            if edge.index == edge_index {
                return false;
            }
            if target_face_for_edge.get(&edge.index).copied() != Some(primary_face) {
                return false;
            }
            target_fraction_for_edge
                .get(&edge.index)
                .is_some_and(|fraction| (*fraction - aligned_fraction).abs() <= f64::EPSILON)
        });
        if aligned_slot_occupied {
            return;
        }
        target_fraction_for_edge.insert(edge_index, aligned_fraction);
    }
}

fn near_alignment_threshold(source_rect: &FRect, target_rect: &FRect, direction: Direction) -> f64 {
    match direction {
        Direction::TopDown | Direction::BottomTop => 0.5 * source_rect.width.min(target_rect.width),
        Direction::LeftRight | Direction::RightLeft => {
            0.5 * source_rect.height.min(target_rect.height)
        }
    }
}

fn cross_axis_to_face_fraction(cross: f64, rect: &FRect, direction: Direction) -> f64 {
    const EPS: f64 = 0.000_001;
    let raw = match direction {
        Direction::TopDown | Direction::BottomTop => {
            if rect.width.abs() <= EPS {
                0.5
            } else {
                (cross - rect.x) / rect.width
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if rect.height.abs() <= EPS {
                0.5
            } else {
                (cross - rect.y) / rect.height
            }
        }
    };
    raw.clamp(0.0, 1.0)
}

fn face_cross_axis(rect: &FRect, direction: Direction) -> f64 {
    match direction {
        Direction::TopDown | Direction::BottomTop => rect.x + rect.width / 2.0,
        Direction::LeftRight | Direction::RightLeft => rect.y + rect.height / 2.0,
    }
}

pub(super) fn stagger_primary_face_shared_axis_segment(
    path: &mut [crate::graph::space::FPoint],
    direction: Direction,
    target_primary_channel_depth: Option<f64>,
) {
    const EPS: f64 = 0.000_001;
    const MIN_SOURCE_STEM: f64 = 8.0;
    const MIN_TARGET_STEM: f64 = 8.0;

    let Some(depth) = target_primary_channel_depth else {
        return;
    };
    if path.len() < 4 {
        return;
    }
    let depth = depth.clamp(0.0, 1.0);

    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    for i in 1..path.len().saturating_sub(2) {
        let seg_is_gathering = if primary_vertical {
            (path[i].y - path[i + 1].y).abs() <= EPS && (path[i].x - path[i + 1].x).abs() > EPS
        } else {
            (path[i].x - path[i + 1].x).abs() <= EPS && (path[i].y - path[i + 1].y).abs() > EPS
        };
        if !seg_is_gathering {
            continue;
        }

        let prev_is_normal = if primary_vertical {
            (path[i - 1].x - path[i].x).abs() <= EPS && (path[i - 1].y - path[i].y).abs() > EPS
        } else {
            (path[i - 1].y - path[i].y).abs() <= EPS && (path[i - 1].x - path[i].x).abs() > EPS
        };
        let next_is_normal = if primary_vertical {
            (path[i + 1].x - path[i + 2].x).abs() <= EPS
                && (path[i + 1].y - path[i + 2].y).abs() > EPS
        } else {
            (path[i + 1].y - path[i + 2].y).abs() <= EPS
                && (path[i + 1].x - path[i + 2].x).abs() > EPS
        };
        if !prev_is_normal || !next_is_normal {
            continue;
        }

        if primary_vertical {
            if let Some(y) = stagger_axis_value(
                path[0].y,
                path[path.len() - 1].y,
                depth,
                MIN_SOURCE_STEM,
                MIN_TARGET_STEM,
            ) {
                path[i].y = y;
                path[i + 1].y = y;
            }
        } else if let Some(x) = stagger_axis_value(
            path[0].x,
            path[path.len() - 1].x,
            depth,
            MIN_SOURCE_STEM,
            MIN_TARGET_STEM,
        ) {
            path[i].x = x;
            path[i + 1].x = x;
        }
        return;
    }
}

fn stagger_axis_value(
    start: f64,
    end: f64,
    depth: f64,
    min_source_stem: f64,
    min_target_stem: f64,
) -> Option<f64> {
    const EPS: f64 = 0.000_001;
    let delta = end - start;
    if delta.abs() <= min_source_stem + min_target_stem + EPS {
        return None;
    }

    let sign = delta.signum();
    let shallow = start + sign * min_source_stem;
    let deep = end - sign * min_target_stem;
    if (deep - shallow).abs() <= EPS {
        return None;
    }
    Some(shallow + (deep - shallow) * depth.clamp(0.0, 1.0))
}

pub(super) fn edge_rank_span(geometry: &GraphGeometry, edge: &LayoutEdge) -> Option<usize> {
    let crate::graph::geometry::EngineHints::Layered(hints) = geometry.engine_hints.as_ref()?;
    let src_rank = *hints.node_ranks.get(&edge.from)?;
    let dst_rank = *hints.node_ranks.get(&edge.to)?;
    Some(src_rank.abs_diff(dst_rank) as usize)
}
