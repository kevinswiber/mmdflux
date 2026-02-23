//! Text adapter: converts engine `GraphGeometry` to the integer `Layout`
//! struct consumed by the text renderer.
//!
//! This is the bridge between the engine pipeline (which produces float
//! coordinates via `MeasurementMode::Text`) and text rendering (which
//! operates on character-grid integer coordinates).
//!
//! All phases (B-N) are implemented inline, reading directly from
//! `GraphGeometry` and its `LayeredHints`. Direction-override subgraphs
//! are handled by Phase M (sublayout reconciliation).

use std::collections::{HashMap, HashSet};

use super::text_layout::{
    CoordTransform, Layout, RawCenter, SelfEdgeDrawData, TextLayoutConfig, TransformContext,
    align_cross_boundary_siblings_draw, clip_waypoints_to_subgraph, collision_repair,
    compute_ascii_scale_factors, compute_grid_positions, compute_layer_starts, compute_sublayouts,
    ensure_external_edge_spacing, expand_parent_subgraph_bounds, layered_config_for_layout,
    nudge_colliding_waypoints, rank_gap_repair, reconcile_sublayouts_draw,
    resolve_sibling_overlaps_draw, shrink_subgraph_horizontal_gaps, shrink_subgraph_vertical_gaps,
    subgraph_bounds_to_draw, text_edge_label_dimensions, transform_label_positions_direct,
    transform_waypoints_direct,
};
use super::text_shape::{NodeBounds, node_dimensions};
use crate::diagrams::flowchart::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::{Diagram, Direction, Shape};
use crate::layered::{Direction as LayeredDirection, Rect};

/// Convenience: run the full engine → adapter pipeline to produce a `Layout`.
///
/// This is the canonical way to compute a text layout from a `Diagram` and
/// `TextLayoutConfig`. Internally runs `FluxLayeredEngine::text().solve()` then
/// `geometry_to_text_layout()`.
pub fn compute_layout(diagram: &Diagram, config: &TextLayoutConfig) -> Layout {
    use crate::diagram::{
        EngineConfig, GraphEngine, GraphSolveRequest, OutputFormat, RenderConfig,
    };
    use crate::diagrams::flowchart::engine::FluxLayeredEngine;
    use crate::layered::LayoutConfig as LayeredConfig;

    let engine = FluxLayeredEngine::text();
    // Construct raw LayeredConfig without pre-applying cluster_rank_sep.
    // The engine's internal round-trip applies it exactly once.
    let engine_config = EngineConfig::Layered(LayeredConfig {
        direction: match diagram.direction {
            Direction::TopDown => LayeredDirection::TopBottom,
            Direction::BottomTop => LayeredDirection::BottomTop,
            Direction::LeftRight => LayeredDirection::LeftRight,
            Direction::RightLeft => LayeredDirection::RightLeft,
        },
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep: config.rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or_default(),
    });
    let request = GraphSolveRequest::from_config(&RenderConfig::default(), OutputFormat::Text);
    let result = engine
        .solve(diagram, &engine_config, &request)
        .expect("engine solve failed");
    geometry_to_text_layout(diagram, &result.geometry, config)
}

/// Convert engine-produced `GraphGeometry` (with text-scale node dimensions)
/// to the integer-coordinate `Layout` struct consumed by the text renderer.
///
/// All phases (B-N) are implemented inline, reading directly from
/// `GraphGeometry`. Direction-override subgraphs are handled by
/// Phase M (sublayout reconciliation).
pub fn geometry_to_text_layout(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    config: &TextLayoutConfig,
) -> Layout {
    geometry_to_text_layout_with_routed(diagram, geometry, None, config)
}

/// Convert engine-produced `GraphGeometry` (with optional routed edge paths)
/// to the integer-coordinate `Layout` consumed by the text renderer.
pub fn geometry_to_text_layout_with_routed(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    routed: Option<&RoutedGraphGeometry>,
    config: &TextLayoutConfig,
) -> Layout {
    let is_vertical = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);
    let direction = diagram.direction;
    let layered_config = layered_config_for_layout(diagram, config);

    // Pre-compute sub-layouts for subgraphs with direction overrides.
    let sublayouts = compute_sublayouts(
        diagram,
        &layered_config,
        |node| {
            let (w, h) = node_dimensions(node, direction);
            (w as f64, h as f64)
        },
        |edge| {
            edge.label
                .as_ref()
                .map(|label| text_edge_label_dimensions(label))
        },
        false,
    );

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
        .map(|(id, node)| (id.clone(), node_dimensions(node, direction)))
        .collect();

    // --- Phase D: Scale layout coordinates to ASCII ---
    let ranks_doubled_for_scale = false;
    let (scale_x, scale_y) = compute_ascii_scale_factors(
        &node_dims,
        layered_config.rank_sep,
        layered_config.node_sep,
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
    let has_backward_edges = !geometry.reversed_edges.is_empty();
    let backward_margin = if has_backward_edges {
        super::text_router::BACKWARD_ROUTE_GAP + 2
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
    let engine_hints = match &geometry.engine_hints {
        Some(crate::diagrams::flowchart::geometry::EngineHints::Layered(h)) => h,
        _ => unreachable!("text adapter requires layered engine hints"),
    };
    let layer_starts = compute_layer_starts(&engine_hints.node_ranks, &node_bounds, is_vertical);

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

    let edge_waypoints_converted = transform_waypoints_direct(
        &engine_hints.edge_waypoints,
        &diagram.edges,
        &ctx,
        &layer_starts,
        is_vertical,
        width,
        height,
    );
    let mut routed_edge_paths: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    if let Some(routed) = routed {
        for edge in &routed.edges {
            if edge.path.is_empty() {
                continue;
            }
            let mut converted: Vec<(usize, usize)> = Vec::with_capacity(edge.path.len());
            for point in &edge.path {
                converted.push(ctx.to_ascii(point.x, point.y));
            }
            converted.dedup();
            if converted.len() >= 2 {
                routed_edge_paths.insert(edge.index, converted);
            }
        }
    }

    let mut edge_label_positions = transform_label_positions_direct(
        &engine_hints.label_positions,
        &diagram.edges,
        &ctx,
        &layer_starts,
        is_vertical,
        width,
        height,
    );

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
                && super::text_router::is_backward_edge(from_b, to_b, diagram.direction)
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
    let layout_sg_bounds: HashMap<String, Rect> = geometry
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.clone(), sg.rect.into()))
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

    // --- Phase L: Compute self-edge loop paths in draw coordinates ---
    let layered_direction = layered_config.direction;
    let self_edges: Vec<SelfEdgeDrawData> = geometry
        .self_edges
        .iter()
        .filter_map(|se| {
            let bounds = node_bounds.get(&se.node_id)?;
            let loop_extent = 3;

            let points = match layered_direction {
                LayeredDirection::TopBottom => {
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
                LayeredDirection::BottomTop => {
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
                LayeredDirection::LeftRight => {
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
                LayeredDirection::RightLeft => {
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
    if !sublayouts.is_empty() {
        reconcile_sublayouts_draw(
            diagram,
            config,
            &sublayouts,
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

        // --- Phase N: Ensure external-edge spacing ---
        ensure_external_edge_spacing(
            diagram,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
        );

        // Invalidate/adjust waypoints for edges touching override subgraphs.
        for sg in diagram.subgraphs.values() {
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
                            edge_waypoints.remove(&key);
                            routed_edge_paths.remove(&key);
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

    let node_directions = geometry.node_directions.clone();

    Layout {
        grid_positions,
        draw_positions,
        node_bounds,
        width,
        height,
        h_spacing: config.h_spacing,
        v_spacing: config.v_spacing,
        edge_waypoints,
        routed_edge_paths,
        edge_label_positions,
        node_shapes,
        subgraph_bounds,
        self_edges,
        node_directions,
    }
}
