//! Subgraph-bounds derivation and containment repair for grid-space layout.
//!
//! These helpers convert float-space subgraph rectangles into grid-space
//! bounds, then adjust those bounds to preserve titles, containment, and
//! spacing around nested subgraphs.

use std::collections::{HashMap, HashSet};

use super::super::layout::CoordTransform;
use crate::graph::grid::{NodeBounds, SubgraphBounds};
use crate::graph::space::FRect;
use crate::graph::{Direction, Edge, Graph, Subgraph};

#[cfg(test)]
pub(super) fn build_children_map(
    subgraphs: &HashMap<String, Subgraph>,
) -> HashMap<String, Vec<String>> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for sg in subgraphs.values() {
        if let Some(ref parent_id) = sg.parent {
            children
                .entry(parent_id.clone())
                .or_default()
                .push(sg.id.clone());
        }
    }
    children
}

/// Convert subgraph member-node positions to draw-coordinate SubgraphBounds.
///
/// Uses inside-out (bottom-up) computation: leaf subgraphs first, then parents
/// expand to contain their children. This ensures proper nesting of bounds.
pub(super) fn subgraph_bounds_to_draw(
    subgraphs: &HashMap<String, Subgraph>,
    layout_bounds: &HashMap<String, FRect>,
    transform: &CoordTransform,
) -> HashMap<String, SubgraphBounds> {
    let mut bounds: HashMap<String, SubgraphBounds> = HashMap::new();

    for (sg_id, rect) in layout_bounds {
        let sg = match subgraphs.get(sg_id) {
            Some(sg) => sg,
            None => continue,
        };

        let (x0, y0) = transform.to_draw(rect.x, rect.y);
        let (x1, y1) = transform.to_draw(rect.x + rect.width, rect.y + rect.height);

        let mut final_x = x0;
        let mut final_width = x1.saturating_sub(x0);
        let final_height = y1.saturating_sub(y0);

        // Enforce title-width minimum: ┌─ Title ─┐
        // Overhead: 2 corners + "─ " prefix (2) + " ─" suffix (2) = 6
        let has_visible_title = !sg.title.trim().is_empty();
        let min_title_width = if has_visible_title {
            sg.title.len() + 6
        } else {
            0
        };
        if min_title_width > 0 && final_width < min_title_width {
            let expand = min_title_width - final_width;
            final_x = final_x.saturating_sub(expand / 2);
            final_width = min_title_width;
        }

        // Compute nesting depth by walking parent chain
        let mut depth = 0;
        let mut cur = sg_id.as_str();
        while let Some(s) = subgraphs.get(cur) {
            if let Some(ref p) = s.parent {
                depth += 1;
                cur = p;
            } else {
                break;
            }
        }

        bounds.insert(
            sg_id.clone(),
            SubgraphBounds {
                x: final_x,
                y: y0,
                width: final_width,
                height: final_height,
                title: sg.title.clone(),
                depth,
            },
        );
    }

    expand_parent_subgraph_bounds(subgraphs, &mut bounds);

    bounds
}

pub(super) fn shrink_subgraph_vertical_gaps(
    subgraphs: &HashMap<String, Subgraph>,
    edges: &[Edge],
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    direction: Direction,
) {
    let parent_map = build_subgraph_parent_map(subgraphs);
    let incoming_map = build_subgraph_incoming_map(subgraphs, edges, &parent_map);
    let outgoing_map = build_subgraph_outgoing_map(subgraphs, edges, &parent_map);

    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();

    for sg_id in ids {
        let Some(bounds) = subgraph_bounds.get(&sg_id).cloned() else {
            continue;
        };
        let Some(sg) = subgraphs.get(&sg_id) else {
            continue;
        };

        let mut min_y: Option<usize> = None;
        let mut max_y: Option<usize> = None;
        for member in &sg.nodes {
            if let Some(node) = node_bounds.get(member) {
                let node_bottom = node.y.saturating_add(node.height.saturating_sub(1));
                min_y = Some(min_y.map_or(node.y, |cur| cur.min(node.y)));
                max_y = Some(max_y.map_or(node_bottom, |cur| cur.max(node_bottom)));
                continue;
            }
            if let Some(child_bounds) = subgraph_bounds.get(member) {
                let child_bottom = child_bounds
                    .y
                    .saturating_add(child_bounds.height.saturating_sub(1));
                min_y = Some(min_y.map_or(child_bounds.y, |cur| cur.min(child_bounds.y)));
                max_y = Some(max_y.map_or(child_bottom, |cur| cur.max(child_bottom)));
            }
        }

        let (Some(min_y), Some(max_y)) = (min_y, max_y) else {
            continue;
        };

        let content_top = bounds.y.saturating_add(1);
        let content_bottom = bounds.y.saturating_add(bounds.height.saturating_sub(2));
        let top_gap = min_y.saturating_sub(content_top);
        let bottom_gap = content_bottom.saturating_sub(max_y);

        let has_incoming = incoming_map.get(&sg_id).copied().unwrap_or(false);
        let has_outgoing = outgoing_map.get(&sg_id).copied().unwrap_or(false);

        // Each side needs 1 row of gap only if cross-boundary edges route
        // through it; blank rows without routing should be eliminated.
        let (min_top_gap, min_bottom_gap) = match direction {
            Direction::TopDown => (
                if has_incoming { 1 } else { 0 },
                if has_outgoing { 1 } else { 0 },
            ),
            Direction::BottomTop => (
                if has_outgoing { 1 } else { 0 },
                if has_incoming { 1 } else { 0 },
            ),
            _ => (0, 0),
        };

        // Only shrink; never expand beyond the current gap.
        let desired_top = min_top_gap.min(top_gap);
        let desired_bottom = min_bottom_gap.min(bottom_gap);
        let shrink_top = top_gap.saturating_sub(desired_top);
        let shrink_bottom = bottom_gap.saturating_sub(desired_bottom);
        let expand_top = desired_top.saturating_sub(top_gap);
        let expand_bottom = desired_bottom.saturating_sub(bottom_gap);

        if shrink_top == 0 && shrink_bottom == 0 && expand_top == 0 && expand_bottom == 0 {
            continue;
        }

        let new_y = bounds
            .y
            .saturating_sub(expand_top)
            .saturating_add(shrink_top);
        let new_height = bounds
            .height
            .saturating_add(expand_top.saturating_add(expand_bottom))
            .saturating_sub(shrink_top.saturating_add(shrink_bottom));

        if new_height < 2 {
            continue;
        }

        if let Some(entry) = subgraph_bounds.get_mut(&sg_id) {
            entry.y = new_y;
            entry.height = new_height;
        }
    }

    expand_parent_subgraph_bounds(subgraphs, subgraph_bounds);
}

/// Ensure at least 1 row/column of space between a direction-override
/// subgraph border and external predecessor/successor nodes.
///
/// After sublayout reconciliation, the subgraph bounds are recomputed from
/// the sublayout dimensions.  This can leave the border flush against nodes
/// above (TD) or below (BT), making edge entry visually cluttered.
///
/// For each direction-override subgraph with external edges, this pushes the
/// border inward so there is a 1-cell gap on the entry side.
pub(super) fn ensure_external_edge_spacing(
    diagram: &Graph,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let Some(sb) = subgraph_bounds.get(sg_id).cloned() else {
            continue;
        };
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();

        // Classify each external predecessor/successor by its position
        // relative to the subgraph, not by the diagram's main direction.
        // This avoids false positives for nested subgraphs whose parent
        // has a different direction (e.g. inner BT inside outer LR).
        let mut max_pred_bottom: Option<usize> = None; // preds above
        let mut min_succ_top: Option<usize> = None; // succs below

        for edge in &diagram.edges {
            if sg_node_set.contains(edge.to.as_str())
                && !sg_node_set.contains(edge.from.as_str())
                && let Some(nb) = node_bounds.get(&edge.from)
            {
                let nb_cy = nb.y + nb.height / 2;
                // Only count predecessors whose center is above the border.
                if nb_cy < sb.y {
                    let bottom = nb.y + nb.height.saturating_sub(1);
                    max_pred_bottom = Some(max_pred_bottom.map_or(bottom, |c| c.max(bottom)));
                }
            }
            if sg_node_set.contains(edge.from.as_str())
                && !sg_node_set.contains(edge.to.as_str())
                && let Some(nb) = node_bounds.get(&edge.to)
            {
                let nb_cy = nb.y + nb.height / 2;
                let sg_bottom = sb.y + sb.height.saturating_sub(1);
                if nb_cy > sg_bottom {
                    min_succ_top = Some(min_succ_top.map_or(nb.y, |c| c.min(nb.y)));
                }
            }
        }

        // Top side: shift entire subgraph down if too close to predecessor bottom.
        // Use +4 to leave room for horizontal edge routing + 1 clear row
        // above the subgraph border.
        if let Some(pred_bottom) = max_pred_bottom {
            let min_y = pred_bottom + 4;
            let current_y = subgraph_bounds[sg_id].y;
            if current_y < min_y {
                let adjust = min_y - current_y;
                // Shift the subgraph bounds down (keep height, move y).
                subgraph_bounds.get_mut(sg_id).unwrap().y = min_y;
                // Shift all member nodes down by the same amount.
                for member_id in &sg.nodes {
                    if let Some(nb) = node_bounds.get_mut(member_id) {
                        nb.y += adjust;
                    }
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.1 += adjust;
                    }
                }
                // Shift nested child subgraph bounds down too.
                let children: Vec<String> = diagram
                    .subgraphs
                    .iter()
                    .filter(|(cid, _)| *cid != sg_id && sg_node_set.contains(cid.as_str()))
                    .map(|(cid, _)| cid.clone())
                    .collect();
                for child_id in &children {
                    if let Some(cb) = subgraph_bounds.get_mut(child_id) {
                        cb.y += adjust;
                    }
                }
            }
        }
        // Bottom side: shift entire subgraph up if border too close to successor top.
        if let Some(succ_top) = min_succ_top {
            let max_bottom = succ_top.saturating_sub(4);
            let sb = &subgraph_bounds[sg_id];
            let current_bottom = sb.y + sb.height.saturating_sub(1);
            if current_bottom > max_bottom {
                let adjust = current_bottom - max_bottom;
                subgraph_bounds.get_mut(sg_id).unwrap().y =
                    subgraph_bounds[sg_id].y.saturating_sub(adjust);
                for member_id in &sg.nodes {
                    if let Some(nb) = node_bounds.get_mut(member_id) {
                        nb.y = nb.y.saturating_sub(adjust);
                    }
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.1 = pos.1.saturating_sub(adjust);
                    }
                }
                let children: Vec<String> = diagram
                    .subgraphs
                    .iter()
                    .filter(|(cid, _)| *cid != sg_id && sg_node_set.contains(cid.as_str()))
                    .map(|(cid, _)| cid.clone())
                    .collect();
                for child_id in &children {
                    if let Some(cb) = subgraph_bounds.get_mut(child_id) {
                        cb.y = cb.y.saturating_sub(adjust);
                    }
                }
            }
        }
    }
}

pub(super) fn shrink_subgraph_horizontal_gaps(
    subgraphs: &HashMap<String, Subgraph>,
    edges: &[Edge],
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    direction: Direction,
) {
    let parent_map = build_subgraph_parent_map(subgraphs);
    let incoming_map = build_subgraph_incoming_map(subgraphs, edges, &parent_map);

    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();

    for sg_id in ids {
        let Some(bounds) = subgraph_bounds.get(&sg_id).cloned() else {
            continue;
        };
        let Some(sg) = subgraphs.get(&sg_id) else {
            continue;
        };

        let mut min_x: Option<usize> = None;
        let mut max_x: Option<usize> = None;
        for member in &sg.nodes {
            if let Some(node) = node_bounds.get(member) {
                let node_right = node.x.saturating_add(node.width.saturating_sub(1));
                min_x = Some(min_x.map_or(node.x, |cur| cur.min(node.x)));
                max_x = Some(max_x.map_or(node_right, |cur| cur.max(node_right)));
                continue;
            }
            if let Some(child_bounds) = subgraph_bounds.get(member) {
                let child_right = child_bounds
                    .x
                    .saturating_add(child_bounds.width.saturating_sub(1));
                min_x = Some(min_x.map_or(child_bounds.x, |cur| cur.min(child_bounds.x)));
                max_x = Some(max_x.map_or(child_right, |cur| cur.max(child_right)));
            }
        }

        let (Some(min_x), Some(max_x)) = (min_x, max_x) else {
            continue;
        };

        let content_left = bounds.x.saturating_add(1);
        let content_right = bounds.x.saturating_add(bounds.width.saturating_sub(2));
        let left_gap = min_x.saturating_sub(content_left);
        let right_gap = content_right.saturating_sub(max_x);

        let has_incoming = incoming_map.get(&sg_id).copied().unwrap_or(false);
        let incoming_gap = if has_incoming { 1 } else { 0 };

        let (min_left_gap, min_right_gap) = match direction {
            Direction::LeftRight => (incoming_gap, 0),
            Direction::RightLeft => (0, incoming_gap),
            _ => (0, 0),
        };

        let base_target = left_gap.min(right_gap);
        let desired_left = base_target.max(min_left_gap);
        let desired_right = base_target.max(min_right_gap);
        let mut shrink_left = left_gap.saturating_sub(desired_left);
        let mut shrink_right = right_gap.saturating_sub(desired_right);
        let expand_left = desired_left.saturating_sub(left_gap);
        let expand_right = desired_right.saturating_sub(right_gap);

        let mut new_width = bounds
            .width
            .saturating_add(expand_left.saturating_add(expand_right))
            .saturating_sub(shrink_left.saturating_add(shrink_right));

        if new_width < 2 {
            continue;
        }

        let inner_width = bounds.width.saturating_sub(2);
        let visible_title_len = if !bounds.title.trim().is_empty() && inner_width >= 5 {
            let max_title_len = inner_width.saturating_sub(4);
            bounds.title.len().min(max_title_len)
        } else {
            0
        };
        let title_width = if visible_title_len > 0 {
            visible_title_len.saturating_add(6)
        } else {
            2
        };
        let max_width_without_shrink = bounds
            .width
            .saturating_add(expand_left.saturating_add(expand_right));
        let min_width = title_width.min(max_width_without_shrink);

        if new_width < min_width {
            let deficit = min_width.saturating_sub(new_width);
            let reduce_left = deficit.min(shrink_left);
            shrink_left = shrink_left.saturating_sub(reduce_left);
            let reduce_right = deficit.saturating_sub(reduce_left);
            shrink_right = shrink_right.saturating_sub(reduce_right);
            new_width = bounds
                .width
                .saturating_add(expand_left.saturating_add(expand_right))
                .saturating_sub(shrink_left.saturating_add(shrink_right));
        }

        if new_width < 2 {
            continue;
        }

        let new_x = bounds
            .x
            .saturating_sub(expand_left)
            .saturating_add(shrink_left);

        if let Some(entry) = subgraph_bounds.get_mut(&sg_id) {
            entry.x = new_x;
            entry.width = new_width;
        }
    }

    expand_parent_subgraph_bounds(subgraphs, subgraph_bounds);
}

pub(super) fn build_subgraph_parent_map(
    subgraphs: &HashMap<String, Subgraph>,
) -> HashMap<String, String> {
    let mut parent_map = HashMap::new();

    let mut ids: Vec<&String> = subgraphs.keys().collect();
    ids.sort_by(|a, b| {
        let depth_a = subgraph_depth(subgraphs, a.as_str());
        let depth_b = subgraph_depth(subgraphs, b.as_str());
        depth_b.cmp(&depth_a).then_with(|| a.cmp(b))
    });

    for sg_id in ids {
        if let Some(sg) = subgraphs.get(sg_id) {
            for node_id in &sg.nodes {
                parent_map
                    .entry(node_id.clone())
                    .or_insert_with(|| sg.id.clone());
            }
        }
    }

    parent_map
}

fn subgraph_depth(subgraphs: &HashMap<String, Subgraph>, sg_id: &str) -> usize {
    let mut depth = 0usize;
    let mut cur = sg_id;
    while let Some(sg) = subgraphs.get(cur) {
        if let Some(ref parent) = sg.parent {
            depth += 1;
            cur = parent;
        } else {
            break;
        }
    }
    depth
}

pub(super) fn build_subgraph_incoming_map(
    subgraphs: &HashMap<String, Subgraph>,
    edges: &[Edge],
    parent_map: &HashMap<String, String>,
) -> HashMap<String, bool> {
    let mut incoming: HashMap<String, bool> = HashMap::new();
    for edge in edges {
        let dst_ancestors = collect_subgraph_ancestors(&edge.to, subgraphs, parent_map);
        if dst_ancestors.is_empty() {
            continue;
        }
        for sg_id in dst_ancestors {
            if !is_node_in_subgraph(&edge.from, &sg_id, subgraphs, parent_map) {
                incoming.insert(sg_id, true);
            }
        }
    }
    incoming
}

pub(super) fn build_subgraph_outgoing_map(
    subgraphs: &HashMap<String, Subgraph>,
    edges: &[Edge],
    parent_map: &HashMap<String, String>,
) -> HashMap<String, bool> {
    let mut outgoing: HashMap<String, bool> = HashMap::new();
    for edge in edges {
        let src_ancestors = collect_subgraph_ancestors(&edge.from, subgraphs, parent_map);
        if src_ancestors.is_empty() {
            continue;
        }
        for sg_id in src_ancestors {
            if !is_node_in_subgraph(&edge.to, &sg_id, subgraphs, parent_map) {
                outgoing.insert(sg_id, true);
            }
        }
    }
    outgoing
}

fn collect_subgraph_ancestors(
    node_id: &str,
    subgraphs: &HashMap<String, Subgraph>,
    parent_map: &HashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = parent_map.get(node_id).cloned();
    while let Some(parent_id) = cur {
        out.push(parent_id.clone());
        cur = subgraphs
            .get(&parent_id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
    }
    out
}

fn is_node_in_subgraph(
    node_id: &str,
    sg_id: &str,
    subgraphs: &HashMap<String, Subgraph>,
    parent_map: &HashMap<String, String>,
) -> bool {
    let mut cur = parent_map.get(node_id).cloned();
    while let Some(parent_id) = cur {
        if parent_id == sg_id {
            return true;
        }
        cur = subgraphs
            .get(&parent_id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
    }
    false
}

pub(super) fn expand_parent_subgraph_bounds(
    subgraphs: &HashMap<String, Subgraph>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    // Expand parent bounds to contain child bounds (inside-out).
    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();
    for id in ids {
        let parent_id = subgraphs
            .get(&id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
        let (Some(parent_id), Some(child_bounds)) = (parent_id, subgraph_bounds.get(&id).cloned())
        else {
            continue;
        };
        let Some(parent_bounds) = subgraph_bounds.get_mut(&parent_id) else {
            continue;
        };

        let pad = 1usize;
        let child_left = child_bounds.x.saturating_sub(pad);
        let child_top = child_bounds.y.saturating_sub(pad);
        let child_right = child_bounds.x + child_bounds.width + pad;
        let child_bottom = child_bounds.y + child_bounds.height + pad;
        let parent_right = parent_bounds.x + parent_bounds.width;
        let parent_bottom = parent_bounds.y + parent_bounds.height;

        let new_left = parent_bounds.x.min(child_left);
        let new_top = parent_bounds.y.min(child_top);
        let new_right = parent_right.max(child_right);
        let new_bottom = parent_bottom.max(child_bottom);

        parent_bounds.x = new_left;
        parent_bounds.y = new_top;
        parent_bounds.width = new_right.saturating_sub(new_left);
        parent_bounds.height = new_bottom.saturating_sub(new_top);
    }
}

/// Ensure each subgraph's draw-coordinate bounds contain all member nodes.
///
/// After coordinate transformation (float→integer) and shrink passes, rounding
/// can cause subgraph bounds to be 1-2 characters too small. This post-pass
/// expands any deficient bounds to guarantee containment.
pub(super) fn ensure_subgraph_contains_members(
    diagram: &Graph,
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    for (sg_id, sg) in &diagram.subgraphs {
        let Some(sb) = subgraph_bounds.get_mut(sg_id) else {
            continue;
        };
        let mut sg_right = sb.x + sb.width;
        let mut sg_bottom = sb.y + sb.height;

        for member_id in &sg.nodes {
            let Some(nb) = node_bounds.get(member_id.as_str()) else {
                continue;
            };
            let nb_right = nb.x + nb.width;
            let nb_bottom = nb.y + nb.height;

            if nb.x < sb.x {
                let expand = sb.x - nb.x;
                sb.x = nb.x;
                sb.width += expand;
                sg_right = sb.x + sb.width;
            }
            if nb.y < sb.y {
                let expand = sb.y - nb.y;
                sb.y = nb.y;
                sb.height += expand;
                sg_bottom = sb.y + sb.height;
            }
            if nb_right > sg_right {
                sb.width += nb_right - sg_right;
                sg_right = sb.x + sb.width;
            }
            if nb_bottom > sg_bottom {
                sb.height += nb_bottom - sg_bottom;
                sg_bottom = sb.y + sb.height;
            }
        }
    }
}
