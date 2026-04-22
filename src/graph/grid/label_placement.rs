//! Corridor-aware label placement primitives for the text grid.
//!
//! Projects a routed polyline into grid space and classifies each
//! occupied cell by role. Task 3.3 consumes this footprint to steer an
//! authoritative label anchor off load-bearing corridor glyphs.

// Task 3.3 wires the primitives below into anchor selection; Task 3.4
// wires anchor selection into derive/mod.rs and text/edge.rs.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::graph::grid::layout::TransformContext;
use crate::graph::space::FPoint;

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

pub(crate) fn project_path_to_grid(path: &[FPoint], ctx: &TransformContext) -> PathFootprint {
    let mut cells = BTreeMap::new();
    if path.len() < 2 {
        return PathFootprint { cells };
    }

    for window in path.windows(2) {
        fill_segment_cells(window[0], window[1], ctx, &mut cells);
    }

    for i in 1..path.len() - 1 {
        let prev_axis = segment_axis(path[i - 1], path[i]);
        let next_axis = segment_axis(path[i], path[i + 1]);
        if prev_axis != next_axis && prev_axis.is_some() && next_axis.is_some() {
            let bend_cell = ctx.to_grid(path[i].x, path[i].y);
            cells.entry(bend_cell).and_modify(|role| {
                if !matches!(role, CellRole::Terminal) {
                    *role = CellRole::Corner;
                }
            });
        }
    }

    let first = ctx.to_grid(path[0].x, path[0].y);
    let last = ctx.to_grid(path[path.len() - 1].x, path[path.len() - 1].y);
    cells.insert(first, CellRole::Terminal);
    cells.insert(last, CellRole::Terminal);

    PathFootprint { cells }
}

fn fill_segment_cells(
    a: FPoint,
    b: FPoint,
    ctx: &TransformContext,
    cells: &mut BTreeMap<GridCell, CellRole>,
) {
    let start = ctx.to_grid(a.x, a.y);
    let end = ctx.to_grid(b.x, b.y);
    match segment_axis(a, b) {
        Some(Axis::Horizontal) => {
            let (c_min, c_max) = (start.0.min(end.0), start.0.max(end.0));
            for col in c_min..=c_max {
                cells.entry((col, start.1)).or_insert(CellRole::Corridor);
            }
        }
        Some(Axis::Vertical) => {
            let (r_min, r_max) = (start.1.min(end.1), start.1.max(end.1));
            for row in r_min..=r_max {
                cells.entry((start.0, row)).or_insert(CellRole::Corridor);
            }
        }
        None => {
            fill_bresenham(start, end, cells);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

fn segment_axis(a: FPoint, b: FPoint) -> Option<Axis> {
    const EPS: f64 = 0.001;
    let dx = (b.x - a.x).abs();
    let dy = (b.y - a.y).abs();
    if dx < EPS && dy < EPS {
        None
    } else if dx < EPS {
        Some(Axis::Vertical)
    } else if dy < EPS {
        Some(Axis::Horizontal)
    } else {
        None
    }
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

    fn grid_ctx(cell_width_px: f64, cell_height_px: f64) -> TransformContext {
        TransformContext::for_test(cell_width_px, cell_height_px)
    }

    #[test]
    fn project_straight_horizontal_segment_fills_corridor_cells() {
        let ctx = grid_ctx(14.0, 14.0);
        let path = vec![FPoint::new(0.0, 28.0), FPoint::new(84.0, 28.0)];
        let footprint = project_path_to_grid(&path, &ctx);
        let corridor_cells: Vec<_> = footprint
            .cells
            .iter()
            .filter(|(_, role)| matches!(role, CellRole::Corridor))
            .map(|(cell, _)| *cell)
            .collect();
        // The two end-cells are tagged Terminal; only the interior is Corridor.
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
        let ctx = grid_ctx(14.0, 14.0);
        let path = vec![FPoint::new(42.0, 0.0), FPoint::new(42.0, 70.0)];
        let footprint = project_path_to_grid(&path, &ctx);
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
        let ctx = grid_ctx(14.0, 14.0);
        let path = vec![
            FPoint::new(0.0, 0.0),
            FPoint::new(42.0, 0.0),
            FPoint::new(42.0, 42.0),
        ];
        let footprint = project_path_to_grid(&path, &ctx);
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
        let ctx = grid_ctx(14.0, 14.0);
        let path = vec![
            FPoint::new(84.0, 14.0),
            FPoint::new(84.0, 42.0),
            FPoint::new(14.0, 42.0),
            FPoint::new(14.0, 14.0),
        ];
        let footprint = project_path_to_grid(&path, &ctx);

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
        let ctx = grid_ctx(14.0, 14.0);
        let path = vec![FPoint::new(0.0, 0.0)];
        let footprint = project_path_to_grid(&path, &ctx);
        assert!(footprint.cells.is_empty());
    }
}
