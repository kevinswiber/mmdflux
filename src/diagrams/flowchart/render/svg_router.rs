//! SVG edge routing for direction-override subgraphs.
//!
//! After sublayout reconciliation repositions nodes inside direction-override
//! subgraphs, the layout's pre-computed Bézier paths are stale. This module computes
//! fresh orthogonal edge paths in float coordinates for all edges touching
//! override subgraphs.

use std::collections::{HashMap, HashSet};

// Re-export shared routing policy functions under their SVG-specific names
// for backward compatibility with callers in svg.rs.
pub use super::route_policy::build_node_directions as build_node_directions_svg;
pub use super::route_policy::{
    build_override_node_map, effective_edge_direction as effective_edge_direction_svg,
};
use super::routing_core::{
    AttachmentCandidate, AttachmentSide, Face, build_orthogonal_path_float,
    edge_faces as shared_edge_faces, plan_attachment_candidates,
    point_on_face_float as shared_point_on_face_float,
};
use crate::diagrams::flowchart::geometry::FRect;
use crate::graph::{Diagram, Direction};
use crate::layered::{LayoutResult, NodeId, Point, Rect};

/// The face an edge exits from in the given flow direction.
fn exit_face(direction: Direction) -> Face {
    shared_edge_faces(direction, false).0
}

/// The face an edge enters through in the given flow direction.
fn entry_face(direction: Direction) -> Face {
    shared_edge_faces(direction, false).1
}

/// Compute a point on a face at the given fraction (0.0 = start, 0.5 = center, 1.0 = end).
///
/// For horizontal faces (Top/Bottom), fraction runs left-to-right.
/// For vertical faces (Left/Right), fraction runs top-to-bottom.
fn point_on_face(rect: &Rect, face: Face, fraction: f64) -> Point {
    let rect = FRect::from(*rect);
    shared_point_on_face_float(rect, face, fraction).into()
}

/// Compute the exit point from a rectangular node along a given direction (center of face).
#[cfg(test)]
fn exit_point(rect: &Rect, direction: Direction) -> Point {
    point_on_face(rect, exit_face(direction), 0.5)
}

/// Compute the entry point into a rectangular node along a given direction (center of face).
#[cfg(test)]
fn entry_point(rect: &Rect, direction: Direction) -> Point {
    point_on_face(rect, entry_face(direction), 0.5)
}

/// Route an orthogonal edge path between two nodes in float space.
///
/// Computes a straight or L-shaped path using the effective direction.
#[cfg(test)]
pub fn route_svg_edge_direct(from_rect: &Rect, to_rect: &Rect, direction: Direction) -> Vec<Point> {
    let start = exit_point(from_rect, direction);
    let end = entry_point(to_rect, direction);
    build_orthogonal_path_float(start.into(), end.into(), direction, &[])
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Route an edge with explicit port fractions for exit and entry faces.
///
/// `from_port` and `to_port` are fractions (0.0–1.0) along the face,
/// where 0.5 is the center.  This allows multiple edges sharing a face
/// to attach at different positions, preventing overlap.
fn route_svg_edge_ported(
    from_rect: &Rect,
    to_rect: &Rect,
    direction: Direction,
    from_port: f64,
    to_port: f64,
) -> Vec<Point> {
    let start = point_on_face(from_rect, exit_face(direction), from_port);
    let end = point_on_face(to_rect, entry_face(direction), to_port);
    build_orthogonal_path_float(start.into(), end.into(), direction, &[])
        .into_iter()
        .map(Into::into)
        .collect()
}

fn svg_spread_fraction_from_plan(fraction: f64, group_size: usize) -> f64 {
    if group_size <= 1 {
        0.5
    } else {
        let margin = 0.25; // keep away from corners, matching prior SVG behavior
        margin + (1.0 - 2.0 * margin) * fraction
    }
}

/// Route an edge that crosses a subgraph boundary.
///
/// Uses a simple L-shaped path with the outside (diagram) direction for both
/// endpoints.  Cross-boundary edges are routed the same way as normal edges —
/// exit source along the flow direction, elbow at the midpoint, enter target
/// along the flow direction — so paths swing outward rather than cutting
/// through the interior of the diagram.
#[cfg(test)]
pub fn route_svg_edge_with_boundary(
    from_rect: &Rect,
    to_rect: &Rect,
    _sg_rect: &Rect,
    _from_is_inside: bool,
    outside_direction: Direction,
) -> Vec<Point> {
    route_svg_edge_direct(from_rect, to_rect, outside_direction)
}

/// Statistics about rerouted edges for debugging.
#[derive(Debug, Default)]
pub struct RerouteStats {
    pub unaffected: usize,
    pub internal: usize,
    pub cross_boundary: usize,
}

/// Resolve the direction policy for a cross-boundary edge.
///
/// This must stay in sync between spacing repair and rerouting:
/// - if endpoints are in different override subgraphs and one is ancestor of the
///   other, use the ancestor override direction;
/// - if endpoints are in different non-ancestor branches, use root direction;
/// - if only one endpoint is in an override subgraph, use the outside node's
///   effective direction.
fn cross_boundary_edge_direction(
    diagram: &Diagram,
    node_directions: &HashMap<String, Direction>,
    from_sg: Option<&String>,
    to_sg: Option<&String>,
    from_node: &str,
    to_node: &str,
) -> Direction {
    if let (Some(sg_a), Some(sg_b)) = (from_sg, to_sg) {
        if is_ancestor_sg(diagram, sg_a, sg_b) {
            return diagram
                .subgraphs
                .get(sg_a.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(diagram.direction);
        }
        if is_ancestor_sg(diagram, sg_b, sg_a) {
            return diagram
                .subgraphs
                .get(sg_b.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(diagram.direction);
        }
        return diagram.direction;
    }

    let outside_node = if from_sg.is_some() && to_sg.is_none() {
        to_node
    } else {
        from_node
    };

    node_directions
        .get(outside_node)
        .copied()
        .unwrap_or(diagram.direction)
}

/// Reroute all edges affected by direction-override subgraphs.
///
/// Modifies the `LayoutResult` in-place with fresh paths for edges touching
/// override subgraphs. Edges where both endpoints are outside override subgraphs
/// are left untouched.
///
/// Uses a two-pass approach: first collects routing decisions, then groups edges
/// by shared node faces to spread attachment points so that multiple edges
/// arriving at the same face don't overlap.
pub fn reroute_override_edges(
    diagram: &Diagram,
    layout: &mut LayoutResult,
    node_directions: &HashMap<String, Direction>,
) -> (RerouteStats, HashSet<usize>) {
    // Check if any subgraphs have direction overrides
    let has_overrides = diagram.subgraphs.values().any(|sg| sg.dir.is_some());
    if !has_overrides {
        return (RerouteStats::default(), HashSet::new());
    }

    // Build override node map: node_id -> subgraph_id (deepest wins)
    let override_nodes = build_override_node_map(diagram);

    let mut stats = RerouteStats::default();
    let mut rerouted_indices = HashSet::new();

    // --- Pass 1: Collect routing decisions ---
    struct PendingRoute {
        layout_pos: usize, // position in layout.edges
        edge_index: usize, // edge_layout.index (into diagram.edges)
        direction: Direction,
        from_id: String,
        to_id: String,
    }

    let mut pending: Vec<PendingRoute> = Vec::new();

    for (pos, edge_layout) in layout.edges.iter().enumerate() {
        let Some(edge) = diagram.edges.get(edge_layout.index) else {
            continue;
        };

        // Skip subgraph-as-node edges
        if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
            stats.unaffected += 1;
            continue;
        }

        let from_sg = override_nodes.get(&edge.from);
        let to_sg = override_nodes.get(&edge.to);

        match (from_sg, to_sg) {
            (None, None) => {
                stats.unaffected += 1;
            }
            (Some(sg_a), Some(sg_b)) if sg_a == sg_b => {
                stats.internal += 1;
                let dir = effective_edge_direction_svg(
                    node_directions,
                    &edge.from,
                    &edge.to,
                    diagram.direction,
                );
                pending.push(PendingRoute {
                    layout_pos: pos,
                    edge_index: edge_layout.index,
                    direction: dir,
                    from_id: edge.from.clone(),
                    to_id: edge.to.clone(),
                });
            }
            _ => {
                stats.cross_boundary += 1;
                let outside_dir = cross_boundary_edge_direction(
                    diagram,
                    node_directions,
                    from_sg,
                    to_sg,
                    &edge.from,
                    &edge.to,
                );

                pending.push(PendingRoute {
                    layout_pos: pos,
                    edge_index: edge_layout.index,
                    direction: outside_dir,
                    from_id: edge.from.clone(),
                    to_id: edge.to.clone(),
                });
            }
        }
    }

    // --- Pass 2: Compute port fractions for shared faces via shared planner ---
    let mut planner_inputs: Vec<AttachmentCandidate> = Vec::with_capacity(pending.len() * 2);
    for (pi, pr) in pending.iter().enumerate() {
        let from_rect = layout.nodes.get(&NodeId(pr.from_id.clone()));
        let to_rect = layout.nodes.get(&NodeId(pr.to_id.clone()));
        if let (Some(fr), Some(tr)) = (from_rect, to_rect) {
            let horizontal_face = matches!(pr.direction, Direction::TopDown | Direction::BottomTop);
            let exit_sort = if horizontal_face {
                tr.x + tr.width / 2.0
            } else {
                tr.y + tr.height / 2.0
            };
            let entry_sort = if horizontal_face {
                fr.x + fr.width / 2.0
            } else {
                fr.y + fr.height / 2.0
            };

            planner_inputs.push(AttachmentCandidate {
                edge_index: pi,
                node_id: pr.from_id.clone(),
                side: AttachmentSide::Source,
                face: exit_face(pr.direction),
                cross_axis: exit_sort,
            });
            planner_inputs.push(AttachmentCandidate {
                edge_index: pi,
                node_id: pr.to_id.clone(),
                side: AttachmentSide::Target,
                face: entry_face(pr.direction),
                cross_axis: entry_sort,
            });
        }
    }

    let attachment_plan = plan_attachment_candidates(planner_inputs);

    let mut from_fractions: Vec<f64> = vec![0.5; pending.len()];
    let mut to_fractions: Vec<f64> = vec![0.5; pending.len()];

    for (pi, pr) in pending.iter().enumerate() {
        if let Some(edge_plan) = attachment_plan.edge(pi) {
            if let Some(source) = edge_plan.source {
                from_fractions[pi] = svg_spread_fraction_from_plan(
                    source.fraction,
                    attachment_plan.group_size(&pr.from_id, source.face),
                );
            }
            if let Some(target) = edge_plan.target {
                to_fractions[pi] = svg_spread_fraction_from_plan(
                    target.fraction,
                    attachment_plan.group_size(&pr.to_id, target.face),
                );
            }
        }
    }

    // --- Pass 3: Route each edge with its port fractions ---
    for (pi, pr) in pending.iter().enumerate() {
        if let (Some(from_rect), Some(to_rect)) = (
            layout.nodes.get(&NodeId(pr.from_id.clone())),
            layout.nodes.get(&NodeId(pr.to_id.clone())),
        ) {
            layout.edges[pr.layout_pos].points = route_svg_edge_ported(
                from_rect,
                to_rect,
                pr.direction,
                from_fractions[pi],
                to_fractions[pi],
            );
            rerouted_indices.insert(pr.edge_index);
        }
    }

    (stats, rerouted_indices)
}

/// Ensure adequate spacing for cross-boundary edges in direction-override subgraphs.
///
/// When nodes are placed by different sublayouts (e.g., one in an LR subgraph,
/// another in a nested BT subgraph), their gap along the effective edge direction
/// can be very small because the sublayouts optimise for different axes.  This
/// function pushes the shallower (less-constrained) node away to create at least
/// `min_gap` pixels of space.
///
/// Must run **before** `reroute_override_edges` so that rerouted paths use the
/// corrected node positions.
pub fn ensure_cross_boundary_edge_spacing(
    diagram: &Diagram,
    layout: &mut LayoutResult,
    node_directions: &HashMap<String, Direction>,
    min_gap: f64,
) {
    let has_overrides = diagram.subgraphs.values().any(|sg| sg.dir.is_some());
    if !has_overrides {
        return;
    }

    let override_nodes = build_override_node_map(diagram);

    for edge in &diagram.edges {
        if edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
            continue; // subgraph-as-node edges handled separately
        }

        let from_sg = override_nodes.get(&edge.from);
        let to_sg = override_nodes.get(&edge.to);

        // Only cross-boundary edges (different override subgraphs, or one
        // inside an override and the other outside).
        let is_cross = match (from_sg, to_sg) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if !is_cross {
            continue;
        }

        let direction = cross_boundary_edge_direction(
            diagram,
            node_directions,
            from_sg,
            to_sg,
            &edge.from,
            &edge.to,
        );

        let from_key = NodeId(edge.from.clone());
        let to_key = NodeId(edge.to.clone());

        let from_rect = match layout.nodes.get(&from_key) {
            Some(r) => *r,
            None => continue,
        };
        let to_rect = match layout.nodes.get(&to_key) {
            Some(r) => *r,
            None => continue,
        };

        // Gap along the flow direction (source trailing edge → target leading edge).
        let gap = match direction {
            Direction::TopDown => to_rect.y - (from_rect.y + from_rect.height),
            Direction::BottomTop => from_rect.y - (to_rect.y + to_rect.height),
            Direction::LeftRight => to_rect.x - (from_rect.x + from_rect.width),
            Direction::RightLeft => from_rect.x - (to_rect.x + to_rect.width),
        };

        // Only adjust when nodes are in the correct order but too close.
        // Negative gap means backward order — let the edge router handle that.
        if gap < 0.0 || gap >= min_gap {
            continue;
        }

        let shift = min_gap - gap;

        // Push the node in the shallower (less-constrained) subgraph.
        let from_depth = from_sg.map(|sg| diagram.subgraph_depth(sg)).unwrap_or(0);
        let to_depth = to_sg.map(|sg| diagram.subgraph_depth(sg)).unwrap_or(0);
        let push_source = from_depth <= to_depth;

        if push_source {
            let r = layout.nodes.get_mut(&from_key).unwrap();
            match direction {
                Direction::TopDown => r.y -= shift,
                Direction::BottomTop => r.y += shift,
                Direction::LeftRight => r.x -= shift,
                Direction::RightLeft => r.x += shift,
            }
        } else {
            let r = layout.nodes.get_mut(&to_key).unwrap();
            match direction {
                Direction::TopDown => r.y += shift,
                Direction::BottomTop => r.y -= shift,
                Direction::LeftRight => r.x += shift,
                Direction::RightLeft => r.x -= shift,
            }
        }
    }
}

/// Reroute edges where one or both endpoints target a subgraph (subgraph-as-node).
///
/// The layout routes these through resolved child nodes inside the subgraph, creating
/// waypoints with small horizontal offsets.  The B-spline curve amplifies these
/// into visible curves.  This function replaces those paths with fresh orthogonal
/// routes computed from the subgraph bounds, producing straight lines or clean
/// L-shaped elbows.
///
/// Returns the set of diagram edge indices that were rerouted so that downstream
/// code can skip redundant shape-adjustment and clipping.
pub fn reroute_subgraph_node_edges(diagram: &Diagram, layout: &mut LayoutResult) -> HashSet<usize> {
    // --- Pass 1: Collect routing decisions ---
    struct PendingRoute {
        layout_pos: usize,
        edge_index: usize,
        direction: Direction,
        from_id: String,
        to_id: String,
    }

    let mut pending: Vec<PendingRoute> = Vec::new();

    for (pos, edge_layout) in layout.edges.iter().enumerate() {
        let Some(edge) = diagram.edges.get(edge_layout.index) else {
            continue;
        };

        if edge.from_subgraph.is_none() && edge.to_subgraph.is_none() {
            continue;
        }

        // Resolve the rect key for each endpoint
        let from_id = edge
            .from_subgraph
            .as_ref()
            .cloned()
            .unwrap_or_else(|| edge.from.clone());
        let to_id = edge
            .to_subgraph
            .as_ref()
            .cloned()
            .unwrap_or_else(|| edge.to.clone());

        pending.push(PendingRoute {
            layout_pos: pos,
            edge_index: edge_layout.index,
            direction: diagram.direction,
            from_id,
            to_id,
        });
    }

    if pending.is_empty() {
        return HashSet::new();
    }

    // --- Pass 2: Compute port fractions for shared faces ---
    let mut face_edges: HashMap<(String, Face), Vec<(usize, f64)>> = HashMap::new();

    for (pi, pr) in pending.iter().enumerate() {
        let from_rect = get_rect(layout, &pr.from_id);
        let to_rect = get_rect(layout, &pr.to_id);
        if let (Some(fr), Some(tr)) = (from_rect, to_rect) {
            let horizontal_face = matches!(pr.direction, Direction::TopDown | Direction::BottomTop);

            let ef = exit_face(pr.direction);
            let exit_sort = if horizontal_face {
                tr.x + tr.width / 2.0
            } else {
                tr.y + tr.height / 2.0
            };
            face_edges
                .entry((pr.from_id.clone(), ef))
                .or_default()
                .push((pi, exit_sort));

            let enf = entry_face(pr.direction);
            let entry_sort = if horizontal_face {
                fr.x + fr.width / 2.0
            } else {
                fr.y + fr.height / 2.0
            };
            face_edges
                .entry((pr.to_id.clone(), enf))
                .or_default()
                .push((pi, entry_sort));
        }
    }

    let mut from_fractions: Vec<f64> = vec![0.5; pending.len()];
    let mut to_fractions: Vec<f64> = vec![0.5; pending.len()];

    for ((node_id, face), mut entries) in face_edges {
        if entries.len() <= 1 {
            continue;
        }

        entries.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let n = entries.len();
        let margin = 0.25;

        for (rank, &(pi, _)) in entries.iter().enumerate() {
            let frac = margin + (1.0 - 2.0 * margin) * (rank as f64) / ((n - 1) as f64);

            let pr = &pending[pi];
            let is_exit = pr.from_id == node_id && exit_face(pr.direction) == face;
            if is_exit {
                from_fractions[pi] = frac;
            } else {
                to_fractions[pi] = frac;
            }
        }
    }

    // --- Pass 3: Route each edge with its port fractions ---
    let mut rerouted = HashSet::new();

    for (pi, pr) in pending.iter().enumerate() {
        let from_rect = get_rect(layout, &pr.from_id);
        let to_rect = get_rect(layout, &pr.to_id);
        if let (Some(fr), Some(tr)) = (from_rect, to_rect) {
            layout.edges[pr.layout_pos].points =
                route_svg_edge_ported(fr, tr, pr.direction, from_fractions[pi], to_fractions[pi]);
            rerouted.insert(pr.edge_index);
        }
    }

    rerouted
}

/// Look up a rect by ID, checking subgraph_bounds first, then nodes.
fn get_rect<'a>(layout: &'a LayoutResult, id: &str) -> Option<&'a Rect> {
    layout
        .subgraph_bounds
        .get(id)
        .or_else(|| layout.nodes.get(&NodeId(id.to_string())))
}

/// Check whether `ancestor` is a (transitive) ancestor of `descendant` in the
/// subgraph hierarchy by walking up the parent chain from `descendant`.
fn is_ancestor_sg(diagram: &Diagram, ancestor: &str, descendant: &str) -> bool {
    let mut current = descendant;
    while let Some(parent) = diagram
        .subgraphs
        .get(current)
        .and_then(|sg| sg.parent.as_deref())
    {
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

/// After sublayout reconciliation and overlap resolution, align direct sibling
/// nodes with their cross-boundary edge targets on the cross-axis of the parent
/// direction in layout float coordinates.  This is the SVG-pipeline equivalent of
/// `align_cross_boundary_siblings_draw` in the text pipeline.
pub fn align_cross_boundary_siblings(diagram: &Diagram, layout: &mut LayoutResult) {
    for (sg_id, sg) in &diagram.subgraphs {
        let Some(sub_dir) = sg.dir else { continue };
        let is_horizontal = matches!(sub_dir, Direction::LeftRight | Direction::RightLeft);

        // Collect nodes that belong to any child subgraph of this parent.
        let child_sg_nodes: HashSet<&str> = diagram
            .subgraphs
            .iter()
            .filter(|(_, child)| child.parent.as_deref() == Some(sg_id.as_str()))
            .flat_map(|(_, child)| child.nodes.iter().map(|s| s.as_str()))
            .collect();

        if child_sg_nodes.is_empty() {
            continue;
        }

        // Direct member nodes: in this subgraph but not inside any child subgraph.
        let direct_nodes: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| !diagram.is_subgraph(n) && !child_sg_nodes.contains(n.as_str()))
            .map(|s| s.as_str())
            .collect();

        for node_id in &direct_nodes {
            // Collect cross-boundary edge target centers on the cross-axis.
            let mut target_cross: Vec<f64> = Vec::new();
            for edge in &diagram.edges {
                let target = if edge.from == *node_id && child_sg_nodes.contains(edge.to.as_str()) {
                    Some(edge.to.as_str())
                } else if edge.to == *node_id && child_sg_nodes.contains(edge.from.as_str()) {
                    Some(edge.from.as_str())
                } else {
                    None
                };
                if let Some(tid) = target {
                    let nid = NodeId(tid.to_string());
                    if let Some(r) = layout.nodes.get(&nid) {
                        let center = if is_horizontal {
                            r.y + r.height / 2.0
                        } else {
                            r.x + r.width / 2.0
                        };
                        target_cross.push(center);
                    }
                }
            }

            if target_cross.is_empty() {
                continue;
            }

            let avg: f64 = target_cross.iter().sum::<f64>() / target_cross.len() as f64;
            let nid = NodeId(node_id.to_string());
            let Some(rect) = layout.nodes.get_mut(&nid) else {
                continue;
            };

            if is_horizontal {
                let cy = rect.y + rect.height / 2.0;
                if (avg - cy).abs() < 0.5 {
                    continue;
                }
                rect.y = avg - rect.height / 2.0;
            } else {
                let cx = rect.x + rect.width / 2.0;
                if (avg - cx).abs() < 0.5 {
                    continue;
                }
                rect.x = avg - rect.width / 2.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::build_diagram;
    use crate::parser::parse_flowchart;

    #[test]
    fn test_build_node_directions_svg_basic() {
        let input = "graph TD\nsubgraph sg1\ndirection LR\nA --> B\nend\nC --> D\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let dirs = build_node_directions_svg(&diagram);

        assert_eq!(dirs.get("A").copied(), Some(Direction::LeftRight));
        assert_eq!(dirs.get("B").copied(), Some(Direction::LeftRight));
        assert_eq!(dirs.get("C").copied(), Some(Direction::TopDown));
        assert_eq!(dirs.get("D").copied(), Some(Direction::TopDown));
    }

    #[test]
    fn test_build_node_directions_svg_nested_deepest_wins() {
        let input = "graph TD\nsubgraph outer\ndirection LR\nsubgraph inner\ndirection BT\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let dirs = build_node_directions_svg(&diagram);

        // Deepest override wins
        assert_eq!(dirs.get("A").copied(), Some(Direction::BottomTop));
        assert_eq!(dirs.get("B").copied(), Some(Direction::BottomTop));
    }

    #[test]
    fn test_effective_edge_direction_svg_same_override() {
        let mut dirs = HashMap::new();
        dirs.insert("A".to_string(), Direction::LeftRight);
        dirs.insert("B".to_string(), Direction::LeftRight);
        dirs.insert("C".to_string(), Direction::TopDown);

        assert_eq!(
            effective_edge_direction_svg(&dirs, "A", "B", Direction::TopDown),
            Direction::LeftRight,
        );
        // Cross-boundary: falls back to root
        assert_eq!(
            effective_edge_direction_svg(&dirs, "A", "C", Direction::TopDown),
            Direction::TopDown,
        );
    }

    #[test]
    fn test_cross_boundary_direction_uses_ancestor_override() {
        let input = "graph TD\nsubgraph outer\ndirection LR\nA\nsubgraph inner\ndirection BT\nB\nend\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let dirs = build_node_directions_svg(&diagram);
        let override_nodes = build_override_node_map(&diagram);

        let direction = cross_boundary_edge_direction(
            &diagram,
            &dirs,
            override_nodes.get("A"),
            override_nodes.get("B"),
            "A",
            "B",
        );

        assert_eq!(direction, Direction::LeftRight);
    }

    #[test]
    fn test_cross_boundary_direction_uses_outside_node_direction() {
        let input = "graph TD\nsubgraph sg1\ndirection LR\nA --> B\nend\nB --> C\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let dirs = build_node_directions_svg(&diagram);
        let override_nodes = build_override_node_map(&diagram);

        let direction = cross_boundary_edge_direction(
            &diagram,
            &dirs,
            override_nodes.get("B"),
            override_nodes.get("C"),
            "B",
            "C",
        );

        // C is outside overrides, so this should use the root direction.
        assert_eq!(direction, Direction::TopDown);
    }

    #[test]
    fn test_route_svg_edge_direct_aligned_td() {
        let from = Rect {
            x: 90.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };
        let to = Rect {
            x: 90.0,
            y: 60.0,
            width: 20.0,
            height: 20.0,
        };
        let points = route_svg_edge_direct(&from, &to, Direction::TopDown);
        assert_eq!(points.len(), 2);
        assert!((points[0].x - 100.0).abs() < 0.01);
        assert!((points[0].y - 30.0).abs() < 0.01);
        assert!((points[1].x - 100.0).abs() < 0.01);
        assert!((points[1].y - 60.0).abs() < 0.01);
    }

    #[test]
    fn test_route_svg_edge_direct_aligned_lr() {
        let from = Rect {
            x: 10.0,
            y: 90.0,
            width: 20.0,
            height: 20.0,
        };
        let to = Rect {
            x: 60.0,
            y: 90.0,
            width: 20.0,
            height: 20.0,
        };
        let points = route_svg_edge_direct(&from, &to, Direction::LeftRight);
        assert_eq!(points.len(), 2);
        assert!((points[0].x - 30.0).abs() < 0.01);
        assert!((points[0].y - 100.0).abs() < 0.01);
        assert!((points[1].x - 60.0).abs() < 0.01);
        assert!((points[1].y - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_route_svg_edge_direct_offset_needs_elbow() {
        let from = Rect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };
        let to = Rect {
            x: 60.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };
        let points = route_svg_edge_direct(&from, &to, Direction::TopDown);
        // Offset: needs elbow
        assert!(points.len() >= 3);
    }

    #[test]
    fn test_route_svg_edge_with_boundary_exit() {
        let from = Rect {
            x: 40.0,
            y: 40.0,
            width: 20.0,
            height: 20.0,
        };
        let to = Rect {
            x: 40.0,
            y: 150.0,
            width: 20.0,
            height: 20.0,
        };
        let sg = Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        let points = route_svg_edge_with_boundary(&from, &to, &sg, true, Direction::TopDown);
        assert!(!points.is_empty());
        // No NaN
        for p in &points {
            assert!(p.x.is_finite() && p.y.is_finite(), "point has NaN: {:?}", p);
        }
    }

    #[test]
    fn test_route_svg_edge_ported_center_matches_direct() {
        let from = Rect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        };
        let to = Rect {
            x: 80.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        };
        let direct = route_svg_edge_direct(&from, &to, Direction::TopDown);
        let ported = route_svg_edge_ported(&from, &to, Direction::TopDown, 0.5, 0.5);
        assert_eq!(direct.len(), ported.len());
        for (d, p) in direct.iter().zip(ported.iter()) {
            assert!((d.x - p.x).abs() < 0.01, "x mismatch: {} vs {}", d.x, p.x);
            assert!((d.y - p.y).abs() < 0.01, "y mismatch: {} vs {}", d.y, p.y);
        }
    }

    #[test]
    fn test_route_svg_edge_ported_spread_endpoints() {
        let from = Rect {
            x: 100.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        };
        let to = Rect {
            x: 100.0,
            y: 100.0,
            width: 60.0,
            height: 40.0,
        };
        // Two edges entering `to` from top face at different ports
        let left = route_svg_edge_ported(&from, &to, Direction::TopDown, 0.5, 0.25);
        let right = route_svg_edge_ported(&from, &to, Direction::TopDown, 0.5, 0.75);

        // Both share the same from-exit (center of bottom face: x=130)
        assert!((left[0].x - 130.0).abs() < 0.01);
        assert!((right[0].x - 130.0).abs() < 0.01);

        // Entry points differ on to's top face
        let left_end = left.last().unwrap();
        let right_end = right.last().unwrap();
        assert!(
            (left_end.x - 115.0).abs() < 0.01,
            "left entry x={}",
            left_end.x
        ); // 100 + 60*0.25
        assert!(
            (right_end.x - 145.0).abs() < 0.01,
            "right entry x={}",
            right_end.x
        ); // 100 + 60*0.75
        assert!((left_end.y - 100.0).abs() < 0.01); // top of `to`
        assert!((right_end.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_point_on_face_fractions() {
        let rect = Rect {
            x: 50.0,
            y: 100.0,
            width: 80.0,
            height: 40.0,
        };

        // Top face: x varies, y = rect.y
        let top_left = point_on_face(&rect, Face::Top, 0.0);
        assert!((top_left.x - 50.0).abs() < 0.01);
        assert!((top_left.y - 100.0).abs() < 0.01);
        let top_center = point_on_face(&rect, Face::Top, 0.5);
        assert!((top_center.x - 90.0).abs() < 0.01);
        let top_right = point_on_face(&rect, Face::Top, 1.0);
        assert!((top_right.x - 130.0).abs() < 0.01);

        // Right face: x = rect.x + width, y varies
        let right_mid = point_on_face(&rect, Face::Right, 0.5);
        assert!((right_mid.x - 130.0).abs() < 0.01);
        assert!((right_mid.y - 120.0).abs() < 0.01);
    }

    #[test]
    fn test_point_on_face_clamps_fraction_bounds() {
        let rect = Rect {
            x: 50.0,
            y: 100.0,
            width: 80.0,
            height: 40.0,
        };

        let below_zero = point_on_face(&rect, Face::Top, -1.0);
        assert!((below_zero.x - 50.0).abs() < 0.01);
        assert!((below_zero.y - 100.0).abs() < 0.01);

        let above_one = point_on_face(&rect, Face::Left, 2.0);
        assert!((above_one.x - 50.0).abs() < 0.01);
        assert!((above_one.y - 140.0).abs() < 0.01);
    }

    #[test]
    fn test_reroute_spreads_shared_face_attachment_points() {
        // Two cross-boundary edges entering the same node A from its top face.
        // Check that the SVG output has different x positions for C→A and D→A edges.
        let input = "graph TD\nsubgraph s1\ndirection LR\nA --> B\nend\nC --> A\nD --> A\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let options = crate::render::RenderOptions::default();
        let svg = crate::render::render_svg(&diagram, &options);

        // Parse out edge paths — look for paths ending near A's top
        // The two edges should have different endpoint x coordinates.
        // We verify by checking the SVG contains no duplicate endpoints.
        let paths: Vec<&str> = svg
            .lines()
            .filter(|l| l.trim().starts_with("<path d=\"M"))
            .collect();

        // At least 3 edges: A→B (internal), C→A, D→A
        assert!(
            paths.len() >= 3,
            "expected at least 3 edges, got {}",
            paths.len()
        );

        // Collect final L coordinates from each path (the last "L" segment)
        let mut endpoints: Vec<String> = Vec::new();
        for path in &paths {
            // Extract last "Lx,y" from the path
            if let Some(last_l) = path.rfind(" L") {
                let after = &path[last_l + 2..];
                if let Some(end) = after.find('"') {
                    endpoints.push(after[..end].to_string());
                }
            }
        }

        // Check that no two endpoints are identical (the spreading should make them unique)
        for (i, a) in endpoints.iter().enumerate() {
            for b in endpoints.iter().skip(i + 1) {
                assert_ne!(a, b, "endpoints should not overlap: {}", a);
            }
        }
    }
}
