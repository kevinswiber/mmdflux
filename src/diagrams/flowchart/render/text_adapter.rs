//! Text adapter: converts engine `GraphGeometry` to the integer `Layout`
//! struct consumed by the text renderer.
//!
//! This is the bridge between the engine pipeline (which produces float
//! coordinates via `MeasurementMode::Text`) and text rendering (which
//! operates on character-grid integer coordinates).
//!
//! **Migration status:** Phases B-F (node placement, scaling, collision repair,
//! canvas sizing) are implemented inline for diagrams without direction
//! overrides. Phases G+ delegate to `compute_layout_from_geometry()`.
//! Direction-override diagrams fully delegate until Phase M is migrated.

use std::collections::{HashMap, HashSet};

use super::layout::{
    Layout, LayoutConfig, RawCenter, collision_repair, compute_ascii_scale_factors,
    compute_grid_positions, compute_layout_from_geometry, layered_config_for_layout,
    rank_gap_repair,
};
use super::shape::{NodeBounds, node_dimensions};
use crate::diagrams::flowchart::geometry::GraphGeometry;
use crate::graph::{Diagram, Direction};

/// Convert engine-produced `GraphGeometry` (with text-scale node dimensions)
/// to the integer-coordinate `Layout` struct consumed by the text renderer.
///
/// Phases B-F (node placement, scaling, collision repair, canvas sizing)
/// read directly from `GraphGeometry` for diagrams without direction overrides.
/// Diagrams with direction overrides fully delegate to
/// `compute_layout_from_geometry()` until Phase M is migrated.
pub fn geometry_to_text_layout(
    diagram: &Diagram,
    geometry: &GraphGeometry,
    config: &LayoutConfig,
) -> Layout {
    // Phase M (sublayout reconciliation) modifies draw_positions and node_bounds
    // for direction-override subgraphs. Until Phase M is migrated to the adapter,
    // delegate entirely for those diagrams.
    let has_dir_overrides = diagram.subgraphs.values().any(|sg| sg.dir.is_some());

    // Delegate handles all phases consistently from the same geometry.
    let delegate = compute_layout_from_geometry(diagram, geometry, config);

    if has_dir_overrides {
        return delegate;
    }

    // --- Phase B: Group nodes into layers ---
    let is_vertical = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);
    let direction = diagram.direction;
    let layered_config = layered_config_for_layout(diagram, config);

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

    // --- Phases G+: take from delegate ---
    // width/height from delegate includes subgraph/self-edge canvas expansion.
    // draw_positions/node_bounds from adapter match delegate (same geometry,
    // same code, no Phase M for this branch).
    Layout {
        grid_positions,
        draw_positions,
        node_bounds,
        width: delegate.width,
        height: delegate.height,
        h_spacing: config.h_spacing,
        v_spacing: config.v_spacing,
        edge_waypoints: delegate.edge_waypoints,
        edge_label_positions: delegate.edge_label_positions,
        node_shapes: delegate.node_shapes,
        subgraph_bounds: delegate.subgraph_bounds,
        self_edges: delegate.self_edges,
        node_directions: delegate.node_directions,
    }
}
