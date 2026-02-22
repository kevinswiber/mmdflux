//! Float-coordinate subgraph operations on `layered::LayoutResult`.
//!
//! These functions reconcile direction-override sublayouts, center external
//! nodes around subgraph bounds, expand parent bounds, and resolve overlaps —
//! all in the layout engine's float coordinate space.  They are consumed by
//! the SVG pipeline (`svg.rs`) and the engine solve path (`engine.rs`).

use std::collections::{HashMap, HashSet};

use super::layout_building::SubLayoutResult;
use crate::graph::{Diagram, Direction};
use crate::layered::{self, Rect};

/// Reconcile direction-override sub-layouts into a LayoutResult (SVG pipeline).
///
/// This updates node positions, internal edge paths, label positions, and subgraph bounds
/// for subgraphs that override direction.
pub(crate) fn reconcile_sublayouts(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    sublayouts: &HashMap<String, SubLayoutResult>,
    title_pad_y: f64,
    content_pad_y: f64,
) {
    if sublayouts.is_empty() {
        return;
    }

    // Process sublayouts in deterministic depth order (shallowest first).
    let mut sorted_sg_ids: Vec<&String> = sublayouts.keys().collect();
    sorted_sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });

    for sg_id in sorted_sg_ids {
        let sublayout = &sublayouts[sg_id];
        let Some(parent_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        // Compute sublayout bounds from node rects.
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for rect in sublayout.result.nodes.values() {
            min_x = min_x.min(rect.x);
            min_y = min_y.min(rect.y);
            max_x = max_x.max(rect.x + rect.width);
            max_y = max_y.max(rect.y + rect.height);
        }

        if !min_x.is_finite() || !min_y.is_finite() {
            continue;
        }

        let sub_w = (max_x - min_x).max(0.0);
        let sub_h = (max_y - min_y).max(0.0);

        // Use the center of the layout's internal node positions as anchor,
        // not the oversized parent cluster bounds.  The compound node
        // bounds span many ranks for long cross-boundary edges, but the
        // sublayout should sit where the internal nodes were ranked.
        let sg = &diagram.subgraphs[sg_id];
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        let (nodes_cx, nodes_cy) = {
            let mut nx_min = f64::INFINITY;
            let mut ny_min = f64::INFINITY;
            let mut nx_max = f64::NEG_INFINITY;
            let mut ny_max = f64::NEG_INFINITY;
            for (nid, rect) in &layout.nodes {
                if sg_node_set.contains(nid.0.as_str()) {
                    nx_min = nx_min.min(rect.x);
                    ny_min = ny_min.min(rect.y);
                    nx_max = nx_max.max(rect.x + rect.width);
                    ny_max = ny_max.max(rect.y + rect.height);
                }
            }
            if nx_min.is_finite() {
                ((nx_min + nx_max) / 2.0, (ny_min + ny_max) / 2.0)
            } else {
                (
                    parent_bounds.x + parent_bounds.width / 2.0,
                    parent_bounds.y + parent_bounds.height / 2.0,
                )
            }
        };

        let has_title = diagram
            .subgraphs
            .get(sg_id)
            .is_some_and(|sg| !sg.title.trim().is_empty());
        let title_pad = if has_title { title_pad_y } else { 0.0 };

        let final_w = sub_w;
        let final_h = sub_h + title_pad + content_pad_y * 2.0;

        let new_sg_x = nodes_cx - final_w / 2.0;
        let new_sg_y = nodes_cy - final_h / 2.0;

        let offset_x = new_sg_x + (final_w - sub_w) / 2.0 - min_x;
        let offset_y = new_sg_y + content_pad_y + title_pad - min_y;

        // Update node positions for sublayout nodes.
        for (node_id, rect) in &sublayout.result.nodes {
            if let Some(existing) = layout.nodes.get_mut(node_id) {
                *existing = Rect {
                    x: rect.x + offset_x,
                    y: rect.y + offset_y,
                    width: rect.width,
                    height: rect.height,
                };
            }
        }

        // Update subgraph bounds and compound node rect.
        let new_bounds = Rect {
            x: new_sg_x,
            y: new_sg_y,
            width: final_w,
            height: final_h,
        };
        layout.subgraph_bounds.insert(sg_id.clone(), new_bounds);
        if let Some(existing) = layout.nodes.get_mut(&layered::NodeId(sg_id.clone())) {
            *existing = new_bounds;
        }

        // Remap edge paths for internal edges.
        let mut edge_points_by_orig_idx: HashMap<usize, Vec<layered::Point>> = HashMap::new();
        for edge in &sublayout.result.edges {
            if let Some(orig_idx) = sublayout.edge_index_map.get(edge.index) {
                let points: Vec<layered::Point> = edge
                    .points
                    .iter()
                    .map(|p| layered::Point {
                        x: p.x + offset_x,
                        y: p.y + offset_y,
                    })
                    .collect();
                edge_points_by_orig_idx.insert(*orig_idx, points);
            }
        }

        for edge in layout.edges.iter_mut() {
            if let Some(points) = edge_points_by_orig_idx.get(&edge.index) {
                edge.points = points.clone();
            }
        }

        // Remap label positions for internal edges.
        for (sub_idx, pos) in &sublayout.result.label_positions {
            if let Some(orig_idx) = sublayout.edge_index_map.get(*sub_idx) {
                let mut updated = *pos;
                updated.point = layered::Point {
                    x: pos.point.x + offset_x,
                    y: pos.point.y + offset_y,
                };
                layout.label_positions.insert(*orig_idx, updated);
            }
        }

        // Remap self-edge paths for internal edges.
        let mut self_edge_points_by_idx: HashMap<usize, Vec<layered::Point>> = HashMap::new();
        for edge in &sublayout.result.self_edges {
            if let Some(orig_idx) = sublayout.edge_index_map.get(edge.edge_index) {
                let points: Vec<layered::Point> = edge
                    .points
                    .iter()
                    .map(|p| layered::Point {
                        x: p.x + offset_x,
                        y: p.y + offset_y,
                    })
                    .collect();
                self_edge_points_by_idx.insert(*orig_idx, points);
            }
        }

        for edge in layout.self_edges.iter_mut() {
            if let Some(points) = self_edge_points_by_idx.get(&edge.edge_index) {
                edge.points = points.clone();
            }
        }
    }
}

/// Shift external predecessor and successor nodes of subgraphs on the
/// cross-axis so they align with the subgraph center.
///
/// Applies to two kinds of subgraphs:
/// 1. **Direction overrides** (`dir.is_some()`): internal layout is computed
///    separately, so external nodes should align with the subgraph as a whole.
/// 2. **Subgraph-as-node targets**: edges like `Client --> sg1` conceptually
///    connect to the subgraph, not a specific child — centering the external
///    node over the subgraph is the natural position.
///
/// Only the earliest-rank predecessors and latest-rank successors are shifted —
/// intermediate chain nodes are left in place so their internal edges remain
/// intact.
///
/// Edge points connected to shifted nodes are interpolated: full shift at the
/// shifted endpoint, tapering to zero at the unshifted endpoint.
pub(crate) fn center_override_subgraphs(diagram: &Diagram, layout: &mut layered::LayoutResult) {
    // Cross-axis: x for TD/BT, y for LR/RL.
    let horizontal = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);

    // Subgraphs referenced as edge endpoints (subgraph-as-node).
    let sg_as_node_ids: HashSet<&str> = diagram
        .edges
        .iter()
        .filter_map(|e| e.to_subgraph.as_deref())
        .chain(
            diagram
                .edges
                .iter()
                .filter_map(|e| e.from_subgraph.as_deref()),
        )
        .collect();

    // Nodes belonging to any subgraph — these should never be shifted since
    // they are positioned by their own subgraph's layout.
    let all_sg_members: HashSet<&str> = diagram
        .subgraphs
        .values()
        .flat_map(|sg| sg.nodes.iter().map(|s| s.as_str()))
        .collect();

    // Collect all shifts to apply: node_id → (delta on cross-axis, primary-axis distance).
    // When a node is claimed by multiple subgraphs (e.g. it's a successor
    // of one and a predecessor of another), keep the shift from the
    // subgraph whose members are closest on the primary axis.
    let mut node_shifts: HashMap<String, (f64, f64)> = HashMap::new();

    for sg_id in &diagram.subgraph_order {
        let sg = &diagram.subgraphs[sg_id];
        let is_dir_override = sg.dir.is_some();
        let is_sg_as_node = sg_as_node_ids.contains(sg_id.as_str());

        if !is_dir_override && !is_sg_as_node {
            continue;
        }

        let Some(sg_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();

        let sg_center = if horizontal {
            sg_bounds.x + sg_bounds.width / 2.0
        } else {
            sg_bounds.y + sg_bounds.height / 2.0
        };

        // Collect external predecessors: edges entering the subgraph.
        // Two sources:
        // 1. Edges where `to` is a direct member and `from` is outside (direction overrides)
        // 2. Edges where `to_subgraph` references this subgraph (subgraph-as-node)
        let mut predecessors: Vec<String> = Vec::new();
        for edge in &diagram.edges {
            let is_incoming = (is_dir_override
                && sg_node_set.contains(edge.to.as_str())
                && !sg_node_set.contains(edge.from.as_str()))
                || (is_sg_as_node && edge.to_subgraph.as_deref() == Some(sg_id.as_str()));

            if is_incoming
                && edge.from != *sg_id
                && !sg_node_set.contains(edge.from.as_str())
                && !all_sg_members.contains(edge.from.as_str())
                && !predecessors.contains(&edge.from)
            {
                predecessors.push(edge.from.clone());
            }
        }

        // Collect external successors: edges leaving the subgraph.
        let mut successors: Vec<String> = Vec::new();
        for edge in &diagram.edges {
            let is_outgoing = (is_dir_override
                && sg_node_set.contains(edge.from.as_str())
                && !sg_node_set.contains(edge.to.as_str()))
                || (is_sg_as_node && edge.from_subgraph.as_deref() == Some(sg_id.as_str()));

            if is_outgoing
                && edge.to != *sg_id
                && !sg_node_set.contains(edge.to.as_str())
                && !all_sg_members.contains(edge.to.as_str())
                && !successors.contains(&edge.to)
            {
                successors.push(edge.to.clone());
            }
        }

        if predecessors.is_empty() && successors.is_empty() {
            continue;
        }

        // Filter predecessors to earliest-rank only.  This avoids shifting
        // intermediate chain nodes (e.g. Z in C→X→Y→Z→A) which would break
        // the chain's internal edges.
        if predecessors.len() > 1 {
            let min_rank = predecessors
                .iter()
                .filter_map(|id| layout.node_ranks.get(&layered::NodeId(id.clone())).copied())
                .min();

            if let Some(min_rank) = min_rank {
                predecessors.retain(|id| {
                    layout.node_ranks.get(&layered::NodeId(id.clone())).copied() == Some(min_rank)
                });
            }
        }

        // Filter successors to latest-rank only (symmetric with predecessors).
        if successors.len() > 1 {
            let max_rank = successors
                .iter()
                .filter_map(|id| layout.node_ranks.get(&layered::NodeId(id.clone())).copied())
                .max();

            if let Some(max_rank) = max_rank {
                successors.retain(|id| {
                    layout.node_ranks.get(&layered::NodeId(id.clone())).copied() == Some(max_rank)
                });
            }
        }

        // Compute tight member-node bounds on the primary axis.  The layout engine's
        // compound subgraph bounds span all ranks reachable from border nodes,
        // which can be much larger than the actual member nodes.  Use member
        // bounds for the inside_primary check so external nodes at distant
        // ranks are correctly identified as outside.
        let (member_primary_min, member_primary_max) = {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for nid in &sg_node_set {
                if let Some(r) = layout.nodes.get(&layered::NodeId(nid.to_string())) {
                    if horizontal {
                        lo = lo.min(r.y);
                        hi = hi.max(r.y + r.height);
                    } else {
                        lo = lo.min(r.x);
                        hi = hi.max(r.x + r.width);
                    }
                }
            }
            (lo, hi)
        };

        // Shift predecessors and successors toward the subgraph center,
        // but only those outside the subgraph bounds on the primary axis.
        // Nodes at the same rank as internal nodes (e.g. Logs beside
        // Database) would overlap after a cross-axis shift.
        //
        // To avoid collapsing multiple nodes onto the same position, compute
        // a single uniform delta from the group centroid to the subgraph
        // center.  This preserves relative ordering within the group.
        let member_primary_center = (member_primary_min + member_primary_max) / 2.0;
        let pred_set: HashSet<&str> = predecessors.iter().map(|s| s.as_str()).collect();
        let mut eligible: Vec<(String, f64, f64)> = Vec::new(); // (id, cross_center, primary_pos)
        let mut eligible_pred_crosses: Vec<f64> = Vec::new();
        for node_id in predecessors.iter().chain(successors.iter()) {
            if let Some(rect) = layout.nodes.get(&layered::NodeId(node_id.clone())) {
                let node_cy = rect.y + rect.height / 2.0;
                let node_cx = rect.x + rect.width / 2.0;
                let inside_primary = if horizontal {
                    node_cy >= member_primary_min && node_cy <= member_primary_max
                } else {
                    node_cx >= member_primary_min && node_cx <= member_primary_max
                };
                if inside_primary {
                    continue;
                }
                let cross = if horizontal { node_cx } else { node_cy };
                let primary = if horizontal { node_cy } else { node_cx };
                eligible.push((node_id.clone(), cross, primary));
                if pred_set.contains(node_id.as_str()) {
                    eligible_pred_crosses.push(cross);
                }
            }
        }
        if !eligible.is_empty() {
            let centroid = eligible.iter().map(|(_, c, _)| *c).sum::<f64>() / eligible.len() as f64;

            // Determine shift direction: when the external predecessor
            // centroid falls outside the subgraph's cross-axis bounds, the
            // subgraph is mispositioned and should move toward the centroid.
            // When the predecessor centroid is inside the bounds (or there
            // are no predecessors), the subgraph is well-placed and external
            // nodes should shift toward the subgraph center instead.
            // Use predecessors (not successors) for the inside/outside check
            // because predecessor positions drive incoming-edge routing.
            let (sg_cross_min, sg_cross_max) = if horizontal {
                (sg_bounds.x, sg_bounds.x + sg_bounds.width)
            } else {
                (sg_bounds.y, sg_bounds.y + sg_bounds.height)
            };
            let shift_subgraph = is_dir_override && !eligible_pred_crosses.is_empty() && {
                let pred_centroid =
                    eligible_pred_crosses.iter().sum::<f64>() / eligible_pred_crosses.len() as f64;
                pred_centroid < sg_cross_min || pred_centroid > sg_cross_max
            };

            if shift_subgraph {
                // Direction-override subgraph is mispositioned: shift the
                // subgraph toward the external centroid.
                let delta = centroid - sg_center;
                if delta.abs() >= 1.0 {
                    let primary_dist = eligible
                        .iter()
                        .map(|(_, _, p)| (*p - member_primary_center).abs())
                        .fold(f64::INFINITY, f64::min);
                    // Shift all member nodes.
                    for member_id in &sg.nodes {
                        if let Some(&(_, existing_dist)) = node_shifts.get(member_id) {
                            if primary_dist < existing_dist {
                                node_shifts.insert(member_id.clone(), (delta, primary_dist));
                            }
                        } else {
                            node_shifts.insert(member_id.clone(), (delta, primary_dist));
                        }
                    }
                    // Shift this subgraph's bounds.
                    if let Some(bounds) = layout.subgraph_bounds.get_mut(sg_id) {
                        if horizontal {
                            bounds.x += delta;
                        } else {
                            bounds.y += delta;
                        }
                    }
                    // Shift nested child subgraph bounds recursively.
                    let mut children_to_shift: Vec<String> = diagram
                        .subgraph_children(sg_id)
                        .into_iter()
                        .cloned()
                        .collect();
                    let mut idx = 0;
                    while idx < children_to_shift.len() {
                        let child_id = &children_to_shift[idx];
                        if let Some(cb) = layout.subgraph_bounds.get_mut(child_id) {
                            if horizontal {
                                cb.x += delta;
                            } else {
                                cb.y += delta;
                            }
                        }
                        let grandchildren: Vec<String> = diagram
                            .subgraph_children(child_id)
                            .into_iter()
                            .cloned()
                            .collect();
                        children_to_shift.extend(grandchildren);
                        idx += 1;
                    }
                }
            } else {
                // Subgraph-as-node: shift external nodes toward the subgraph
                // center (existing behavior).
                let delta = sg_center - centroid;
                if delta.abs() >= 1.0 {
                    let primary_dist = eligible
                        .iter()
                        .map(|(_, _, p)| (*p - member_primary_center).abs())
                        .fold(f64::INFINITY, f64::min);
                    for (id, _, _) in &eligible {
                        if let Some(&(_, existing_dist)) = node_shifts.get(id) {
                            if primary_dist < existing_dist {
                                node_shifts.insert(id.clone(), (delta, primary_dist));
                            }
                        } else {
                            node_shifts.insert(id.clone(), (delta, primary_dist));
                        }
                    }
                }
            }
        }
    }

    if node_shifts.is_empty() {
        return;
    }

    // Apply node position shifts.
    for (node_id, &(delta, _)) in &node_shifts {
        if let Some(rect) = layout.nodes.get_mut(&layered::NodeId(node_id.clone())) {
            if horizontal {
                rect.x += delta;
            } else {
                rect.y += delta;
            }
        }
    }

    // Adjust edge points: interpolate shift from shifted endpoint to unshifted endpoint.
    for edge in &mut layout.edges {
        let from_delta = node_shifts.get(edge.from.0.as_str()).map(|&(d, _)| d);
        let to_delta = node_shifts.get(edge.to.0.as_str()).map(|&(d, _)| d);

        let n = edge.points.len();
        if n == 0 {
            continue;
        }

        match (from_delta, to_delta) {
            (Some(d1), Some(d2)) => {
                // Both endpoints shifted — interpolate between the two deltas.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        0.5
                    };
                    let shift = d1 * (1.0 - t) + d2 * t;
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (Some(d), None) => {
                // Only source shifted — taper from full shift to zero.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        0.0
                    };
                    let shift = d * (1.0 - t);
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (None, Some(d)) => {
                // Only target shifted — taper from zero to full shift.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        1.0
                    };
                    let shift = d * t;
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (None, None) => {}
        }
    }

    // Adjust self-edge points for shifted nodes.
    for se in &mut layout.self_edges {
        if let Some(&(d, _)) = node_shifts.get(se.node.0.as_str()) {
            for p in &mut se.points {
                if horizontal {
                    p.x += d;
                } else {
                    p.y += d;
                }
            }
        }
    }

    // Adjust label positions for edges with shifted endpoints.
    for (key, pos) in &mut layout.label_positions {
        if let Some(diag_edge) = diagram.edges.get(*key) {
            let from_delta = node_shifts.get(diag_edge.from.as_str()).map(|&(d, _)| d);
            let to_delta = node_shifts.get(diag_edge.to.as_str()).map(|&(d, _)| d);
            let avg_delta = match (from_delta, to_delta) {
                (Some(d1), Some(d2)) => (d1 + d2) / 2.0,
                (Some(d), None) | (None, Some(d)) => d / 2.0,
                (None, None) => continue,
            };
            if horizontal {
                pos.point.x += avg_delta;
            } else {
                pos.point.y += avg_delta;
            }
        }
    }
}

/// Expand parent subgraph bounds to encompass all member nodes and child
/// subgraph bounds.
///
/// After sublayout reconciliation and centering, child subgraphs may have been
/// repositioned (e.g., an LR inner subgraph is wider than the layout predicted).
/// This walks subgraphs inner-first and expands each parent's bounds to be the
/// union of its current bounds and all member content.
///
/// `child_margin` adds space between parent and child subgraph borders.
/// In SVG this should match the subgraph padding so borders don't overlap;
/// in the text pipeline pass 0.0 (draw-coordinate expansion handles padding).
///
/// `title_margin` adds extra top space when the parent has a visible title,
/// so the child border doesn't overlap the parent's title text.
pub(crate) fn expand_parent_bounds(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    child_margin: f64,
    title_margin: f64,
) {
    // Process inner-first so child bounds are finalized before parents.
    let order: Vec<&String> = if !diagram.subgraph_order.is_empty() {
        diagram.subgraph_order.iter().collect()
    } else {
        let mut keys: Vec<&String> = diagram.subgraphs.keys().collect();
        keys.sort();
        keys
    };

    for sg_id in &order {
        let sg = match diagram.subgraphs.get(*sg_id) {
            Some(sg) => sg,
            None => continue,
        };
        let Some(current) = layout.subgraph_bounds.get(*sg_id).copied() else {
            continue;
        };
        let recompute_tight = child_margin > 0.0 || title_margin > 0.0;
        let mut min_x = if recompute_tight {
            f64::INFINITY
        } else {
            current.x
        };
        let mut min_y = if recompute_tight {
            f64::INFINITY
        } else {
            current.y
        };
        let mut max_x = if recompute_tight {
            f64::NEG_INFINITY
        } else {
            current.x + current.width
        };
        let mut max_y = if recompute_tight {
            f64::NEG_INFINITY
        } else {
            current.y + current.height
        };
        let mut found_content = !recompute_tight;

        // Check member nodes.
        for member_id in &sg.nodes {
            if let Some(rect) = layout.nodes.get(&layered::NodeId(member_id.clone())) {
                found_content = true;
                min_x = min_x.min(rect.x);
                min_y = min_y.min(rect.y);
                max_x = max_x.max(rect.x + rect.width);
                max_y = max_y.max(rect.y + rect.height);
            }
        }

        // Check child subgraph bounds (subgraphs whose parent is this subgraph).
        // Add child_margin so the parent border sits outside the child border.
        // In expansion-only mode (text path), preserve the previous title-margin behavior.
        let has_title = !sg.title.trim().is_empty();
        let top_margin = if recompute_tight {
            child_margin
        } else {
            child_margin + if has_title { title_margin } else { 0.0 }
        };
        for (child_sg_id, child_sg) in &diagram.subgraphs {
            if child_sg.parent.as_deref() == Some(sg_id.as_str())
                && let Some(child_bounds) = layout.subgraph_bounds.get(child_sg_id)
            {
                found_content = true;
                min_x = min_x.min(child_bounds.x - child_margin);
                min_y = min_y.min(child_bounds.y - top_margin);
                max_x = max_x.max(child_bounds.x + child_bounds.width + child_margin);
                max_y = max_y.max(child_bounds.y + child_bounds.height + child_margin);
            }
        }

        // Empty subgraph: preserve current bounds if we have nothing to recompute from.
        if !found_content {
            continue;
        }

        if recompute_tight && has_title {
            min_y -= title_margin;
        }

        if let Some(bounds) = layout.subgraph_bounds.get_mut(*sg_id) {
            bounds.x = min_x;
            bounds.y = min_y;
            bounds.width = max_x - min_x;
            bounds.height = max_y - min_y;
        }
        if let Some(node_rect) = layout.nodes.get_mut(&layered::NodeId((*sg_id).clone())) {
            node_rect.x = min_x;
            node_rect.y = min_y;
            node_rect.width = max_x - min_x;
            node_rect.height = max_y - min_y;
        }
    }
}

/// Push external nodes that overlap with reconciled subgraph bounds downward.
///
/// After sublayout reconciliation, the subgraph may now occupy space where the layout
/// placed external nodes.  This shifts those nodes (and everything below them)
/// down to maintain a minimum gap.
pub(crate) fn resolve_sublayout_overlaps(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    min_gap: f64,
) {
    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let Some(sg_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        let sg_bottom = sg_bounds.y + sg_bounds.height;
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        // Treat nested subgraph compound nodes as internal to this subgraph.
        // `sg.nodes` contains leaf members, so without this descendants are
        // misclassified as external blockers during overlap resolution.
        let mut internal_subgraph_ids: HashSet<&str> = HashSet::new();
        let mut stack = vec![sg_id.as_str()];
        while let Some(parent_id) = stack.pop() {
            for (child_id, child) in &diagram.subgraphs {
                if child.parent.as_deref() == Some(parent_id)
                    && internal_subgraph_ids.insert(child_id.as_str())
                {
                    stack.push(child_id.as_str());
                }
            }
        }

        // Find the maximum shift needed to clear overlapping external nodes.
        let mut max_shift = 0.0f64;
        for (nid, rect) in &layout.nodes {
            if sg_node_set.contains(nid.0.as_str())
                || nid.0 == *sg_id
                || internal_subgraph_ids.contains(nid.0.as_str())
            {
                continue;
            }
            // Only consider nodes whose center is below the subgraph center
            // (i.e., they should be below the subgraph, not above it).
            let node_cy = rect.y + rect.height / 2.0;
            let sg_cy = sg_bounds.y + sg_bounds.height / 2.0;
            if node_cy > sg_cy && rect.y < sg_bottom + min_gap {
                let needed = sg_bottom + min_gap - rect.y;
                if needed > max_shift {
                    max_shift = needed;
                }
            }
        }

        if max_shift < 0.01 {
            continue;
        }

        // Shift all nodes below the subgraph center.
        let sg_cy = sg_bounds.y + sg_bounds.height / 2.0;
        for (nid, rect) in layout.nodes.iter_mut() {
            if sg_node_set.contains(nid.0.as_str())
                || nid.0 == *sg_id
                || internal_subgraph_ids.contains(nid.0.as_str())
            {
                continue;
            }
            if rect.y + rect.height / 2.0 > sg_cy {
                rect.y += max_shift;
            }
        }

        // Shift edge points that are below the subgraph center.
        for edge in &mut layout.edges {
            for point in &mut edge.points {
                if point.y > sg_cy {
                    point.y += max_shift;
                }
            }
        }

        // Shift self-edge points.
        for se in &mut layout.self_edges {
            for point in &mut se.points {
                if point.y > sg_cy {
                    point.y += max_shift;
                }
            }
        }

        // Shift label positions.
        for pos in layout.label_positions.values_mut() {
            if pos.point.y > sg_cy {
                pos.point.y += max_shift;
            }
        }

        // Shift sibling subgraph bounds that are below the pushing subgraph.
        let sibling_ids: Vec<String> = layout
            .subgraph_bounds
            .keys()
            .filter(|id| *id != sg_id)
            .cloned()
            .collect();
        for sibling_id in sibling_ids {
            if internal_subgraph_ids.contains(sibling_id.as_str()) {
                continue;
            }
            if let Some(bounds) = layout.subgraph_bounds.get_mut(&sibling_id)
                && bounds.y + bounds.height / 2.0 > sg_cy
            {
                bounds.y += max_shift;
            }
        }

        // Update layout height.
        layout.height += max_shift;
    }
}
