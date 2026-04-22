//! Corridor-aware label placement primitives for the text grid.
//!
//! Projects a routed polyline into grid space and classifies each
//! occupied cell by role. `choose_corridor_aware_anchor` consumes the
//! footprint to steer an authoritative label anchor off load-bearing
//! corridor glyphs. Called from `derive/mod.rs` for every routed edge
//! with an authoritative `label_geometry`.

use std::collections::BTreeMap;

use crate::graph::geometry::EdgeLabelSide;

pub(crate) type GridCell = (usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellRole {
    Corridor,
    Corner,
    Terminal,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PathFootprint {
    pub cells: BTreeMap<GridCell, CellRole>,
}

/// Build a `PathFootprint` directly from a grid-space polyline (e.g.
/// the post-processed `routed_edge_paths` used by the text renderer).
/// Callers draw from this exact polyline, so building the footprint
/// from grid cells avoids the float→grid quantization mismatch where
/// a corner glyph would otherwise land one cell off from a float-space
/// bend point.
///
/// The integration path in `derive/mod.rs` only needs
/// `extend_grid_polyline_into` (merging many edges into one footprint);
/// this single-polyline entry point exists for unit tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn project_grid_polyline(path: &[GridCell]) -> PathFootprint {
    let mut footprint = PathFootprint::default();
    extend_grid_polyline_into(path, &mut footprint);
    footprint
}

/// Union the footprint of a grid-space polyline into `dest`. Used by
/// callers that need a single footprint across several edges (labels
/// on one edge must not stomp another edge's corner/terminal glyphs).
pub(crate) fn extend_grid_polyline_into(path: &[GridCell], dest: &mut PathFootprint) {
    if path.len() < 2 {
        return;
    }

    // Corridor cells first, then tag corners, then terminals. Using
    // `entry(...).or_insert` respects the priority: Corner upgrades
    // Corridor; Terminal upgrades both.
    for window in path.windows(2) {
        fill_grid_segment_cells(window[0], window[1], &mut dest.cells);
    }

    for i in 1..path.len() - 1 {
        let prev_axis = grid_segment_axis(path[i - 1], path[i]);
        let next_axis = grid_segment_axis(path[i], path[i + 1]);
        if prev_axis != next_axis && prev_axis.is_some() && next_axis.is_some() {
            dest.cells
                .entry(path[i])
                .and_modify(|role| {
                    if !matches!(role, CellRole::Terminal) {
                        *role = CellRole::Corner;
                    }
                })
                .or_insert(CellRole::Corner);
        }
    }

    dest.cells.insert(path[0], CellRole::Terminal);
    dest.cells.insert(path[path.len() - 1], CellRole::Terminal);
}

fn fill_grid_segment_cells(a: GridCell, b: GridCell, cells: &mut BTreeMap<GridCell, CellRole>) {
    match grid_segment_axis(a, b) {
        Some(Axis::Horizontal) => {
            let (c_min, c_max) = (a.0.min(b.0), a.0.max(b.0));
            for col in c_min..=c_max {
                cells.entry((col, a.1)).or_insert(CellRole::Corridor);
            }
        }
        Some(Axis::Vertical) => {
            let (r_min, r_max) = (a.1.min(b.1), a.1.max(b.1));
            for row in r_min..=r_max {
                cells.entry((a.0, row)).or_insert(CellRole::Corridor);
            }
        }
        None => {
            fill_bresenham(a, b, cells);
        }
    }
}

fn grid_segment_axis(a: GridCell, b: GridCell) -> Option<Axis> {
    let same_col = a.0 == b.0;
    let same_row = a.1 == b.1;
    match (same_col, same_row) {
        (true, true) => None,
        (true, false) => Some(Axis::Vertical),
        (false, true) => Some(Axis::Horizontal),
        (false, false) => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

/// Pick a grid cell for an authoritative label anchor that respects
/// the edge's path footprint.
///
/// The candidate is treated as the *center* of a `label_width`-by-
/// `label_height` block. A placement is "safe" when every cell that
/// block would occupy is either off the path entirely or on a
/// `Corridor` straight-segment cell — `Terminal` and `Corner` cells
/// carry load-bearing glyphs (arrowheads, bend characters) and must
/// never be overwritten. The function walks a prioritized step-set
/// in the direction declared by `side` and returns the first safe
/// neighbor, widening the search ring until a safe slot is found or
/// the ring exceeds `label_height + 2`. If no neighbor is safe, the
/// original candidate is returned (best-effort fallback).
pub(crate) fn choose_corridor_aware_anchor(
    candidate: GridCell,
    side: EdgeLabelSide,
    footprint: &PathFootprint,
    grid_width: usize,
    grid_height: usize,
    label_width: usize,
    label_height: usize,
) -> GridCell {
    if is_safe_block(candidate, label_width, label_height, footprint) {
        return candidate;
    }
    // Widen the shift ring progressively. The label_height + 2 cap
    // keeps the search bounded even for tall labels on dense paths.
    let max_ring = label_height.max(label_width).saturating_add(2);
    for ring in 1..=max_ring {
        for (dx, dy) in shift_steps(side) {
            let (rdx, rdy) = (dx * ring as isize, dy * ring as isize);
            if let Some(shifted) = apply_step(candidate, rdx, rdy, grid_width, grid_height)
                && is_safe_block(shifted, label_width, label_height, footprint)
            {
                return shifted;
            }
        }
    }
    candidate
}

fn is_safe_block(
    center: GridCell,
    label_width: usize,
    label_height: usize,
    footprint: &PathFootprint,
) -> bool {
    let base_x = center.0.saturating_sub(label_width / 2);
    let base_y = center.1.saturating_sub(label_height / 2);
    for row in base_y..base_y.saturating_add(label_height.max(1)) {
        for col in base_x..base_x.saturating_add(label_width.max(1)) {
            if matches!(
                footprint.cells.get(&(col, row)),
                Some(CellRole::Corner | CellRole::Terminal)
            ) {
                return false;
            }
        }
    }
    true
}

fn shift_steps(side: EdgeLabelSide) -> [(isize, isize); 4] {
    // Primary steps follow the declared side first; fallbacks cover
    // the remaining cardinals so the placer always gets a chance to
    // escape a load-bearing cell.
    match side {
        EdgeLabelSide::Below => [(0, 1), (1, 0), (-1, 0), (0, -1)],
        EdgeLabelSide::Above => [(0, -1), (-1, 0), (1, 0), (0, 1)],
        EdgeLabelSide::Center => [(0, 1), (0, -1), (1, 0), (-1, 0)],
    }
}

fn apply_step(
    cell: GridCell,
    dx: isize,
    dy: isize,
    grid_width: usize,
    grid_height: usize,
) -> Option<GridCell> {
    let new_x = (cell.0 as isize).checked_add(dx).filter(|v| *v >= 0)? as usize;
    let new_y = (cell.1 as isize).checked_add(dy).filter(|v| *v >= 0)? as usize;
    if new_x >= grid_width || new_y >= grid_height {
        return None;
    }
    Some((new_x, new_y))
}

fn fill_bresenham(start: GridCell, end: GridCell, cells: &mut BTreeMap<GridCell, CellRole>) {
    let (mut x0, mut y0) = (start.0 as isize, start.1 as isize);
    let (x1, y1) = (end.0 as isize, end.1 as isize);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 {
            cells
                .entry((x0 as usize, y0 as usize))
                .or_insert(CellRole::Corridor);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID_W: usize = 16;
    const GRID_H: usize = 16;

    #[test]
    fn project_straight_horizontal_segment_fills_corridor_cells() {
        // Grid cells (0,2) and (6,2) are terminals; interior is Corridor.
        let path = vec![(0usize, 2usize), (6, 2)];
        let footprint = project_grid_polyline(&path);
        let corridor_cells: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Corridor))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(corridor_cells, (1..=5).map(|c| (c, 2)).collect::<Vec<_>>());

        let terminals: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Terminal))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(terminals, vec![(0, 2), (6, 2)]);
    }

    #[test]
    fn project_straight_vertical_segment_fills_corridor_cells() {
        let path = vec![(3usize, 0usize), (3, 5)];
        let footprint = project_grid_polyline(&path);
        let corridor_cells: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Corridor))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(corridor_cells, (1..=4).map(|r| (3, r)).collect::<Vec<_>>());

        let terminals: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Terminal))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(terminals, vec![(3, 0), (3, 5)]);
    }

    #[test]
    fn project_l_bend_marks_corner_cell() {
        let path = vec![(0usize, 0usize), (3, 0), (3, 3)];
        let footprint = project_grid_polyline(&path);
        let corner_cells: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Corner))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(corner_cells, vec![(3, 0)]);

        let terminals: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Terminal))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(terminals, vec![(0, 0), (3, 3)]);
    }

    #[test]
    fn project_u_channel_marks_terminals_and_two_corners() {
        let path = vec![(6usize, 1usize), (6, 3), (1, 3), (1, 1)];
        let footprint = project_grid_polyline(&path);

        let terminals: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Terminal))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(terminals, vec![(1, 1), (6, 1)]);

        let corners: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Corner))
            .map(|(cell, _)| *cell)
            .collect();
        assert_eq!(corners, vec![(1, 3), (6, 3)]);

        let long_leg_corridors: usize = footprint
            .cells
            .iter()
            .filter(|((col, row), role)| {
                matches!(role, CellRole::Corridor) && *row == 3 && *col > 1 && *col < 6
            })
            .count();
        assert_eq!(long_leg_corridors, 4, "4 interior cells on the long leg");
    }

    #[test]
    fn project_degenerate_single_point_returns_empty_footprint() {
        let path = vec![(0usize, 0usize)];
        let footprint = project_grid_polyline(&path);
        assert!(footprint.cells.is_empty());
    }

    // Task 3.3 — choose_corridor_aware_anchor
    use crate::graph::geometry::EdgeLabelSide;

    fn u_channel_footprint() -> PathFootprint {
        project_grid_polyline(&[(6usize, 1usize), (6, 3), (1, 3), (1, 1)])
    }

    #[test]
    fn anchor_on_terminal_cell_shifts_off() {
        let footprint = u_channel_footprint();
        let candidate = (6, 1); // terminal
        let anchor = choose_corridor_aware_anchor(
            candidate,
            EdgeLabelSide::Below,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        assert_ne!(anchor, candidate);
        assert!(
            !matches!(
                footprint.cells.get(&anchor),
                Some(CellRole::Terminal | CellRole::Corner)
            ),
            "anchor landed on a load-bearing cell: {:?}",
            footprint.cells.get(&anchor)
        );
    }

    #[test]
    fn anchor_on_corner_cell_shifts_off() {
        let footprint = u_channel_footprint();
        let candidate = (1, 3); // corner
        let anchor = choose_corridor_aware_anchor(
            candidate,
            EdgeLabelSide::Below,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        assert_ne!(anchor, candidate);
        assert!(
            !matches!(
                footprint.cells.get(&anchor),
                Some(CellRole::Terminal | CellRole::Corner)
            ),
            "anchor landed on a load-bearing cell: {:?}",
            footprint.cells.get(&anchor)
        );
    }

    #[test]
    fn anchor_on_corridor_cell_remains_unchanged() {
        let footprint = u_channel_footprint();
        let candidate = (6, 2); // corridor on the right vertical leg
        let anchor = choose_corridor_aware_anchor(
            candidate,
            EdgeLabelSide::Below,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        assert_eq!(anchor, candidate);
    }

    #[test]
    fn anchor_off_path_remains_unchanged() {
        let footprint = u_channel_footprint();
        // (4, 2) is not on any footprint cell — off-path is safe by
        // definition. The placer preserves anchors that fall off the
        // edge entirely; the caller owns any "snap toward corridor"
        // policy above this layer.
        let candidate = (4, 2);
        let anchor = choose_corridor_aware_anchor(
            candidate,
            EdgeLabelSide::Below,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        assert_eq!(anchor, candidate);
    }

    #[test]
    fn below_side_prefers_shift_down_first() {
        // A horizontal bar (terminal + corner on top row, corridor
        // below). Candidate lands on the corner; Below should shift
        // down to (col, row+1), not up.
        let footprint = project_grid_polyline(&[(0usize, 0usize), (3, 0), (3, 2)]);
        let anchor = choose_corridor_aware_anchor(
            (3, 0),
            EdgeLabelSide::Below,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        assert_eq!(anchor, (3, 1));

        let anchor_above = choose_corridor_aware_anchor(
            (3, 0),
            EdgeLabelSide::Above,
            &footprint,
            GRID_W,
            GRID_H,
            1,
            1,
        );
        // Above can't go up from row 0; the fallback picks (2,0) or (4,0).
        assert_ne!(anchor_above, (3, 0));
        assert!(
            !matches!(
                footprint.cells.get(&anchor_above),
                Some(CellRole::Terminal | CellRole::Corner)
            ),
            "Above fallback landed on a load-bearing cell: {:?}",
            anchor_above
        );
    }
}
