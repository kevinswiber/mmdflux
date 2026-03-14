use std::collections::HashMap;

use super::super::attachments::{
    Face as SharedFace, edge_faces as shared_edge_faces,
    plan_attachments as shared_plan_attachments,
};
use super::super::backward::is_backward_edge;
use super::super::bounds::{bounds_for_node_id, resolve_edge_bounds};
use super::super::intersect::{
    NodeFace, calculate_attachment_points, classify_face, face_extent, face_fixed_coord,
    spread_points_on_face,
};
use super::super::layout::{GridLayout, NodeBounds};
use super::types::{AttachmentOverride, EdgeEndpoints, Point};
use crate::graph::{Direction, Edge, Stroke};

/// Compute pre-assigned attachment points for edges that share a node face.
///
/// Only produces overrides for faces with >1 edge. Single-edge faces
/// use the default intersect_rect() calculation (no override).
pub fn compute_attachment_plan(
    edges: &[Edge],
    layout: &GridLayout,
    direction: Direction,
) -> HashMap<usize, AttachmentOverride> {
    compute_attachment_plan_from_shared_planner(edges, layout, direction)
}

pub(super) fn compute_attachment_plan_from_shared_planner(
    edges: &[Edge],
    layout: &GridLayout,
    direction: Direction,
) -> HashMap<usize, AttachmentOverride> {
    let shared = shared_plan_attachments(edges, layout, direction);
    let mut overrides: HashMap<usize, AttachmentOverride> = HashMap::new();

    for edge in edges {
        if edge.from == edge.to || edge.stroke == Stroke::Invisible {
            continue;
        }

        let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
        let tgt_id = edge.to_subgraph.as_deref().unwrap_or(edge.to.as_str());

        let Some(attachments) = shared.edge(edge.index) else {
            continue;
        };

        let entry = overrides.entry(edge.index).or_insert(AttachmentOverride {
            source: None,
            target: None,
            source_first_vertical: false,
        });

        if let Some(source_attachment) = attachments.source
            && shared.group_size(src_id, source_attachment.face) > 1
            && let Some(src_bounds) = bounds_for_node_id(layout, src_id)
        {
            let group_size = shared.group_size(src_id, source_attachment.face);
            entry.source = Some(point_on_face_grid(
                &src_bounds,
                source_attachment.face.to_node_face(),
                source_attachment.fraction,
                group_size,
            ));
        }

        if let Some(target_attachment) = attachments.target
            && shared.group_size(tgt_id, target_attachment.face) > 1
            && let Some(tgt_bounds) = bounds_for_node_id(layout, tgt_id)
        {
            let group_size = shared.group_size(tgt_id, target_attachment.face);
            entry.target = Some(point_on_face_grid(
                &tgt_bounds,
                target_attachment.face.to_node_face(),
                target_attachment.fraction,
                group_size,
            ));
        }
    }

    let flow_face = match direction {
        Direction::TopDown => Some(SharedFace::Bottom),
        Direction::BottomTop => Some(SharedFace::Top),
        _ => None,
    };
    if let Some(flow_face) = flow_face {
        let mut side_lanes: HashMap<(String, i8), Vec<(usize, f64)>> = HashMap::new();
        let mut override_side_lanes: HashMap<(String, i8), Vec<(usize, f64)>> = HashMap::new();
        for edge in edges {
            if edge.from == edge.to || edge.stroke == Stroke::Invisible {
                continue;
            }
            let has_waypoints = edge.from_subgraph.is_none()
                && edge.to_subgraph.is_none()
                && layout
                    .edge_waypoints
                    .get(&edge.index)
                    .is_some_and(|wps| !wps.is_empty());
            let Some(source_attachment) = shared.edge(edge.index).and_then(|a| a.source) else {
                continue;
            };
            if source_attachment.face != flow_face {
                continue;
            }
            let Some((src_bounds, tgt_bounds)) = resolve_edge_bounds(layout, edge) else {
                continue;
            };
            let cross = if has_waypoints {
                let Some(first_wp) = layout
                    .edge_waypoints
                    .get(&edge.index)
                    .and_then(|wps| wps.first())
                    .copied()
                else {
                    continue;
                };
                match source_attachment.face {
                    SharedFace::Top | SharedFace::Bottom => first_wp.0 as f64,
                    SharedFace::Left | SharedFace::Right => first_wp.1 as f64,
                }
            } else {
                match source_attachment.face {
                    SharedFace::Top | SharedFace::Bottom => tgt_bounds.center_x() as f64,
                    SharedFace::Left | SharedFace::Right => tgt_bounds.center_y() as f64,
                }
            };
            let center_cross = match source_attachment.face {
                SharedFace::Top | SharedFace::Bottom => src_bounds.center_x() as f64,
                SharedFace::Left | SharedFace::Right => src_bounds.center_y() as f64,
            };
            let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
            let side = if cross >= center_cross { 1 } else { -1 };
            if has_waypoints {
                side_lanes
                    .entry((src_id.to_string(), side))
                    .or_default()
                    .push((edge.index, cross));
            } else {
                let target_in_override = layout
                    .node_directions
                    .get(&edge.to)
                    .is_some_and(|d| *d != direction);
                if target_in_override {
                    override_side_lanes
                        .entry((src_id.to_string(), side))
                        .or_default()
                        .push((edge.index, cross));
                }
            }
        }

        for ((_node_id, _side), mut lanes) in side_lanes {
            if lanes.len() <= 1 {
                continue;
            }
            lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (idx, (edge_index, _)) in lanes.into_iter().enumerate() {
                if let Some(entry) = overrides.get_mut(&edge_index) {
                    entry.source_first_vertical = idx % 2 == 1;
                }
            }
        }

        for ((_node_id, _side), mut lanes) in override_side_lanes {
            if lanes.len() <= 1 {
                continue;
            }
            lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            for (idx, (edge_index, _)) in lanes.into_iter().enumerate() {
                if let Some(entry) = overrides.get_mut(&edge_index) {
                    entry.source_first_vertical = idx % 2 == 0;
                }
            }
        }
    }

    overrides.retain(|_, ov| ov.source.is_some() || ov.target.is_some());
    overrides
}

fn point_on_face_grid(
    bounds: &NodeBounds,
    face: NodeFace,
    fraction: f64,
    group_size: usize,
) -> (usize, usize) {
    if group_size == 0 {
        return (bounds.center_x(), bounds.center_y());
    }

    let points = spread_points_on_face(
        face,
        face_fixed_coord(bounds, &face),
        face_extent(bounds, &face),
        group_size,
    );
    if group_size == 1 {
        return points[0];
    }

    let fraction = fraction.clamp(0.0, 1.0);
    let rank = ((group_size - 1) as f64 * fraction).round() as usize;
    points[rank.min(group_size - 1)]
}

/// Resolve attachment points, using overrides when provided, falling back to
/// `calculate_attachment_points()` for non-overridden sides.
pub(super) fn resolve_attachment_points(
    src_override: Option<(usize, usize)>,
    tgt_override: Option<(usize, usize)>,
    ep: &EdgeEndpoints,
    waypoints: &[(usize, usize)],
    direction: Direction,
) -> ((usize, usize), (usize, usize)) {
    let from_bounds = ep.from_bounds;
    let to_bounds = ep.to_bounds;

    let is_backward = is_backward_edge(&from_bounds, &to_bounds, direction);

    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            if is_backward
                && let (Some(&first_wp), Some(&last_wp)) = (waypoints.first(), waypoints.last())
            {
                let src_face = classify_face(&from_bounds, first_wp, ep.from_shape);
                let tgt_face = classify_face(&to_bounds, last_wp, ep.to_shape);
                let src =
                    src_override.unwrap_or_else(|| clamp_to_face(&from_bounds, src_face, first_wp));
                let tgt =
                    tgt_override.unwrap_or_else(|| clamp_to_face(&to_bounds, tgt_face, last_wp));
                return (src, tgt);
            }

            let flows_right = matches!(direction, Direction::LeftRight) != is_backward;
            let y = if is_backward {
                from_bounds.center_y()
            } else {
                consensus_y(&from_bounds, &to_bounds)
            };
            let tgt_y = if is_backward { to_bounds.center_y() } else { y };
            let (src, tgt) = if flows_right {
                (
                    src_override.unwrap_or((from_bounds.x + from_bounds.width - 1, y)),
                    tgt_override.unwrap_or((to_bounds.x, tgt_y)),
                )
            } else {
                (
                    src_override.unwrap_or((from_bounds.x, y)),
                    tgt_override.unwrap_or((to_bounds.x + to_bounds.width - 1, tgt_y)),
                )
            };
            return (src, tgt);
        }
        _ => {}
    }

    if matches!(direction, Direction::TopDown | Direction::BottomTop)
        && let (Some(&first_wp), Some(&last_wp)) = (waypoints.first(), waypoints.last())
    {
        let (src_face, tgt_face) = if is_backward && waypoints.len() <= 1 {
            let (default_src_face, default_tgt_face) = edge_faces(direction, is_backward);
            let inferred_src_face = classify_face(&from_bounds, first_wp, ep.from_shape);
            let inferred_tgt_face = classify_face(&to_bounds, last_wp, ep.to_shape);
            (
                if matches!(inferred_src_face, NodeFace::Left | NodeFace::Right) {
                    inferred_src_face
                } else {
                    default_src_face
                },
                if matches!(inferred_tgt_face, NodeFace::Left | NodeFace::Right) {
                    inferred_tgt_face
                } else {
                    default_tgt_face
                },
            )
        } else {
            edge_faces(direction, is_backward)
        };
        let src = src_override.unwrap_or_else(|| clamp_to_face(&from_bounds, src_face, first_wp));
        let tgt = tgt_override.unwrap_or_else(|| clamp_to_face(&to_bounds, tgt_face, last_wp));
        return (src, tgt);
    }

    let fallback = || {
        calculate_attachment_points(
            &from_bounds,
            ep.from_shape,
            &to_bounds,
            ep.to_shape,
            waypoints,
        )
    };
    let src = src_override.unwrap_or_else(|| fallback().0);
    let tgt = tgt_override.unwrap_or_else(|| fallback().1);
    (src, tgt)
}

pub(super) fn clamp_to_face(
    bounds: &NodeBounds,
    face: NodeFace,
    waypoint: (usize, usize),
) -> (usize, usize) {
    let (min, max) = face_extent(bounds, &face);
    let fixed = face_fixed_coord(bounds, &face);
    match face {
        NodeFace::Top | NodeFace::Bottom => (waypoint.0.clamp(min, max), fixed),
        NodeFace::Left | NodeFace::Right => (fixed, waypoint.1.clamp(min, max)),
    }
}

pub(super) fn infer_face_from_attachment(
    bounds: &NodeBounds,
    attach: (usize, usize),
    fallback: NodeFace,
) -> NodeFace {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    if attach.0 == left {
        NodeFace::Left
    } else if attach.0 == right {
        NodeFace::Right
    } else if attach.1 == top {
        NodeFace::Top
    } else if attach.1 == bottom {
        NodeFace::Bottom
    } else {
        fallback
    }
}

fn consensus_y(a: &NodeBounds, b: &NodeBounds) -> usize {
    let avg = (a.center_y() + b.center_y()) / 2;
    avg.max(a.y)
        .min(a.y + a.height - 1)
        .max(b.y)
        .min(b.y + b.height - 1)
}

pub(super) fn clamp_to_boundary(point: (usize, usize), bounds: &NodeBounds) -> Point {
    let (x, y) = point;
    let left = bounds.x;
    let right = bounds.x + bounds.width - 1;
    let top = bounds.y;
    let bottom = bounds.y + bounds.height - 1;

    Point::new(x.clamp(left, right), y.clamp(top, bottom))
}

pub(super) fn edge_faces(direction: Direction, is_backward: bool) -> (NodeFace, NodeFace) {
    let (src, tgt) = shared_edge_faces(direction, is_backward);
    (src.to_node_face(), tgt.to_node_face())
}

pub(super) fn offset_for_face(point: (usize, usize), face: NodeFace) -> Point {
    let (x, y) = point;
    match face {
        NodeFace::Top => Point::new(x, y.saturating_sub(1)),
        NodeFace::Bottom => Point::new(x, y + 1),
        NodeFace::Left => Point::new(x.saturating_sub(1), y),
        NodeFace::Right => Point::new(x + 1, y),
    }
}
