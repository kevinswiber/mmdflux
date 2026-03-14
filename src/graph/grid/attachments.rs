//! Grid-routing attachment helpers for derived grid geometry.

use super::GridLayout;
use super::backward::is_backward_edge;
use super::bounds::{resolve_edge_bounds, subgraph_edge_face};
use super::intersect::{NodeFace, classify_face};
use crate::graph::attachment::{AttachmentCandidate, AttachmentPlan, AttachmentSide};
pub(crate) use crate::graph::attachment::{
    Face, LARGE_HORIZONTAL_OFFSET_THRESHOLD, edge_faces, plan_attachment_candidates,
    prefer_backward_side_channel,
};
use crate::graph::{Direction, Edge, Shape, Stroke};

impl Face {
    /// Convert shared routing-policy face to the text router face type.
    pub(crate) fn to_node_face(self) -> NodeFace {
        match self {
            Face::Top => NodeFace::Top,
            Face::Bottom => NodeFace::Bottom,
            Face::Left => NodeFace::Left,
            Face::Right => NodeFace::Right,
        }
    }

    /// Convert text router face type to the shared routing-policy face.
    pub(crate) fn from_node_face(face: NodeFace) -> Self {
        match face {
            NodeFace::Top => Face::Top,
            NodeFace::Bottom => Face::Bottom,
            NodeFace::Left => Face::Left,
            NodeFace::Right => Face::Right,
        }
    }
}

/// Build a deterministic per-edge attachment-fraction plan from text layout bounds.
pub(crate) fn plan_attachments(
    edges: &[Edge],
    layout: &GridLayout,
    fallback_direction: Direction,
) -> AttachmentPlan {
    let mut candidates: Vec<AttachmentCandidate> = Vec::with_capacity(edges.len() * 2);

    for edge in edges {
        if edge.from == edge.to || edge.stroke == Stroke::Invisible {
            continue;
        }

        let (src_bounds, tgt_bounds) = match resolve_edge_bounds(layout, edge) {
            Some(bounds) => bounds,
            None => continue,
        };
        let src_shape = if edge.from_subgraph.is_some() {
            Shape::Rectangle
        } else {
            layout
                .node_shapes
                .get(&edge.from)
                .copied()
                .unwrap_or(Shape::Rectangle)
        };
        let tgt_shape = if edge.to_subgraph.is_some() {
            Shape::Rectangle
        } else {
            layout
                .node_shapes
                .get(&edge.to)
                .copied()
                .unwrap_or(Shape::Rectangle)
        };

        let allow_waypoints = edge.from_subgraph.is_none() && edge.to_subgraph.is_none();
        let waypoints = if allow_waypoints {
            layout.edge_waypoints.get(&edge.index)
        } else {
            None
        };
        let src_approach = waypoints
            .and_then(|wps| wps.first().copied())
            .unwrap_or((tgt_bounds.center_x(), tgt_bounds.center_y()));
        let tgt_approach = waypoints
            .and_then(|wps| wps.last().copied())
            .unwrap_or((src_bounds.center_x(), src_bounds.center_y()));

        let edge_dir = layout.effective_edge_direction(&edge.from, &edge.to, fallback_direction);
        let is_subgraph_edge = edge.from_subgraph.is_some() || edge.to_subgraph.is_some();
        let is_backward = is_backward_edge(&src_bounds, &tgt_bounds, edge_dir);
        let has_layout_waypoints = waypoints.is_some_and(|wps| !wps.is_empty());
        let use_backward_channel = !is_subgraph_edge
            && prefer_backward_side_channel(is_backward, has_layout_waypoints, None);
        let (mut src_face, mut tgt_face) = if use_backward_channel {
            backward_routing_faces(edge_dir)
        } else if matches!(edge_dir, Direction::TopDown | Direction::BottomTop)
            && !is_backward
            && !is_subgraph_edge
        {
            edge_faces(edge_dir, false)
        } else {
            match edge_dir {
                Direction::LeftRight | Direction::RightLeft => edge_faces(edge_dir, is_backward),
                _ => (
                    Face::from_node_face(classify_face(&src_bounds, src_approach, src_shape)),
                    Face::from_node_face(classify_face(&tgt_bounds, tgt_approach, tgt_shape)),
                ),
            }
        };
        if edge.from_subgraph.is_some() {
            src_face = Face::from_node_face(subgraph_edge_face(&src_bounds, &tgt_bounds, edge_dir));
        }
        if edge.to_subgraph.is_some() {
            tgt_face = Face::from_node_face(subgraph_edge_face(&tgt_bounds, &src_bounds, edge_dir));
        }

        let src_id = edge.from_subgraph.as_deref().unwrap_or(edge.from.as_str());
        let tgt_id = edge.to_subgraph.as_deref().unwrap_or(edge.to.as_str());
        let tgt_in_override = layout
            .node_directions
            .get(&edge.to)
            .is_some_and(|d| *d != fallback_direction);
        let src_cross = if tgt_in_override {
            match src_face {
                Face::Top | Face::Bottom => tgt_bounds.center_x() as f64,
                Face::Left | Face::Right => tgt_bounds.center_y() as f64,
            }
        } else {
            match src_face {
                Face::Top | Face::Bottom => src_approach.0 as f64,
                Face::Left | Face::Right => src_approach.1 as f64,
            }
        };
        let tgt_cross = match tgt_face {
            Face::Top | Face::Bottom => tgt_approach.0 as f64,
            Face::Left | Face::Right => tgt_approach.1 as f64,
        };

        candidates.push(AttachmentCandidate {
            edge_index: edge.index,
            node_id: src_id.to_string(),
            side: AttachmentSide::Source,
            face: src_face,
            cross_axis: src_cross,
        });
        candidates.push(AttachmentCandidate {
            edge_index: edge.index,
            node_id: tgt_id.to_string(),
            side: AttachmentSide::Target,
            face: tgt_face,
            cross_axis: tgt_cross,
        });
    }

    plan_attachment_candidates(candidates)
}

fn backward_routing_faces(direction: Direction) -> (Face, Face) {
    match direction {
        Direction::TopDown | Direction::BottomTop => (Face::Right, Face::Right),
        Direction::LeftRight | Direction::RightLeft => (Face::Bottom, Face::Bottom),
    }
}
