//! Layout computation for flowchart diagrams.
//!
//! Translates layout float coordinates into ASCII character-grid positions using
//! uniform scale factors, collision repair, and waypoint transformation.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
pub(crate) use super::layout_building::build_layered_layout;
// Re-export shared layout building functions from their canonical location.
pub(crate) use super::layout_building::{
    SubLayoutResult, compute_sublayouts, layered_config_for_layout,
};
use super::text_shape::{NodeBounds, node_dimensions};
pub(crate) use super::text_types::{CoordTransform, RawCenter, TransformContext};
// Re-export text types from their canonical location.
pub use super::text_types::{GridPos, Layout, SelfEdgeDrawData, SubgraphBounds, TextLayoutConfig};
use crate::diagrams::flowchart::geometry::FPoint;
use crate::graph::{Diagram, Direction, Edge};
use crate::layered::Rect;

/// Reconcile direction-override sub-layout positions in draw coordinates.
///
/// For each subgraph with a direction override:
/// 1. Get the current subgraph draw bounds (from the main layout's compound pipeline)
/// 2. Convert sub-layout positions to draw coordinates using simple spacing
/// 3. Center the sub-layout's draw positions within the subgraph bounds
/// 4. Override draw_positions, node_bounds, and subgraph_bounds
#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_sublayouts_draw(
    diagram: &Diagram,
    config: &TextLayoutConfig,
    sublayouts: &HashMap<String, SubLayoutResult>,
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
            .result
            .nodes
            .iter()
            .filter_map(|(id, _)| {
                diagram
                    .nodes
                    .get(&id.0)
                    .map(|n| (id.0.clone(), node_dimensions(n, sub_dir)))
            })
            .collect();

        let sub_rank_sep = config.rank_sep + config.cluster_rank_sep;
        let (sub_scale_x, sub_scale_y) = compute_ascii_scale_factors(
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
            .result
            .nodes
            .values()
            .map(|r| r.x)
            .fold(f64::INFINITY, f64::min);
        let sub_layout_min_y = sublayout
            .result
            .nodes
            .values()
            .map(|r| r.y)
            .fold(f64::INFINITY, f64::min);

        // Convert each sub-layout node to draw coordinates (relative)
        for (node_id, rect) in &sublayout.result.nodes {
            let (w, h) = match sub_node_dims.get(&node_id.0) {
                Some(&dims) => dims,
                None => continue,
            };

            let cx =
                ((rect.x + rect.width / 2.0 - sub_layout_min_x) * sub_scale_x).round() as usize;
            let cy =
                ((rect.y + rect.height / 2.0 - sub_layout_min_y) * sub_scale_y).round() as usize;
            let x = cx.saturating_sub(w / 2);
            let y = cy.saturating_sub(h / 2);

            sub_draw_nodes.push((node_id.0.clone(), x, y, w, h));
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
pub(crate) fn resolve_sibling_overlaps_draw(
    diagram: &Diagram,
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
pub(crate) fn align_cross_boundary_siblings_draw(
    diagram: &Diagram,
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

// Re-export float-coordinate subgraph operations from their canonical location.
pub(crate) use super::layout_subgraph_ops::{
    center_override_subgraphs, expand_parent_bounds, reconcile_sublayouts,
    resolve_sublayout_overlaps,
};

pub(crate) fn text_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Assign grid positions to nodes based on layers.
pub(crate) fn compute_grid_positions(layers: &[Vec<String>]) -> HashMap<String, GridPos> {
    let mut positions = HashMap::new();

    for (layer_idx, layer) in layers.iter().enumerate() {
        for (pos_idx, node_id) in layer.iter().enumerate() {
            positions.insert(
                node_id.clone(),
                GridPos {
                    layer: layer_idx,
                    pos: pos_idx,
                },
            );
        }
    }

    positions
}

/// Compute per-axis ASCII scale factors for translating layout float coordinates
/// to character grid positions.
///
/// Returns `(scale_x, scale_y)` where each factor maps layout coordinate deltas
/// to ASCII character deltas along that axis.
///
/// For vertical layouts (TD/BT):
///   - scale_y (primary) = (max_h + v_spacing) / (max_h + rank_sep)
///   - scale_x (cross)   = (avg_w + h_spacing) / (avg_w + node_sep)
///
/// For horizontal layouts (LR/RL):
///   - scale_x (primary) = (max_w + h_spacing) / (max_w + rank_sep)
///   - scale_y (cross)   = (avg_h + v_spacing) / (avg_h + node_sep)
pub(crate) fn compute_ascii_scale_factors(
    node_dims: &HashMap<String, (usize, usize)>,
    rank_sep: f64,
    node_sep: f64,
    v_spacing: usize,
    h_spacing: usize,
    is_vertical: bool,
    ranks_doubled: bool,
) -> (f64, f64) {
    let (total_w, total_h, max_w, max_h, count) = node_dims.values().fold(
        (0usize, 0usize, 0usize, 0usize, 0usize),
        |(tw, th, mw, mh, c), &(w, h)| (tw + w, th + h, mw.max(w), mh.max(h), c + 1),
    );
    let count_f = count.max(1) as f64;
    let avg_w = total_w as f64 / count_f;
    let avg_h = total_h as f64 / count_f;

    if is_vertical {
        // When ranks are doubled, the layout positions nodes 2× further apart.
        // To compensate exactly, we need: eff_rs = max_h + 2 * rank_sep
        // This gives scale_primary_new = scale_primary_old / 2, so that
        // (2 * rank_sep) * scale_new = rank_sep * scale_old.
        let effective_rank_sep = if ranks_doubled {
            max_h as f64 + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_h as f64 + v_spacing as f64) / (max_h as f64 + effective_rank_sep);
        let scale_cross = (avg_w + h_spacing as f64) / (avg_w + node_sep);
        (scale_cross, scale_primary)
    } else {
        let effective_rank_sep = if ranks_doubled {
            max_w as f64 + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_w as f64 + h_spacing as f64) / (max_w as f64 + effective_rank_sep);
        let scale_cross = (avg_h + v_spacing as f64) / (avg_h + node_sep);
        (scale_primary, scale_cross)
    }
}

/// Enforce minimum spacing between adjacent nodes within each layer after
/// scaling and rounding.
///
/// Nodes are sorted by their cross-axis position within each layer, then
/// scanned left-to-right (or top-to-bottom for horizontal layouts). If any
/// adjacent pair overlaps or is too close, the later node is pushed forward.
/// This cascades: pushing node B may cause it to overlap C, which also gets pushed.
///
/// For vertical layouts (`is_vertical = true`), the cross-axis is X.
/// For horizontal layouts (`is_vertical = false`), the cross-axis is Y.
pub(crate) fn collision_repair(
    layers: &[Vec<String>],
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_dims: &HashMap<String, (usize, usize)>,
    is_vertical: bool,
    min_gap: usize,
) {
    for layer in layers {
        if layer.len() <= 1 {
            continue;
        }

        let mut sorted: Vec<String> = layer.clone();
        sorted.sort_by_key(|id| {
            let &(x, y) = &draw_positions[id];
            if is_vertical { x } else { y }
        });

        for i in 1..sorted.len() {
            let prev_id = &sorted[i - 1];
            let curr_id = &sorted[i];
            let &(pw, ph) = &node_dims[prev_id];
            let (prev_x, prev_y) = draw_positions[prev_id];
            let (curr_x, curr_y) = draw_positions[curr_id];

            if is_vertical {
                let min_x = prev_x + pw + min_gap;
                if curr_x < min_x {
                    draw_positions.insert(curr_id.clone(), (min_x, curr_y));
                }
            } else {
                let min_y = prev_y + ph + min_gap;
                if curr_y < min_y {
                    draw_positions.insert(curr_id.clone(), (curr_x, min_y));
                }
            }
        }
    }
}

/// Enforce minimum spacing between adjacent layers along the primary axis.
///
/// For vertical layouts, layers stack along Y; for horizontal, along X.
/// If the closest node in the next layer is too close to the farthest node
/// in the previous layer, shift the entire next layer (and all subsequent layers)
/// forward to maintain the minimum gap.
pub(crate) fn rank_gap_repair(
    layers: &[Vec<String>],
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_dims: &HashMap<String, (usize, usize)>,
    is_vertical: bool,
    min_gap: usize,
) {
    if layers.len() <= 1 {
        return;
    }

    for i in 1..layers.len() {
        // Find the maximum primary-axis extent of the previous layer
        let prev_max = layers[i - 1]
            .iter()
            .filter_map(|id| {
                let &(x, y) = draw_positions.get(id)?;
                let &(w, h) = node_dims.get(id)?;
                Some(if is_vertical { y + h } else { x + w })
            })
            .max()
            .unwrap_or(0);

        // Find the minimum primary-axis position in the current layer
        let curr_min = layers[i]
            .iter()
            .filter_map(|id| {
                let &(x, y) = draw_positions.get(id)?;
                Some(if is_vertical { y } else { x })
            })
            .min()
            .unwrap_or(0);

        let required = prev_max + min_gap;
        if curr_min < required {
            let shift = required - curr_min;
            // Shift all nodes in this layer and all subsequent layers
            for layer in &layers[i..] {
                for id in layer {
                    if let Some(&(x, y)) = draw_positions.get(id) {
                        let new_pos = if is_vertical {
                            (x, y + shift)
                        } else {
                            (x + shift, y)
                        };
                        draw_positions.insert(id.clone(), new_pos);
                    }
                }
            }
        }
    }
}

/// Compute rank-to-draw-coordinate mapping for waypoint transformation.
///
/// For each layout rank that contains real nodes, computes the primary-axis
/// draw-coordinate extent from `node_bounds`. For ranks without real nodes
/// (dummy/label ranks from edge normalization), linearly interpolates between
/// the nearest neighboring real-node ranks.
///
/// Returns a `Vec<usize>` indexed by rank, where `layer_starts[rank]` gives
/// the primary-axis draw coordinate for that rank.
pub(crate) fn compute_layer_starts(
    node_ranks: &HashMap<String, i32>,
    node_bounds: &HashMap<String, NodeBounds>,
    is_vertical: bool,
) -> Vec<usize> {
    // Map each rank to its draw-coordinate extent (start, end) from real nodes
    let mut rank_to_actual_bounds: HashMap<i32, (usize, usize)> = HashMap::new();
    for (node_id, &rank) in node_ranks {
        if let Some(bounds) = node_bounds.get(node_id.as_str()) {
            let (start, end) = if is_vertical {
                (bounds.y, bounds.y + bounds.height)
            } else {
                (bounds.x, bounds.x + bounds.width)
            };
            rank_to_actual_bounds
                .entry(rank)
                .and_modify(|(s, e)| {
                    *s = (*s).min(start);
                    *e = (*e).max(end);
                })
                .or_insert((start, end));
        }
    }

    let max_rank = node_ranks.values().copied().max().unwrap_or(0).max(0) as usize;

    (0..=max_rank)
        .map(|rank| {
            let rank_i32 = rank as i32;
            if let Some(&(start, _end)) = rank_to_actual_bounds.get(&rank_i32) {
                start
            } else {
                // Interpolate between nearest real-node ranks
                let lower = (0..rank_i32)
                    .rev()
                    .find_map(|r| rank_to_actual_bounds.get(&r).map(|&(_, end)| (r, end)));
                let upper = ((rank_i32 + 1)..=(max_rank as i32))
                    .find_map(|r| rank_to_actual_bounds.get(&r).map(|&(start, _)| (r, start)));

                match (lower, upper) {
                    (Some((lower_rank, lower_end)), Some((upper_rank, upper_start))) => {
                        let rank_span = upper_rank - lower_rank;
                        let rank_offset = rank_i32 - lower_rank;
                        let pos_span = upper_start as i32 - lower_end as i32;
                        (lower_end as i32 + (pos_span * rank_offset) / rank_span) as usize
                    }
                    (Some((_, lower_end)), None) => lower_end,
                    (None, Some((_, upper_start))) => upper_start,
                    (None, None) => 0,
                }
            }
        })
        .collect()
}

/// Build a map from parent subgraph ID to list of direct child subgraph IDs.
#[cfg(test)]
fn build_children_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
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
pub(crate) fn subgraph_bounds_to_draw(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    layout_bounds: &HashMap<String, Rect>,
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

pub(crate) fn shrink_subgraph_vertical_gaps(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
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
pub(crate) fn ensure_external_edge_spacing(
    diagram: &Diagram,
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

pub(crate) fn shrink_subgraph_horizontal_gaps(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
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

fn build_subgraph_parent_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
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

fn subgraph_depth(subgraphs: &HashMap<String, crate::graph::Subgraph>, sg_id: &str) -> usize {
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

fn build_subgraph_incoming_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
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

fn build_subgraph_outgoing_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
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
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
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
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
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

pub(crate) fn expand_parent_subgraph_bounds(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
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
pub(crate) fn ensure_subgraph_contains_members(
    diagram: &crate::graph::Diagram,
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

/// Nudge waypoints that overlap with node bounding boxes.
///
/// If a waypoint falls inside a node, push it just past the node's edge along the
/// cross-axis (X for vertical layouts, Y for horizontal). The waypoint is then
/// clamped to stay within canvas bounds.
pub(crate) fn nudge_colliding_waypoints(
    edge_waypoints: &mut HashMap<usize, Vec<(usize, usize)>>,
    node_bounds: &HashMap<String, NodeBounds>,
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) {
    let mut sorted_bounds: Vec<NodeBounds> = node_bounds.values().copied().collect();
    sorted_bounds.sort_by_key(|bounds| (bounds.y, bounds.x, bounds.width, bounds.height));

    for waypoints in edge_waypoints.values_mut() {
        nudge_waypoint_points(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
        *waypoints = repair_quantized_waypoint_segments(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
        nudge_waypoint_points(
            waypoints,
            &sorted_bounds,
            is_vertical,
            canvas_width,
            canvas_height,
        );
    }
}

fn nudge_waypoint_points(
    waypoints: &mut [(usize, usize)],
    node_bounds: &[NodeBounds],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) {
    for wp in waypoints.iter_mut() {
        for bounds in node_bounds {
            if bounds.contains(wp.0, wp.1) {
                if is_vertical {
                    wp.0 = bounds.x + bounds.width + 1;
                } else {
                    wp.1 = bounds.y + bounds.height + 1;
                }
                break;
            }
        }
        clamp_waypoint(wp, canvas_width, canvas_height);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaypointSegment {
    start: (usize, usize),
    end: (usize, usize),
    axis: SegmentAxis,
}

fn repair_quantized_waypoint_segments(
    waypoints: &[(usize, usize)],
    node_bounds: &[NodeBounds],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> Vec<(usize, usize)> {
    if waypoints.len() < 2 || node_bounds.is_empty() {
        return waypoints.to_vec();
    }

    let mut repaired = waypoints.to_vec();
    let max_repairs = node_bounds.len().saturating_mul(waypoints.len().max(1)) * 2;
    let mut repairs = 0usize;

    loop {
        let mut changed = false;

        for idx in 0..repaired.len().saturating_sub(1) {
            let from = repaired[idx];
            let to = repaired[idx + 1];
            let Some((blocker, axis)) = first_blocking_segment(from, to, node_bounds, is_vertical)
            else {
                continue;
            };

            let detour = detour_waypoints_around_blocker(
                from,
                to,
                blocker,
                axis,
                canvas_width,
                canvas_height,
            );
            if detour.is_empty() {
                continue;
            }

            repaired.splice(idx + 1..idx + 1, detour);
            changed = true;
            repairs += 1;
            break;
        }

        if !changed || repairs >= max_repairs {
            break;
        }
    }

    repaired.dedup();
    repaired
}

fn first_blocking_segment(
    from: (usize, usize),
    to: (usize, usize),
    node_bounds: &[NodeBounds],
    is_vertical: bool,
) -> Option<(NodeBounds, SegmentAxis)> {
    let segments = orthogonal_segments_between_waypoints(from, to, is_vertical);

    for segment in segments {
        for bounds in node_bounds {
            if orthogonal_segment_intersects_bounds(segment.start, segment.end, bounds) {
                return Some((*bounds, segment.axis));
            }
        }
    }

    None
}

fn orthogonal_segments_between_waypoints(
    from: (usize, usize),
    to: (usize, usize),
    is_vertical: bool,
) -> Vec<WaypointSegment> {
    if from == to {
        return Vec::new();
    }

    if from.0 == to.0 {
        return vec![WaypointSegment {
            start: from,
            end: to,
            axis: SegmentAxis::Vertical,
        }];
    }

    if from.1 == to.1 {
        return vec![WaypointSegment {
            start: from,
            end: to,
            axis: SegmentAxis::Horizontal,
        }];
    }

    if is_vertical {
        let elbow = (to.0, from.1);
        vec![
            WaypointSegment {
                start: from,
                end: elbow,
                axis: SegmentAxis::Horizontal,
            },
            WaypointSegment {
                start: elbow,
                end: to,
                axis: SegmentAxis::Vertical,
            },
        ]
    } else {
        let elbow = (from.0, to.1);
        vec![
            WaypointSegment {
                start: from,
                end: elbow,
                axis: SegmentAxis::Vertical,
            },
            WaypointSegment {
                start: elbow,
                end: to,
                axis: SegmentAxis::Horizontal,
            },
        ]
    }
}

fn orthogonal_segment_intersects_bounds(
    start: (usize, usize),
    end: (usize, usize),
    bounds: &NodeBounds,
) -> bool {
    let left = bounds.x;
    let right = bounds.x + bounds.width.saturating_sub(1);
    let top = bounds.y;
    let bottom = bounds.y + bounds.height.saturating_sub(1);

    if start.0 == end.0 {
        let x = start.0;
        let (y_min, y_max) = if start.1 <= end.1 {
            (start.1, end.1)
        } else {
            (end.1, start.1)
        };
        return x >= left && x <= right && y_min <= bottom && top <= y_max;
    }

    if start.1 == end.1 {
        let y = start.1;
        let (x_min, x_max) = if start.0 <= end.0 {
            (start.0, end.0)
        } else {
            (end.0, start.0)
        };
        return y >= top && y <= bottom && x_min <= right && left <= x_max;
    }

    false
}

fn detour_waypoints_around_blocker(
    from: (usize, usize),
    to: (usize, usize),
    blocker: NodeBounds,
    axis: SegmentAxis,
    canvas_width: usize,
    canvas_height: usize,
) -> Vec<(usize, usize)> {
    let mut detour = Vec::with_capacity(2);

    match axis {
        SegmentAxis::Horizontal => {
            let detour_y =
                choose_detour_coordinate(from.1, to.1, blocker.y, blocker.height, canvas_height);
            if detour_y != from.1 {
                detour.push((from.0, detour_y));
            }
            if detour.last().copied() != Some((to.0, detour_y)) {
                detour.push((to.0, detour_y));
            }
        }
        SegmentAxis::Vertical => {
            let detour_x =
                choose_detour_coordinate(from.0, to.0, blocker.x, blocker.width, canvas_width);
            if detour_x != from.0 {
                detour.push((detour_x, from.1));
            }
            if detour.last().copied() != Some((detour_x, to.1)) {
                detour.push((detour_x, to.1));
            }
        }
    }

    detour
}

fn choose_detour_coordinate(
    start_coord: usize,
    end_coord: usize,
    blocker_origin: usize,
    blocker_span: usize,
    canvas_limit: usize,
) -> usize {
    let max_coord = canvas_limit.saturating_sub(1);
    let before = blocker_origin.saturating_sub(1);
    let after = blocker_origin
        .saturating_add(blocker_span)
        .saturating_add(1)
        .min(max_coord);

    let mut candidates = [before, after];
    candidates.sort_by_key(|candidate| {
        (
            start_coord.abs_diff(*candidate) + end_coord.abs_diff(*candidate),
            usize::MAX - *candidate,
        )
    });
    candidates[0]
}

fn clamp_waypoint(waypoint: &mut (usize, usize), canvas_width: usize, canvas_height: usize) {
    waypoint.0 = waypoint.0.min(canvas_width.saturating_sub(1));
    waypoint.1 = waypoint.1.min(canvas_height.saturating_sub(1));
}

/// Transform layout waypoints to ASCII draw coordinates using uniform scale factors.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses `layer_starts` to snap to
/// the correct rank position. The cross axis uses uniform scaling from layout
/// coordinates, ensuring consistency with node positions.
pub(crate) fn transform_waypoints_direct(
    edge_waypoints: &HashMap<usize, Vec<(FPoint, i32)>>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, Vec<(usize, usize)>> {
    let mut converted = HashMap::new();

    for (edge_idx, waypoints) in edge_waypoints {
        if edges.get(*edge_idx).is_some() {
            let wps: Vec<(usize, usize)> = waypoints
                .iter()
                .map(|(fp, rank)| {
                    let rank_idx = *rank as usize;
                    let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
                    let (scaled_x, scaled_y) = ctx.to_ascii(fp.x, fp.y);

                    if is_vertical {
                        (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
                    } else {
                        (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
                    }
                })
                .collect();

            converted.insert(*edge_idx, wps);
        }
    }

    converted
}

/// Transform layout label positions to ASCII draw coordinates.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses rank-based snapping via
/// `layer_starts[rank]`, matching how `transform_waypoints_direct()` works.
/// The cross axis uses uniform scaling from layout coordinates.
pub(crate) fn transform_label_positions_direct(
    label_positions: &HashMap<usize, (FPoint, i32)>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, (usize, usize)> {
    let mut converted = HashMap::new();

    for (edge_idx, (fp, rank)) in label_positions {
        if edges.get(*edge_idx).is_some() {
            let rank_idx = *rank as usize;
            let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
            let (scaled_x, scaled_y) = ctx.to_ascii(fp.x, fp.y);

            let pos = if is_vertical {
                (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
            } else {
                (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
            };
            converted.insert(*edge_idx, pos);
        }
    }

    converted
}

fn waypoint_inside_bounds(bounds: &SubgraphBounds, point: (usize, usize)) -> bool {
    let (x, y) = point;
    let max_x = bounds.x + bounds.width.saturating_sub(1);
    let max_y = bounds.y + bounds.height.saturating_sub(1);
    x > bounds.x && x < max_x && y > bounds.y && y < max_y
}

fn segment_bounds_intersection(
    start: (usize, usize),
    end: (usize, usize),
    bounds: &SubgraphBounds,
) -> Option<(usize, usize)> {
    let (x0, y0) = (start.0 as f64, start.1 as f64);
    let (x1, y1) = (end.0 as f64, end.1 as f64);
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return None;
    }

    let x_min = bounds.x as f64;
    let x_max = (bounds.x + bounds.width) as f64;
    let y_min = bounds.y as f64;
    let y_max = (bounds.y + bounds.height) as f64;

    let mut candidates: Vec<(f64, (usize, usize))> = Vec::new();

    if dx.abs() > f64::EPSILON {
        let t_left = (x_min - x0) / dx;
        if (0.0..=1.0).contains(&t_left) {
            let y = y0 + t_left * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_left, (x_min.round() as usize, y.round() as usize)));
            }
        }
        let t_right = (x_max - x0) / dx;
        if (0.0..=1.0).contains(&t_right) {
            let y = y0 + t_right * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_right, (x_max.round() as usize, y.round() as usize)));
            }
        }
    }

    if dy.abs() > f64::EPSILON {
        let t_top = (y_min - y0) / dy;
        if (0.0..=1.0).contains(&t_top) {
            let x = x0 + t_top * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_top, (x.round() as usize, y_min.round() as usize)));
            }
        }
        let t_bottom = (y_max - y0) / dy;
        if (0.0..=1.0).contains(&t_bottom) {
            let x = x0 + t_bottom * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_bottom, (x.round() as usize, y_max.round() as usize)));
            }
        }
    }

    candidates
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

pub(crate) fn clip_waypoints_to_subgraph(
    waypoints: &[(usize, usize)],
    bounds: &SubgraphBounds,
    clip_start: bool,
    clip_end: bool,
) -> Vec<(usize, usize)> {
    if waypoints.len() < 2 {
        return waypoints.to_vec();
    }
    let mut out = waypoints.to_vec();

    if clip_start && waypoint_inside_bounds(bounds, out[0]) {
        let mut idx = 0usize;
        while idx + 1 < out.len() && waypoint_inside_bounds(bounds, out[idx]) {
            idx += 1;
        }
        if idx < out.len() {
            let inside = out[idx.saturating_sub(1)];
            let outside = out[idx];
            let intersection =
                segment_bounds_intersection(inside, outside, bounds).unwrap_or(inside);
            let mut new_points = Vec::new();
            new_points.push(intersection);
            new_points.extend_from_slice(&out[idx..]);
            out = new_points;
        }
    }

    if clip_end && out.len() >= 2 {
        let last_idx = out.len() - 1;
        if waypoint_inside_bounds(bounds, out[last_idx]) {
            let mut idx = last_idx;
            while idx > 0 && waypoint_inside_bounds(bounds, out[idx]) {
                idx -= 1;
            }
            if idx < last_idx {
                let outside = out[idx];
                let inside = out[idx + 1];
                let intersection =
                    segment_bounds_intersection(outside, inside, bounds).unwrap_or(inside);
                let mut new_points = out[..=idx].to_vec();
                new_points.push(intersection);
                out = new_points;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::super::text_adapter::compute_layout;
    use super::*;
    use crate::layered::{self, Direction as LayeredDirection, LayoutConfig as LayeredConfig};

    fn test_node_bounds(x: usize, y: usize, width: usize, height: usize) -> NodeBounds {
        NodeBounds {
            x,
            y,
            width,
            height,
            layout_center_x: None,
            layout_center_y: None,
        }
    }

    fn segment_intersects_node(a: (usize, usize), b: (usize, usize), bounds: &NodeBounds) -> bool {
        let left = bounds.x;
        let right = bounds.x + bounds.width.saturating_sub(1);
        let top = bounds.y;
        let bottom = bounds.y + bounds.height.saturating_sub(1);

        if a.0 == b.0 {
            let x = a.0;
            let (y_min, y_max) = if a.1 <= b.1 { (a.1, b.1) } else { (b.1, a.1) };
            return x >= left && x <= right && y_min <= bottom && top <= y_max;
        }

        if a.1 == b.1 {
            let y = a.1;
            let (x_min, x_max) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
            return y >= top && y <= bottom && x_min <= right && left <= x_max;
        }

        false
    }

    fn segment_chain_clears_nodes(waypoints: &[(usize, usize)], bounds: &[NodeBounds]) -> bool {
        waypoints.windows(2).all(|pair| {
            bounds
                .iter()
                .all(|bounds| !segment_intersects_node(pair[0], pair[1], bounds))
        })
    }

    // =========================================================================
    // Scale Factor Tests (Phase 2)
    // =========================================================================

    #[test]
    fn scale_factors_td_typical() {
        // Typical TD: 3 nodes with widths 9,7,11 and heights all 3
        // avg_w = 9.0, max_h = 3
        // rank_sep = 50.0, node_sep = 50.0, v_spacing = 3, h_spacing = 4
        // scale_y (primary) = (3 + 3) / (3 + 50) = 6/53
        // scale_x (cross)   = (9 + 4) / (9 + 50) = 13/59
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (7, 3));
        dims.insert("C".into(), (11, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);

        let expected_sy = 6.0 / 53.0;
        let expected_sx = 13.0 / 59.0;
        assert!(
            (sx - expected_sx).abs() < 1e-6,
            "sx: got {sx}, expected {expected_sx}"
        );
        assert!(
            (sy - expected_sy).abs() < 1e-6,
            "sy: got {sy}, expected {expected_sy}"
        );
    }

    #[test]
    fn scale_factors_lr_direction_aware() {
        // LR: nodes widths 9,9, heights 3,3 → avg_h = 3, max_w = 9
        // scale_x (primary) = (9 + 4) / (9 + 50) = 13/59
        // scale_y (cross)   = (3 + 3) / (3 + 6) = 6/9
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (9, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 6.0, 3, 4, false, false);

        let expected_sx = 13.0 / 59.0;
        let expected_sy = 6.0 / 9.0;
        assert!(
            (sx - expected_sx).abs() < 1e-6,
            "sx: got {sx}, expected {expected_sx}"
        );
        assert!(
            (sy - expected_sy).abs() < 1e-6,
            "sy: got {sy}, expected {expected_sy}"
        );
    }

    #[test]
    fn scale_factors_single_node() {
        let mut dims = HashMap::new();
        dims.insert("X".into(), (5, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        assert!(sx > 0.0, "sx should be positive, got {sx}");
        assert!(sy > 0.0, "sy should be positive, got {sy}");
        assert!(sx.is_finite());
        assert!(sy.is_finite());
    }

    // =========================================================================
    // Layered Layout Helper Tests
    // =========================================================================

    #[test]
    fn build_layered_layout_includes_label_positions() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA -- yes --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = build_layered_layout(
            &diagram,
            &TextLayoutConfig::default(),
            |node| (node.label.len() as f64 + 4.0, 3.0),
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| text_edge_label_dimensions(label))
            },
        );

        assert!(result.label_positions.contains_key(&0));
    }

    #[test]
    fn scale_factors_halved_for_doubled_ranks() {
        // With ranks_doubled=true, effective_rank_sep = max_h + 2*rank_sep = 3 + 100 = 103
        // scale_y = (max_h + v_spacing) / (max_h + eff_rs) = 6/106
        // This is exactly half of the non-doubled scale: 6/53 / 2 = 6/106
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (7, 3));

        let (_, sy_normal) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        let (_, sy_doubled) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, true);

        // Doubled-rank scale should be exactly half of normal scale
        let expected_sy = sy_normal / 2.0;
        assert!(
            (sy_doubled - expected_sy).abs() < 1e-6,
            "sy_doubled: got {sy_doubled}, expected {expected_sy} (half of {sy_normal})"
        );

        // Verify: gap_new = 2*rank_sep*scale_doubled = gap_old = rank_sep*scale_normal
        let gap_normal = 50.0 * sy_normal;
        let gap_doubled = 100.0 * sy_doubled;
        assert!(
            (gap_normal - gap_doubled).abs() < 1e-6,
            "Gaps should match: normal={gap_normal}, doubled={gap_doubled}"
        );
    }

    #[test]
    fn scale_factors_empty_nodes() {
        let dims: HashMap<String, (usize, usize)> = HashMap::new();
        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        assert!(sx.is_finite());
        assert!(sy.is_finite());
    }

    // =========================================================================
    // Collision Repair Tests (Phase 3)
    // =========================================================================

    #[test]
    fn collision_repair_pushes_overlapping_nodes_apart() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (5, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (0, 0), "A should not move");
        assert_eq!(positions["B"], (12, 0), "B pushed to right edge of A + gap");
    }

    #[test]
    fn collision_repair_cascading() {
        let layers = vec![vec!["A".into(), "B".into(), "C".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (3, 0));
        positions.insert("C".into(), (8, 0));
        let dims: HashMap<String, (usize, usize)> = [
            ("A".into(), (6, 3)),
            ("B".into(), (6, 3)),
            ("C".into(), (6, 3)),
        ]
        .into();

        collision_repair(&layers, &mut positions, &dims, true, 2);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (8, 0));
        assert_eq!(positions["C"], (16, 0));
    }

    #[test]
    fn collision_repair_no_change_when_spaced() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (20, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (20, 0));
    }

    #[test]
    fn collision_repair_horizontal_layout() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (0, 2));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, false, 3);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (0, 6));
    }

    #[test]
    fn collision_repair_single_node_layer_noop() {
        let layers = vec![vec!["A".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (5, 5));
        let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (5, 5));
    }

    #[test]
    fn collision_repair_sorts_by_cross_axis() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (20, 0));
        positions.insert("B".into(), (0, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["B"], (0, 0));
        assert_eq!(positions["A"], (20, 0));
    }

    // =========================================================================
    // Waypoint Transform Tests (Phase 4)
    // =========================================================================

    #[test]
    fn waypoint_transform_vertical_basic() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "C")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(0usize, vec![(FPoint::new(100.0, 75.0), 1)]);

        let layer_starts = vec![1, 5, 9];
        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 25.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 80, 20);

        assert!(
            result.contains_key(&0),
            "should have waypoints for edge 0 (A→C)"
        );
        let wps = &result[&0];
        assert_eq!(wps.len(), 1);
        assert_eq!(wps[0].1, 5, "y should be layer_starts[1]");
        assert_eq!(wps[0].0, 12, "x should be scaled layout x + padding");
    }

    #[test]
    fn waypoint_transform_horizontal_basic() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "C")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(0usize, vec![(FPoint::new(75.0, 100.0), 1)]);

        let layer_starts = vec![1, 8, 15];
        let ctx = TransformContext {
            layout_min_x: 25.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.67,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, false, 40, 80);

        let wps = &result[&0];
        assert_eq!(wps[0].0, 8, "x should be layer_starts[1]");
        assert_eq!(wps[0].1, 35, "y should be scaled layout y + padding");
    }

    #[test]
    fn waypoint_transform_clamps_to_canvas() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(0usize, vec![(FPoint::new(5000.0, 50.0), 0)]);

        let layer_starts = vec![1];
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.5,
            scale_y: 0.5,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 30, 20);

        let wps = &result[&0];
        assert!(wps[0].0 <= 29, "x clamped to canvas_width - 1");
    }

    #[test]
    fn waypoint_transform_empty_input() {
        let edges: Vec<Edge> = vec![];
        let waypoints: HashMap<usize, Vec<(FPoint, i32)>> = HashMap::new();
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &[], true, 80, 20);
        assert!(result.is_empty());
    }

    #[test]
    fn nudge_colliding_waypoints_repairs_segment_collision_not_just_point_collision() {
        let mut edge_waypoints = HashMap::from([(0usize, vec![(20, 10), (40, 10)])]);
        let blocking_node = test_node_bounds(28, 8, 8, 4);
        let node_bounds = HashMap::from([("blocker".to_string(), blocking_node)]);

        nudge_colliding_waypoints(&mut edge_waypoints, &node_bounds, true, 80, 40);

        let repaired = edge_waypoints
            .get(&0)
            .expect("test edge should still have waypoints");
        assert!(
            segment_chain_clears_nodes(repaired, &[blocking_node]),
            "segment-wise repair should clear nodes even when waypoint points stay outside the node: {repaired:?}"
        );
    }

    // =========================================================================
    // Label Transform Tests (Phase 5)
    // =========================================================================

    #[test]
    fn label_transform_basic_scaling() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("yes")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(0usize, (FPoint::new(150.0, 100.0), 1));

        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        // layer_starts: rank 0 → y=0, rank 1 → y=8, rank 2 → y=16
        let layer_starts = vec![0, 8, 16];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        assert!(result.contains_key(&0));
        // x uses uniform scale: (150-50)*0.22 + 1 = 23
        // y = layer_starts[rank=1] = 8
        assert_eq!(result[&0], (23, 8));
    }

    #[test]
    fn label_transform_with_left_margin() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("yes")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(0usize, (FPoint::new(150.0, 100.0), 1));

        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 3,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts = vec![0, 8, 16];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        // x = 23 + 3 (left_label_margin) = 26
        assert_eq!(result[&0].0, 26);
    }

    #[test]
    fn label_transform_empty_input() {
        let edges: Vec<Edge> = vec![];
        let labels: HashMap<usize, (FPoint, i32)> = HashMap::new();
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts: Vec<usize> = vec![];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);
        assert!(result.is_empty());
    }

    // =========================================================================
    // Compound Graph Wiring Tests
    // =========================================================================

    #[test]
    fn test_layout_subgraph_bounds_present() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        assert!(
            layout.subgraph_bounds.contains_key("sg1"),
            "should have bounds for sg1"
        );
        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(bounds.width > 0, "width should be positive");
        assert!(bounds.height > 0, "height should be positive");
        assert_eq!(bounds.title, "Group");
    }

    #[test]
    fn test_nested_subgraph_layout_produces_both_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA[Node A]\nsubgraph inner[Inner]\nB[Node B]\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        assert!(
            layout.subgraph_bounds.contains_key("outer"),
            "should have outer bounds"
        );
        assert!(
            layout.subgraph_bounds.contains_key("inner"),
            "should have inner bounds"
        );
    }

    #[test]
    fn test_layout_no_subgraph_bounds_simple() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        assert!(layout.subgraph_bounds.is_empty());
    }

    #[test]
    fn test_layout_canvas_dimensions_include_borders() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(
            layout.width >= bounds.x + bounds.width,
            "canvas width {} should contain border x+w={}",
            layout.width,
            bounds.x + bounds.width
        );
        assert!(
            layout.height >= bounds.y + bounds.height,
            "canvas height {} should contain border y+h={}",
            layout.height,
            bounds.y + bounds.height
        );
    }

    #[test]
    fn test_compute_layout_subgraph_diagram_succeeds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // Should not panic
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        assert!(layout.draw_positions.contains_key("A"));
        assert!(layout.draw_positions.contains_key("B"));
        assert!(layout.draw_positions.contains_key("C"));
    }

    #[test]
    fn test_compute_layout_simple_diagram_no_compound() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        assert!(!diagram.has_subgraphs());

        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        assert!(layout.draw_positions.contains_key("A"));
    }

    #[test]
    fn label_position_within_canvas_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n    A -->|yes| B";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        // Label position should exist — edge A→B is at index 0
        let edge_idx = diagram
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == "B")
            .unwrap()
            .index;
        assert!(
            layout.edge_label_positions.contains_key(&edge_idx),
            "Should have precomputed label position for A->B, got keys: {:?}",
            layout.edge_label_positions.keys().collect::<Vec<_>>()
        );

        let (lx, ly) = layout.edge_label_positions[&edge_idx];
        // Should be within canvas bounds
        assert!(
            lx < layout.width && ly < layout.height,
            "Label position ({}, {}) should be within canvas ({}, {})",
            lx,
            ly,
            layout.width,
            layout.height
        );
    }

    #[test]
    fn label_transform_skips_missing_edge() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("x")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(5usize, (FPoint::new(100.0, 100.0), 0));

        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts = vec![0];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        assert!(
            result.is_empty(),
            "out-of-bounds edge index should be skipped"
        );
    }

    // =========================================================================
    // Nested Subgraph Tests (Plan 0032)
    // =========================================================================

    #[test]
    fn test_nested_borders_inner_visible() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;
        use crate::render::{RenderOptions, render};

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let output = render(&diagram, &RenderOptions::default());
        assert!(
            output.contains("Outer"),
            "Output should contain 'Outer' title"
        );
        assert!(
            output.contains("Inner"),
            "Output should contain 'Inner' title"
        );
    }

    #[test]
    fn test_nested_subgraph_depth_values() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        assert_eq!(layout.subgraph_bounds["outer"].depth, 0);
        assert_eq!(layout.subgraph_bounds["inner"].depth, 1);
    }

    #[test]
    fn test_nested_subgraph_parent_contains_child_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        let outer = &layout.subgraph_bounds["outer"];
        let inner = &layout.subgraph_bounds["inner"];
        // Parent must fully contain child
        assert!(
            outer.x <= inner.x,
            "outer.x ({}) should be <= inner.x ({})",
            outer.x,
            inner.x
        );
        assert!(
            outer.y <= inner.y,
            "outer.y ({}) should be <= inner.y ({})",
            outer.y,
            inner.y
        );
        assert!(
            outer.x + outer.width >= inner.x + inner.width,
            "outer right ({}) should be >= inner right ({})",
            outer.x + outer.width,
            inner.x + inner.width
        );
        assert!(
            outer.y + outer.height >= inner.y + inner.height,
            "outer bottom ({}) should be >= inner bottom ({})",
            outer.y + outer.height,
            inner.y + inner.height
        );
    }

    #[test]
    fn test_nested_outer_only_subgraph_gets_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());
        assert!(
            layout.subgraph_bounds.contains_key("outer"),
            "outer should have bounds"
        );
        let outer = &layout.subgraph_bounds["outer"];
        assert!(outer.width > 0, "width should be positive");
        assert!(outer.height > 0, "height should be positive");
    }

    #[test]
    fn test_build_children_map() {
        use crate::graph::Subgraph;
        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "inner".to_string(),
            Subgraph {
                id: "inner".to_string(),
                title: "Inner".to_string(),
                nodes: vec!["A".to_string()],
                parent: Some("outer".to_string()),
                dir: None,
            },
        );
        subgraphs.insert(
            "outer".to_string(),
            Subgraph {
                id: "outer".to_string(),
                title: "Outer".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );
        let children_map = build_children_map(&subgraphs);
        assert_eq!(children_map["outer"], vec!["inner".to_string()]);
        assert!(!children_map.contains_key("inner"));
    }

    // =========================================================================
    // Subgraph Bounds Tests (Layout-derived bounds)
    // =========================================================================

    #[test]
    fn test_subgraph_bounds_no_overlap_from_separated_rects() {
        use crate::graph::Subgraph;

        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Left".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );
        subgraphs.insert(
            "sg2".to_string(),
            Subgraph {
                id: "sg2".to_string(),
                title: "Right".to_string(),
                nodes: vec!["B".to_string()],
                parent: None,
                dir: None,
            },
        );

        let mut layout_bounds = HashMap::new();
        layout_bounds.insert(
            "sg1".to_string(),
            Rect {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 5.0,
            },
        );
        layout_bounds.insert(
            "sg2".to_string(),
            Rect {
                x: 40.0,
                y: 10.0,
                width: 10.0,
                height: 5.0,
            },
        );

        let config = TextLayoutConfig {
            padding: 0,
            left_label_margin: 0,
            ..TextLayoutConfig::default()
        };

        let transform = CoordTransform {
            scale_x: 1.0,
            scale_y: 1.0,
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            max_overhang_x: 0,
            max_overhang_y: 0,
            config: &config,
        };

        let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

        let a = &result["sg1"];
        let b = &result["sg2"];

        // Separated member nodes should produce non-overlapping draw bounds
        let no_x_overlap = a.x + a.width <= b.x || b.x + b.width <= a.x;
        let no_y_overlap = a.y + a.height <= b.y || b.y + b.height <= a.y;
        assert!(
            no_x_overlap || no_y_overlap,
            "Bounds should not overlap: sg1=({},{} {}x{}) sg2=({},{} {}x{})",
            a.x,
            a.y,
            a.width,
            a.height,
            b.x,
            b.y,
            b.width,
            b.height
        );
    }

    #[test]
    fn test_subgraph_bounds_maps_rects() {
        use crate::graph::Subgraph;

        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "G".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );

        let mut layout_bounds = HashMap::new();
        layout_bounds.insert(
            "sg1".to_string(),
            Rect {
                x: 10.0,
                y: 10.0,
                width: 5.0,
                height: 3.0,
            },
        );

        let config = TextLayoutConfig {
            padding: 0,
            left_label_margin: 0,
            ..TextLayoutConfig::default()
        };

        let transform = CoordTransform {
            scale_x: 1.0,
            scale_y: 1.0,
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            max_overhang_x: 0,
            max_overhang_y: 0,
            config: &config,
        };

        let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

        let b = &result["sg1"];
        // Title "G" requires min width = len("G") + 6 = 7, which exceeds rect width 5.
        // Title-width enforcement expands by (7-5)=2 and shifts x left by 2/2=1.
        assert_eq!(b.x, 9, "x shifted left by 1 due to title-width expansion");
        assert_eq!(b.y, 10, "y should match layout rect y");
        assert_eq!(b.width, 7, "width expanded to fit title");
        assert_eq!(b.height, 3, "height should match layout rect height");
    }

    // =========================================================================
    // Title Width Enforcement Tests (Plan 0026, Task 2.3)
    // =========================================================================

    #[test]
    fn test_subgraph_bounds_expanded_for_title() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[This Is A Very Long Title]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        let bounds = layout
            .subgraph_bounds
            .values()
            .next()
            .expect("Expected subgraph bounds");

        // Border must be wide enough for: corners (2) + "─ " (2) + title + " ─" (2)
        let min_width = "This Is A Very Long Title".len() + 6;
        assert!(
            bounds.width >= min_width,
            "Border width {} too narrow for title (need >= {})",
            bounds.width,
            min_width
        );
    }

    #[test]
    fn test_titled_subgraph_creates_title_rank() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = r#"graph TD
    subgraph sg1[Processing]
        A[Step 1] --> B[Step 2]
    end"#;

        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        assert!(layout.subgraph_bounds.contains_key("sg1"));
        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(bounds.height > 0);
    }

    // =========================================================================
    // to_ascii_rect() Tests (Plan 0028, Task 1.1)
    // =========================================================================

    #[test]
    fn to_ascii_rect_at_layout_minimum() {
        // A rect centered at the layout minimum should produce draw coords near origin + padding
        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 30.0,
            scale_x: 0.2,
            scale_y: 0.1,
            overhang_x: 2,
            overhang_y: 1,
            padding: 1,
            left_label_margin: 0,
        };
        let rect = Rect {
            x: 50.0,
            y: 30.0,
            width: 40.0,
            height: 20.0,
        };
        let (_x, _y, w, h) = ctx.to_ascii_rect(&rect);
        assert!(w > 0, "width should be positive, got {w}");
        assert!(h > 0, "height should be positive, got {h}");
    }

    #[test]
    fn to_ascii_rect_offset_from_minimum() {
        // A rect offset from layout minimum should have proportionally offset draw coords
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            overhang_x: 0,
            overhang_y: 0,
            padding: 0,
            left_label_margin: 0,
        };
        let rect1 = Rect {
            x: 50.0,
            y: 50.0,
            width: 40.0,
            height: 20.0,
        };
        let rect2 = Rect {
            x: 100.0,
            y: 100.0,
            width: 40.0,
            height: 20.0,
        };
        let (x1, y1, _, _) = ctx.to_ascii_rect(&rect1);
        let (x2, y2, _, _) = ctx.to_ascii_rect(&rect2);
        assert!(x2 > x1, "rect2 should be further right: x2={x2} vs x1={x1}");
        assert!(y2 > y1, "rect2 should be further down: y2={y2} vs y1={y1}");
    }

    #[test]
    fn to_ascii_rect_dimensions_scale_with_layout_size() {
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.5,
            scale_y: 0.5,
            overhang_x: 0,
            overhang_y: 0,
            padding: 0,
            left_label_margin: 0,
        };
        let small = Rect {
            x: 50.0,
            y: 50.0,
            width: 20.0,
            height: 10.0,
        };
        let large = Rect {
            x: 50.0,
            y: 50.0,
            width: 60.0,
            height: 30.0,
        };
        let (_, _, w1, h1) = ctx.to_ascii_rect(&small);
        let (_, _, w2, h2) = ctx.to_ascii_rect(&large);
        assert!(
            w2 > w1,
            "larger rect should have larger width: w2={w2} vs w1={w1}"
        );
        assert!(
            h2 > h1,
            "larger rect should have larger height: h2={h2} vs h1={h1}"
        );
    }

    // =========================================================================
    // Non-overlap Tests (Plan 0028, Task 2.1)
    // =========================================================================

    #[test]
    fn stacked_subgraphs_do_not_overlap() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n\
            subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
            subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
            A --> C\nB --> D";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        let sg1 = &layout.subgraph_bounds["sg1"];
        let sg2 = &layout.subgraph_bounds["sg2"];

        let sg1_bottom = sg1.y + sg1.height;
        let sg2_bottom = sg2.y + sg2.height;

        // Determine which is "upper" and which is "lower"
        let (_upper, lower, upper_bottom) = if sg1.y < sg2.y {
            (sg1, sg2, sg1_bottom)
        } else {
            (sg2, sg1, sg2_bottom)
        };

        // Upper subgraph's bottom must be strictly above lower's top
        assert!(
            upper_bottom <= lower.y,
            "Subgraphs should not overlap vertically: upper bottom={upper_bottom}, lower top={}",
            lower.y
        );
    }

    // =========================================================================
    // Containment Tests (Plan 0028, Task 1.2)
    // =========================================================================

    #[test]
    fn subgraph_bounds_contain_member_node_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA[Node1]\nB[Node2]\nend\nA --> B";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
    }

    #[test]
    fn stacked_subgraph_bounds_contain_member_nodes_after_overlap_resolution() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n\
            subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
            subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
            A --> C\nB --> D";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout(&diagram, &TextLayoutConfig::default());

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
        assert_subgraph_contains_members(&layout, "sg2", &["C", "D"]);
    }

    fn assert_subgraph_contains_members(layout: &Layout, sg_id: &str, members: &[&str]) {
        let sg = &layout.subgraph_bounds[sg_id];
        let sg_right = sg.x + sg.width;
        let sg_bottom = sg.y + sg.height;

        for member_id in members {
            let nb = &layout.node_bounds[*member_id];
            let nb_right = nb.x + nb.width;
            let nb_bottom = nb.y + nb.height;

            assert!(
                sg.x <= nb.x,
                "{sg_id} left ({}) should be <= {member_id} left ({})",
                sg.x,
                nb.x
            );
            assert!(
                sg.y <= nb.y,
                "{sg_id} top ({}) should be <= {member_id} top ({})",
                sg.y,
                nb.y
            );
            assert!(
                sg_right >= nb_right,
                "{sg_id} right ({sg_right}) should be >= {member_id} right ({nb_right})"
            );
            assert!(
                sg_bottom >= nb_bottom,
                "{sg_id} bottom ({sg_bottom}) should be >= {member_id} bottom ({nb_bottom})"
            );
        }
    }

    // =========================================================================
    // Direction Override: Field Plumbing (Phase 4, Task 4.1)
    // =========================================================================

    #[test]
    fn direction_override_field_available_at_layout() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // Direction override is present on the subgraph
        assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::LeftRight));

        // Layout computation succeeds without panic
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);
        assert!(!layout.node_bounds.is_empty());
    }

    #[test]
    fn direction_override_none_when_not_specified() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // No direction override: field should be None
        assert_eq!(diagram.subgraphs["sg1"].dir, None);
    }

    // =========================================================================
    // Direction Override Sub-Layout Tests (Phase 4, Tasks 4.2-4.4)
    // =========================================================================

    /// Helper: compute a sub-layout for a direction-override subgraph.
    /// Returns the LayoutResult for just the subgraph's internal nodes/edges.
    fn run_sublayout_for_sg(diagram: &Diagram, sg_id: &str) -> layered::LayoutResult {
        let sg = &diagram.subgraphs[sg_id];
        let sub_dir = sg.dir.expect("subgraph should have direction override");

        let layered_direction = match sub_dir {
            Direction::TopDown => LayeredDirection::TopBottom,
            Direction::BottomTop => LayeredDirection::BottomTop,
            Direction::LeftRight => LayeredDirection::LeftRight,
            Direction::RightLeft => LayeredDirection::RightLeft,
        };

        let mut sub_graph: layered::DiGraph<(f64, f64)> = layered::DiGraph::new();

        // Add leaf nodes (not child subgraphs)
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id)
                && let Some(node) = diagram.nodes.get(node_id)
            {
                let (w, h) = node_dimensions(node, sub_dir);
                sub_graph.add_node(node_id.as_str(), (w as f64, h as f64));
            }
        }

        // Add internal edges
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        for edge in &diagram.edges {
            if sg_node_set.contains(edge.from.as_str()) && sg_node_set.contains(edge.to.as_str()) {
                sub_graph.add_edge(edge.from.as_str(), edge.to.as_str());
            }
        }

        let sub_config = LayeredConfig {
            direction: layered_direction,
            ..LayeredConfig::default()
        };

        layered::layout(&sub_graph, &sub_config, |_, dims| *dims)
    }

    #[test]
    fn sublayout_lr_nodes_arranged_horizontally() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        // In LR layout, nodes should be arranged horizontally (increasing x, similar y)
        let a = &result.nodes[&layered::NodeId::from("A")];
        let b = &result.nodes[&layered::NodeId::from("B")];
        let c = &result.nodes[&layered::NodeId::from("C")];

        // Centers should have increasing x
        let a_cx = a.x + a.width / 2.0;
        let b_cx = b.x + b.width / 2.0;
        let c_cx = c.x + c.width / 2.0;

        assert!(
            a_cx < b_cx,
            "A center_x ({a_cx}) should be < B center_x ({b_cx})"
        );
        assert!(
            b_cx < c_cx,
            "B center_x ({b_cx}) should be < C center_x ({c_cx})"
        );

        // Centers should have similar y (within tolerance for same-rank nodes)
        let a_cy = a.y + a.height / 2.0;
        let b_cy = b.y + b.height / 2.0;
        let c_cy = c.y + c.height / 2.0;

        assert!(
            (a_cy - b_cy).abs() < 1.0,
            "A and B should be at similar y: {a_cy} vs {b_cy}"
        );
        assert!(
            (b_cy - c_cy).abs() < 1.0,
            "B and C should be at similar y: {b_cy} vs {c_cy}"
        );
    }

    #[test]
    fn sublayout_dimensions_wider_than_tall_for_lr() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        assert!(
            result.width > result.height,
            "LR sub-layout should be wider than tall: {}x{}",
            result.width,
            result.height
        );
    }

    #[test]
    fn sublayout_bt_nodes_arranged_bottom_to_top() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Start] --> B[End]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        let a = &result.nodes[&layered::NodeId::from("A")];
        let b = &result.nodes[&layered::NodeId::from("B")];

        // BT: A should be below B (higher y means lower on screen)
        let a_cy = a.y + a.height / 2.0;
        let b_cy = b.y + b.height / 2.0;

        assert!(
            a_cy > b_cy,
            "In BT layout, A (start) should be below B (end): A_cy={a_cy} B_cy={b_cy}"
        );
    }

    #[test]
    fn sublayout_rl_reverses_node_order() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Reverse]\ndirection RL\nA[Left] --> B[Right]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let a = layout.get_bounds("A").unwrap();
        let b = layout.get_bounds("B").unwrap();

        // RL: A (start) should be RIGHT of B (end) since flow goes right-to-left
        assert!(
            a.center_x() > b.center_x(),
            "In RL layout, A should be right of B: A_cx={} B_cx={}",
            a.center_x(),
            b.center_x()
        );

        // Both should be at similar y
        let y_tolerance = 2;
        assert!(
            (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
            "A and B should be at similar y in RL: {} vs {}",
            a.center_y(),
            b.center_y()
        );
    }

    #[test]
    fn direction_override_nodes_horizontal_in_final_layout() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal Section]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let a = layout.get_bounds("A").unwrap();
        let b = layout.get_bounds("B").unwrap();
        let c = layout.get_bounds("C").unwrap();

        // In an LR subgraph within a TD parent:
        // A, B, C should be arranged horizontally (increasing x, similar y)
        assert!(
            a.center_x() < b.center_x(),
            "A ({}) should be left of B ({})",
            a.center_x(),
            b.center_x()
        );
        assert!(
            b.center_x() < c.center_x(),
            "B ({}) should be left of C ({})",
            b.center_x(),
            c.center_x()
        );

        // All should be at similar y (within a small tolerance for rounding)
        let y_tolerance = 2;
        assert!(
            (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
            "A and B should be at similar y: {} vs {}",
            a.center_y(),
            b.center_y()
        );
        assert!(
            (b.center_y() as isize - c.center_y() as isize).abs() <= y_tolerance,
            "B and C should be at similar y: {} vs {}",
            b.center_y(),
            c.center_y()
        );
    }

    #[test]
    fn direction_override_subgraph_wider_than_tall() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        assert!(
            sg.width > sg.height,
            "LR subgraph should be wider than tall: {}x{}",
            sg.width,
            sg.height
        );
    }

    #[test]
    fn direction_override_bt_subgraph_taller_than_wide() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // BT subgraph inside an LR parent: subgraph should be taller than wide
        let input =
            "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Top] --> B[Mid] --> C[Bot]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        assert!(
            sg.height > sg.width,
            "BT subgraph should be taller than wide: {}w x {}h",
            sg.width,
            sg.height
        );
    }

    #[test]
    fn direction_override_subgraph_title_width_minimum() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // Subgraph with a long title should have bounds wide enough for the title
        let input =
            "graph TD\nsubgraph sg1[A Very Long Section Title]\ndirection LR\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        let title = "A Very Long Section Title";
        // Title with padding characters on either side
        assert!(
            sg.width >= title.len(),
            "Subgraph width ({}) should accommodate title length ({})",
            sg.width,
            title.len()
        );
    }

    #[test]
    fn direction_override_nodes_inside_subgraph_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B", "C"]);
    }

    #[test]
    fn direction_override_no_node_overlap() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        // Verify no overlap between A, B, C
        let nodes = ["A", "B", "C"];
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = layout.get_bounds(nodes[i]).unwrap();
                let b = layout.get_bounds(nodes[j]).unwrap();
                let overlap_x = a.x < b.x + b.width && b.x < a.x + a.width;
                let overlap_y = a.y < b.y + b.height && b.y < a.y + a.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "Nodes {} and {} should not overlap: {:?} vs {:?}",
                    nodes[i],
                    nodes[j],
                    (a.x, a.y, a.width, a.height),
                    (b.x, b.y, b.width, b.height)
                );
            }
        }
    }

    #[test]
    fn direction_override_external_nodes_outside_subgraph() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2]\nend\nStart --> A\nB --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];

        // Start and End should NOT be inside the subgraph bounds
        // (they are external to sg1)
        for ext_node in &["Start", "End"] {
            let bounds = layout.get_bounds(ext_node).unwrap();
            let inside_x = bounds.x >= sg.x && bounds.x + bounds.width <= sg.x + sg.width;
            let inside_y = bounds.y >= sg.y && bounds.y + bounds.height <= sg.y + sg.height;
            // At least one dimension should be outside
            assert!(
                !(inside_x && inside_y),
                "External node {} should not be fully inside sg1 bounds",
                ext_node
            );
        }
    }

    // =========================================================================
    // Cross-Boundary Edge Routing (Phase 4, Task 4.5)
    // =========================================================================

    #[test]
    fn cross_boundary_edge_no_panic() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;
        use crate::render::{RenderOptions, render};

        let input =
            "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let options = RenderOptions::default();

        // Full render pipeline should not panic
        let output = render(&diagram, &options);
        assert!(output.contains("A"));
        assert!(output.contains("B"));
        assert!(output.contains("C"));
        assert!(output.contains("D"));
        assert!(output.contains("Horizontal"));
    }

    #[test]
    fn node_effective_direction_populated() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        // Nodes inside the LR subgraph should have LR effective direction
        assert_eq!(
            layout.node_directions.get("A"),
            Some(&Direction::LeftRight),
            "A should have LR direction"
        );
        assert_eq!(
            layout.node_directions.get("B"),
            Some(&Direction::LeftRight),
            "B should have LR direction"
        );

        // Nodes outside the subgraph should have the parent direction (TD)
        assert_eq!(
            layout.node_directions.get("C"),
            Some(&Direction::TopDown),
            "C should have TD direction"
        );
        assert_eq!(
            layout.node_directions.get("D"),
            Some(&Direction::TopDown),
            "D should have TD direction"
        );
    }

    #[test]
    fn sublayout_excludes_cross_boundary_edges() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input =
            "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nStart --> A\nB --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        // Sub-layout should only have A and B, not Start or End
        assert!(result.nodes.contains_key(&layered::NodeId::from("A")));
        assert!(result.nodes.contains_key(&layered::NodeId::from("B")));
        assert!(!result.nodes.contains_key(&layered::NodeId::from("Start")));
        assert!(!result.nodes.contains_key(&layered::NodeId::from("End")));
    }

    #[test]
    fn compute_sublayouts_skips_non_isolated_when_flag_set() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // sg1 has direction LR but cross-boundary edge C --> A
        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layered_config = LayeredConfig::default(); // direction = TopBottom

        // With flag false: sublayout uses override direction (LR)
        let subs_false = compute_sublayouts(
            &diagram,
            &layered_config,
            |_node| (40.0, 20.0),
            |_edge| None,
            false,
        );
        let lr_result = &subs_false["sg1"];
        let a_lr = lr_result.result.nodes[&layered::NodeId::from("A")];
        let b_lr = lr_result.result.nodes[&layered::NodeId::from("B")];
        // LR: A and B should be side-by-side (different x, similar y)
        assert!(
            (a_lr.y - b_lr.y).abs() < 1.0,
            "LR: A.y={} B.y={} should be similar",
            a_lr.y,
            b_lr.y
        );

        // With flag true: non-isolated override is skipped entirely.
        let subs_true = compute_sublayouts(
            &diagram,
            &layered_config,
            |_node| (40.0, 20.0),
            |_edge| None,
            true,
        );
        assert!(
            !subs_true.contains_key("sg1"),
            "non-isolated sublayout should be skipped"
        );
    }
}
