//! Shared attachment, port, and face policy for graph-family routing.
//!
//! Grid replay and float routing both need the same attachment planning,
//! face classification, and backward-edge policy. Keeping those contracts out
//! of `routing` avoids a grid -> routing dependency and gives the shared
//! policy a single owner.

use std::collections::HashMap;

use crate::graph::Direction;
use crate::graph::space::{FPoint, FRect};

/// Which face of a node boundary an edge port attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortFace {
    Top,
    Bottom,
    Left,
    Right,
}

impl PortFace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl std::str::FromStr for PortFace {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(()),
        }
    }
}

/// Port attachment information for one end of a routed edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePort {
    /// Face on the node boundary where the edge attaches.
    pub face: PortFace,
    /// Fractional position along the face (0.0 = start, 1.0 = end).
    /// For top/bottom: 0.0 is left, 1.0 is right.
    /// For left/right: 0.0 is top, 1.0 is bottom.
    pub fraction: f64,
    /// Computed position on the node boundary in layout coordinate space.
    pub position: FPoint,
    /// Number of edges attached to this face of this node.
    pub group_size: usize,
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

/// Direction-specific overflow lane for fan-in spill candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowSide {
    LeftOrTop,
    RightOrBottom,
}

/// Primary face capacity for deterministic overflow policy.
pub(crate) const FAN_IN_PRIMARY_FACE_CAPACITY_TD_BT: usize = 4;
pub(crate) const FAN_IN_PRIMARY_FACE_CAPACITY_LR_RL: usize = 2;

/// Long backward edges (3+ user-visible rank gaps, normalized rank_span >= 6)
/// use side-face channel routing.
pub(crate) const BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN: usize = 6;

/// Shared threshold for choosing wide horizontal detours.
pub(crate) const LARGE_HORIZONTAL_OFFSET_THRESHOLD: usize = 30;

const TD_BT_PARITY_MIN_RECT_SPAN: f64 = 20.0;

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

/// Whether a backward edge should prefer the canonical backward side channel.
pub(crate) fn prefer_backward_side_channel(
    is_backward: bool,
    has_layout_waypoints: bool,
    rank_span: Option<usize>,
) -> bool {
    if !is_backward {
        return false;
    }
    if rank_span.is_some_and(|span| span >= BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN) {
        return true;
    }
    !has_layout_waypoints
}

/// Whether TD/BT backward hint-parity overrides can be applied safely.
pub(crate) fn can_apply_td_bt_backward_hint_parity(
    direction: Direction,
    is_backward: bool,
    has_subgraph_endpoint: bool,
    rank_span: usize,
    source_rect: FRect,
    target_rect: FRect,
    source_center_x: f64,
) -> bool {
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) {
        return false;
    }
    if has_subgraph_endpoint {
        return false;
    }
    if prefer_backward_side_channel(is_backward, true, Some(rank_span)) {
        return false;
    }
    if source_rect.width < TD_BT_PARITY_MIN_RECT_SPAN
        || source_rect.height < TD_BT_PARITY_MIN_RECT_SPAN
        || target_rect.width < TD_BT_PARITY_MIN_RECT_SPAN
        || target_rect.height < TD_BT_PARITY_MIN_RECT_SPAN
    {
        return false;
    }

    let target_right = target_rect.x + target_rect.width;
    source_center_x <= target_right
}

/// Classify which face a point approaches, using slope-vs-diagonal comparison.
pub(crate) fn classify_face_float(center: FPoint, rect: FRect, approach: FPoint) -> Face {
    let dx = approach.x - center.x;
    let dy = approach.y - center.y;

    if dx.abs() < 0.5 && dy.abs() < 0.5 {
        return Face::Bottom;
    }

    let half_w = rect.width / 2.0;
    let half_h = rect.height / 2.0;

    if dy.abs() * half_w > dx.abs() * half_h {
        if dy < 0.0 { Face::Top } else { Face::Bottom }
    } else if dx < 0.0 {
        Face::Left
    } else {
        Face::Right
    }
}

/// Compute a point on a rectangle face at the given fraction.
pub(crate) fn point_on_face_float(rect: FRect, face: Face, fraction: f64) -> FPoint {
    let fraction = fraction.clamp(0.0, 1.0);
    match face {
        Face::Top => FPoint::new(rect.x + rect.width * fraction, rect.y),
        Face::Bottom => FPoint::new(rect.x + rect.width * fraction, rect.y + rect.height),
        Face::Left => FPoint::new(rect.x, rect.y + rect.height * fraction),
        Face::Right => FPoint::new(rect.x + rect.width, rect.y + rect.height * fraction),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_face_round_trips() {
        assert_eq!(PortFace::Top.as_str(), "top");
        assert_eq!(PortFace::Bottom.as_str(), "bottom");
        assert_eq!(PortFace::Left.as_str(), "left");
        assert_eq!(PortFace::Right.as_str(), "right");
        assert_eq!("top".parse::<PortFace>(), Ok(PortFace::Top));
        assert_eq!("bottom".parse::<PortFace>(), Ok(PortFace::Bottom));
        assert_eq!("left".parse::<PortFace>(), Ok(PortFace::Left));
        assert_eq!("right".parse::<PortFace>(), Ok(PortFace::Right));
        assert_eq!("invalid".parse::<PortFace>(), Err(()));
    }

    #[test]
    fn edge_port_construction() {
        let port = EdgePort {
            face: PortFace::Top,
            fraction: 0.5,
            position: FPoint::new(50.0, 10.0),
            group_size: 1,
        };
        assert_eq!(port.face, PortFace::Top);
        assert!((port.fraction - 0.5).abs() < f64::EPSILON);
        assert_eq!(port.position, FPoint::new(50.0, 10.0));
        assert_eq!(port.group_size, 1);
    }

    #[test]
    fn prefer_backward_side_channel_uses_no_waypoint_fallback() {
        assert!(prefer_backward_side_channel(true, false, None));
        assert!(!prefer_backward_side_channel(true, true, None));
    }

    #[test]
    fn prefer_backward_side_channel_uses_long_span_override() {
        assert!(prefer_backward_side_channel(
            true,
            true,
            Some(BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN)
        ));
        assert!(!prefer_backward_side_channel(
            true,
            true,
            Some(BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN - 1)
        ));
    }

    #[test]
    fn prefer_backward_side_channel_ignores_forward_edges() {
        assert!(!prefer_backward_side_channel(false, false, Some(10)));
    }

    #[test]
    fn td_bt_backward_hint_parity_requires_safe_geometry() {
        let source_rect = FRect::new(10.0, 10.0, 40.0, 40.0);
        let target_rect = FRect::new(20.0, 0.0, 40.0, 40.0);
        let source_center_x = source_rect.x + source_rect.width / 2.0;

        assert!(can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            2,
            source_rect,
            target_rect,
            source_center_x
        ));
    }

    #[test]
    fn td_bt_backward_hint_parity_rejects_long_span_and_crossing_topology() {
        let source_rect = FRect::new(80.0, 10.0, 40.0, 40.0);
        let target_rect = FRect::new(10.0, 0.0, 40.0, 40.0);
        let source_center_x = source_rect.x + source_rect.width / 2.0;

        assert!(!can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN,
            source_rect,
            target_rect,
            source_center_x
        ));
        assert!(!can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            2,
            source_rect,
            target_rect,
            source_center_x
        ));
    }
}
