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
    if edge_count <= 3 {
        return base_fraction.clamp(0.0, 1.0);
    }

    let exponent = (8.0 / edge_count as f64).clamp(1.0, 2.5);
    let centered = (base_fraction.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let remapped = centered.signum() * centered.abs().powf(exponent);
    ((remapped + 1.0) * 0.5).clamp(0.0, 1.0)
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
