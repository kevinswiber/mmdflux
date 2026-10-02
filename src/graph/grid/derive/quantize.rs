//! Grid quantization helpers for float-to-grid derivation.
//!
//! These functions assign abstract grid coordinates, convert float-space
//! spacing into grid scale factors, and repair spacing after integer rounding.

use std::collections::HashMap;

use crate::graph::grid::{GridPos, NodeBounds};

pub(super) fn compute_grid_positions(layers: &[Vec<String>]) -> HashMap<String, GridPos> {
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

/// A node's size on the character grid and in the layout geometry being scaled.
///
/// Grid-measured geometry has the same size in both spaces. Proportional
/// geometry (for example a hydrated MMDS document) carries pixel sizes, so the
/// scale must divide by the layout size or the text output stretches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScaleNodeDims {
    pub grid: (usize, usize),
    pub layout: (f64, f64),
}

impl ScaleNodeDims {
    pub(super) fn new(grid: (usize, usize), layout: (f64, f64)) -> Self {
        Self { grid, layout }
    }

    /// Dimensions for grid-measured geometry, where layout size equals grid size.
    #[cfg(test)]
    pub(super) fn grid_measured(w: usize, h: usize) -> Self {
        Self::new((w, h), (w as f64, h as f64))
    }
}

/// Compute per-axis ASCII scale factors for translating layout float coordinates
/// to character grid positions.
///
/// Returns `(scale_x, scale_y)` where each factor maps layout coordinate deltas
/// to ASCII character deltas along that axis. Numerators use grid sizes and
/// denominators use layout sizes (`layout_*`), so the node pitch in the layout
/// maps onto the node pitch on the grid.
///
/// For vertical layouts (TD/BT):
///   - scale_y (primary) = (max_h + v_spacing) / (layout_max_h + rank_sep)
///   - scale_x (cross)   = (avg_w + h_spacing) / (layout_avg_w + node_sep)
///
/// For horizontal layouts (LR/RL):
///   - scale_x (primary) = (max_w + h_spacing) / (layout_max_w + rank_sep)
///   - scale_y (cross)   = (avg_h + v_spacing) / (layout_avg_h + node_sep)
pub(super) fn compute_grid_scale_factors(
    node_dims: &HashMap<String, ScaleNodeDims>,
    rank_sep: f64,
    node_sep: f64,
    v_spacing: usize,
    h_spacing: usize,
    is_vertical: bool,
    ranks_doubled: bool,
) -> (f64, f64) {
    let count_f = node_dims.len().max(1) as f64;
    let mut total_w = 0.0;
    let mut total_h = 0.0;
    let mut max_w = 0.0_f64;
    let mut max_h = 0.0_f64;
    let mut layout_total_w = 0.0;
    let mut layout_total_h = 0.0;
    let mut layout_max_w = 0.0_f64;
    let mut layout_max_h = 0.0_f64;
    for dims in node_dims.values() {
        let (w, h) = (dims.grid.0 as f64, dims.grid.1 as f64);
        let (layout_w, layout_h) = dims.layout;
        total_w += w;
        total_h += h;
        max_w = max_w.max(w);
        max_h = max_h.max(h);
        layout_total_w += layout_w;
        layout_total_h += layout_h;
        layout_max_w = layout_max_w.max(layout_w);
        layout_max_h = layout_max_h.max(layout_h);
    }
    let avg_w = total_w / count_f;
    let avg_h = total_h / count_f;
    let layout_avg_w = layout_total_w / count_f;
    let layout_avg_h = layout_total_h / count_f;

    if is_vertical {
        // When ranks are doubled, the layout positions nodes 2× further apart.
        // To compensate exactly, we need: eff_rs = max_h + 2 * rank_sep
        // This gives scale_primary_new = scale_primary_old / 2, so that
        // (2 * rank_sep) * scale_new = rank_sep * scale_old.
        let effective_rank_sep = if ranks_doubled {
            layout_max_h + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_h + v_spacing as f64) / (layout_max_h + effective_rank_sep);
        let scale_cross = (avg_w + h_spacing as f64) / (layout_avg_w + node_sep);
        (scale_cross, scale_primary)
    } else {
        let effective_rank_sep = if ranks_doubled {
            layout_max_w + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_w + h_spacing as f64) / (layout_max_w + effective_rank_sep);
        let scale_cross = (avg_h + v_spacing as f64) / (layout_avg_h + node_sep);
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
pub(super) fn collision_repair(
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
pub(super) fn rank_gap_repair(
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
pub(super) fn compute_layer_starts(
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
