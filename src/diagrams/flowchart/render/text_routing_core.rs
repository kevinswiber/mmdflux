//! Shared routing primitives used by text and SVG routing paths.

use std::collections::HashMap;

use super::text_layout::{Layout, SubgraphBounds};
use super::text_shape::NodeBounds;
use crate::diagrams::flowchart::geometry::{FPoint, FRect};
use crate::graph::{Direction, Edge, Shape, Stroke};
use crate::render::intersect::{NodeFace, classify_face};

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
    /// Convert shared routing-core face to the text router face type.
    pub(crate) fn to_node_face(self) -> NodeFace {
        match self {
            Face::Top => NodeFace::Top,
            Face::Bottom => NodeFace::Bottom,
            Face::Left => NodeFace::Left,
            Face::Right => NodeFace::Right,
        }
    }

    /// Convert text router face type to the shared routing-core face.
    pub(crate) fn from_node_face(face: NodeFace) -> Self {
        match face {
            NodeFace::Top => Face::Top,
            NodeFace::Bottom => Face::Bottom,
            NodeFace::Left => Face::Left,
            NodeFace::Right => Face::Right,
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

/// Build a deterministic per-edge attachment-fraction plan from text layout bounds.
pub(crate) fn plan_attachments(
    edges: &[Edge],
    layout: &Layout,
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
        let (mut src_face, mut tgt_face) = if is_backward && !has_layout_waypoints {
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
            src_face = subgraph_edge_face(&src_bounds, &tgt_bounds, edge_dir);
        }
        if edge.to_subgraph.is_some() {
            tgt_face = subgraph_edge_face(&tgt_bounds, &src_bounds, edge_dir);
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

/// Build a deterministic attachment plan from precomputed face candidates.
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

fn subgraph_edge_face(bounds: &NodeBounds, other: &NodeBounds, direction: Direction) -> Face {
    let bounds_right = bounds.x + bounds.width.saturating_sub(1);
    let bounds_bottom = bounds.y + bounds.height.saturating_sub(1);
    let other_right = other.x + other.width.saturating_sub(1);
    let other_bottom = other.y + other.height.saturating_sub(1);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            if other_bottom < bounds.y {
                return Face::Top;
            }
            if other.y > bounds_bottom {
                return Face::Bottom;
            }
            if other_right < bounds.x {
                return Face::Left;
            }
            if other.x > bounds_right {
                return Face::Right;
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if other_right < bounds.x {
                return Face::Left;
            }
            if other.x > bounds_right {
                return Face::Right;
            }
            if other_bottom < bounds.y {
                return Face::Top;
            }
            if other.y > bounds_bottom {
                return Face::Bottom;
            }
        }
    }

    Face::from_node_face(classify_face(
        bounds,
        (other.center_x(), other.center_y()),
        Shape::Rectangle,
    ))
}

fn backward_routing_faces(direction: Direction) -> (Face, Face) {
    match direction {
        Direction::TopDown | Direction::BottomTop => (Face::Right, Face::Right),
        Direction::LeftRight | Direction::RightLeft => (Face::Bottom, Face::Bottom),
    }
}

fn resolve_edge_bounds(layout: &Layout, edge: &Edge) -> Option<(NodeBounds, NodeBounds)> {
    let from_bounds = if let Some(sg_id) = edge.from_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.from)?
    };
    let to_bounds = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.to)?
    };
    Some((from_bounds, to_bounds))
}

fn subgraph_bounds_as_node(bounds: &SubgraphBounds) -> NodeBounds {
    NodeBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

fn is_backward_edge(
    from_bounds: &NodeBounds,
    to_bounds: &NodeBounds,
    direction: Direction,
) -> bool {
    match direction {
        Direction::TopDown => to_bounds.y < from_bounds.y,
        Direction::BottomTop => to_bounds.y > from_bounds.y,
        Direction::LeftRight => to_bounds.x < from_bounds.x,
        Direction::RightLeft => to_bounds.x > from_bounds.x,
    }
}

/// Determine source and target attachment faces for the flow direction.
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

/// Build an orthogonal polyline in float space from start to end through optional waypoints.
///
/// Diagonal spans are split into two elbows using midpoint routing on the
/// diagram's primary axis to keep paths axis-aligned and symmetric.
pub(crate) const ROUTE_ALIGN_EPS: f64 = 0.5;
pub(crate) const ROUTE_POINT_EPS: f64 = 0.000_001;
pub(crate) const LARGE_HORIZONTAL_OFFSET_THRESHOLD: usize = 30;
const MIN_TERMINAL_SUPPORT: f64 = 8.0;

pub(crate) fn build_orthogonal_path_float(
    start: FPoint,
    end: FPoint,
    direction: Direction,
    waypoints: &[FPoint],
) -> Vec<FPoint> {
    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let mut control_points: Vec<FPoint> = Vec::with_capacity(waypoints.len() + 2);
    control_points.push(start);
    control_points.extend_from_slice(waypoints);
    control_points.push(end);

    let mut output: Vec<FPoint> = Vec::with_capacity(control_points.len() * 3);
    output.push(start);
    let span_count = control_points.len().saturating_sub(1);

    for (span_idx, target) in control_points.into_iter().skip(1).enumerate() {
        let current = output.last().copied().unwrap_or(start);
        let is_first_span = span_idx == 0;
        let is_last_span = span_idx + 1 == span_count;

        if (current.x - target.x).abs() < ROUTE_POINT_EPS
            && (current.y - target.y).abs() < ROUTE_POINT_EPS
        {
            continue;
        }

        let x_aligned = (current.x - target.x).abs() < ROUTE_ALIGN_EPS;
        let y_aligned = (current.y - target.y).abs() < ROUTE_ALIGN_EPS;
        if x_aligned && y_aligned {
            continue;
        }

        if x_aligned {
            output.push(FPoint::new(current.x, target.y));
            continue;
        }

        if y_aligned {
            output.push(FPoint::new(target.x, current.y));
            continue;
        }

        // For diagonal spans, choose elbow orientation by span role:
        // - first span: preserve source-face normal support
        // - last span: preserve target-face normal support
        // - single diagonal span: keep balanced V-H-V / H-V-H fallback
        if primary_vertical && is_first_span && is_last_span {
            let mid_y = (current.y + target.y) / 2.0;
            output.push(FPoint::new(current.x, mid_y));
            output.push(FPoint::new(target.x, mid_y));
        } else if !primary_vertical && is_first_span && is_last_span {
            let mid_x = (current.x + target.x) / 2.0;
            output.push(FPoint::new(mid_x, current.y));
            output.push(FPoint::new(mid_x, target.y));
        } else if primary_vertical {
            if is_first_span {
                output.push(FPoint::new(current.x, target.y));
            } else {
                output.push(FPoint::new(target.x, current.y));
            }
        } else if is_first_span {
            output.push(FPoint::new(target.x, current.y));
        } else {
            output.push(FPoint::new(current.x, target.y));
        }
        output.push(target);
    }

    output.dedup_by(|a, b| {
        (a.x - b.x).abs() < ROUTE_POINT_EPS && (a.y - b.y).abs() < ROUTE_POINT_EPS
    });
    output
}

/// Enforce shared polyline contracts used by routed-preview outputs.
///
/// Contracts:
/// - no adjacent duplicate points
/// - no zero-length segments
/// - no redundant collinear interior points
/// - non-zero terminal support segment on the primary axis
pub(crate) fn normalize_orthogonal_route_contracts(
    points: &[FPoint],
    direction: Direction,
) -> Vec<FPoint> {
    if points.len() <= 1 {
        return points.to_vec();
    }

    let mut normalized = dedupe_adjacent_points(points);
    if normalized.len() <= 1 {
        return normalized;
    }

    normalized = remove_collinear_points(&normalized);
    normalized = reduce_midfield_jogs_for_large_horizontal_offset(&normalized, direction);
    normalized = compact_terminal_staircase(&normalized, direction);
    normalized = remove_axial_turnbacks(&normalized);
    ensure_terminal_support_segment(&mut normalized, direction);
    normalized = remove_axial_turnbacks(&normalized);
    normalized = dedupe_adjacent_points(&normalized);
    remove_collinear_points(&normalized)
}

fn dedupe_adjacent_points(points: &[FPoint]) -> Vec<FPoint> {
    let mut deduped = Vec::with_capacity(points.len());
    for point in points {
        let keep = deduped.last().is_none_or(|prev: &FPoint| {
            (prev.x - point.x).abs() > ROUTE_POINT_EPS || (prev.y - point.y).abs() > ROUTE_POINT_EPS
        });
        if keep {
            deduped.push(*point);
        }
    }
    deduped
}

fn remove_collinear_points(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(points.len());
    result.push(points[0]);
    for idx in 1..(points.len() - 1) {
        let prev = result.last().copied().expect("result is non-empty");
        let curr = points[idx];
        let next = points[idx + 1];

        let dx1 = curr.x - prev.x;
        let dy1 = curr.y - prev.y;
        let dx2 = next.x - curr.x;
        let dy2 = next.y - curr.y;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= ROUTE_POINT_EPS && dot >= -ROUTE_POINT_EPS;

        if !collinear_same_direction {
            result.push(curr);
        }
    }
    result.push(*points.last().expect("points has at least two elements"));
    result
}

fn remove_axial_turnbacks(points: &[FPoint]) -> Vec<FPoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut current = points.to_vec();
    loop {
        let mut changed = false;
        let mut result = Vec::with_capacity(current.len());
        result.push(current[0]);

        for idx in 1..(current.len() - 1) {
            let prev = *result.last().expect("result is non-empty");
            let curr = current[idx];
            let next = current[idx + 1];
            let dx1 = curr.x - prev.x;
            let dy1 = curr.y - prev.y;
            let dx2 = next.x - curr.x;
            let dy2 = next.y - curr.y;
            let cross = dx1 * dy2 - dy1 * dx2;
            let dot = dx1 * dx2 + dy1 * dy2;
            let is_collinear = cross.abs() <= ROUTE_POINT_EPS;
            let reverses_direction = dot < -ROUTE_POINT_EPS;
            if is_collinear && reverses_direction {
                changed = true;
                continue;
            }
            result.push(curr);
        }

        result.push(*current.last().expect("points has at least two elements"));
        let deduped = dedupe_adjacent_points(&result);
        if !changed {
            return deduped;
        }
        current = deduped;
        if current.len() <= 2 {
            return current;
        }
    }
}

fn ensure_terminal_support_segment(points: &mut Vec<FPoint>, direction: Direction) {
    if points.len() < 2 {
        return;
    }

    let primary_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);
    let end = *points.last().expect("len >= 2");
    let prev = points[points.len() - 2];

    if primary_vertical {
        let already_supported =
            (prev.x - end.x).abs() <= ROUTE_POINT_EPS && (prev.y - end.y).abs() > ROUTE_POINT_EPS;
        if already_supported {
            return;
        }

        if (prev.y - end.y).abs() > ROUTE_POINT_EPS {
            points.insert(points.len() - 1, FPoint::new(end.x, prev.y));
            return;
        }

        let support_y = match direction {
            Direction::TopDown => end.y - MIN_TERMINAL_SUPPORT,
            Direction::BottomTop => end.y + MIN_TERMINAL_SUPPORT,
            _ => end.y - MIN_TERMINAL_SUPPORT,
        };
        points.insert(points.len() - 1, FPoint::new(prev.x, support_y));
        points.insert(points.len() - 1, FPoint::new(end.x, support_y));
    } else {
        let already_supported =
            (prev.y - end.y).abs() <= ROUTE_POINT_EPS && (prev.x - end.x).abs() > ROUTE_POINT_EPS;
        if already_supported {
            return;
        }

        if (prev.x - end.x).abs() > ROUTE_POINT_EPS {
            points.insert(points.len() - 1, FPoint::new(prev.x, end.y));
            return;
        }

        let support_x = match direction {
            Direction::LeftRight => end.x - MIN_TERMINAL_SUPPORT,
            Direction::RightLeft => end.x + MIN_TERMINAL_SUPPORT,
            _ => end.x - MIN_TERMINAL_SUPPORT,
        };
        points.insert(points.len() - 1, FPoint::new(support_x, prev.y));
        points.insert(points.len() - 1, FPoint::new(support_x, end.y));
    }
}

fn reduce_midfield_jogs_for_large_horizontal_offset(
    points: &[FPoint],
    direction: Direction,
) -> Vec<FPoint> {
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) || points.len() <= 4 {
        return points.to_vec();
    }

    let start = points[0];
    let end = *points.last().expect("points has at least two elements");
    let horizontal_offset = (start.x - end.x).abs();
    if horizontal_offset <= LARGE_HORIZONTAL_OFFSET_THRESHOLD as f64 {
        return points.to_vec();
    }

    let mid_y = preferred_mid_y_for_vertical_layout(start, end, direction);
    vec![
        start,
        FPoint::new(start.x, mid_y),
        FPoint::new(end.x, mid_y),
        end,
    ]
}

fn compact_terminal_staircase(points: &[FPoint], direction: Direction) -> Vec<FPoint> {
    // Keep short 4-point routes intact so source-face support is not converted
    // into a border-slide start segment.
    if points.len() <= 4 {
        return points.to_vec();
    }

    let mut compacted = points.to_vec();
    let len = compacted.len();
    let a = compacted[len - 4];
    let b = compacted[len - 3];
    let c = compacted[len - 2];
    let d = compacted[len - 1];

    if matches!(direction, Direction::TopDown | Direction::BottomTop) {
        if segment_is_vertical(a, b)
            && segment_is_horizontal(b, c)
            && segment_is_vertical(c, d)
            && segment_sign(b.y - a.y) == segment_sign(d.y - c.y)
            && segment_sign(b.y - a.y) != 0
        {
            let elbow = FPoint::new(c.x, a.y);
            let would_reverse_with_prefix =
                would_introduce_axial_turnback_with_prefix(&compacted, len - 4, a, elbow);
            if !points_equal(a, elbow) && !points_equal(elbow, d) && !would_reverse_with_prefix {
                compacted.truncate(len - 3);
                compacted.push(elbow);
                compacted.push(d);
            }
        }
    } else if segment_is_horizontal(a, b)
        && segment_is_vertical(b, c)
        && segment_is_horizontal(c, d)
        && segment_sign(b.x - a.x) == segment_sign(d.x - c.x)
        && segment_sign(b.x - a.x) != 0
    {
        let elbow = FPoint::new(a.x, c.y);
        let would_reverse_with_prefix =
            would_introduce_axial_turnback_with_prefix(&compacted, len - 4, a, elbow);
        if !points_equal(a, elbow) && !points_equal(elbow, d) && !would_reverse_with_prefix {
            compacted.truncate(len - 3);
            compacted.push(elbow);
            compacted.push(d);
        }
    }

    compacted
}

fn would_introduce_axial_turnback_with_prefix(
    points: &[FPoint],
    anchor_idx: usize,
    anchor: FPoint,
    elbow: FPoint,
) -> bool {
    if anchor_idx == 0 || anchor_idx >= points.len() {
        return false;
    }

    let prefix = points[anchor_idx - 1];
    let dx1 = anchor.x - prefix.x;
    let dy1 = anchor.y - prefix.y;
    let dx2 = elbow.x - anchor.x;
    let dy2 = elbow.y - anchor.y;
    let cross = dx1 * dy2 - dy1 * dx2;
    let dot = dx1 * dx2 + dy1 * dy2;
    cross.abs() <= ROUTE_POINT_EPS && dot < -ROUTE_POINT_EPS
}

fn segment_is_vertical(start: FPoint, end: FPoint) -> bool {
    (start.x - end.x).abs() <= ROUTE_POINT_EPS && (start.y - end.y).abs() > ROUTE_POINT_EPS
}

fn segment_is_horizontal(start: FPoint, end: FPoint) -> bool {
    (start.y - end.y).abs() <= ROUTE_POINT_EPS && (start.x - end.x).abs() > ROUTE_POINT_EPS
}

fn points_equal(a: FPoint, b: FPoint) -> bool {
    (a.x - b.x).abs() <= ROUTE_POINT_EPS && (a.y - b.y).abs() <= ROUTE_POINT_EPS
}

fn segment_sign(delta: f64) -> i8 {
    if delta.abs() <= ROUTE_POINT_EPS {
        0
    } else if delta.is_sign_positive() {
        1
    } else {
        -1
    }
}

/// Compute the point on a node's shape boundary closest to the approach ray
/// from `approach` toward the node center.
///
/// This is the single source of truth for endpoint geometry used by both the
/// orthogonal router and SVG renderer.
///
/// Coordinate convention: `FRect` uses top-left origin `(rect.x, rect.y)`
/// with `width`/`height` extending right and down, matching SVG's `layered::Rect`.
pub(crate) fn intersect_shape_boundary_float(
    rect: FRect,
    shape: Shape,
    approach: FPoint,
) -> FPoint {
    match shape {
        Shape::Hexagon => {
            let verts = hexagon_vertices(rect);
            let center = FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            intersect_convex_polygon(&verts, approach, center)
        }
        Shape::Diamond => intersect_diamond_boundary(rect, approach),
        _ => intersect_rect_boundary(rect, approach),
    }
}

/// Diamond boundary intersection using closed-form `|dx|/w + |dy|/h = 1`.
/// Verified equivalent to `intersect_convex_polygon` by oracle property test.
fn intersect_diamond_boundary(rect: FRect, approach: FPoint) -> FPoint {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = approach.x - cx;
    let dy = approach.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return FPoint::new(cx, cy + h);
    }

    let t = 1.0 / (dx.abs() / w + dy.abs() / h);
    FPoint::new(cx + t * dx, cy + t * dy)
}

/// Indent fraction for hexagon flat top/bottom edges (matches SVG polygon).
pub(crate) const HEXAGON_INDENT_FACTOR: f64 = 0.2;

/// Return the 6 vertices of a hexagon inscribed in `rect`.
/// Order: top-left, top-right, right, bottom-right, bottom-left, left (clockwise).
pub(crate) fn hexagon_vertices(rect: FRect) -> [FPoint; 6] {
    let indent = rect.width * HEXAGON_INDENT_FACTOR;
    let cy = rect.y + rect.height / 2.0;
    [
        FPoint::new(rect.x + indent, rect.y),              // top-left
        FPoint::new(rect.x + rect.width - indent, rect.y), // top-right
        FPoint::new(rect.x + rect.width, cy),              // right
        FPoint::new(rect.x + rect.width - indent, rect.y + rect.height), // bottom-right
        FPoint::new(rect.x + indent, rect.y + rect.height), // bottom-left
        FPoint::new(rect.x, cy),                           // left
    ]
}

/// Intersect a ray from `center` toward `approach` with a convex polygon.
///
/// Returns the point where the ray first crosses the polygon boundary.
/// For degenerate cases (approach == center), returns the bottom-most vertex.
/// Vertices must be in order (clockwise or counter-clockwise).
pub(crate) fn intersect_convex_polygon(
    vertices: &[FPoint],
    approach: FPoint,
    center: FPoint,
) -> FPoint {
    let dx = approach.x - center.x;
    let dy = approach.y - center.y;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        // Degenerate: return bottom-most vertex
        return vertices
            .iter()
            .copied()
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap_or(center);
    }

    let n = vertices.len();
    let mut best_t = f64::INFINITY;
    let mut best_point = center;

    for i in 0..n {
        let a = vertices[i];
        let b = vertices[(i + 1) % n];

        // Edge vector
        let ex = b.x - a.x;
        let ey = b.y - a.y;

        // Solve: center + t * (dx, dy) = a + s * (ex, ey)
        let denom = dx * ey - dy * ex;
        if denom.abs() < f64::EPSILON {
            continue; // Parallel
        }

        let t = ((a.x - center.x) * ey - (a.y - center.y) * ex) / denom;
        let s = ((a.x - center.x) * dy - (a.y - center.y) * dx) / denom;

        if t > 0.0 && (0.0..=1.0).contains(&s) && t < best_t {
            best_t = t;
            best_point = FPoint::new(center.x + t * dx, center.y + t * dy);
        }
    }

    best_point
}

/// Return the 4 vertices of a diamond (rhombus) inscribed in `rect`.
/// Order: top, right, bottom, left (clockwise from top).
#[cfg(test)]
pub(crate) fn diamond_vertices(rect: FRect) -> [FPoint; 4] {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let hw = rect.width / 2.0;
    let hh = rect.height / 2.0;
    [
        FPoint::new(cx, cy - hh), // top
        FPoint::new(cx + hw, cy), // right
        FPoint::new(cx, cy + hh), // bottom
        FPoint::new(cx - hw, cy), // left
    ]
}

fn intersect_rect_boundary(rect: FRect, approach: FPoint) -> FPoint {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = approach.x - cx;
    let dy = approach.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return FPoint::new(cx, cy + h);
    }

    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        let signed_h = if dy < 0.0 { -h } else { h };
        (signed_h * dx / dy, signed_h)
    } else {
        let signed_w = if dx < 0.0 { -w } else { w };
        (signed_w, signed_w * dy / dx)
    };

    FPoint::new(cx + sx, cy + sy)
}

fn preferred_mid_y_for_vertical_layout(start: FPoint, end: FPoint, direction: Direction) -> f64 {
    let mut mid_y = (start.y + end.y) / 2.0;

    if (start.x - end.x).abs() > LARGE_HORIZONTAL_OFFSET_THRESHOLD as f64 {
        let target_mid = match direction {
            Direction::TopDown => end.y - (MIN_TERMINAL_SUPPORT * 2.0),
            Direction::BottomTop => end.y + (MIN_TERMINAL_SUPPORT * 2.0),
            _ => mid_y,
        };
        mid_y = match direction {
            Direction::TopDown => target_mid.max(mid_y),
            Direction::BottomTop => target_mid.min(mid_y),
            _ => mid_y,
        };
    }

    if (mid_y - end.y).abs() <= ROUTE_POINT_EPS {
        mid_y = if start.y > end.y {
            end.y + MIN_TERMINAL_SUPPORT
        } else {
            end.y - MIN_TERMINAL_SUPPORT
        };
    }

    mid_y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Shape;

    /// Assert that a point lies on the diamond boundary within `eps`.
    fn assert_on_diamond_boundary(point: FPoint, rect: FRect, eps: f64) {
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let w = rect.width / 2.0;
        let h = rect.height / 2.0;
        let boundary = (point.x - cx).abs() / w + (point.y - cy).abs() / h;
        assert!(
            (boundary - 1.0).abs() < eps,
            "point ({}, {}) boundary value {boundary} should be ~1.0 (eps={eps})",
            point.x,
            point.y,
        );
    }

    #[test]
    fn intersect_shape_boundary_diamond_from_below() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0); // center (20, 20)
        let approach = FPoint::new(20.0, 40.0); // directly below
        let result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
        // Should hit bottom vertex (20, 30)
        assert!((result.x - 20.0).abs() < 0.01);
        assert!((result.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn intersect_shape_boundary_diamond_from_right() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0);
        let approach = FPoint::new(40.0, 20.0); // directly right
        let result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
        // Should hit right vertex (30, 20)
        assert!((result.x - 30.0).abs() < 0.01);
        assert!((result.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn intersect_shape_boundary_diamond_diagonal() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0);
        let approach = FPoint::new(35.0, 35.0); // bottom-right diagonal
        let result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
        assert_on_diamond_boundary(result, rect, 0.001);
    }

    #[test]
    fn diamond_intersection_on_boundary_at_all_cardinal_angles() {
        let rect = FRect::new(50.0, 30.0, 40.0, 40.0); // center (70, 50)
        let center = FPoint::new(70.0, 50.0);
        let angles = [0.0_f64, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let radius = 100.0;
        for angle_deg in angles {
            let angle = angle_deg.to_radians();
            let approach = FPoint::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            );
            let result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
            assert_on_diamond_boundary(result, rect, 1e-6);
        }
    }

    #[test]
    fn diamond_vertices_match_svg_polygon() {
        let rect = FRect::new(50.0, 30.0, 40.0, 40.0);
        let verts = diamond_vertices(rect);
        // SVG polygon: (cx, y), (x+w, cy), (cx, y+h), (x, cy)
        let cx = 70.0;
        let cy = 50.0;
        assert!((verts[0].x - cx).abs() < 1e-6 && (verts[0].y - 30.0).abs() < 1e-6); // top
        assert!((verts[1].x - 90.0).abs() < 1e-6 && (verts[1].y - cy).abs() < 1e-6); // right
        assert!((verts[2].x - cx).abs() < 1e-6 && (verts[2].y - 70.0).abs() < 1e-6); // bottom
        assert!((verts[3].x - 50.0).abs() < 1e-6 && (verts[3].y - cy).abs() < 1e-6); // left
    }

    #[test]
    fn rect_intersection_on_boundary_at_all_cardinal_angles() {
        let rect = FRect::new(50.0, 30.0, 40.0, 20.0); // center (70, 40)
        let center = FPoint::new(70.0, 40.0);
        let angles = [0.0_f64, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let radius = 100.0;
        for angle_deg in angles {
            let angle = angle_deg.to_radians();
            let approach = FPoint::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            );
            let result = intersect_shape_boundary_float(rect, Shape::Rectangle, approach);
            // Point must be on the rect boundary (within epsilon of an edge)
            let on_left = (result.x - rect.x).abs() < 1e-6;
            let on_right = (result.x - (rect.x + rect.width)).abs() < 1e-6;
            let on_top = (result.y - rect.y).abs() < 1e-6;
            let on_bottom = (result.y - (rect.y + rect.height)).abs() < 1e-6;
            assert!(
                on_left || on_right || on_top || on_bottom,
                "Result ({}, {}) at {angle_deg}° should be on rect boundary",
                result.x,
                result.y,
            );
        }
    }

    #[test]
    fn diamond_vertices_clockwise_from_top() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0); // center (20,20)
        let verts = diamond_vertices(rect);
        assert!((verts[0].x - 20.0).abs() < 0.01 && (verts[0].y - 10.0).abs() < 0.01); // top
        assert!((verts[1].x - 30.0).abs() < 0.01 && (verts[1].y - 20.0).abs() < 0.01); // right
        assert!((verts[2].x - 20.0).abs() < 0.01 && (verts[2].y - 30.0).abs() < 0.01); // bottom
        assert!((verts[3].x - 10.0).abs() < 0.01 && (verts[3].y - 20.0).abs() < 0.01); // left
    }

    #[test]
    fn hexagon_vertices_match_svg_polygon() {
        let rect = FRect::new(50.0, 30.0, 100.0, 60.0);
        let verts = hexagon_vertices(rect);
        let indent = 100.0 * HEXAGON_INDENT_FACTOR; // 20.0
        let cy = 30.0 + 30.0; // 60.0
        assert_eq!(verts.len(), 6);
        // top-left
        assert!((verts[0].x - (50.0 + indent)).abs() < 1e-6);
        assert!((verts[0].y - 30.0).abs() < 1e-6);
        // top-right
        assert!((verts[1].x - (50.0 + 100.0 - indent)).abs() < 1e-6);
        assert!((verts[1].y - 30.0).abs() < 1e-6);
        // right
        assert!((verts[2].x - 150.0).abs() < 1e-6);
        assert!((verts[2].y - cy).abs() < 1e-6);
        // bottom-right
        assert!((verts[3].x - (50.0 + 100.0 - indent)).abs() < 1e-6);
        assert!((verts[3].y - 90.0).abs() < 1e-6);
        // bottom-left
        assert!((verts[4].x - (50.0 + indent)).abs() < 1e-6);
        assert!((verts[4].y - 90.0).abs() < 1e-6);
        // left
        assert!((verts[5].x - 50.0).abs() < 1e-6);
        assert!((verts[5].y - cy).abs() < 1e-6);
    }

    #[test]
    fn polygon_ray_hexagon_vertical_approach_hits_flat_top() {
        let rect = FRect::new(50.0, 30.0, 100.0, 60.0);
        let verts = hexagon_vertices(rect);
        let center = FPoint::new(100.0, 60.0);
        // Approach from directly above, slightly off-center
        let approach = FPoint::new(90.0, 0.0);
        let result = intersect_convex_polygon(&verts, approach, center);
        // Should hit the flat top edge at x=90 (between indent points 70 and 130)
        assert!(
            (result.y - 30.0).abs() < 0.01,
            "should hit top edge, got y={}",
            result.y
        );
        // Ray from (100,60) toward (90,0) hits top edge at x=95 (not 90, due to diagonal)
        assert!(
            (result.x - 95.0).abs() < 0.01,
            "should hit top edge at projected x, got x={}",
            result.x
        );
    }

    #[test]
    fn polygon_ray_hexagon_vertical_approach_center_hits_flat_top() {
        let rect = FRect::new(50.0, 30.0, 100.0, 60.0);
        let verts = hexagon_vertices(rect);
        let center = FPoint::new(100.0, 60.0);
        let approach = FPoint::new(100.0, 0.0);
        let result = intersect_convex_polygon(&verts, approach, center);
        assert!((result.y - 30.0).abs() < 0.01);
        assert!((result.x - 100.0).abs() < 0.01);
    }

    #[test]
    fn polygon_ray_hexagon_lateral_approach_hits_right_vertex() {
        let rect = FRect::new(50.0, 30.0, 100.0, 60.0);
        let verts = hexagon_vertices(rect);
        let center = FPoint::new(100.0, 60.0);
        let approach = FPoint::new(200.0, 60.0);
        let result = intersect_convex_polygon(&verts, approach, center);
        assert!((result.x - 150.0).abs() < 0.01, "should hit right vertex");
        assert!((result.y - 60.0).abs() < 0.01);
    }

    #[test]
    fn polygon_ray_diamond_matches_closed_form() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0);
        let verts = diamond_vertices(rect);
        let center = FPoint::new(20.0, 20.0);
        let angles = [
            0.0_f64, 30.0, 45.0, 60.0, 90.0, 120.0, 150.0, 180.0, 210.0, 240.0, 270.0, 300.0, 330.0,
        ];
        for angle_deg in angles {
            let angle = angle_deg.to_radians();
            let approach =
                FPoint::new(center.x + 50.0 * angle.cos(), center.y + 50.0 * angle.sin());
            let polygon_result = intersect_convex_polygon(&verts, approach, center);
            let closed_result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
            assert!(
                (polygon_result.x - closed_result.x).abs() < 1e-6
                    && (polygon_result.y - closed_result.y).abs() < 1e-6,
                "Oracle mismatch at {angle_deg}°: poly=({}, {}), closed=({}, {})",
                polygon_result.x,
                polygon_result.y,
                closed_result.x,
                closed_result.y,
            );
        }
    }

    #[test]
    fn diamond_closed_form_matches_polygon_all_angles() {
        let rects = [
            FRect::new(0.0, 0.0, 20.0, 20.0),    // square
            FRect::new(0.0, 0.0, 40.0, 20.0),    // wide
            FRect::new(0.0, 0.0, 20.0, 40.0),    // tall
            FRect::new(50.0, 30.0, 100.0, 60.0), // offset
        ];
        for rect in rects {
            let center = FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let verts = diamond_vertices(rect);
            for angle_deg in (0..360).step_by(5) {
                let angle = (angle_deg as f64).to_radians();
                let approach = FPoint::new(
                    center.x + 200.0 * angle.cos(),
                    center.y + 200.0 * angle.sin(),
                );
                let closed = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
                let polygon = intersect_convex_polygon(&verts, approach, center);
                assert!(
                    (closed.x - polygon.x).abs() < 1e-10 && (closed.y - polygon.y).abs() < 1e-10,
                    "Mismatch at {angle_deg}° for rect {:?}: closed=({}, {}), polygon=({}, {})",
                    rect,
                    closed.x,
                    closed.y,
                    polygon.x,
                    polygon.y,
                );
            }
        }
    }

    #[test]
    fn intersect_shape_boundary_diamond_degenerate_center() {
        let rect = FRect::new(10.0, 10.0, 20.0, 20.0);
        let approach = FPoint::new(20.0, 20.0); // at center
        let result = intersect_shape_boundary_float(rect, Shape::Diamond, approach);
        // Should return bottom vertex as default
        assert!((result.x - 20.0).abs() < 0.01);
        assert!((result.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn intersect_shape_boundary_rect_from_below() {
        let rect = FRect::new(10.0, 10.0, 20.0, 10.0);
        let approach = FPoint::new(20.0, 30.0);
        let result = intersect_shape_boundary_float(rect, Shape::Rectangle, approach);
        // Should hit bottom edge at center
        assert!((result.x - 20.0).abs() < 0.01);
        assert!((result.y - 20.0).abs() < 0.01);
    }
}
