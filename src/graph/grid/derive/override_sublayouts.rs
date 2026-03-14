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
            sub_draw_nodes.sort_by_key(|n| n.1); // sort by x
            for i in 1..sub_draw_nodes.len() {
                let prev_right = sub_draw_nodes[i - 1].1 + sub_draw_nodes[i - 1].3;
                let needed = prev_right + min_gap;
                if sub_draw_nodes[i].1 < needed {
                    sub_draw_nodes[i].1 = needed;
                }
            }
        } else {
            sub_draw_nodes.sort_by_key(|n| n.2); // sort by y
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
