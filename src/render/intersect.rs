//! Edge-node intersection calculation.
//!
//! This module implements dynamic edge attachment points based on the approach
//! angle of an edge. Instead of always attaching edges at fixed center points,
//! we calculate where a line from an external point would intersect the node's
//! boundary.
//!
//! This is a key part of the Sugiyama framework that enables edges to
//! fan out naturally from nodes rather than overlapping at the center.

use crate::diagrams::flowchart::geometry::{FPoint, FRect};
use crate::diagrams::flowchart::render::routing_core::classify_face_float as shared_classify_face_float;
use crate::diagrams::flowchart::render::shape::NodeBounds;
use crate::graph::Shape;

/// Minimum gap between adjacent attachment points on a face.
/// Prevents arrow characters from visually colliding on narrow faces.
const MIN_ATTACHMENT_GAP: usize = 2;

/// Which face of a node an edge attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeFace {
    Top,
    Bottom,
    Left,
    Right,
}

/// Classify which face of a node a line from `approach_point` to the node center
/// would intersect. Uses the same slope-vs-diagonal comparison as `intersect_rect`.
///
/// Since rendered diamonds have rectangular boundaries (angle brackets on middle row),
/// the same slope logic works for all shapes.
pub fn classify_face(
    bounds: &NodeBounds,
    approach_point: (usize, usize),
    _shape: Shape,
) -> NodeFace {
    let center = FPoint::new(bounds.center_x() as f64, bounds.center_y() as f64);
    let rect = FRect::new(
        bounds.x as f64,
        bounds.y as f64,
        bounds.width as f64,
        bounds.height as f64,
    );
    let approach = FPoint::new(approach_point.0 as f64, approach_point.1 as f64);

    shared_classify_face_float(center, rect, approach).to_node_face()
}

/// Compute N evenly-spaced attachment positions along a face.
///
/// Uses endpoint-maximizing placement: edges are spread to the full extent
/// of the face for maximum separation. For N=1, returns the center.
/// Returns (x, y) coordinates.
pub fn spread_points_on_face(
    face: NodeFace,
    fixed_coord: usize,
    extent: (usize, usize),
    count: usize,
) -> Vec<(usize, usize)> {
    if count == 0 {
        return vec![];
    }

    let (start, end) = extent;
    let range = end.saturating_sub(start);

    // Helper to convert a position along the face to (x, y) coordinates
    let to_point = |pos: usize| match face {
        NodeFace::Top | NodeFace::Bottom => (pos, fixed_coord),
        NodeFace::Left | NodeFace::Right => (fixed_coord, pos),
    };

    if count == 1 {
        return vec![to_point(start + range / 2)];
    }

    // Endpoint-maximizing: place edges at extremes of range for maximum separation
    let mut positions: Vec<usize> = (0..count)
        .map(|i| {
            let pos = start + (i * range) / (count - 1);
            pos.min(end)
        })
        .collect();

    // Enforce minimum gap between adjacent positions
    let needed_span = (count - 1) * MIN_ATTACHMENT_GAP;
    if range >= needed_span {
        // Forward pass: push positions right to enforce minimum gap
        for i in 1..positions.len() {
            let min_pos = positions[i - 1] + MIN_ATTACHMENT_GAP;
            if positions[i] < min_pos {
                positions[i] = min_pos;
            }
        }
        // If enforcement pushed past end, shift everything left
        if let Some(&last) = positions.last()
            && last > end
        {
            let overshoot = last - end;
            for pos in &mut positions {
                *pos = pos.saturating_sub(overshoot);
            }
        }
    }
    // When range < needed_span, keep endpoint formula as-is (graceful degradation)

    positions.into_iter().map(to_point).collect()
}

/// A point in 2D space with floating-point coordinates.
///
/// Used for intermediate calculations before rounding to integer grid.
#[derive(Debug, Clone, Copy)]
pub struct FloatPoint {
    pub x: f64,
    pub y: f64,
}

impl FloatPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Convert to integer coordinates by rounding.
    pub fn to_usize(self) -> (usize, usize) {
        (self.x.round() as usize, self.y.round() as usize)
    }
}

impl From<(usize, usize)> for FloatPoint {
    fn from((x, y): (usize, usize)) -> Self {
        Self {
            x: x as f64,
            y: y as f64,
        }
    }
}

/// Calculate where a line from an external point to the rectangle's center
/// intersects the rectangle's boundary.
///
/// # Arguments
/// * `bounds` - The node's bounding box
/// * `point` - The external point (e.g., a waypoint or the center of another node)
///
/// # Returns
/// The intersection point on the rectangle's boundary.
pub fn intersect_rect(bounds: &NodeBounds, point: FloatPoint) -> FloatPoint {
    let x = bounds.center_x() as f64;
    let y = bounds.center_y() as f64;
    let dx = point.x - x;
    let dy = point.y - y;
    let w = bounds.width as f64 / 2.0;
    let h = bounds.height as f64 / 2.0;

    // Edge case: point is at center
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        // Return bottom center as a sensible default
        return FloatPoint::new(x, y + h);
    }

    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        // Line is steeper than the rectangle's diagonal
        // Intersection is on TOP or BOTTOM edge
        let h = if dy < 0.0 { -h } else { h };
        (h * dx / dy, h)
    } else {
        // Line is shallower than the rectangle's diagonal
        // Intersection is on LEFT or RIGHT edge
        let w = if dx < 0.0 { -w } else { w };
        (w, w * dy / dx)
    };

    FloatPoint::new(x + sx, y + sy)
}

/// Calculate where a line from an external point to a diamond's center
/// intersects the diamond's boundary.
///
/// A diamond is a rhombus with vertices at the center of each edge of its
/// bounding box. The boundary equation is: |dx|/w + |dy|/h = 1
///
/// # Arguments
/// * `bounds` - The node's bounding box
/// * `point` - The external point
///
/// # Returns
/// The intersection point on the diamond's boundary.
pub fn intersect_diamond(bounds: &NodeBounds, point: FloatPoint) -> FloatPoint {
    let x = bounds.center_x() as f64;
    let y = bounds.center_y() as f64;
    let dx = point.x - x;
    let dy = point.y - y;

    // Diamond half-diagonals
    let w = bounds.width as f64 / 2.0;
    let h = bounds.height as f64 / 2.0;

    // Edge case: point is at center
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        // Return bottom vertex as a sensible default
        return FloatPoint::new(x, y + h);
    }

    // For a diamond with equation |x/w| + |y/h| = 1,
    // the intersection with line from center is at parameter t where:
    // |t*dx|/w + |t*dy|/h = 1
    // Solving: t = 1 / (|dx|/w + |dy|/h)
    let t = 1.0 / (dx.abs() / w + dy.abs() / h);

    FloatPoint::new(x + t * dx, y + t * dy)
}

/// Calculate the intersection point for any node shape.
///
/// This dispatches to the appropriate intersection function based on the
/// node's shape.
///
/// # Arguments
/// * `bounds` - The node's bounding box
/// * `point` - The external point (waypoint or other node center)
/// * `shape` - The node's shape
///
/// # Returns
/// The intersection point on the node's boundary, as integer coordinates.
pub fn intersect_node(bounds: &NodeBounds, point: (usize, usize), shape: Shape) -> (usize, usize) {
    let float_point = FloatPoint::from(point);

    let result = match shape {
        Shape::Diamond | Shape::Hexagon => intersect_diamond(bounds, float_point),
        _ => intersect_rect(bounds, float_point),
    };

    result.to_usize()
}

/// Calculate intersection points for both ends of an edge, given waypoints.
///
/// # Arguments
/// * `source_bounds` - Bounding box of the source node
/// * `source_shape` - Shape of the source node
/// * `target_bounds` - Bounding box of the target node
/// * `target_shape` - Shape of the target node
/// * `waypoints` - Intermediate waypoints (may be empty)
///
/// # Returns
/// A tuple of (source_attachment, target_attachment) points.
pub fn calculate_attachment_points(
    source_bounds: &NodeBounds,
    source_shape: Shape,
    target_bounds: &NodeBounds,
    target_shape: Shape,
    waypoints: &[(usize, usize)],
) -> ((usize, usize), (usize, usize)) {
    let source_center = (source_bounds.center_x(), source_bounds.center_y());
    let target_center = (target_bounds.center_x(), target_bounds.center_y());

    // Source attachment: intersect towards first waypoint or target center
    let source_attach = if let Some(&first_wp) = waypoints.first() {
        intersect_node(source_bounds, first_wp, source_shape)
    } else {
        intersect_node(source_bounds, target_center, source_shape)
    };

    // Target attachment: intersect towards last waypoint or source center
    let target_attach = if let Some(&last_wp) = waypoints.last() {
        intersect_node(target_bounds, last_wp, target_shape)
    } else {
        intersect_node(target_bounds, source_center, target_shape)
    };

    (source_attach, target_attach)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bounds() -> NodeBounds {
        NodeBounds {
            x: 10,
            y: 5,
            width: 10,
            height: 5,
            layout_center_x: None,
            layout_center_y: None,
        }
    }

    #[test]
    fn test_intersect_rect_from_below() {
        let bounds = test_bounds();
        // Point directly below center
        let point = FloatPoint::new(15.0, 20.0);
        let result = intersect_rect(&bounds, point);

        // Should hit bottom edge at x=15
        // center_y = 5 + 5/2 = 7.5 (as f64), half_height = 2.5
        // intersection y = 7.5 + 2.5 = 10
        assert_eq!(result.x.round() as usize, 15);
        assert_eq!(result.y.round() as usize, 10);
    }

    #[test]
    fn test_intersect_rect_from_above() {
        let bounds = test_bounds();
        // Point directly above center
        let point = FloatPoint::new(15.0, 0.0);
        let result = intersect_rect(&bounds, point);

        // Should hit top edge
        assert_eq!(result.x.round() as usize, 15);
        assert!(result.y < bounds.center_y() as f64);
    }

    #[test]
    fn test_intersect_rect_from_right() {
        let bounds = test_bounds();
        // Point directly to the right
        let point = FloatPoint::new(30.0, 7.5);
        let result = intersect_rect(&bounds, point);

        // Should hit right edge
        assert!(result.x > bounds.center_x() as f64);
        assert_eq!(result.y.round() as usize, bounds.center_y());
    }

    #[test]
    fn test_intersect_rect_from_left() {
        let bounds = test_bounds();
        // Point directly to the left
        let point = FloatPoint::new(0.0, 7.5);
        let result = intersect_rect(&bounds, point);

        // Should hit left edge
        assert!(result.x < bounds.center_x() as f64);
        assert_eq!(result.y.round() as usize, bounds.center_y());
    }

    #[test]
    fn test_intersect_rect_diagonal() {
        let bounds = test_bounds();
        // Point at a diagonal
        let point = FloatPoint::new(25.0, 15.0);
        let result = intersect_rect(&bounds, point);

        // Should be on the boundary
        let on_right = (result.x - (bounds.x + bounds.width) as f64).abs() < 1.0;
        let on_bottom = (result.y - (bounds.y + bounds.height) as f64).abs() < 1.0;
        assert!(on_right || on_bottom);
    }

    #[test]
    fn test_intersect_diamond_from_below() {
        let bounds = test_bounds();
        // Point directly below center
        let point = FloatPoint::new(15.0, 20.0);
        let result = intersect_diamond(&bounds, point);

        // Should hit bottom vertex
        assert_eq!(result.x.round() as usize, bounds.center_x());
        // y should be at bottom vertex
        assert!(result.y > bounds.center_y() as f64);
    }

    #[test]
    fn test_intersect_diamond_from_right() {
        let bounds = test_bounds();
        // Point directly to the right
        let point = FloatPoint::new(30.0, 7.5);
        let result = intersect_diamond(&bounds, point);

        // Should hit right vertex
        assert!(result.x > bounds.center_x() as f64);
        assert_eq!(result.y.round() as usize, bounds.center_y());
    }

    #[test]
    fn test_intersect_node_rectangle() {
        let bounds = test_bounds();
        let point = (15, 20);
        let result = intersect_node(&bounds, point, Shape::Rectangle);

        // Should be on the boundary
        assert!(result.1 >= bounds.y);
        assert!(result.1 <= bounds.y + bounds.height);
    }

    #[test]
    fn test_intersect_node_diamond() {
        let bounds = test_bounds();
        let point = (15, 20);
        let result = intersect_node(&bounds, point, Shape::Diamond);

        // Should be on the boundary
        assert!(result.1 >= bounds.y);
        assert!(result.1 <= bounds.y + bounds.height);
    }

    #[test]
    fn test_calculate_attachment_points_direct() {
        let source = NodeBounds {
            x: 10,
            y: 5,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        let target = NodeBounds {
            x: 10,
            y: 15,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };

        let (src_attach, tgt_attach) =
            calculate_attachment_points(&source, Shape::Rectangle, &target, Shape::Rectangle, &[]);

        // Source should attach at bottom
        assert!(src_attach.1 > source.y);
        // Target should attach at top
        assert!(tgt_attach.1 < target.y + target.height);
    }

    #[test]
    fn test_calculate_attachment_points_with_waypoints() {
        let source = NodeBounds {
            x: 10,
            y: 5,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        let target = NodeBounds {
            x: 30,
            y: 15,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        let waypoints = [(20, 10), (25, 12)];

        let (src_attach, tgt_attach) = calculate_attachment_points(
            &source,
            Shape::Rectangle,
            &target,
            Shape::Rectangle,
            &waypoints,
        );

        // Source attaches towards first waypoint
        // Target attaches towards last waypoint
        assert!(src_attach.0 >= source.x && src_attach.0 <= source.x + source.width);
        assert!(tgt_attach.0 >= target.x && tgt_attach.0 <= target.x + target.width);
    }

    #[test]
    fn test_intersect_diamond_from_above() {
        let bounds = test_bounds();
        // Point directly above center
        let point = FloatPoint::new(15.0, 0.0);
        let result = intersect_diamond(&bounds, point);

        // Should hit top vertex
        assert_eq!(result.x.round() as usize, bounds.center_x());
        // y should be at top vertex
        assert!(result.y < bounds.center_y() as f64);
    }

    #[test]
    fn test_intersect_diamond_from_left() {
        let bounds = test_bounds();
        // Point directly to the left
        let point = FloatPoint::new(0.0, 7.5);
        let result = intersect_diamond(&bounds, point);

        // Should hit left vertex
        assert!(result.x < bounds.center_x() as f64);
        assert_eq!(result.y.round() as usize, bounds.center_y());
    }

    #[test]
    fn test_intersect_diamond_diagonal() {
        let bounds = test_bounds();
        // Point at a diagonal (bottom-right quadrant)
        let point = FloatPoint::new(25.0, 15.0);
        let result = intersect_diamond(&bounds, point);

        // Should be on the diamond boundary
        // For a diamond, |dx|/w + |dy|/h = 1 at the boundary
        let center_x = bounds.center_x() as f64;
        let center_y = bounds.center_y() as f64;
        let dx = (result.x - center_x).abs();
        let dy = (result.y - center_y).abs();
        let w = bounds.width as f64 / 2.0;
        let h = bounds.height as f64 / 2.0;

        let boundary_check = dx / w + dy / h;
        assert!(
            (boundary_check - 1.0).abs() < 0.1,
            "Point should be on diamond boundary, got {}",
            boundary_check
        );
    }

    #[test]
    fn test_intersect_rect_point_at_center() {
        let bounds = test_bounds();
        // Point exactly at center
        let point = FloatPoint::new(bounds.center_x() as f64, bounds.center_y() as f64);
        let result = intersect_rect(&bounds, point);

        // Should return bottom center as default
        assert_eq!(result.x.round() as usize, bounds.center_x());
    }

    #[test]
    fn test_intersect_diamond_point_at_center() {
        let bounds = test_bounds();
        // Point exactly at center
        let point = FloatPoint::new(bounds.center_x() as f64, bounds.center_y() as f64);
        let result = intersect_diamond(&bounds, point);

        // Should return bottom vertex as default
        assert_eq!(result.x.round() as usize, bounds.center_x());
    }

    #[test]
    fn test_intersect_node_round_uses_rect() {
        let bounds = test_bounds();
        let point = (15, 20);

        // Round shape should use rectangle intersection (approximation)
        let rect_result = intersect_node(&bounds, point, Shape::Rectangle);
        let round_result = intersect_node(&bounds, point, Shape::Round);

        assert_eq!(rect_result, round_result);
    }

    #[test]
    fn test_calculate_attachment_points_diamond_source() {
        let source = NodeBounds {
            x: 10,
            y: 5,
            width: 10,
            height: 5,
            layout_center_x: None,
            layout_center_y: None,
        };
        let target = NodeBounds {
            x: 10,
            y: 20,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };

        let (src_attach, tgt_attach) =
            calculate_attachment_points(&source, Shape::Diamond, &target, Shape::Rectangle, &[]);

        // Source (diamond) should attach at bottom vertex
        assert_eq!(src_attach.0, source.center_x());
        // Target should attach at top
        assert!(tgt_attach.1 < target.y + target.height);
    }

    #[test]
    fn test_float_point_to_usize() {
        let p = FloatPoint::new(10.4, 20.6);
        let (x, y) = p.to_usize();
        assert_eq!(x, 10);
        assert_eq!(y, 21);
    }

    #[test]
    fn test_float_point_from_tuple() {
        let p = FloatPoint::from((15_usize, 25_usize));
        assert_eq!(p.x, 15.0);
        assert_eq!(p.y, 25.0);
    }

    // --- classify_face tests ---

    #[test]
    fn test_classify_face_from_above() {
        let bounds = test_bounds(); // x=10, y=5, w=10, h=5, center=(15,7)
        let result = classify_face(&bounds, (15, 0), Shape::Rectangle);
        assert_eq!(result, NodeFace::Top);
    }

    #[test]
    fn test_classify_face_from_below() {
        let bounds = test_bounds();
        let result = classify_face(&bounds, (15, 20), Shape::Rectangle);
        assert_eq!(result, NodeFace::Bottom);
    }

    #[test]
    fn test_classify_face_from_left() {
        let bounds = test_bounds();
        let result = classify_face(&bounds, (0, 7), Shape::Rectangle);
        assert_eq!(result, NodeFace::Left);
    }

    #[test]
    fn test_classify_face_from_right() {
        let bounds = test_bounds();
        let result = classify_face(&bounds, (30, 7), Shape::Rectangle);
        assert_eq!(result, NodeFace::Right);
    }

    #[test]
    fn test_classify_face_degenerate_center() {
        let bounds = test_bounds();
        let result = classify_face(&bounds, (15, 7), Shape::Rectangle);
        assert_eq!(result, NodeFace::Bottom);
    }

    #[test]
    fn test_classify_face_diamond_same_as_rect() {
        let bounds = test_bounds();
        assert_eq!(
            classify_face(&bounds, (15, 0), Shape::Diamond),
            NodeFace::Top
        );
        assert_eq!(
            classify_face(&bounds, (15, 20), Shape::Diamond),
            NodeFace::Bottom
        );
    }

    // --- spread_points_on_face tests ---

    #[test]
    fn test_spread_points_count_zero() {
        let result = spread_points_on_face(NodeFace::Top, 5, (2, 10), 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_spread_points_count_one() {
        // N=1 on range (2, 10) => mid = 2 + (10-2)/2 = 6
        let result = spread_points_on_face(NodeFace::Bottom, 5, (2, 10), 1);
        assert_eq!(result, vec![(6, 5)]);
    }

    #[test]
    fn test_spread_points_endpoint_maximizing() {
        // 2 edges on range (10, 13), range=3
        // Endpoint: positions at 10, 13 (maximized separation)
        let points = spread_points_on_face(NodeFace::Top, 5, (10, 13), 2);
        assert_eq!(points, vec![(10, 5), (13, 5)]);
    }

    #[test]
    fn test_spread_points_two_on_wide_range() {
        // 2 edges on range (0, 8), range=8
        // Endpoint: positions at 0, 8
        let result = spread_points_on_face(NodeFace::Top, 0, (0, 8), 2);
        assert_eq!(result, vec![(0, 0), (8, 0)]);
    }

    #[test]
    fn test_spread_points_count_three() {
        // N=3 on range (0, 8) => endpoint positions: 0, 4, 8
        let result = spread_points_on_face(NodeFace::Bottom, 10, (0, 8), 3);
        assert_eq!(result, vec![(0, 10), (4, 10), (8, 10)]);
    }

    #[test]
    fn test_spread_points_left_right_face() {
        // For Left/Right faces, coordinates are (fixed, pos)
        // 2 edges on range (0, 8) => endpoint: 0, 8
        let result = spread_points_on_face(NodeFace::Left, 5, (0, 8), 2);
        assert_eq!(result[0], (5, 0));
        assert_eq!(result[1], (5, 8));
    }

    #[test]
    fn test_spread_points_narrow_range() {
        // N=3 on range (0, 2): can't enforce MIN_GAP=2, falls back to endpoint formula
        let result = spread_points_on_face(NodeFace::Top, 0, (0, 2), 3);
        assert_eq!(result, vec![(0, 0), (1, 0), (2, 0)]);
    }

    #[test]
    fn test_spread_points_min_gap_sufficient_range() {
        // 4 edges on range (0, 7), range=7
        // Endpoint formula: 0, 2, 4, 7 (gaps: 2, 2, 3) — all >= MIN_GAP
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 7), 4);
        let xs: Vec<usize> = points.iter().map(|&(x, _)| x).collect();
        for w in xs.windows(2) {
            assert!(
                w[1] - w[0] >= 2,
                "gap too small between {} and {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn test_spread_points_min_gap_insufficient_range() {
        // 4 edges on range (0, 5), range=5
        // Endpoint formula: 0, 1, 3, 5 (gap of 1 between 0 and 1)
        // needed_span = 3*2 = 6 > 5, so gap can't be fully enforced
        // Graceful degradation: keep endpoint positions, don't panic
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 5), 4);
        let xs: Vec<usize> = points.iter().map(|&(x, _)| x).collect();
        assert_eq!(xs, vec![0, 1, 3, 5]);
    }

    #[test]
    fn test_spread_points_min_gap_barely_sufficient() {
        // 4 edges on range (0, 8), range=8
        // needed_span = 3*2 = 6 <= 8, gap enforcement active
        // Endpoint formula: 0, 2, 5, 8 (gaps: 2, 3, 3) — already >= 2
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 8), 4);
        let xs: Vec<usize> = points.iter().map(|&(x, _)| x).collect();
        assert_eq!(xs, vec![0, 2, 5, 8]);
        for w in xs.windows(2) {
            assert!(w[1] - w[0] >= 2, "gap too small: {} to {}", w[0], w[1]);
        }
    }

    #[test]
    fn test_spread_points_min_gap_three_on_three() {
        // 3 edges on range (0, 3), range=3
        // Endpoint formula: 0, 1, 3 (gap of 1 between first two — violates MIN_GAP=2)
        // With MIN_GAP: needed span = 2*2 = 4 > 3, can't enforce fully
        // Should still produce valid output within range
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 3), 3);
        let xs: Vec<usize> = points.iter().map(|&(x, _)| x).collect();
        assert_eq!(xs.len(), 3);
        // All within range and non-decreasing
        for &x in &xs {
            assert!(x <= 3);
        }
        for w in xs.windows(2) {
            assert!(w[1] >= w[0]);
        }
    }

    #[test]
    fn test_spread_points_min_gap_wide_face() {
        // 3 edges on range (0, 20), range=20
        // Endpoint formula: positions 0, 10, 20 — all gaps >= 2
        // MIN_GAP should not alter these positions
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 20), 3);
        assert_eq!(points, vec![(0, 0), (10, 0), (20, 0)]);
    }

    #[test]
    fn test_spread_points_min_gap_exact_fit() {
        // 3 edges on range (0, 4), range=4, MIN_GAP=2
        // Endpoint: 0, 2, 4 — exactly fits with gap=2
        let points = spread_points_on_face(NodeFace::Top, 0, (0, 4), 3);
        assert_eq!(points, vec![(0, 0), (2, 0), (4, 0)]);
    }

    #[test]
    fn test_spread_points_five_on_wide_face() {
        // N=5 on range (0, 12) => endpoint positions: 0, 3, 6, 9, 12
        let result = spread_points_on_face(NodeFace::Bottom, 0, (0, 12), 5);
        assert_eq!(result, vec![(0, 0), (3, 0), (6, 0), (9, 0), (12, 0)]);
    }
}
