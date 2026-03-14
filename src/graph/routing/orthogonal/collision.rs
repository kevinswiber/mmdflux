use super::constants::POINT_EPS;
use crate::graph::geometry::{GraphGeometry, LayoutEdge};
use crate::graph::space::{FPoint, FRect};

pub(super) fn axis_aligned_segment_crosses_rect_interior(
    a: FPoint,
    b: FPoint,
    rect: FRect,
    margin: f64,
) -> bool {
    let left = rect.x + margin;
    let right = rect.x + rect.width - margin;
    let top = rect.y + margin;
    let bottom = rect.y + rect.height - margin;
    if left >= right || top >= bottom {
        return false;
    }

    if (a.y - b.y).abs() <= POINT_EPS {
        let seg_y = a.y;
        if seg_y <= top || seg_y >= bottom {
            return false;
        }
        let seg_min_x = a.x.min(b.x);
        let seg_max_x = a.x.max(b.x);
        return seg_max_x > left && seg_min_x < right;
    }

    if (a.x - b.x).abs() <= POINT_EPS {
        let seg_x = a.x;
        if seg_x <= left || seg_x >= right {
            return false;
        }
        let seg_min_y = a.y.min(b.y);
        let seg_max_y = a.y.max(b.y);
        return seg_max_y > top && seg_min_y < bottom;
    }

    false
}

pub(super) fn segment_crosses_any_other_node_interior(
    edge: &LayoutEdge,
    geometry: &GraphGeometry,
    a: FPoint,
    b: FPoint,
    margin: f64,
) -> bool {
    geometry.nodes.iter().any(|(node_id, node)| {
        if node_id == &edge.from || node_id == &edge.to {
            return false;
        }
        axis_aligned_segment_crosses_rect_interior(a, b, node.rect, margin)
    })
}
