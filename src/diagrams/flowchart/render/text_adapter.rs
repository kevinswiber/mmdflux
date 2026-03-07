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
    ensure_external_edge_spacing, ensure_subgraph_contains_members, expand_parent_subgraph_bounds,
    layered_config_for_layout, nudge_colliding_waypoints, rank_gap_repair,
    reconcile_sublayouts_draw, resolve_sibling_overlaps_draw, shrink_subgraph_horizontal_gaps,
    shrink_subgraph_vertical_gaps, subgraph_bounds_to_draw, text_edge_label_dimensions,
    transform_label_positions_direct, transform_waypoints_direct,
};
use super::text_shape::{NodeBounds, node_dimensions};
use crate::diagrams::flowchart::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::{Diagram, Direction, Edge, Shape};
use crate::layered::{Direction as LayeredDirection, Rect};

type DrawPath = Vec<(usize, usize)>;
type DrawPathPair = (DrawPath, DrawPath);
type IndexedDrawPathPair = (usize, DrawPath, usize, DrawPath);

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
        ..Default::default()
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
    // Detect backward edges using two complementary methods:
    // 1. Cycle-reversed edges (tracked by the acyclic/DFS step in the layered layout).
    // 2. Position-based visual backward edges: edges that go against the layout direction
    //    due to cross-graph rank constraints (e.g. a long cross-edge forces a node to a
    //    higher rank, making its shorter outgoing edges appear to go "upward"). These are
    //    not cycles so the DFS does not mark them in reversed_edges, yet the text router's
    //    is_backward_edge() detects them by position and routes them to the right/bottom,
    //    requiring the same extra canvas margin as cycle-reversed backward edges.
    let has_backward_edges = !geometry.reversed_edges.is_empty() || {
        use super::text_router::is_backward_edge as position_is_backward;
        diagram.edges.iter().any(|edge| {
            match (node_bounds.get(&edge.from), node_bounds.get(&edge.to)) {
                (Some(from_b), Some(to_b)) => position_is_backward(from_b, to_b, diagram.direction),
                _ => false,
            }
        })
    };
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
    let mut routed_edge_paths: HashMap<usize, DrawPath> = HashMap::new();
    let mut preserve_routed_path_topology: HashSet<usize> = HashSet::new();
    if let Some(routed) = routed {
        for edge in &routed.edges {
            if edge.path.is_empty() {
                continue;
            }
            let mut converted: DrawPath = Vec::with_capacity(edge.path.len());
            for point in &edge.path {
                converted.push(ctx.to_ascii(point.x, point.y));
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
    compact_vertical_criss_cross_draw_paths(
        diagram,
        &node_bounds,
        &mut routed_edge_paths,
        &preserve_routed_path_topology,
    );

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

    // Ensure subgraph bounds contain all member nodes after coordinate
    // transformation and shrink passes (rounding can cause 1-2 char losses).
    ensure_subgraph_contains_members(diagram, &node_bounds, &mut subgraph_bounds);

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
        preserve_routed_path_topology,
        edge_label_positions,
        node_shapes,
        subgraph_bounds,
        self_edges,
        node_directions,
    }
}

fn compact_vertical_criss_cross_draw_paths(
    diagram: &Diagram,
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
