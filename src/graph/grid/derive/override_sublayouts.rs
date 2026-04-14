//! Direction-override reconciliation for derived grid layouts.
//!
//! These helpers adapt override sublayouts into the parent grid layout and
//! repair the resulting node and subgraph positions.

use std::collections::{HashMap, HashSet};

use super::super::layout::{NodeBounds, SubgraphBounds};
use super::super::{GridLayoutConfig, OverrideSubgraphProjection};
use super::quantize::compute_grid_scale_factors;
use super::subgraph_bounds::{
    build_subgraph_incoming_map, build_subgraph_outgoing_map, build_subgraph_parent_map,
};
use crate::graph::measure::grid_node_dimensions;
use crate::graph::{Direction, Graph};

#[allow(clippy::too_many_arguments)]
pub(super) fn reconcile_sublayouts_draw(
    diagram: &Graph,
    config: &GridLayoutConfig,
    sublayouts: &HashMap<String, OverrideSubgraphProjection>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    canvas_width: &mut usize,
    canvas_height: &mut usize,
) {
    // Process sublayouts in deterministic depth order (shallowest first).
    // This ensures that when both parent and child subgraphs have direction
    // overrides, the deeper override writes last and wins.
    let mut sorted_sg_ids: Vec<&String> = sublayouts.keys().collect();
    sorted_sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });

    // Build cross-boundary edge maps for directional padding.
    let parent_map = build_subgraph_parent_map(&diagram.subgraphs);
    let incoming_map = build_subgraph_incoming_map(&diagram.subgraphs, &diagram.edges, &parent_map);
    let outgoing_map = build_subgraph_outgoing_map(&diagram.subgraphs, &diagram.edges, &parent_map);

    for sg_id in sorted_sg_ids {
        let sublayout = &sublayouts[sg_id];
        let sg = &diagram.subgraphs[sg_id];

        // Get the current subgraph draw bounds as the anchor position
        let sg_draw = match subgraph_bounds.get(sg_id) {
            Some(b) => b.clone(),
            None => continue,
        };

        // Compute draw coordinates for sub-layout nodes.
        // Each node's position in the sub-layout is in layout float coords.
        // We convert them to character positions using a simple approach:
        // node draw (x, y) = layout position scaled to fit draw space.
        //
        // For the sub-layout, we use the node dimensions directly and add spacing.
        let sub_dir = sg.dir.unwrap_or(diagram.direction);
        let sub_is_vertical = matches!(sub_dir, Direction::TopDown | Direction::BottomTop);

        // Collect sub-layout node draw positions relative to (0,0)
        let mut sub_draw_nodes: Vec<(String, usize, usize, usize, usize)> = Vec::new();

        // Compute sub-layout-specific scale factors
        let sub_node_dims: HashMap<String, (usize, usize)> = sublayout
            .nodes
            .iter()
            .filter_map(|(id, _)| {
                diagram
                    .nodes
                    .get(id)
                    .map(|n| (id.clone(), grid_node_dimensions(n, sub_dir)))
            })
            .collect();

        let sub_rank_sep = config.rank_sep + config.cluster_rank_sep;
        let (sub_scale_x, sub_scale_y) = compute_grid_scale_factors(
            &sub_node_dims,
            sub_rank_sep,
            config.node_sep,
            config.v_spacing,
            config.h_spacing,
            sub_is_vertical,
            false,
        );

        // Find sub-layout bounding box min
        let sub_layout_min_x = sublayout
            .nodes
            .values()
            .map(|r| r.x)
            .fold(f64::INFINITY, f64::min);
        let sub_layout_min_y = sublayout
            .nodes
            .values()
            .map(|r| r.y)
            .fold(f64::INFINITY, f64::min);

        // Convert each sub-layout node to draw coordinates (relative)
        for (node_id, rect) in &sublayout.nodes {
            let (w, h) = match sub_node_dims.get(node_id) {
                Some(&dims) => dims,
                None => continue,
            };

            let cx =
                ((rect.x + rect.width / 2.0 - sub_layout_min_x) * sub_scale_x).round() as usize;
            let cy =
                ((rect.y + rect.height / 2.0 - sub_layout_min_y) * sub_scale_y).round() as usize;
            let x = cx.saturating_sub(w / 2);
            let y = cy.saturating_sub(h / 2);

            sub_draw_nodes.push((node_id.clone(), x, y, w, h));
        }

        // Repair overlapping/touching nodes along the primary axis.
        // After scaling and rounding, the leftmost (or topmost) node can clip
        // to position 0 via saturating_sub, collapsing the gap to its neighbor.
        // Push only the affected nodes apart by the minimum amount needed for
        // edge rendering (2 chars: stem + arrowhead).
        let min_gap = 2;
        if !sub_is_vertical {
            sub_draw_nodes.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))); // sort by x, then node_id
            for i in 1..sub_draw_nodes.len() {
                let prev_right = sub_draw_nodes[i - 1].1 + sub_draw_nodes[i - 1].3;
                let needed = prev_right + min_gap;
                if sub_draw_nodes[i].1 < needed {
                    sub_draw_nodes[i].1 = needed;
                }
            }
        } else {
            sub_draw_nodes.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0))); // sort by y, then node_id
            for i in 1..sub_draw_nodes.len() {
                let prev_bottom = sub_draw_nodes[i - 1].2 + sub_draw_nodes[i - 1].4;
                let needed = prev_bottom + min_gap;
                if sub_draw_nodes[i].2 < needed {
                    sub_draw_nodes[i].2 = needed;
                }
            }
        }

        if sub_draw_nodes.is_empty() {
            continue;
        }

        // Find the bounding box of the sub-layout in draw coordinates
        let sub_draw_min_x = sub_draw_nodes
            .iter()
            .map(|(_, x, _, _, _)| *x)
            .min()
            .unwrap_or(0);
        let sub_draw_min_y = sub_draw_nodes
            .iter()
            .map(|(_, _, y, _, _)| *y)
            .min()
            .unwrap_or(0);
        let sub_draw_max_x = sub_draw_nodes
            .iter()
            .map(|(_, x, _, w, _)| x + w)
            .max()
            .unwrap_or(0);
        let sub_draw_max_y = sub_draw_nodes
            .iter()
            .map(|(_, _, y, _, h)| y + h)
            .max()
            .unwrap_or(0);

        let sub_draw_w = sub_draw_max_x - sub_draw_min_x;
        let sub_draw_h = sub_draw_max_y - sub_draw_min_y;

        // Padding around sub-layout content within the subgraph border.
        // Each side gets 1 char for the border itself.  An extra spacing row
        // is added only on sides where cross-boundary edges route through,
        // so blank rows are eliminated on sides with no routing.
        let has_incoming = incoming_map.get(sg_id).copied().unwrap_or(false);
        let has_outgoing = outgoing_map.get(sg_id).copied().unwrap_or(false);
        let (top_pad, bottom_pad) = match diagram.direction {
            Direction::TopDown => (
                if has_incoming { 2 } else { 1 },
                if has_outgoing { 2 } else { 1 },
            ),
            Direction::BottomTop => (
                if has_outgoing { 2 } else { 1 },
                if has_incoming { 2 } else { 1 },
            ),
            _ => (2, 2),
        };
        let left_pad = 2;
        let right_pad = 2;

        // Compute the total subgraph bounds needed
        let sg_needed_w = sub_draw_w + left_pad + right_pad;
        let sg_needed_h = sub_draw_h + top_pad + bottom_pad;

        // Enforce title-width minimum
        let min_title_width = if !sg.title.trim().is_empty() {
            sg.title.len() + 6
        } else {
            0
        };
        let sg_final_w = sg_needed_w.max(min_title_width);

        // Use the current subgraph center as the anchor point
        let sg_cx = sg_draw.x + sg_draw.width / 2;
        let sg_cy = sg_draw.y + sg_draw.height / 2;

        // Compute new subgraph bounds centered on the old center
        let new_sg_x = sg_cx.saturating_sub(sg_final_w / 2);
        let new_sg_y = sg_cy.saturating_sub(sg_needed_h / 2);

        // Offset to place sub-layout content within the new subgraph bounds
        let content_x = new_sg_x + left_pad + (sg_final_w - sg_needed_w) / 2;
        let content_y = new_sg_y + top_pad;

        let offset_x = content_x.saturating_sub(sub_draw_min_x);
        let offset_y = content_y.saturating_sub(sub_draw_min_y);

        // Override node positions
        for (node_id, rel_x, rel_y, w, h) in &sub_draw_nodes {
            let final_x = rel_x + offset_x;
            let final_y = rel_y + offset_y;

            draw_positions.insert(node_id.clone(), (final_x, final_y));
            node_bounds.insert(
                node_id.clone(),
                NodeBounds {
                    x: final_x,
                    y: final_y,
                    width: *w,
                    height: *h,
                    layout_center_x: Some(final_x + w / 2),
                    layout_center_y: Some(final_y + h / 2),
                },
            );
        }

        // Update subgraph bounds
        let depth = diagram.subgraph_depth(sg_id);
        subgraph_bounds.insert(
            sg_id.clone(),
            SubgraphBounds {
                x: new_sg_x,
                y: new_sg_y,
                width: sg_final_w,
                height: sg_needed_h,
                title: sg.title.clone(),
                depth,
                invisible: sg.invisible,
                concurrent_regions: Vec::new(),
            },
        );

        // Expand canvas if needed
        *canvas_width = (*canvas_width).max(new_sg_x + sg_final_w + config.padding);
        *canvas_height = (*canvas_height).max(new_sg_y + sg_needed_h + config.padding);
    }
}

/// After draw-coordinate reconciliation, sibling nodes and child subgraphs
/// within a direction-override parent may overlap.  This happens because the
/// parent's sublayout positions its member nodes individually without knowing
/// the final dimensions of child subgraphs (which are reconciled separately).
///
/// For each direction-override parent, detect nodes that overlap with sibling
/// child subgraph bounds and shift the subgraph (and all its contents) away
/// to create separation.
pub(super) fn resolve_sibling_overlaps_draw(
    diagram: &Graph,
    node_bounds: &mut HashMap<String, NodeBounds>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    // Build set of nodes that belong to each child subgraph, so we can
    // identify "direct" children of the parent (nodes not in any grandchild).
    let child_sg_nodes: HashMap<&str, HashSet<&str>> = diagram
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.as_str(), sg.nodes.iter().map(|s| s.as_str()).collect()))
        .collect();

    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let sub_dir = sg.dir.unwrap();

        // Find child subgraphs (subgraphs whose parent is this one).
        let child_sgs: Vec<&str> = diagram
            .subgraphs
            .iter()
            .filter(|(_, child)| child.parent.as_deref() == Some(sg_id.as_str()))
            .map(|(id, _)| id.as_str())
            .collect();

        if child_sgs.is_empty() {
            continue;
        }

        // Find direct member nodes (in this subgraph but not inside any child subgraph).
        let direct_nodes: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| {
                !diagram.is_subgraph(n)
                    && !child_sgs.iter().any(|cs| {
                        child_sg_nodes
                            .get(cs)
                            .is_some_and(|set| set.contains(n.as_str()))
                    })
            })
            .map(|s| s.as_str())
            .collect();

        // For each child subgraph, check if any direct node overlaps.
        for child_sg_id in &child_sgs {
            let Some(sg_b) = subgraph_bounds.get(*child_sg_id).cloned() else {
                continue;
            };

            for node_id in &direct_nodes {
                let Some(nb) = node_bounds.get(*node_id) else {
                    continue;
                };

                // Check overlap based on the parent's direction.
                // For LR/RL, the primary axis is x; check if node and subgraph
                // share y-range (cross axis) and overlap on x (primary axis).
                let (shift_x, shift_y) = match sub_dir {
                    Direction::LeftRight | Direction::RightLeft => {
                        // Check y-range overlap (cross axis).
                        let y_overlap = nb.y < sg_b.y + sg_b.height && nb.y + nb.height > sg_b.y;
                        if !y_overlap {
                            continue;
                        }
                        // Check x overlap.
                        let node_right = nb.x + nb.width;
                        if node_right <= sg_b.x {
                            continue; // no overlap
                        }
                        let node_left = nb.x;
                        if node_left >= sg_b.x + sg_b.width {
                            continue; // no overlap
                        }
                        // Node overlaps with subgraph on x.  Determine which
                        // side the node is on and push the subgraph away.
                        let node_cx = nb.x + nb.width / 2;
                        let sg_cx = sg_b.x + sg_b.width / 2;
                        if node_cx < sg_cx {
                            // Node is to the left — push subgraph right.
                            let shift = node_right + 1 - sg_b.x;
                            (shift, 0)
                        } else {
                            // Node is to the right — push subgraph left (shift node right).
                            let shift = sg_b.x + sg_b.width + 1 - nb.x;
                            // Shift the node instead.
                            if let Some(pos) = draw_positions.get_mut(*node_id) {
                                pos.0 += shift;
                            }
                            if let Some(b) = node_bounds.get_mut(*node_id) {
                                b.x += shift;
                                if let Some(ref mut cx) = b.layout_center_x {
                                    *cx += shift;
                                }
                            }
                            continue;
                        }
                    }
                    Direction::TopDown | Direction::BottomTop => {
                        // Check x-range overlap (cross axis).
                        let x_overlap = nb.x < sg_b.x + sg_b.width && nb.x + nb.width > sg_b.x;
                        if !x_overlap {
                            continue;
                        }
                        // Check y overlap.
                        let node_bottom = nb.y + nb.height;
                        if node_bottom <= sg_b.y {
                            continue;
                        }
                        let node_top = nb.y;
                        if node_top >= sg_b.y + sg_b.height {
                            continue;
                        }
                        let node_cy = nb.y + nb.height / 2;
                        let sg_cy = sg_b.y + sg_b.height / 2;
                        if node_cy < sg_cy {
                            let shift = node_bottom + 1 - sg_b.y;
                            (0, shift)
                        } else {
                            let shift = sg_b.y + sg_b.height + 1 - nb.y;
                            if let Some(pos) = draw_positions.get_mut(*node_id) {
                                pos.1 += shift;
                            }
                            if let Some(b) = node_bounds.get_mut(*node_id) {
                                b.y += shift;
                                if let Some(ref mut cy) = b.layout_center_y {
                                    *cy += shift;
                                }
                            }
                            continue;
                        }
                    }
                };

                if shift_x == 0 && shift_y == 0 {
                    continue;
                }

                // Shift the child subgraph and all its contents.
                if let Some(b) = subgraph_bounds.get_mut(*child_sg_id) {
                    b.x += shift_x;
                    b.y += shift_y;
                }
                // Shift nodes inside the child subgraph.
                let child_sg = &diagram.subgraphs[*child_sg_id];
                for member_id in &child_sg.nodes {
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.0 += shift_x;
                        pos.1 += shift_y;
                    }
                    if let Some(b) = node_bounds.get_mut(member_id) {
                        b.x += shift_x;
                        b.y += shift_y;
                        if let Some(ref mut cx) = b.layout_center_x {
                            *cx += shift_x;
                        }
                        if let Some(ref mut cy) = b.layout_center_y {
                            *cy += shift_y;
                        }
                    }
                }
                // Shift grandchild subgraph bounds too.
                for (gc_id, gc_sg) in &diagram.subgraphs {
                    if gc_sg.parent.as_deref() == Some(*child_sg_id)
                        && let Some(b) = subgraph_bounds.get_mut(gc_id)
                    {
                        b.x += shift_x;
                        b.y += shift_y;
                    }
                }
            }
        }
    }
}

/// After sublayout reconciliation and overlap resolution, align direct sibling
/// nodes with their cross-boundary edge targets on the cross-axis of the parent
/// direction.  Without this, a node like C in an LR subgraph may stay vertically
/// aligned with B (top of a BT child subgraph) instead of A (its actual target at
/// the bottom), forcing the C→A edge to route diagonally through B's area.
pub(super) fn align_cross_boundary_siblings_draw(
    diagram: &Graph,
    node_bounds: &mut HashMap<String, NodeBounds>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    let mut affected_parents: HashSet<String> = HashSet::new();

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
            // Collect cross-boundary edge targets inside child subgraphs.
            let mut target_cross_positions: Vec<usize> = Vec::new();
            for edge in &diagram.edges {
                let target = if edge.from == *node_id && child_sg_nodes.contains(edge.to.as_str()) {
                    Some(edge.to.as_str())
                } else if edge.to == *node_id && child_sg_nodes.contains(edge.from.as_str()) {
                    Some(edge.from.as_str())
                } else {
                    None
                };

                if let Some(target_id) = target
                    && let Some(tb) = node_bounds.get(target_id)
                {
                    if is_horizontal {
                        target_cross_positions.push(tb.y + tb.height / 2);
                    } else {
                        target_cross_positions.push(tb.x + tb.width / 2);
                    }
                }
            }

            if target_cross_positions.is_empty() {
                continue;
            }

            let avg_target =
                target_cross_positions.iter().sum::<usize>() / target_cross_positions.len();
            let Some(nb) = node_bounds.get(*node_id).cloned() else {
                continue;
            };

            if is_horizontal {
                let node_cy = nb.y + nb.height / 2;
                if avg_target == node_cy {
                    continue;
                }
                let new_y = avg_target.saturating_sub(nb.height / 2);
                if let Some(pos) = draw_positions.get_mut(*node_id) {
                    pos.1 = new_y;
                }
                if let Some(b) = node_bounds.get_mut(*node_id) {
                    b.y = new_y;
                    b.layout_center_y = Some(new_y + nb.height / 2);
                }
            } else {
                let node_cx = nb.x + nb.width / 2;
                if avg_target == node_cx {
                    continue;
                }
                let new_x = avg_target.saturating_sub(nb.width / 2);
                if let Some(pos) = draw_positions.get_mut(*node_id) {
                    pos.0 = new_x;
                }
                if let Some(b) = node_bounds.get_mut(*node_id) {
                    b.x = new_x;
                    b.layout_center_x = Some(new_x + nb.width / 2);
                }
            }
            affected_parents.insert(sg_id.clone());
        }
    }

    if affected_parents.is_empty() {
        return;
    }

    // Re-expand bounds only for parent subgraphs where nodes were moved.
    for sg_id in &affected_parents {
        let Some(sg) = diagram.subgraphs.get(sg_id.as_str()) else {
            continue;
        };
        let Some(sb) = subgraph_bounds.get_mut(sg_id.as_str()) else {
            continue;
        };
        let pad = 2usize; // border + spacing
        for node_id in &sg.nodes {
            if diagram.is_subgraph(node_id) {
                continue;
            }
            let Some(nb) = node_bounds.get(node_id.as_str()) else {
                continue;
            };
            let need_left = nb.x.saturating_sub(pad);
            let need_top = nb.y.saturating_sub(pad);
            let need_right = nb.x + nb.width + pad;
            let need_bottom = nb.y + nb.height + pad;

            let title_rows = if !sg.title.trim().is_empty() { 1 } else { 0 };
            let need_top_with_title = need_top.saturating_sub(title_rows);

            let cur_right = sb.x + sb.width;
            let cur_bottom = sb.y + sb.height;
            let new_left = sb.x.min(need_left);
            let new_top = sb.y.min(need_top_with_title);
            let new_right = cur_right.max(need_right);
            let new_bottom = cur_bottom.max(need_bottom);
            sb.x = new_left;
            sb.y = new_top;
            sb.width = new_right.saturating_sub(new_left);
            sb.height = new_bottom.saturating_sub(new_top);
        }
    }
}

/// After sublayout reconciliation, re-layout direct children of override
/// subgraphs that contain compound child subgraphs (nested overrides).
///
/// The sublayout engine only positions leaf nodes.  When a parent override
/// subgraph contains compound children (nested override subgraphs), those
/// children take up space that the leaf-node-only sublayout doesn't account
/// for.  This pass lays out all direct children — compound subgraphs and
/// leaf nodes — in the override direction using declaration order, which
/// mirrors the synthetic-edge chaining the sublayout engine would use.
pub(super) fn layout_compound_parent_members(
    diagram: &Graph,
    sublayouts: &HashMap<String, OverrideSubgraphProjection>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    canvas_width: &mut usize,
    canvas_height: &mut usize,
) {
    let parent_map = build_subgraph_parent_map(&diagram.subgraphs);

    for (sg_id, sg) in &diagram.subgraphs {
        let Some(sub_dir) = sg.dir else { continue };
        let is_horizontal = matches!(sub_dir, Direction::LeftRight | Direction::RightLeft);

        // Find compound children (child subgraphs that are also override subgraphs).
        let child_overrides: Vec<&str> = diagram
            .subgraphs
            .iter()
            .filter(|(_, child)| child.parent.as_deref() == Some(sg_id.as_str()))
            .map(|(id, _)| id.as_str())
            .collect();

        if child_overrides.is_empty() {
            continue;
        }

        // Nodes belonging to any child subgraph (to identify direct leaf nodes).
        let child_sg_members: HashSet<&str> = child_overrides
            .iter()
            .filter_map(|id| diagram.subgraphs.get(*id))
            .flat_map(|child| child.nodes.iter().map(|s| s.as_str()))
            .collect();

        // Collect direct children in declaration order: compound children and
        // direct leaf nodes (not inside any child subgraph).
        struct Member<'a> {
            id: &'a str,
            is_compound: bool,
            primary_size: usize,
            cross_size: usize,
            sort_key: f64,
        }

        // Use sublayout float positions to determine ordering.  The sublayout
        // has correct edge-based ordering for leaf nodes.  Compound children
        // are ordered by the average primary-axis position of their members
        // in the sublayout.
        let sublayout = match sublayouts.get(sg_id) {
            Some(sl) => sl,
            None => continue,
        };

        // Helper: primary-axis float position from the sublayout.
        let primary_float = |id: &str| -> Option<f64> {
            sublayout.nodes.get(id).map(|r| {
                if is_horizontal {
                    r.x + r.width / 2.0
                } else {
                    r.y + r.height / 2.0
                }
            })
        };

        let mut members: Vec<Member> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();

        // Add compound children with average primary-axis position from
        // their members' sublayout positions.
        for &cc_id in &child_overrides {
            if let Some(cb) = subgraph_bounds.get(cc_id) {
                let cc_sg = &diagram.subgraphs[cc_id];
                let member_positions: Vec<f64> = cc_sg
                    .nodes
                    .iter()
                    .filter_map(|n| primary_float(n.as_str()))
                    .collect();
                let avg_primary = if member_positions.is_empty() {
                    f64::INFINITY
                } else {
                    member_positions.iter().sum::<f64>() / member_positions.len() as f64
                };
                let (ps, cs) = if is_horizontal {
                    (cb.width, cb.height)
                } else {
                    (cb.height, cb.width)
                };
                members.push(Member {
                    id: cc_id,
                    is_compound: true,
                    primary_size: ps,
                    cross_size: cs,
                    sort_key: avg_primary,
                });
            }
        }

        // Add direct leaf nodes.
        for member_id in &sg.nodes {
            let id = member_id.as_str();
            if !seen.insert(id) {
                continue;
            }
            if child_sg_members.contains(id) || diagram.is_subgraph(member_id) {
                continue;
            }
            if let Some(nb) = node_bounds.get(id) {
                let float_pos = primary_float(id).unwrap_or(f64::INFINITY);
                let (ps, cs) = if is_horizontal {
                    (nb.width, nb.height)
                } else {
                    (nb.height, nb.width)
                };
                members.push(Member {
                    id,
                    is_compound: false,
                    primary_size: ps,
                    cross_size: cs,
                    sort_key: float_pos,
                });
            }
        }

        // Sort by sublayout primary-axis position.
        members.sort_by(|a, b| {
            a.sort_key
                .partial_cmp(&b.sort_key)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if members.len() < 2 {
            continue;
        }

        // Determine existing cross-boundary edge routing needs.
        let has_incoming = parent_map.contains_key(sg_id.as_str())
            || diagram.edges.iter().any(|e| {
                let to_in = sg.nodes.contains(&e.to) || child_sg_members.contains(e.to.as_str());
                let from_in =
                    sg.nodes.contains(&e.from) || child_sg_members.contains(e.from.as_str());
                to_in && !from_in
            });
        let has_outgoing = diagram.edges.iter().any(|e| {
            let from_in = sg.nodes.contains(&e.from) || child_sg_members.contains(e.from.as_str());
            let to_in = sg.nodes.contains(&e.to) || child_sg_members.contains(e.to.as_str());
            from_in && !to_in
        });
        let (top_pad, bottom_pad) = match diagram.direction {
            Direction::TopDown => (
                if has_incoming { 2 } else { 1 },
                if has_outgoing { 2 } else { 1 },
            ),
            Direction::BottomTop => (
                if has_outgoing { 2 } else { 1 },
                if has_incoming { 2 } else { 1 },
            ),
            _ => (2, 2),
        };
        let side_pad = 2;

        let min_gap = 2;
        let max_cross: usize = members.iter().map(|m| m.cross_size).max().unwrap_or(0);
        let total_primary: usize = members.iter().map(|m| m.primary_size).sum::<usize>()
            + min_gap * members.len().saturating_sub(1);

        let (content_w, content_h) = if is_horizontal {
            (total_primary, max_cross)
        } else {
            (max_cross, total_primary)
        };

        // Enforce title minimum width.
        let min_title_width = if !sg.title.trim().is_empty() {
            sg.title.len() + 6
        } else {
            0
        };
        let sg_final_w = (content_w + side_pad * 2).max(min_title_width);
        let sg_final_h = content_h + top_pad + bottom_pad;

        // Anchor on the old subgraph center.
        let sg_draw = match subgraph_bounds.get(sg_id) {
            Some(b) => b.clone(),
            None => continue,
        };
        let sg_cx = sg_draw.x + sg_draw.width / 2;
        let sg_cy = sg_draw.y + sg_draw.height / 2;
        let new_sg_x = sg_cx.saturating_sub(sg_final_w / 2);
        let new_sg_y = sg_cy.saturating_sub(sg_final_h / 2);

        let content_x = new_sg_x + side_pad + (sg_final_w - content_w - side_pad * 2) / 2;
        let content_y = new_sg_y + top_pad;

        // Place members along the primary axis in declaration order.
        let mut cursor = 0usize;
        for member in &members {
            let (new_primary, new_cross) = if is_horizontal {
                let x = content_x + cursor;
                let y = content_y + (max_cross.saturating_sub(member.cross_size)) / 2;
                (x, y)
            } else {
                let x = content_x + (max_cross.saturating_sub(member.cross_size)) / 2;
                let y = content_y + cursor;
                (x, y)
            };

            if member.is_compound {
                // Shift compound child and all its content.
                let cb = subgraph_bounds.get(member.id).unwrap().clone();
                let (dx, dy) = if is_horizontal {
                    (
                        new_primary as isize - cb.x as isize,
                        new_cross as isize - cb.y as isize,
                    )
                } else {
                    (
                        new_cross as isize - cb.x as isize,
                        new_primary as isize - cb.y as isize,
                    )
                };
                shift_subgraph_and_contents(
                    diagram,
                    member.id,
                    dx,
                    dy,
                    draw_positions,
                    node_bounds,
                    subgraph_bounds,
                );
            } else {
                // Set leaf node position.
                let (x, y) = if is_horizontal {
                    (new_primary, new_cross)
                } else {
                    (new_cross, new_primary)
                };
                if let Some(pos) = draw_positions.get_mut(member.id) {
                    *pos = (x, y);
                }
                if let Some(nb) = node_bounds.get_mut(member.id) {
                    nb.x = x;
                    nb.y = y;
                    nb.layout_center_x = Some(x + nb.width / 2);
                    nb.layout_center_y = Some(y + nb.height / 2);
                }
            }

            cursor += member.primary_size + min_gap;
        }

        // Update parent subgraph bounds.
        let depth = diagram.subgraph_depth(sg_id);
        subgraph_bounds.insert(
            sg_id.clone(),
            SubgraphBounds {
                x: new_sg_x,
                y: new_sg_y,
                width: sg_final_w,
                height: sg_final_h,
                title: sg.title.clone(),
                depth,
                invisible: sg.invisible,
                concurrent_regions: Vec::new(),
            },
        );

        *canvas_width = (*canvas_width).max(new_sg_x + sg_final_w + 1);
        *canvas_height = (*canvas_height).max(new_sg_y + sg_final_h + 1);
    }
}

/// Shift a subgraph and all its descendant content by (dx, dy).
fn shift_subgraph_and_contents(
    diagram: &Graph,
    sg_id: &str,
    dx: isize,
    dy: isize,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    // Shift the subgraph bounds.
    if let Some(sb) = subgraph_bounds.get_mut(sg_id) {
        sb.x = (sb.x as isize + dx).max(0) as usize;
        sb.y = (sb.y as isize + dy).max(0) as usize;
    }
    // Shift all member nodes.
    if let Some(sg) = diagram.subgraphs.get(sg_id) {
        for member_id in &sg.nodes {
            if diagram.is_subgraph(member_id) {
                continue;
            }
            if let Some(pos) = draw_positions.get_mut(member_id) {
                pos.0 = (pos.0 as isize + dx).max(0) as usize;
                pos.1 = (pos.1 as isize + dy).max(0) as usize;
            }
            if let Some(nb) = node_bounds.get_mut(member_id) {
                nb.x = (nb.x as isize + dx).max(0) as usize;
                nb.y = (nb.y as isize + dy).max(0) as usize;
                if let Some(ref mut cx) = nb.layout_center_x {
                    *cx = (*cx as isize + dx).max(0) as usize;
                }
                if let Some(ref mut cy) = nb.layout_center_y {
                    *cy = (*cy as isize + dy).max(0) as usize;
                }
            }
        }
    }
    // Shift descendant subgraph bounds.
    let descendant_sgs: Vec<String> = diagram
        .subgraphs
        .iter()
        .filter(|(_, child)| child.parent.as_deref() == Some(sg_id))
        .map(|(id, _)| id.clone())
        .collect();
    for child_id in &descendant_sgs {
        if let Some(sb) = subgraph_bounds.get_mut(child_id.as_str()) {
            sb.x = (sb.x as isize + dx).max(0) as usize;
            sb.y = (sb.y as isize + dy).max(0) as usize;
        }
    }
}

/// Compact the vertical gap between external predecessor nodes and
/// top-level override subgraphs in a TopDown diagram.
///
/// The main grid derivation maps float-space compound-node spacing to grid
/// coordinates, which can produce excessive vertical gaps above override
/// subgraphs.  This pass pulls each override subgraph up so its top border
/// is at most `target_gap` rows below the nearest predecessor's bottom.
pub(super) fn compact_override_subgraph_vertical_gaps(
    diagram: &Graph,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    if !matches!(diagram.direction, Direction::TopDown) {
        return;
    }

    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() || sg.parent.is_some() {
            // Only process top-level override subgraphs.
            continue;
        }
        // Only compact subgraphs that contain nested override children.
        // These are the ones whose grid bounds were inflated by compound
        // node rank expansion in the main layout.
        let has_child_override = diagram
            .subgraphs
            .values()
            .any(|child| child.parent.as_deref() == Some(sg_id.as_str()) && child.dir.is_some());
        if !has_child_override {
            continue;
        }
        let Some(sb) = subgraph_bounds.get(sg_id).cloned() else {
            continue;
        };
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();

        // Find the closest predecessor bottom edge above the subgraph.
        let mut nearest_pred_bottom: Option<usize> = None;
        for edge in &diagram.edges {
            if sg_node_set.contains(edge.to.as_str())
                && !sg_node_set.contains(edge.from.as_str())
                && let Some(nb) = node_bounds.get(&edge.from)
            {
                let nb_bottom = nb.y + nb.height;
                if nb_bottom <= sb.y {
                    nearest_pred_bottom =
                        Some(nearest_pred_bottom.map_or(nb_bottom, |c: usize| c.max(nb_bottom)));
                }
            }
        }

        let Some(pred_bottom) = nearest_pred_bottom else {
            continue;
        };

        // Target gap: 4 rows (edge routing room).
        let target_gap = 4usize;
        let current_gap = sb.y.saturating_sub(pred_bottom);
        if current_gap <= target_gap {
            continue;
        }

        let pull_up = current_gap - target_gap;

        // Shift the subgraph and all its content up.
        shift_subgraph_and_contents(
            diagram,
            sg_id,
            0,
            -(pull_up as isize),
            draw_positions,
            node_bounds,
            subgraph_bounds,
        );
    }
}
