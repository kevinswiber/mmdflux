use std::collections::HashMap;

use crate::graph::Direction;
use crate::graph::geometry::PortFace;

/// Direction-specific overflow lane for fan-in spill candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowSide {
    LeftOrTop,
    RightOrBottom,
}

/// Primary face capacity for deterministic overflow policy in `Task 0.2`.
pub(crate) const FAN_IN_PRIMARY_FACE_CAPACITY_TD_BT: usize = 4;
pub(crate) const FAN_IN_PRIMARY_FACE_CAPACITY_LR_RL: usize = 2;

/// Return the deterministic base capacity for the primary incoming face.
pub(crate) fn fan_in_primary_face_capacity(direction: Direction) -> usize {
    match direction {
        Direction::TopDown | Direction::BottomTop => FAN_IN_PRIMARY_FACE_CAPACITY_TD_BT,
        Direction::LeftRight | Direction::RightLeft => FAN_IN_PRIMARY_FACE_CAPACITY_LR_RL,
    }
}

/// Convert canonical fan-in spill slot into an overflow face for a direction.
pub(crate) fn fan_in_overflow_face_for_slot(direction: Direction, slot: OverflowSide) -> Face {
    match direction {
        Direction::TopDown | Direction::BottomTop => match slot {
            OverflowSide::LeftOrTop => Face::Left,
            OverflowSide::RightOrBottom => Face::Right,
        },
        Direction::LeftRight | Direction::RightLeft => match slot {
            OverflowSide::LeftOrTop => Face::Top,
            OverflowSide::RightOrBottom => Face::Bottom,
        },
    }
}

/// Canonical backward channel for backward-channel policy.
pub(crate) fn canonical_backward_channel_face(direction: Direction) -> Face {
    match direction {
        Direction::TopDown | Direction::BottomTop => Face::Right,
        Direction::LeftRight | Direction::RightLeft => Face::Bottom,
    }
}

/// Primary incoming target face for forward edges under fan-in policy.
pub(crate) fn fan_in_primary_target_face(direction: Direction) -> Face {
    match direction {
        Direction::TopDown => Face::Top,
        Direction::BottomTop => Face::Bottom,
        Direction::LeftRight => Face::Left,
        Direction::RightLeft => Face::Right,
    }
}

fn fan_in_non_canonical_overflow_face(direction: Direction) -> Face {
    match direction {
        Direction::TopDown | Direction::BottomTop => Face::Left,
        Direction::LeftRight | Direction::RightLeft => Face::Top,
    }
}

/// Resolve a target/source face with explicit precedence when both fan-in overflow and
/// backward channels are in contention.
pub(crate) fn resolve_overflow_backward_channel_conflict(
    direction: Direction,
    is_backward: bool,
    target_has_backward_conflict: bool,
    overflow_face: Option<Face>,
    proposed_face: Face,
) -> Face {
    if !is_backward || overflow_face.is_none() {
        if target_has_backward_conflict
            && overflow_face.is_some()
            && proposed_face == canonical_backward_channel_face(direction)
        {
            return fan_in_non_canonical_overflow_face(direction);
        }
        return proposed_face;
    }

    canonical_backward_channel_face(direction)
}

/// Which face of a rectangular node an edge attaches to.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub(crate) enum Face {
    Top,
    Bottom,
    Left,
    Right,
}

impl Face {
    /// Convert to the geometry IR port face type.
    pub(crate) fn to_port_face(self) -> PortFace {
        match self {
            Face::Top => PortFace::Top,
            Face::Bottom => PortFace::Bottom,
            Face::Left => PortFace::Left,
            Face::Right => PortFace::Right,
        }
    }
}

/// Per-edge attachment location on a node face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EdgeAttachment {
    pub face: Face,
    pub fraction: f64,
}

/// Source and target attachment assignments for one edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedEdgeAttachments {
    pub source: Option<EdgeAttachment>,
    pub target: Option<EdgeAttachment>,
}

/// Deterministic attachment assignments for all planned edges.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AttachmentPlan {
    edge_attachments: HashMap<usize, PlannedEdgeAttachments>,
    group_sizes: HashMap<(String, Face), usize>,
    source_fractions: HashMap<(String, Face), Vec<f64>>,
    target_fractions: HashMap<(String, Face), Vec<f64>>,
}

impl AttachmentPlan {
    /// Return source-side fractions for a node face in deterministic order.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn source_fractions_for(&self, node_id: &str, face: Face) -> Vec<f64> {
        self.source_fractions
            .get(&(node_id.to_string(), face))
            .cloned()
            .unwrap_or_default()
    }

    /// Return the edge-specific source/target assignments.
    pub(crate) fn edge(&self, edge_index: usize) -> Option<&PlannedEdgeAttachments> {
        self.edge_attachments.get(&edge_index)
    }

    /// Return the number of attachments planned for a node face.
    pub(crate) fn group_size(&self, node_id: &str, face: Face) -> usize {
        self.group_sizes
            .get(&(node_id.to_string(), face))
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn attachments(
        &self,
    ) -> impl Iterator<Item = (&usize, &PlannedEdgeAttachments)> + '_ {
        self.edge_attachments.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AttachmentSide {
    Source,
    Target,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentCandidate {
    pub edge_index: usize,
    pub node_id: String,
    pub side: AttachmentSide,
    pub face: Face,
    pub cross_axis: f64,
}

pub(crate) fn plan_attachment_candidates(candidates: Vec<AttachmentCandidate>) -> AttachmentPlan {
    let mut groups: HashMap<(String, Face), Vec<AttachmentCandidate>> = HashMap::new();
    for candidate in candidates {
        groups
            .entry((candidate.node_id.clone(), candidate.face))
            .or_default()
            .push(candidate);
    }

    let mut plan = AttachmentPlan::default();
    for ((node_id, face), mut group) in groups {
        group.sort_by(compare_attachment_candidates);
        plan.group_sizes
            .insert((node_id.clone(), face), group.len());

        for (idx, candidate) in group.iter().enumerate() {
            let fraction = if group.len() <= 1 {
                0.5
            } else {
                idx as f64 / (group.len() - 1) as f64
            };
            let attachment = EdgeAttachment { face, fraction };
            let edge_entry = plan.edge_attachments.entry(candidate.edge_index).or_insert(
                PlannedEdgeAttachments {
                    source: None,
                    target: None,
                },
            );

            match candidate.side {
                AttachmentSide::Source => {
                    edge_entry.source = Some(attachment);
                    plan.source_fractions
                        .entry((candidate.node_id.clone(), candidate.face))
                        .or_default()
                        .push(fraction);
                }
                AttachmentSide::Target => {
                    edge_entry.target = Some(attachment);
                    plan.target_fractions
                        .entry((candidate.node_id.clone(), candidate.face))
                        .or_default()
                        .push(fraction);
                }
            }
        }
    }
    plan
}

fn compare_attachment_candidates(
    a: &AttachmentCandidate,
    b: &AttachmentCandidate,
) -> std::cmp::Ordering {
    a.cross_axis
        .total_cmp(&b.cross_axis)
        .then_with(|| a.edge_index.cmp(&b.edge_index))
        .then_with(|| a.side.cmp(&b.side))
}

pub(crate) fn edge_faces(direction: Direction, is_backward: bool) -> (Face, Face) {
    let (forward_src, forward_tgt) = match direction {
        Direction::TopDown => (Face::Bottom, Face::Top),
        Direction::BottomTop => (Face::Top, Face::Bottom),
        Direction::LeftRight => (Face::Right, Face::Left),
        Direction::RightLeft => (Face::Left, Face::Right),
    };

    if is_backward {
        (forward_tgt, forward_src)
    } else {
        (forward_src, forward_tgt)
    }
}
