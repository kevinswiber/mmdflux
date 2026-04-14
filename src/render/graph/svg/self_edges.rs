//! Self-edge adjustment helpers for graph SVG rendering.

use std::collections::HashMap;

use super::{Point, Rect};
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::{Direction, Graph, Shape};

pub(super) fn compute_self_edge_paths(
    diagram: &Graph,
    geom: &GraphGeometry,
    metrics: &ProportionalTextMetrics,
) -> HashMap<usize, Vec<Point>> {
    let pad = metrics.node_padding_x.max(metrics.node_padding_y).max(4.0);
    let mut paths = HashMap::new();

    for se in &geom.self_edges {
        let Some(pos_node) = geom.nodes.get(&se.node_id) else {
            continue;
        };
        if se.points.is_empty() {
            continue;
        }
        let layout_rect: Rect = pos_node.rect;
        let layout_points: Vec<Point> = se.points.to_vec();
        let adjusted = adjust_self_edge_points(
            &layout_rect,
            &layout_points,
            diagram.direction,
            pad,
            &pos_node.shape,
        );
        paths.insert(se.edge_index, adjusted);
    }

    paths
}

/// Build a 4-point rectangular self-loop path.
///
/// Both exit and entry are offset from the corners along the node face.
/// The loop extends between their positions so it does not reach the
/// node border.  The terminal segment is axis-aligned (horizontal for
/// TD/BT, vertical for LR/RL).
///
/// For TD the shape is:
/// ```text
///    exit ───────────── loop_x
///                        │
///    entry ───────────── loop_x
/// ```
fn adjust_self_edge_points(
    rect: &Rect,
    points: &[Point],
    direction: Direction,
    pad: f64,
    shape: &Shape,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;

    let (exit, entry) = self_loop_anchor_points(rect, direction, pad, shape);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let loop_x = points
                .iter()
                .map(|point| point.x)
                .fold(right, f64::max)
                .max(right + pad);
            vec![
                exit,
                Point {
                    x: loop_x,
                    y: exit.y,
                },
                Point {
                    x: loop_x,
                    y: entry.y,
                },
                entry,
            ]
        }
        Direction::LeftRight | Direction::RightLeft => {
            let loop_y = points
                .iter()
                .map(|point| point.y)
                .fold(bottom, f64::max)
                .max(bottom + pad);
            vec![
                exit,
                Point {
                    x: exit.x,
                    y: loop_y,
                },
                Point {
                    x: entry.x,
                    y: loop_y,
                },
                entry,
            ]
        }
    }
}

/// Compute exit and entry attachment points for a self-loop.
///
/// Both points are offset from the bounding-rect corners along the node
/// face so the loop does not touch the border.
fn self_loop_anchor_points(
    rect: &Rect,
    direction: Direction,
    pad: f64,
    shape: &Shape,
) -> (Point, Point) {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    // Face offset: how far exit/entry are inset from the corner along
    // the primary face.  Capped so it never exceeds a quarter of the face.
    let face_offset = |face_len: f64| pad.min(face_len / 4.0);

    match shape {
        Shape::Diamond => {
            // Diamond edge parameter t=0.25 (exit, near tip) / t=0.75 (entry,
            // near tip) gives 75 % height span while staying on the border.
            let w8 = rect.width / 8.0;
            let h8 = rect.height / 8.0;
            match direction {
                Direction::TopDown => (
                    Point {
                        x: right - 3.0 * w8,
                        y: top + h8,
                    },
                    Point {
                        x: right - 3.0 * w8,
                        y: bottom - h8,
                    },
                ),
                Direction::BottomTop => (
                    Point {
                        x: right - 3.0 * w8,
                        y: bottom - h8,
                    },
                    Point {
                        x: right - 3.0 * w8,
                        y: top + h8,
                    },
                ),
                Direction::LeftRight => (
                    Point {
                        x: right - w8,
                        y: bottom - 3.0 * h8,
                    },
                    Point {
                        x: left + w8,
                        y: bottom - 3.0 * h8,
                    },
                ),
                Direction::RightLeft => (
                    Point {
                        x: left + w8,
                        y: bottom - 3.0 * h8,
                    },
                    Point {
                        x: right - w8,
                        y: bottom - 3.0 * h8,
                    },
                ),
            }
        }
        Shape::Hexagon => {
            // Hexagon right face: upper-right edge (right-indent,top)→(right,cy)
            // and lower-right edge (right,cy)→(right-indent,bottom).
            // At y = top+h8 (t=0.25), border x = right - 3*indent/4.
            let indent = rect.width * 0.2;
            let border_inset = 3.0 * indent / 4.0;
            let h8 = rect.height / 8.0;
            match direction {
                Direction::TopDown => (
                    Point {
                        x: right - border_inset,
                        y: top + h8,
                    },
                    Point {
                        x: right - border_inset,
                        y: bottom - h8,
                    },
                ),
                Direction::BottomTop => (
                    Point {
                        x: right - border_inset,
                        y: bottom - h8,
                    },
                    Point {
                        x: right - border_inset,
                        y: top + h8,
                    },
                ),
                Direction::LeftRight => (
                    Point {
                        x: right - border_inset,
                        y: bottom - h8,
                    },
                    Point {
                        x: left + border_inset,
                        y: bottom - h8,
                    },
                ),
                Direction::RightLeft => (
                    Point {
                        x: left + border_inset,
                        y: bottom - h8,
                    },
                    Point {
                        x: right - border_inset,
                        y: bottom - h8,
                    },
                ),
            }
        }
        _ => {
            let fo = face_offset(rect.height);
            let fo_w = face_offset(rect.width);
            match direction {
                Direction::TopDown => (
                    Point {
                        x: right,
                        y: top + fo,
                    },
                    Point {
                        x: right,
                        y: bottom - fo,
                    },
                ),
                Direction::BottomTop => (
                    Point {
                        x: right,
                        y: bottom - fo,
                    },
                    Point {
                        x: right,
                        y: top + fo,
                    },
                ),
                Direction::LeftRight => (
                    Point {
                        x: right - fo_w,
                        y: bottom,
                    },
                    Point {
                        x: left + fo_w,
                        y: bottom,
                    },
                ),
                Direction::RightLeft => (
                    Point {
                        x: left + fo_w,
                        y: bottom,
                    },
                    Point {
                        x: right - fo_w,
                        y: bottom,
                    },
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, Rect, adjust_self_edge_points};
    use crate::graph::{Direction, Shape};

    #[test]
    fn adjust_self_edge_points_top_down_four_point_path() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let points = [Point { x: 45.0, y: 25.0 }, Point { x: 52.0, y: 60.0 }];

        let adjusted =
            adjust_self_edge_points(&rect, &points, Direction::TopDown, 8.0, &Shape::Rectangle);

        assert_eq!(adjusted.len(), 4);
        // P1: exit on right face, offset from top corner
        assert_eq!(adjusted[0].x, 40.0);
        assert!(adjusted[0].y > 20.0, "exit should be offset from top");
        // P2: loop extent at exit height
        assert!(adjusted[1].x >= 52.0);
        assert_eq!(adjusted[1].y, adjusted[0].y);
        // P3: loop extent at entry height
        assert_eq!(adjusted[2].x, adjusted[1].x);
        assert!(adjusted[2].y < 60.0, "entry should be offset from bottom");
        // P4: entry on right face, offset from bottom corner
        assert_eq!(adjusted[3].x, 40.0);
        assert!(adjusted[3].y < 60.0, "entry should be offset from bottom");
        assert_eq!(adjusted[2].y, adjusted[3].y, "terminal must be horizontal");
    }

    #[test]
    fn adjust_self_edge_points_left_right_four_point_path() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let points = [Point { x: 42.0, y: 58.0 }, Point { x: 15.0, y: 70.0 }];

        let adjusted =
            adjust_self_edge_points(&rect, &points, Direction::LeftRight, 6.0, &Shape::Rectangle);

        assert_eq!(adjusted.len(), 4);
        // P1: exit on bottom face, offset from right corner
        assert_eq!(adjusted[0].y, 60.0);
        assert!(adjusted[0].x < 40.0, "exit should be offset from right");
        // P2: loop extent at exit x
        assert_eq!(adjusted[1].x, adjusted[0].x);
        assert!(adjusted[1].y >= 70.0);
        // P3: loop extent at entry x
        assert!(adjusted[2].x > 10.0, "entry should be offset from left");
        assert_eq!(adjusted[2].y, adjusted[1].y);
        // P4: entry on bottom face, offset from left corner
        assert_eq!(adjusted[3].y, 60.0);
        assert!(adjusted[3].x > 10.0, "entry should be offset from left");
        assert_eq!(adjusted[2].x, adjusted[3].x, "terminal must be vertical");
    }

    #[test]
    fn adjust_self_edge_points_diamond_td_attaches_on_shape_border() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 40.0,
        };
        let points = [Point { x: 55.0, y: 25.0 }, Point { x: 55.0, y: 55.0 }];

        let adjusted =
            adjust_self_edge_points(&rect, &points, Direction::TopDown, 8.0, &Shape::Diamond);

        assert_eq!(adjusted.len(), 4);
        // Diamond at t=0.25: exit x = right - 3*w/8 = 50 - 15 = 35, y = top + h/8 = 20 + 5 = 25
        assert_eq!(adjusted[0], Point { x: 35.0, y: 25.0 });
        // P2: loop extent at exit height
        assert!(adjusted[1].x >= 55.0);
        assert_eq!(adjusted[1].y, 25.0);
        // Entry: x = 35, y = bottom - h/8 = 60 - 5 = 55
        assert_eq!(adjusted[3], Point { x: 35.0, y: 55.0 });
    }
}
