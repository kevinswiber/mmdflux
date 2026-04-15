//! Derived grid layout builder for graph-family geometry.
//!
//! This is the bridge between the engine pipeline (which produces float
//! coordinates via `MeasurementMode::Grid`) and downstream grid-space replay.
//!
//! All phases (B-N) are implemented inline, reading directly from
//! `GraphGeometry` and its graph-owned grid projection data. Direction-override subgraphs
//! are handled by Phase M (sublayout reconciliation).

mod override_sublayouts;
mod quantize;
mod subgraph_bounds;
#[cfg(test)]
mod tests;
mod waypoints;

use std::collections::{HashMap, HashSet};

use override_sublayouts::{
    align_cross_boundary_siblings_draw, compact_override_subgraph_vertical_gaps,
    layout_compound_parent_members, reconcile_sublayouts_draw, resolve_sibling_overlaps_draw,
};
use quantize::{
    collision_repair, compute_grid_positions, compute_grid_scale_factors, compute_layer_starts,
    rank_gap_repair,
};
#[cfg(test)]
use subgraph_bounds::build_children_map;
use subgraph_bounds::{
    clip_and_repair_override_subgraph_bounds, ensure_external_edge_spacing,
    ensure_subgraph_contains_members, expand_parent_subgraph_bounds,
    expand_subgraphs_for_edge_labels, expand_subgraphs_for_node_collisions,
    shrink_subgraph_horizontal_gaps, shrink_subgraph_vertical_gaps, subgraph_bounds_to_draw,
};
use waypoints::{
    clip_waypoints_to_subgraph, nudge_colliding_waypoints, transform_label_positions_direct,
    transform_waypoints_direct,
};

use super::GridLayoutConfig;
use super::layout::{
    CoordTransform, GridLayout, NodeBounds, RawCenter, SelfEdgeDrawData, TransformContext,
};
use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::measure::grid_node_dimensions;
use crate::graph::space::FRect;
use crate::graph::{Direction, Edge, Graph, Shape};

type DrawPath = Vec<(usize, usize)>;
type DrawPathPair = (DrawPath, DrawPath);
type IndexedDrawPathPair = (usize, DrawPath, usize, DrawPath);

const BACKWARD_ROUTE_GAP: usize = 2;

fn effective_rank_sep(diagram: &Graph, config: &GridLayoutConfig) -> f64 {
    let mut rank_sep = config.rank_sep;
    if diagram.has_subgraphs() && config.cluster_rank_sep > 0.0 {
        // The layered solve expands rank separation for cluster graphs.
        // Grid replay must mirror that effective spacing or text output grows
        // relative to the unchanged float geometry.
        rank_sep += config.cluster_rank_sep;
    }
    rank_sep
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

/// Convert engine-produced `GraphGeometry` (with optional routed edge paths)
/// to an integer-coordinate `GridLayout`.
pub fn geometry_to_grid_layout_with_routed(
    diagram: &Graph,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    config: &GridLayoutConfig,
) -> GridLayout {
    let is_vertical = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);
    let direction = diagram.direction;

    // --- Phase B: Group nodes into layers ---

    let subgraph_ids: HashSet<&str> = diagram.subgraphs.keys().map(|s| s.as_str()).collect();

    let mut layer_coords: Vec<(String, f64, f64)> = geometry
        .nodes
        .iter()
        .filter(|(id, _)| !subgraph_ids.contains(id.as_str()))
        .map(|(id, pos_node)| {
            let primary = if is_vertical {
                pos_node.rect.y
            } else {
                pos_node.rect.x
            };
            let secondary = if is_vertical {
                pos_node.rect.x
            } else {
                pos_node.rect.y
            };
            (id.clone(), primary, secondary)
        })
        .collect();
    layer_coords.sort_by(|a, b| a.1.total_cmp(&b.1));

    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut current_layer: Vec<String> = Vec::new();
    let mut last_primary: Option<f64> = None;
    for (id, primary, _) in &layer_coords {
        if let Some(last) = last_primary
            && (*primary - last).abs() > 25.0
            && !current_layer.is_empty()
        {
            layers.push(std::mem::take(&mut current_layer));
        }
        current_layer.push(id.clone());
        last_primary = Some(*primary);
    }
    if !current_layer.is_empty() {
        layers.push(current_layer);
    }

    let secondary_coord = |id: &String| -> f64 {
        geometry
            .nodes
            .get(id)
            .map(|n| if is_vertical { n.rect.x } else { n.rect.y })
            .unwrap_or(0.0)
    };
    for layer in &mut layers {
        layer.sort_by(|a, b| secondary_coord(a).total_cmp(&secondary_coord(b)));
    }

    let grid_positions = compute_grid_positions(&layers);

    // --- Phase C: Compute node dimensions ---
    let node_dims: HashMap<String, (usize, usize)> = diagram
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), grid_node_dimensions(node, direction)))
        .collect();

    // --- Phase D: Scale layout coordinates to ASCII ---
    let ranks_doubled_for_scale = false;
    let (scale_x, scale_y) = compute_grid_scale_factors(
        &node_dims,
        effective_rank_sep(diagram, config),
        config.node_sep,
        config.v_spacing,
        config.h_spacing,
        is_vertical,
        ranks_doubled_for_scale,
    );

    // Find layout bounding box min
    let mut layout_min_x = geometry
        .nodes
        .values()
        .map(|n| n.rect.x)
        .fold(f64::INFINITY, f64::min);
    let mut layout_min_y = geometry
        .nodes
        .values()
        .map(|n| n.rect.y)
        .fold(f64::INFINITY, f64::min);

    if !geometry.subgraphs.is_empty() {
        let sg_min_x = geometry
            .subgraphs
            .values()
            .map(|sg| sg.rect.x)
            .fold(f64::INFINITY, f64::min);
        let sg_min_y = geometry
            .subgraphs
            .values()
            .map(|sg| sg.rect.y)
            .fold(f64::INFINITY, f64::min);
        layout_min_x = layout_min_x.min(sg_min_x);
        layout_min_y = layout_min_y.min(sg_min_y);
    }

    // Scale each node's center, compute overhang
    let mut raw_centers: Vec<RawCenter> = Vec::new();
    let mut max_overhang_x: usize = 0;
    let mut max_overhang_y: usize = 0;

    for (node_id, pos_node) in &geometry.nodes {
        if let Some(&(w, h)) = node_dims.get(node_id.as_str()) {
            let cx = ((pos_node.rect.x + pos_node.rect.width / 2.0 - layout_min_x) * scale_x)
                .round() as usize;
            let cy = ((pos_node.rect.y + pos_node.rect.height / 2.0 - layout_min_y) * scale_y)
                .round() as usize;
            if w / 2 > cx {
                max_overhang_x = max_overhang_x.max(w / 2 - cx);
            }
            if h / 2 > cy {
                max_overhang_y = max_overhang_y.max(h / 2 - cy);
            }
            raw_centers.push(RawCenter {
                id: node_id.clone(),
                cx,
                cy,
                w,
                h,
            });
        }
    }

    // Apply overhang offset and compute draw positions
    let mut draw_positions: HashMap<String, (usize, usize)> = HashMap::new();
    let mut node_bounds: HashMap<String, NodeBounds> = HashMap::new();

    for rc in &raw_centers {
        let center_x = rc.cx + max_overhang_x;
        let center_y = rc.cy + max_overhang_y;

        let x = center_x - rc.w / 2 + config.padding + config.left_label_margin;
        let y = center_y - rc.h / 2 + config.padding;

        draw_positions.insert(rc.id.clone(), (x, y));
        node_bounds.insert(
            rc.id.clone(),
            NodeBounds {
                x,
                y,
                width: rc.w,
                height: rc.h,
                layout_center_x: Some(center_x + config.padding + config.left_label_margin),
                layout_center_y: Some(center_y + config.padding),
            },
        );
    }

    // --- Phase E: Collision repair ---
    collision_repair(
        &layers,
        &mut draw_positions,
        &node_dims,
        is_vertical,
        if is_vertical {
            config.h_spacing
        } else {
            config.v_spacing
        },
    );
    rank_gap_repair(
        &layers,
        &mut draw_positions,
        &node_dims,
        is_vertical,
        if is_vertical {
            config.v_spacing
        } else {
            config.h_spacing
        },
    );

    // Update node_bounds after collision repair
    for (id, &(x, y)) in &draw_positions {
        if let Some(&(w, h)) = node_dims.get(id.as_str()) {
            let prev = node_bounds.get(id);
            let layout_center_x = prev.and_then(|b| b.layout_center_x);
            let layout_center_y = prev.and_then(|b| b.layout_center_y);
            node_bounds.insert(
                id.clone(),
                NodeBounds {
                    x,
                    y,
                    width: w,
                    height: h,
                    layout_center_x,
                    layout_center_y,
                },
            );
        }
    }

    // --- Phase F: Compute canvas size ---
    // Detect backward edges using two complementary methods:
    // 1. Cycle-reversed edges (tracked by the acyclic/DFS step in the layered layout).
    // 2. Position-based visual backward edges: edges that go against the layout direction
    //    due to cross-graph rank constraints (e.g. a long cross-edge forces a node to a
    //    higher rank, making its shorter outgoing edges appear to go "upward"). These are
    //    not cycles so the DFS does not mark them in reversed_edges, yet the downstream
    //    grid router's
    //    is_backward_edge() detects them by position and routes them to the right/bottom,
    //    requiring the same extra canvas margin as cycle-reversed backward edges.
    let has_backward_edges = !geometry.reversed_edges.is_empty() || {
        diagram.edges.iter().any(|edge| {
            match (node_bounds.get(&edge.from), node_bounds.get(&edge.to)) {
                (Some(from_b), Some(to_b)) => is_backward_edge(from_b, to_b, diagram.direction),
                _ => false,
            }
        })
    };
    let backward_margin = if has_backward_edges {
        BACKWARD_ROUTE_GAP + 2
    } else {
        0
    };

    let base_width = node_bounds
        .values()
        .map(|b| b.x + b.width)
        .max()
        .unwrap_or(0)
        + config.padding
        + config.right_label_margin;
    let base_height = node_bounds
        .values()
        .map(|b| b.y + b.height)
        .max()
        .unwrap_or(0)
        + config.padding;

    let (width, height) = if is_vertical {
        (base_width + backward_margin, base_height)
    } else {
        (base_width, base_height + backward_margin)
    };

    // --- Phase G: Rank-to-draw mapping ---
    let grid_projection = geometry.grid_projection.as_ref();
    let layer_starts = grid_projection
        .map(|projection| compute_layer_starts(&projection.node_ranks, &node_bounds, is_vertical))
        .unwrap_or_default();

    // --- Phase H: Transform waypoints and labels ---
    let ctx = TransformContext {
        layout_min_x,
        layout_min_y,
        scale_x,
        scale_y,
        padding: config.padding,
        left_label_margin: config.left_label_margin,
        overhang_x: max_overhang_x,
        overhang_y: max_overhang_y,
    };

    let edge_waypoints_converted = grid_projection
        .map(|projection| {
            transform_waypoints_direct(
                &projection.edge_waypoints,
                &diagram.edges,
                &ctx,
                &layer_starts,
                is_vertical,
                width,
                height,
            )
        })
        .unwrap_or_default();
    let mut routed_edge_paths: HashMap<usize, DrawPath> = HashMap::new();
    let mut preserve_routed_path_topology: HashSet<usize> = HashSet::new();
    if let Some(routed) = routed {
        for edge in &routed.edges {
            if edge.path.is_empty() {
                continue;
            }
            let mut converted: DrawPath = Vec::with_capacity(edge.path.len());
            for point in &edge.path {
                converted.push(ctx.to_grid(point.x, point.y));
            }
            converted.dedup();
            if converted.len() >= 2 {
                routed_edge_paths.insert(edge.index, converted);
                if edge.preserve_orthogonal_topology {
                    preserve_routed_path_topology.insert(edge.index);
                }
            }
        }
    }
    spread_colocated_backward_draw_path_sources(
        routed.map(|r| r.edges.as_slice()),
        &node_bounds,
        &mut routed_edge_paths,
        direction,
    );
    spread_colocated_backward_draw_path_targets(
        routed.map(|r| r.edges.as_slice()),
        &node_bounds,
        &mut routed_edge_paths,
        direction,
    );
    deconflict_backward_corridor_columns(
        routed.map(|r| r.edges.as_slice()),
        &mut routed_edge_paths,
        direction,
    );
    compact_vertical_criss_cross_draw_paths(
        diagram,
        &node_bounds,
        &mut routed_edge_paths,
        &preserve_routed_path_topology,
    );
    compact_horizontal_criss_cross_draw_paths(
        diagram,
        &node_bounds,
        &mut routed_edge_paths,
        &mut preserve_routed_path_topology,
    );

    let mut edge_label_positions = grid_projection
        .map(|projection| {
            transform_label_positions_direct(
                &projection.label_positions,
                &diagram.edges,
                &ctx,
                &layer_starts,
                is_vertical,
                width,
                height,
            )
        })
        .unwrap_or_default();

    // --- Phase I: Strip layout waypoints from backward edges ---
    // When ranks are doubled (labels present), backward edges get inflated layout
    // waypoints from normalization dummies. Strip them so the router falls through
    // to synthetic compact routing via generate_backward_waypoints().
    let mut edge_waypoints = edge_waypoints_converted;
    const BACKWARD_WAYPOINT_STRIP_THRESHOLD: usize = 6;
    // The engine always doubles minlen for edge labels (ranks_doubled_for_layers=true).
    if is_vertical {
        for edge in &diagram.edges {
            if let (Some(from_b), Some(to_b)) =
                (node_bounds.get(&edge.from), node_bounds.get(&edge.to))
                && is_backward_edge(from_b, to_b, diagram.direction)
                && edge_waypoints
                    .get(&edge.index)
                    .is_some_and(|wps| wps.len() >= BACKWARD_WAYPOINT_STRIP_THRESHOLD)
            {
                edge_waypoints.remove(&edge.index);
            }
        }
    }

    // --- Phase I.5: Nudge waypoints that collide with nodes ---
    nudge_colliding_waypoints(
        &mut edge_waypoints,
        &node_bounds,
        is_vertical,
        width,
        height,
    );

    // --- Phase J: Collect node shapes ---
    let node_shapes: HashMap<String, Shape> = diagram
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node.shape))
        .collect();

    // --- Phase K: Convert subgraph bounds to draw coordinates ---
    let coord_transform = CoordTransform {
        scale_x,
        scale_y,
        layout_min_x,
        layout_min_y,
        max_overhang_x,
        max_overhang_y,
        config,
    };
    let layout_sg_bounds: HashMap<String, FRect> = geometry
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.clone(), sg.rect))
        .collect();
    let mut subgraph_bounds =
        subgraph_bounds_to_draw(&diagram.subgraphs, &layout_sg_bounds, &coord_transform);
    shrink_subgraph_vertical_gaps(
        &diagram.subgraphs,
        &diagram.edges,
        &node_bounds,
        &mut subgraph_bounds,
        diagram.direction,
    );
    shrink_subgraph_horizontal_gaps(
        &diagram.subgraphs,
        &diagram.edges,
        &node_bounds,
        &mut subgraph_bounds,
        diagram.direction,
    );

    // Ensure subgraph bounds contain all member nodes after coordinate
    // transformation and shrink passes (rounding can cause 1-2 char losses).
    ensure_subgraph_contains_members(diagram, &node_bounds, &mut subgraph_bounds);

    // --- Phase K.5: Expand subgraphs for edge labels ---
    // Edge labels in narrow subgraphs (especially concurrent regions) can
    // overflow column boundaries.  Expand each region to fit its widest
    // label and shift subsequent sibling regions to the right.
    let shifted = expand_subgraphs_for_edge_labels(
        diagram,
        &mut node_bounds,
        &mut draw_positions,
        &mut subgraph_bounds,
    );
    if !shifted.is_empty() {
        for edge in &diagram.edges {
            if shifted.contains(&edge.from) || shifted.contains(&edge.to) {
                edge_waypoints.remove(&edge.index);
                routed_edge_paths.remove(&edge.index);
                edge_label_positions.remove(&edge.index);
            }
        }
    }

    // --- Phase L: Compute self-edge loop paths in draw coordinates ---
    let self_edges: Vec<SelfEdgeDrawData> = geometry
        .self_edges
        .iter()
        .filter_map(|se| {
            let bounds = node_bounds.get(&se.node_id)?;
            let loop_extent = 3;

            let points = match diagram.direction {
                Direction::TopDown => {
                    let right = bounds.x + bounds.width;
                    let loop_x = right + loop_extent;
                    let top_y = bounds.y;
                    let bot_y = bounds.y + bounds.height - 1;
                    vec![
                        (right, top_y),
                        (loop_x, top_y),
                        (loop_x, bot_y),
                        (right, bot_y),
                    ]
                }
                Direction::BottomTop => {
                    let right = bounds.x + bounds.width;
                    let loop_x = right + loop_extent;
                    let top_y = bounds.y;
                    let bot_y = bounds.y + bounds.height - 1;
                    vec![
                        (right, bot_y),
                        (loop_x, bot_y),
                        (loop_x, top_y),
                        (right, top_y),
                    ]
                }
                Direction::LeftRight => {
                    let bot = bounds.y + bounds.height;
                    let loop_y = bot + loop_extent;
                    let left_x = bounds.x;
                    let right_x = bounds.x + bounds.width - 1;
                    vec![
                        (right_x, bot),
                        (right_x, loop_y),
                        (left_x, loop_y),
                        (left_x, bot),
                    ]
                }
                Direction::RightLeft => {
                    let bot = bounds.y + bounds.height;
                    let loop_y = bot + loop_extent;
                    let left_x = bounds.x;
                    let right_x = bounds.x + bounds.width - 1;
                    vec![
                        (left_x, bot),
                        (left_x, loop_y),
                        (right_x, loop_y),
                        (right_x, bot),
                    ]
                }
            };

            Some(SelfEdgeDrawData {
                node_id: se.node_id.clone(),
                edge_index: se.edge_index,
                points,
            })
        })
        .collect();

    // Expand canvas to fit subgraph borders and self-edge loops
    let mut width = width;
    let mut height = height;
    for sb in subgraph_bounds.values() {
        width = width.max(sb.x + sb.width + config.padding);
        height = height.max(sb.y + sb.height + config.padding);
    }
    for se in &self_edges {
        for &(x, y) in &se.points {
            width = width.max(x + config.padding + 1);
            height = height.max(y + config.padding + 1);
        }
    }
    for points in routed_edge_paths.values() {
        for &(x, y) in points {
            width = width.max(x + config.padding + 1);
            height = height.max(y + config.padding + 1);
        }
    }

    // --- Phase M: Direction-override sub-layout reconciliation ---
    if let Some(projection) = grid_projection
        && !projection.override_subgraphs.is_empty()
    {
        reconcile_sublayouts_draw(
            diagram,
            config,
            &projection.override_subgraphs,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
            &mut width,
            &mut height,
        );

        expand_parent_subgraph_bounds(&diagram.subgraphs, &mut subgraph_bounds);

        // Re-layout parent override subgraphs that contain compound children.
        // The leaf-only sublayouts don't account for compound child space, so
        // this pass positions all direct children in the override direction.
        layout_compound_parent_members(
            diagram,
            &projection.override_subgraphs,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
            &mut width,
            &mut height,
        );

        expand_parent_subgraph_bounds(&diagram.subgraphs, &mut subgraph_bounds);

        resolve_sibling_overlaps_draw(
            diagram,
            &mut node_bounds,
            &mut draw_positions,
            &mut subgraph_bounds,
        );

        align_cross_boundary_siblings_draw(
            diagram,
            &mut node_bounds,
            &mut draw_positions,
            &mut subgraph_bounds,
        );

        expand_parent_subgraph_bounds(&diagram.subgraphs, &mut subgraph_bounds);

        // Compact excessive vertical gaps above top-level override subgraphs.
        compact_override_subgraph_vertical_gaps(
            diagram,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
        );

        // --- Phase N: Ensure external-edge spacing ---
        ensure_external_edge_spacing(
            diagram,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
        );

        // Invalidate/adjust waypoints for edges touching override subgraphs.
        let override_subgraph_ids: Vec<&String> = if !diagram.subgraph_order.is_empty() {
            diagram.subgraph_order.iter().collect()
        } else {
            let mut ids: Vec<&String> = diagram.subgraphs.keys().collect();
            ids.sort_by(|a, b| {
                diagram
                    .subgraph_depth(a)
                    .cmp(&diagram.subgraph_depth(b))
                    .then_with(|| a.cmp(b))
            });
            ids
        };
        for sg_id in override_subgraph_ids {
            let Some(sg) = diagram.subgraphs.get(sg_id) else {
                continue;
            };
            if sg.dir.is_none() {
                continue;
            }
            let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
            for edge in &diagram.edges {
                let from_in = sg_node_set.contains(edge.from.as_str());
                let to_in = sg_node_set.contains(edge.to.as_str());
                if !(from_in || to_in) {
                    continue;
                }
                let key = edge.index;
                if from_in && to_in {
                    edge_waypoints.remove(&key);
                    routed_edge_paths.remove(&key);
                } else if let Some(bounds) = subgraph_bounds.get(&sg.id)
                    && let Some(wps) = edge_waypoints.get(&key).cloned()
                {
                    let clipped = clip_waypoints_to_subgraph(&wps, bounds, from_in, to_in);
                    if to_in && !from_in {
                        let stale = node_bounds.get(&edge.from).is_some_and(|src_nb| {
                            clipped.last().is_some_and(|last| {
                                let src_cy = src_nb.y + src_nb.height / 2;
                                let src_cx = src_nb.x + src_nb.width / 2;
                                let on_top = last.1 == bounds.y;
                                let on_bottom =
                                    last.1 == bounds.y + bounds.height.saturating_sub(1);
                                let on_left = last.0 == bounds.x;
                                let on_right = last.0 == bounds.x + bounds.width.saturating_sub(1);
                                (src_cy < bounds.y && !on_top)
                                    || (src_cy > bounds.y + bounds.height && !on_bottom)
                                    || (src_cx < bounds.x && !on_left)
                                    || (src_cx > bounds.x + bounds.width && !on_right)
                            })
                        });
                        if stale {
                            // Keep the richer routed draw path even when the
                            // clipped normalization waypoints are stale. The
                            // downstream grid routing now repairs/filters those
                            // draw paths instead of dropping back to a direct
                            // centerline route for cross-subgraph edges.
                            edge_waypoints.remove(&key);
                        } else {
                            edge_waypoints.insert(key, clipped);
                        }
                    } else {
                        edge_waypoints.insert(key, clipped);
                    }
                } else {
                    routed_edge_paths.remove(&key);
                }
                edge_label_positions.remove(&key);
            }
        }

        // Re-expand canvas after Phase N shifts.
        for sb in subgraph_bounds.values() {
            width = width.max(sb.x + sb.width + config.padding);
            height = height.max(sb.y + sb.height + config.padding);
        }
        for nb in node_bounds.values() {
            width = width.max(nb.x + nb.width + config.padding);
            height = height.max(nb.y + nb.height + config.padding);
        }
        for points in routed_edge_paths.values() {
            for &(x, y) in points {
                width = width.max(x + config.padding + 1);
                height = height.max(y + config.padding + 1);
            }
        }
    }

    // Final pass: clip override subgraphs to actual member content, constrain
    // against sibling nodes, re-expand parents, and fix any remaining collisions.
    // Must run after ALL override-subgraph phases are done repositioning.
    clip_and_repair_override_subgraph_bounds(diagram, &node_bounds, &mut subgraph_bounds);
    expand_subgraphs_for_node_collisions(&diagram.subgraphs, &node_bounds, &mut subgraph_bounds);

    let node_directions = geometry.node_directions.clone();

    GridLayout {
        grid_positions,
        draw_positions,
        node_bounds,
        width,
        height,
        h_spacing: config.h_spacing,
        v_spacing: config.v_spacing,
        edge_waypoints,
        routed_edge_paths,
        preserve_routed_path_topology,
        edge_label_positions,
        node_shapes,
        subgraph_bounds,
        self_edges,
        node_directions,
    }
}

/// Spread backward draw path source points that collapsed to the same grid
/// cell during float→grid quantization.
///
/// The float-space routing spreads co-located backward source ports by
/// `MIN_PORT_SPACING` (12px), but a single grid cell spans ~18 float pixels,
/// so the spread can round to the same cell.  This pass detects co-located
/// backward draw path starts and shifts them by 1 grid row/column so they
/// render as distinct departure points in text output.
fn spread_colocated_backward_draw_path_sources(
    routed_edges: Option<&[crate::graph::geometry::RoutedEdgeGeometry]>,
    node_bounds: &HashMap<String, NodeBounds>,
    draw_paths: &mut HashMap<usize, DrawPath>,
    direction: Direction,
) {
    let Some(edges) = routed_edges else {
        return;
    };

    // Group backward edges by source node.
    let mut backward_by_source: HashMap<&str, Vec<usize>> = HashMap::new();
    for edge in edges {
        if edge.is_backward && draw_paths.contains_key(&edge.index) {
            backward_by_source
                .entry(&edge.from)
                .or_default()
                .push(edge.index);
        }
    }

    let is_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    for (source_id, indices) in &backward_by_source {
        if indices.len() <= 1 {
            continue;
        }
        let Some(bounds) = node_bounds.get(*source_id) else {
            continue;
        };

        // Sub-group by co-located start point.  Edges sharing the same
        // departure cell need spreading even when their corridor positions
        // differ — the visual departure point is still the same face cell.
        let mut colocated: Vec<Vec<usize>> = Vec::new();
        for &idx in indices {
            let Some(path) = draw_paths.get(&idx) else {
                continue;
            };
            if path.len() < 2 {
                continue;
            }
            let key = path[0];
            let found = colocated.iter_mut().find(|group| {
                draw_paths
                    .get(&group[0])
                    .is_some_and(|p| !p.is_empty() && p[0] == key)
            });
            if let Some(group) = found {
                group.push(idx);
            } else {
                colocated.push(vec![idx]);
            }
        }

        for group in &colocated {
            if group.len() <= 1 {
                continue;
            }

            // Sort by departure segment length (inner corridors first).
            let mut sorted: Vec<(usize, usize)> = group
                .iter()
                .filter_map(|&idx| {
                    let path = draw_paths.get(&idx)?;
                    if path.len() < 2 {
                        return None;
                    }
                    let (x0, y0) = path[0];
                    let (x1, y1) = path[1];
                    let dep_len = if is_vertical {
                        x1.abs_diff(x0)
                    } else {
                        y1.abs_diff(y0)
                    };
                    Some((idx, dep_len))
                })
                .collect();
            sorted.sort_by_key(|&(idx, len)| (len, idx));

            // Compute the spread zone on the face (1 grid cell per edge).
            let count = sorted.len();
            let face_center = if is_vertical {
                bounds.y + bounds.height / 2
            } else {
                bounds.x + bounds.width / 2
            };
            // Use the full face including border rows — backward edge
            // departures can attach at any position along the face,
            // matching the target-side spreading behaviour.
            let (face_min, face_max) = if is_vertical {
                (bounds.y, bounds.y + bounds.height.saturating_sub(1))
            } else {
                (bounds.x, bounds.x + bounds.width.saturating_sub(1))
            };

            // If face is too narrow to spread, skip.
            if face_max <= face_min || count > face_max - face_min + 1 {
                continue;
            }

            // Center the spread zone around face_center.
            let half = (count - 1) / 2;
            let zone_start = face_center.saturating_sub(half).max(face_min);
            let zone_end = (zone_start + count - 1).min(face_max);
            let zone_start = zone_end + 1 - count; // re-anchor if clamped

            for (slot, &(idx, _)) in sorted.iter().enumerate() {
                let new_coord = zone_start + slot;
                let Some(path) = draw_paths.get_mut(&idx) else {
                    continue;
                };
                if path.is_empty() {
                    continue;
                }
                let (old_x, old_y) = path[0];
                if is_vertical {
                    path[0] = (old_x, new_coord);
                    // Propagate to adjacent point if axis-aligned.
                    if path.len() >= 2 && path[1].1 == old_y {
                        path[1].1 = new_coord;
                    }
                } else {
                    path[0] = (new_coord, old_y);
                    if path.len() >= 2 && path[1].0 == old_x {
                        path[1].0 = new_coord;
                    }
                }
            }
        }
    }
}

/// Spread backward draw path target points that collapsed to the same grid
/// cell during float→grid quantization.
///
/// Mirrors `spread_colocated_backward_draw_path_sources` but operates on the
/// last path point (arrival at target) rather than the first (departure from
/// source).
fn spread_colocated_backward_draw_path_targets(
    routed_edges: Option<&[crate::graph::geometry::RoutedEdgeGeometry]>,
    node_bounds: &HashMap<String, NodeBounds>,
    draw_paths: &mut HashMap<usize, DrawPath>,
    direction: Direction,
) {
    let Some(edges) = routed_edges else {
        return;
    };

    // Group backward edges by target node.
    let mut backward_by_target: HashMap<&str, Vec<usize>> = HashMap::new();
    for edge in edges {
        if edge.is_backward && draw_paths.contains_key(&edge.index) {
            backward_by_target
                .entry(&edge.to)
                .or_default()
                .push(edge.index);
        }
    }

    let is_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    for (target_id, indices) in &backward_by_target {
        if indices.len() <= 1 {
            continue;
        }
        let Some(bounds) = node_bounds.get(*target_id) else {
            continue;
        };

        // Sub-group by co-located last point only — edges approaching from
        // different corridors still converge to the same arrival cell.
        let mut colocated: Vec<Vec<usize>> = Vec::new();
        for &idx in indices {
            let Some(path) = draw_paths.get(&idx) else {
                continue;
            };
            if path.len() < 2 {
                continue;
            }
            let n = path.len();
            let key = path[n - 1];
            let found = colocated.iter_mut().find(|group| {
                draw_paths
                    .get(&group[0])
                    .is_some_and(|p| !p.is_empty() && p[p.len() - 1] == key)
            });
            if let Some(group) = found {
                group.push(idx);
            } else {
                colocated.push(vec![idx]);
            }
        }

        for group in &colocated {
            if group.len() <= 1 {
                continue;
            }

            // Sort by source distance from target along the primary
            // axis — farther sources get ports nearer the start of the
            // face so approach segments nest without crossing.
            let descending = matches!(direction, Direction::TopDown | Direction::LeftRight);
            let mut sorted: Vec<(usize, usize)> = group
                .iter()
                .filter_map(|&idx| {
                    let source_id = edges.iter().find(|e| e.index == idx).map(|e| &e.from)?;
                    let source_bounds = node_bounds.get(source_id.as_str())?;
                    let source_main = if is_vertical {
                        source_bounds.y + source_bounds.height / 2
                    } else {
                        source_bounds.x + source_bounds.width / 2
                    };
                    Some((idx, source_main))
                })
                .collect();
            sorted.sort_by(|a, b| {
                let ord = a.1.cmp(&b.1);
                let ord = if descending { ord.reverse() } else { ord };
                ord.then_with(|| a.0.cmp(&b.0))
            });

            let count = sorted.len();
            let face_center = if is_vertical {
                bounds.y + bounds.height / 2
            } else {
                bounds.x + bounds.width / 2
            };
            // Use the full face including border rows — backward edge
            // arrivals can attach at any position along the face.
            let (face_min, face_max) = if is_vertical {
                (bounds.y, bounds.y + bounds.height.saturating_sub(1))
            } else {
                (bounds.x, bounds.x + bounds.width.saturating_sub(1))
            };

            if face_max <= face_min || count > face_max - face_min + 1 {
                continue;
            }

            let half = (count - 1) / 2;
            let zone_start = face_center.saturating_sub(half).max(face_min);
            let zone_end = (zone_start + count - 1).min(face_max);
            let zone_start = zone_end + 1 - count;

            for (slot, &(idx, _)) in sorted.iter().enumerate() {
                let new_coord = zone_start + slot;
                let Some(path) = draw_paths.get_mut(&idx) else {
                    continue;
                };
                let n = path.len();
                if n == 0 {
                    continue;
                }
                let (old_x, old_y) = path[n - 1];
                if is_vertical {
                    path[n - 1] = (old_x, new_coord);
                    if n >= 2 && path[n - 2].1 == old_y {
                        // Horizontal last segment — already normal to the
                        // vertical face.  Just propagate the new y.
                        path[n - 2].1 = new_coord;
                    } else if n >= 2 {
                        // Vertical last segment on a vertical face.  Push
                        // the corridor outward by 1 cell and insert a
                        // horizontal approach point.
                        let (pen_x, _pen_y) = path[n - 2];
                        let outward_x = if pen_x >= old_x {
                            pen_x.max(old_x + 1)
                        } else {
                            pen_x.min(old_x.saturating_sub(1))
                        };
                        path[n - 2].0 = outward_x;
                        path.insert(n - 1, (outward_x, new_coord));
                    }
                } else {
                    path[n - 1] = (new_coord, old_y);
                    if n >= 2 && path[n - 2].0 == old_x {
                        path[n - 2].0 = new_coord;
                    } else if n >= 2 {
                        let (_pen_x, pen_y) = path[n - 2];
                        let outward_y = if pen_y >= old_y {
                            pen_y.max(old_y + 1)
                        } else {
                            pen_y.min(old_y.saturating_sub(1))
                        };
                        path[n - 2].1 = outward_y;
                        path.insert(n - 1, (new_coord, outward_y));
                    }
                }
            }
        }
    }
}

/// Deconflict backward edge corridor columns that collapsed to the same grid
/// cell during float-to-grid conversion.
///
/// The float-space corridor deconfliction assigns lanes 8 px apart, but the
/// grid scale can map multiple lanes to the same draw column.  This function
/// detects same-target backward edges whose vertical corridor segments share
/// a grid column and spreads them 1 cell apart.
fn deconflict_backward_corridor_columns(
    routed_edges: Option<&[crate::graph::geometry::RoutedEdgeGeometry]>,
    draw_paths: &mut HashMap<usize, DrawPath>,
    direction: Direction,
) {
    let Some(edges) = routed_edges else {
        return;
    };

    let is_vertical = matches!(direction, Direction::TopDown | Direction::BottomTop);

    // Extract corridor info for ALL backward edges, regardless of target.
    // For TD/BT the corridor is the x-coordinate of the vertical segment
    // (path[1].0 == path[2].0 for a 4-point H-V-H path).
    // For LR/RL the corridor is the y-coordinate of the horizontal segment
    // (path[1].1 == path[2].1 for a 4-point V-H-V path).
    struct CorridorInfo {
        edge_index: usize,
        corridor_coord: usize,
        span: usize,
        /// Index of the first corridor point in the path.
        p1: usize,
        /// Index of the second corridor point in the path.
        p2: usize,
    }

    let mut infos: Vec<CorridorInfo> = Vec::new();
    for edge in edges {
        if !edge.is_backward || !draw_paths.contains_key(&edge.index) {
            continue;
        }
        let Some(path) = draw_paths.get(&edge.index) else {
            continue;
        };
        if path.len() < 4 {
            continue;
        }
        // Identify the corridor segment: two consecutive points sharing
        // the corridor axis coordinate.
        let mut found = None;
        for i in 0..path.len() - 1 {
            let shares_axis = if is_vertical {
                path[i].0 == path[i + 1].0 && path[i].1 != path[i + 1].1
            } else {
                path[i].1 == path[i + 1].1 && path[i].0 != path[i + 1].0
            };
            if shares_axis {
                let coord = if is_vertical { path[i].0 } else { path[i].1 };
                let span = if is_vertical {
                    path[i].1.abs_diff(path[i + 1].1)
                } else {
                    path[i].0.abs_diff(path[i + 1].0)
                };
                // Prefer the longest such segment (the corridor, not a stub).
                if found
                    .as_ref()
                    .is_none_or(|f: &(usize, usize, usize, usize)| span > f.3)
                {
                    found = Some((i, i + 1, coord, span));
                }
            }
        }
        if let Some((p1, p2, corridor_coord, span)) = found {
            infos.push(CorridorInfo {
                edge_index: edge.index,
                corridor_coord,
                span,
                p1,
                p2,
            });
        }
    }

    if infos.len() <= 1 {
        return;
    }

    // Sort by corridor coordinate, then by span ascending (shorter span =
    // inner slot) to match the float-level deconfliction order.
    infos.sort_by_key(|info| (info.corridor_coord, info.span, info.edge_index));

    // Minimum gap between adjacent corridor rows/columns.  Must be >= 2
    // so that the terminal vertical/horizontal stub and arrowhead of one
    // corridor don't land on the adjacent corridor's row, which would cause
    // arrow-collision label displacement.
    const MIN_CORRIDOR_GAP: usize = 2;

    let has_collision = infos
        .windows(2)
        .any(|w| w[1].corridor_coord < w[0].corridor_coord + MIN_CORRIDOR_GAP);
    if !has_collision {
        return;
    }

    // Sweep: assign distinct corridor coords with the required gap.  When
    // an edge's coord is too close to the previous assignment, push it
    // forward.  This handles both same-target and cross-target collisions,
    // and cascading pushes if spreading one group encroaches on the next.
    let mut mutations: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut last_assigned: Option<usize> = None;
    for info in &infos {
        let new_coord = match last_assigned {
            Some(last) if info.corridor_coord < last + MIN_CORRIDOR_GAP => last + MIN_CORRIDOR_GAP,
            _ => info.corridor_coord,
        };
        last_assigned = Some(new_coord);
        if new_coord != info.corridor_coord {
            mutations.push((info.edge_index, info.p1, info.p2, new_coord));
        }
    }

    for (edge_index, p1, p2, new_coord) in mutations {
        let path = draw_paths.get_mut(&edge_index).unwrap();
        if is_vertical {
            path[p1].0 = new_coord;
            path[p2].0 = new_coord;
        } else {
            path[p1].1 = new_coord;
            path[p2].1 = new_coord;
        }
    }
}

/// Compact criss-cross draw paths for LR/RL graphs by transposing coordinates
/// and reusing the TD compaction logic.
///
/// The crossing pattern is geometrically identical to TD — just rotated 90°.
/// Swap (x,y) → (y,x) in paths and bounds, run the vertical compaction (which
/// thinks it's processing a TD graph), then swap the results back.
fn compact_horizontal_criss_cross_draw_paths(
    diagram: &Graph,
    _node_bounds: &HashMap<String, NodeBounds>,
    routed_edge_paths: &mut HashMap<usize, DrawPath>,
    preserve_routed_path_topology: &mut HashSet<usize>,
) {
    if !matches!(
        diagram.direction,
        Direction::LeftRight | Direction::RightLeft
    ) {
        return;
    }

    // Detect LR criss-cross pairs: two 4-point H-V-H paths with opposite
    // vertical directions (one goes up, one goes down) sharing the same
    // vertical column.  Shift one path's vertical 2 cells to create a gap.
    let mut adjusted: HashSet<usize> = HashSet::new();

    for i in 0..diagram.edges.len() {
        let a = &diagram.edges[i];
        if a.from == a.to || a.from_subgraph.is_some() || a.to_subgraph.is_some() {
            continue;
        }
        if adjusted.contains(&a.index) {
            continue;
        }
        let Some(a_path) = routed_edge_paths.get(&a.index).cloned() else {
            continue;
        };
        if !is_horizontal_criss_cross_simple_path(&a_path) {
            continue;
        }

        for j in (i + 1)..diagram.edges.len() {
            let b = &diagram.edges[j];
            if b.from == b.to || b.from_subgraph.is_some() || b.to_subgraph.is_some() {
                continue;
            }
            if adjusted.contains(&b.index) {
                continue;
            }
            let Some(b_path) = routed_edge_paths.get(&b.index).cloned() else {
                continue;
            };
            if !is_horizontal_criss_cross_simple_path(&b_path) {
                continue;
            }

            // Both H-V-H. Check: same vertical column, opposite V directions,
            // and different sources AND different targets (true criss-cross,
            // not fan-out from the same node).
            if a.from == b.from || a.to == b.to {
                continue;
            }
            let a_v_x = a_path[1].0;
            let b_v_x = b_path[1].0;
            if a_v_x != b_v_x {
                continue;
            }
            let a_goes_down = a_path[2].1 > a_path[1].1;
            let b_goes_down = b_path[2].1 > b_path[1].1;
            if a_goes_down == b_goes_down {
                continue;
            }

            // Criss-cross found.  Shift the down-going edge's vertical 2
            // cells right so the two edges have visually distinct columns.
            let shift_idx = if a_goes_down { a.index } else { b.index };
            let shift_path = if a_goes_down { &a_path } else { &b_path };
            let new_x = shift_path[1].0 + 2;
            routed_edge_paths.insert(
                shift_idx,
                vec![
                    shift_path[0],
                    (new_x, shift_path[1].1),
                    (new_x, shift_path[2].1),
                    shift_path[3],
                ],
            );
            // Mark both edges for topology preservation so the text router
            // uses these draw paths instead of falling back to direct routing.
            preserve_routed_path_topology.insert(a.index);
            preserve_routed_path_topology.insert(b.index);
            adjusted.insert(a.index);
            adjusted.insert(b.index);
            break;
        }
    }
}

/// Check for a 4-point H-V-H draw path shape (horizontal-vertical-horizontal).
fn is_horizontal_criss_cross_simple_path(points: &[(usize, usize)]) -> bool {
    points.len() == 4
        && points[0].1 == points[1].1 // H: same y
        && points[1].0 == points[2].0 // V: same x
        && points[2].1 == points[3].1 // H: same y
        && points[0].0 != points[1].0 // H: different x
        && points[1].1 != points[2].1 // V: different y
        && points[2].0 != points[3].0 // H: different x
}

fn compact_vertical_criss_cross_draw_paths(
    diagram: &Graph,
    node_bounds: &HashMap<String, NodeBounds>,
    routed_edge_paths: &mut HashMap<usize, DrawPath>,
    preserve_routed_path_topology: &HashSet<usize>,
) {
    if !matches!(diagram.direction, Direction::TopDown | Direction::BottomTop) {
        return;
    }

    let mut adjusted_edges: HashSet<usize> = HashSet::new();

    for i in 0..diagram.edges.len() {
        let first = &diagram.edges[i];
        if !eligible_for_vertical_criss_cross_compaction(
            first,
            &adjusted_edges,
            preserve_routed_path_topology,
        ) {
            continue;
        }

        for second in diagram.edges.iter().skip(i + 1) {
            if !eligible_for_vertical_criss_cross_compaction(
                second,
                &adjusted_edges,
                preserve_routed_path_topology,
            ) {
                continue;
            }

            let Some(first_path) = routed_edge_paths.get(&first.index).cloned() else {
                continue;
            };
            let Some(second_path) = routed_edge_paths.get(&second.index).cloned() else {
                continue;
            };

            let compacted = compact_vertical_criss_cross_match(
                first,
                &first_path,
                second,
                &second_path,
                node_bounds,
                diagram.direction,
            );

            let Some((simple_idx, simple_points, detour_idx, detour_points)) = compacted else {
                continue;
            };

            routed_edge_paths.insert(simple_idx, simple_points);
            routed_edge_paths.insert(detour_idx, detour_points);
            adjusted_edges.insert(simple_idx);
            adjusted_edges.insert(detour_idx);
            break;
        }
    }
}

fn eligible_for_vertical_criss_cross_compaction(
    edge: &Edge,
    adjusted_edges: &HashSet<usize>,
    preserve_routed_path_topology: &HashSet<usize>,
) -> bool {
    !adjusted_edges.contains(&edge.index)
        && edge.from_subgraph.is_none()
        && edge.to_subgraph.is_none()
        && preserve_routed_path_topology.contains(&edge.index)
}

fn compact_vertical_criss_cross_match(
    first: &Edge,
    first_path: &[(usize, usize)],
    second: &Edge,
    second_path: &[(usize, usize)],
    node_bounds: &HashMap<String, NodeBounds>,
    direction: Direction,
) -> Option<IndexedDrawPathPair> {
    if is_vertical_criss_cross_simple_path(first_path)
        && is_vertical_criss_cross_detour_path(second_path)
        && forms_vertical_criss_cross_pair(
            first,
            first_path,
            second,
            second_path,
            node_bounds,
            direction,
        )
    {
        let simple_target = node_bounds.get(&first.to)?;
        return compact_vertical_criss_cross_pair(
            first_path,
            second_path,
            simple_target,
            direction,
        )
        .map(|(simple, detour)| (first.index, simple, second.index, detour));
    }

    if is_vertical_criss_cross_detour_path(first_path)
        && is_vertical_criss_cross_simple_path(second_path)
        && forms_vertical_criss_cross_pair(
            second,
            second_path,
            first,
            first_path,
            node_bounds,
            direction,
        )
    {
        let simple_target = node_bounds.get(&second.to)?;
        return compact_vertical_criss_cross_pair(
            second_path,
            first_path,
            simple_target,
            direction,
        )
        .map(|(simple, detour)| (second.index, simple, first.index, detour));
    }

    None
}

fn is_vertical_criss_cross_simple_path(points: &[(usize, usize)]) -> bool {
    if points.len() != 4 {
        return false;
    }

    point_delta(points[0].0, points[1].0) == 0
        && point_delta(points[1].1, points[2].1) == 0
        && point_delta(points[2].0, points[3].0) == 0
        && point_delta(points[0].1, points[1].1) != 0
        && point_delta(points[1].0, points[2].0) != 0
        && point_delta(points[2].1, points[3].1) != 0
        && point_delta(points[0].1, points[1].1) == point_delta(points[2].1, points[3].1)
}

fn is_vertical_criss_cross_detour_path(points: &[(usize, usize)]) -> bool {
    if points.len() != 6 {
        return false;
    }

    point_delta(points[0].0, points[1].0) == 0
        && point_delta(points[1].1, points[2].1) == 0
        && point_delta(points[2].0, points[3].0) == 0
        && point_delta(points[3].1, points[4].1) == 0
        && point_delta(points[4].0, points[5].0) == 0
        && point_delta(points[0].1, points[1].1) != 0
        && point_delta(points[1].0, points[2].0) != 0
        && point_delta(points[2].1, points[3].1) != 0
        && point_delta(points[3].0, points[4].0) != 0
        && point_delta(points[4].1, points[5].1) != 0
        && point_delta(points[0].1, points[1].1) == point_delta(points[2].1, points[3].1)
        && point_delta(points[2].1, points[3].1) == point_delta(points[4].1, points[5].1)
        && point_delta(points[1].0, points[2].0) == point_delta(points[3].0, points[4].0)
}

fn forms_vertical_criss_cross_pair(
    simple_edge: &Edge,
    simple_path: &[(usize, usize)],
    detour_edge: &Edge,
    detour_path: &[(usize, usize)],
    node_bounds: &HashMap<String, NodeBounds>,
    direction: Direction,
) -> bool {
    let Some(simple_src) = node_bounds.get(&simple_edge.from) else {
        return false;
    };
    let Some(simple_tgt) = node_bounds.get(&simple_edge.to) else {
        return false;
    };
    let Some(detour_src) = node_bounds.get(&detour_edge.from) else {
        return false;
    };
    let Some(detour_tgt) = node_bounds.get(&detour_edge.to) else {
        return false;
    };

    let vertical_sign = point_delta(simple_path[0].1, simple_path[1].1);
    let simple_horizontal_sign = point_delta(simple_path[1].0, simple_path[2].0);
    let detour_horizontal_sign = point_delta(detour_path[1].0, detour_path[2].0);
    if vertical_sign == 0 || simple_horizontal_sign == 0 || detour_horizontal_sign == 0 {
        return false;
    }

    let forward = match direction {
        Direction::TopDown => simple_src.y < simple_tgt.y && detour_src.y < detour_tgt.y,
        Direction::BottomTop => simple_src.y > simple_tgt.y && detour_src.y > detour_tgt.y,
        Direction::LeftRight | Direction::RightLeft => false,
    };
    if !forward {
        return false;
    }

    let sources_share_rank = simple_src.y == detour_src.y;
    let targets_share_rank = simple_tgt.y == detour_tgt.y;
    let sources_cross = simple_src.x > detour_src.x;
    let targets_cross = simple_tgt.x < detour_tgt.x;
    if !(sources_share_rank && targets_share_rank && sources_cross && targets_cross) {
        return false;
    }

    if simple_path[0].1 != detour_path[0].1 || simple_path[3].1 != detour_path[5].1 {
        return false;
    }
    let center_x = detour_path[2].0;
    let simple_crosses_center = simple_path[3].0 < center_x && simple_path[0].0 > center_x;
    let detour_covers_center =
        detour_path[1].0 < detour_path[2].0 && detour_path[2].0 < detour_path[4].0;
    let endpoints_cross =
        simple_path[0].0 > detour_path[0].0 && simple_path[3].0 < detour_path[5].0;
    if !(simple_crosses_center && detour_covers_center && endpoints_cross) {
        return false;
    }

    let center_y = simple_path[1].1;
    let upper_y = detour_path[1].1;
    let lower_y = detour_path[3].1;
    let vertical_order_ok = if vertical_sign > 0 {
        upper_y < center_y && center_y < lower_y
    } else {
        upper_y > center_y && center_y > lower_y
    };
    let collapsed_to_lower =
        simple_path_collapses_to_detour_lower_row(simple_path, detour_path, vertical_sign);
    if !(vertical_order_ok || collapsed_to_lower) {
        return false;
    }

    simple_horizontal_sign < 0 && detour_horizontal_sign > 0
}

fn compact_vertical_criss_cross_pair(
    simple_path: &[(usize, usize)],
    detour_path: &[(usize, usize)],
    simple_target_bounds: &NodeBounds,
    direction: Direction,
) -> Option<DrawPathPair> {
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) {
        return None;
    }

    let vertical_sign = point_delta(simple_path[0].1, simple_path[1].1) as isize;
    let simple_horizontal_sign = point_delta(simple_path[1].0, simple_path[2].0) as isize;
    let detour_horizontal_sign = point_delta(detour_path[3].0, detour_path[4].0) as isize;
    if vertical_sign == 0 || simple_horizontal_sign == 0 || detour_horizontal_sign == 0 {
        return None;
    }

    let collapsed_to_lower =
        simple_path_collapses_to_detour_lower_row(simple_path, detour_path, vertical_sign as i8);
    let center_y = if collapsed_to_lower {
        collapsed_vertical_criss_cross_center_y(detour_path, vertical_sign)?
    } else {
        simple_path[1].1
    };
    let detour_lower_y = if collapsed_to_lower {
        shift_axis(detour_path[3].1, -vertical_sign)?
    } else {
        let lower_gap = detour_path[3].1.abs_diff(center_y);
        if lower_gap < 2 {
            return None;
        }
        shift_axis(detour_path[3].1, -vertical_sign)?
    };
    let detour_run = detour_path[4].0.abs_diff(detour_path[3].0);
    if detour_run < 2 {
        return None;
    }

    let simple_shifted_x = shift_axis(simple_path[0].0, simple_horizontal_sign)?;
    let target_pull = if detour_run >= 4 { 2 } else { 1 } as isize;
    let detour_target_x = shift_axis(detour_path[4].0, -detour_horizontal_sign * target_pull)?;

    let center_x = detour_path[2].0 as isize;
    let simple_shifted_x_i = simple_shifted_x as isize;
    let detour_target_x_i = detour_target_x as isize;
    if simple_shifted_x_i <= center_x || detour_target_x_i <= center_x {
        return None;
    }
    if detour_target_x_i <= center_x + 1 {
        return None;
    }

    let center_y_i = center_y as isize;
    let detour_lower_y_i = detour_lower_y as isize;
    if (vertical_sign > 0 && detour_lower_y_i <= center_y_i)
        || (vertical_sign < 0 && detour_lower_y_i >= center_y_i)
    {
        return None;
    }

    let simple_stem_y = shift_axis(center_y, -vertical_sign)?;
    let compact_simple_target = should_compact_vertical_criss_cross_simple_target(
        simple_shifted_x,
        detour_path[2].0,
        simple_path[3].0,
    );
    let simple_target_x = if compact_simple_target {
        compact_vertical_criss_cross_target_x(
            simple_target_bounds,
            detour_path[2].0,
            simple_horizontal_sign,
        )?
    } else {
        simple_path[3].0
    };
    let simple = vec![
        (simple_shifted_x, simple_path[0].1),
        (simple_shifted_x, simple_stem_y),
        (simple_shifted_x, center_y),
        (simple_target_x, center_y),
        (simple_target_x, simple_path[3].1),
    ];

    let detour = vec![
        detour_path[0],
        detour_path[1],
        detour_path[2],
        (detour_path[2].0, detour_lower_y),
        (detour_target_x, detour_lower_y),
        (detour_target_x, detour_path[5].1),
    ];

    Some((simple, detour))
}

fn simple_path_collapses_to_detour_lower_row(
    simple_path: &[(usize, usize)],
    detour_path: &[(usize, usize)],
    vertical_sign: i8,
) -> bool {
    if vertical_sign > 0 {
        simple_path[1].1 == detour_path[3].1
            && detour_path[1].1 < detour_path[3].1
            && detour_path[3].1 < simple_path[3].1
    } else {
        simple_path[1].1 == detour_path[3].1
            && detour_path[1].1 > detour_path[3].1
            && detour_path[3].1 > simple_path[3].1
    }
}

fn collapsed_vertical_criss_cross_center_y(
    detour_path: &[(usize, usize)],
    vertical_sign: isize,
) -> Option<usize> {
    let upper_y = detour_path[1].1;
    let lower_y = detour_path[3].1;
    let center_y = shift_axis(lower_y, -2 * vertical_sign)?;

    if vertical_sign > 0 {
        (upper_y < center_y && center_y < lower_y).then_some(center_y)
    } else {
        (upper_y > center_y && center_y > lower_y).then_some(center_y)
    }
}

fn should_compact_vertical_criss_cross_simple_target(
    simple_shifted_x: usize,
    center_x: usize,
    simple_target_x: usize,
) -> bool {
    simple_shifted_x.abs_diff(simple_target_x) >= 12 && simple_shifted_x.abs_diff(center_x) >= 6
}

fn compact_vertical_criss_cross_target_x(
    bounds: &NodeBounds,
    center_x: usize,
    simple_horizontal_sign: isize,
) -> Option<usize> {
    let face_left = bounds.x.saturating_add(1);
    let face_right = bounds.x + bounds.width.saturating_sub(2);
    if face_left > face_right {
        return None;
    }

    if simple_horizontal_sign < 0 {
        Some(face_right.min(center_x.saturating_sub(1)))
    } else {
        Some(face_left.max(center_x.saturating_add(1)))
    }
}

fn point_delta(a: usize, b: usize) -> i8 {
    match b.cmp(&a) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn shift_axis(value: usize, delta: isize) -> Option<usize> {
    let shifted = value as isize + delta;
    (shifted >= 0).then_some(shifted as usize)
}
