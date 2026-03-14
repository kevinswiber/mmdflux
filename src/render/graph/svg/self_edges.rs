//! Self-edge adjustment helpers for graph SVG rendering.

use std::collections::HashMap;

use super::{Point, Rect};
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::{Direction, Graph};

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
        let adjusted =
            adjust_self_edge_points(&layout_rect, &layout_points, diagram.direction, pad);
        paths.insert(se.edge_index, adjusted);
    }

    paths
}

fn adjust_self_edge_points(
    rect: &Rect,
    points: &[Point],
    direction: Direction,
    pad: f64,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    match direction {
        Direction::TopDown => {
            let loop_x = points
                .iter()
                .map(|point| point.x)
                .fold(right, f64::max)
                .max(right + pad);
            vec![
                Point { x: right, y: top },
                Point { x: loop_x, y: top },
                Point {
                    x: loop_x,
                    y: bottom,
                },
                Point {
                    x: right,
                    y: bottom,
                },
            ]
        }
        Direction::BottomTop => {
            let loop_x = points
                .iter()
                .map(|point| point.x)
                .fold(right, f64::max)
                .max(right + pad);
            vec![
                Point {
                    x: right,
                    y: bottom,
                },
                Point {
                    x: loop_x,
                    y: bottom,
                },
                Point { x: loop_x, y: top },
                Point { x: right, y: top },
            ]
        }
        Direction::LeftRight => {
            let loop_y = points
                .iter()
                .map(|point| point.y)
                .fold(bottom, f64::max)
                .max(bottom + pad);
            vec![
                Point {
                    x: right,
                    y: bottom,
                },
                Point {
                    x: right,
                    y: loop_y,
                },
                Point { x: left, y: loop_y },
                Point { x: left, y: bottom },
            ]
        }
        Direction::RightLeft => {
            let loop_y = points
                .iter()
                .map(|point| point.y)
                .fold(bottom, f64::max)
                .max(bottom + pad);
            vec![
                Point { x: left, y: bottom },
                Point { x: left, y: loop_y },
                Point {
                    x: right,
                    y: loop_y,
                },
                Point {
                    x: right,
                    y: bottom,
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, Rect, adjust_self_edge_points};
    use crate::graph::Direction;

    #[test]
    fn adjust_self_edge_points_top_down_loops_to_the_right() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let points = [Point { x: 45.0, y: 25.0 }, Point { x: 52.0, y: 60.0 }];

        let adjusted = adjust_self_edge_points(&rect, &points, Direction::TopDown, 8.0);

        assert_eq!(adjusted.len(), 4);
        assert_eq!(adjusted[0], Point { x: 40.0, y: 20.0 });
        assert_eq!(adjusted[1].x, adjusted[2].x);
        assert!(adjusted[1].x >= 52.0);
        assert_eq!(adjusted[3], Point { x: 40.0, y: 60.0 });
    }

    #[test]
    fn adjust_self_edge_points_left_right_loops_below_the_node() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let points = [Point { x: 42.0, y: 58.0 }, Point { x: 15.0, y: 70.0 }];

        let adjusted = adjust_self_edge_points(&rect, &points, Direction::LeftRight, 6.0);

        assert_eq!(adjusted.len(), 4);
        assert_eq!(adjusted[0], Point { x: 40.0, y: 60.0 });
        assert_eq!(adjusted[1].x, 40.0);
        assert!(adjusted[1].y >= 70.0);
        assert_eq!(adjusted[2].y, adjusted[1].y);
        assert_eq!(adjusted[3], Point { x: 10.0, y: 60.0 });
    }
}
