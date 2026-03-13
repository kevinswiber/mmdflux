use super::super::layout::SelfEdgeDrawData;
use super::types::{AttachDirection, Point, RoutedEdge, Segment};
use crate::graph::{Direction, Edge};

/// Route a self-edge as orthogonal segments from pre-computed draw-coordinate points.
pub(super) fn route_self_edge(
    data: &SelfEdgeDrawData,
    edge: &Edge,
    direction: Direction,
) -> RoutedEdge {
    let segments: Vec<Segment> = data
        .points
        .windows(2)
        .flat_map(|window| {
            let (x1, y1) = window[0];
            let (x2, y2) = window[1];

            match (x1 == x2, y1 == y2) {
                (_, true) => vec![Segment::Horizontal {
                    y: y1,
                    x_start: x1.min(x2),
                    x_end: x1.max(x2),
                }],
                (true, _) => vec![Segment::Vertical {
                    x: x1,
                    y_start: y1.min(y2),
                    y_end: y1.max(y2),
                }],
                _ => vec![
                    Segment::Vertical {
                        x: x1,
                        y_start: y1.min(y2),
                        y_end: y1.max(y2),
                    },
                    Segment::Horizontal {
                        y: y2,
                        x_start: x1.min(x2),
                        x_end: x1.max(x2),
                    },
                ],
            }
        })
        .collect();

    let to_point = |&(x, y)| Point::new(x, y);
    let start = data
        .points
        .first()
        .map(to_point)
        .unwrap_or(Point::new(0, 0));
    let end = data.points.last().map(to_point).unwrap_or(Point::new(0, 0));

    let entry_direction = match direction {
        Direction::TopDown | Direction::BottomTop => AttachDirection::Right,
        Direction::LeftRight | Direction::RightLeft => AttachDirection::Bottom,
    };

    RoutedEdge {
        edge: edge.clone(),
        start,
        end,
        segments,
        source_connection: None,
        entry_direction,
        is_backward: false,
        is_self_edge: true,
    }
}
