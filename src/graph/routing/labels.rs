use crate::graph::Graph;
use crate::graph::space::FPoint;

pub(crate) fn arc_length_midpoint(path: &[FPoint]) -> Option<FPoint> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return Some(path[0]);
    }
    let total_len: f64 = path
        .windows(2)
        .map(|seg| point_distance(seg[0], seg[1]))
        .sum();
    if total_len <= 1e-6 {
        return path.get(path.len() / 2).copied();
    }
    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for seg in path.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let seg_len = point_distance(a, b);
        if seg_len <= 1e-6 {
            continue;
        }
        if traversed + seg_len >= target {
            let t = (target - traversed) / seg_len;
            return Some(FPoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
        traversed += seg_len;
    }
    path.last().copied()
}

/// Compute head and tail label positions from a routed edge path.
///
/// Head labels are positioned near the path end (target), tail labels near
/// the start (source), both offset perpendicular to the edge direction.
pub(crate) fn compute_end_label_positions(path: &[FPoint]) -> (Option<FPoint>, Option<FPoint>) {
    if path.len() < 2 {
        return (None, None);
    }

    let perpendicular_offset = 12.0; // px offset from path
    let along_fraction = 0.12; // 12% from endpoint

    let total_len: f64 = path
        .windows(2)
        .map(|seg| point_distance(seg[0], seg[1]))
        .sum();
    if total_len <= 1e-6 {
        return (None, None);
    }

    let tail = interpolate_at_distance(path, total_len * along_fraction).map(|p| {
        let (dx, dy) = (path[1].x - path[0].x, path[1].y - path[0].y);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        FPoint::new(
            p.x + dy / len * perpendicular_offset,
            p.y - dx / len * perpendicular_offset,
        )
    });

    let n = path.len();
    let head = interpolate_at_distance(path, total_len * (1.0 - along_fraction)).map(|p| {
        let (dx, dy) = (path[n - 1].x - path[n - 2].x, path[n - 1].y - path[n - 2].y);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        FPoint::new(
            p.x + dy / len * perpendicular_offset,
            p.y - dx / len * perpendicular_offset,
        )
    });

    (head, tail)
}

/// Look up the diagram edge and compute end label positions if head/tail labels are present.
pub(crate) fn compute_end_labels_for_edge(
    diagram: &Graph,
    edge_index: usize,
    path: &[FPoint],
) -> (Option<FPoint>, Option<FPoint>) {
    let diagram_edge = diagram.edges.get(edge_index);
    let has_head = diagram_edge
        .map(|e| e.head_label.is_some())
        .unwrap_or(false);
    let has_tail = diagram_edge
        .map(|e| e.tail_label.is_some())
        .unwrap_or(false);
    if !has_head && !has_tail {
        return (None, None);
    }
    let (head_pos, tail_pos) = compute_end_label_positions(path);
    (
        if has_head { head_pos } else { None },
        if has_tail { tail_pos } else { None },
    )
}

fn point_distance(a: FPoint, b: FPoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn interpolate_at_distance(path: &[FPoint], distance: f64) -> Option<FPoint> {
    if path.len() < 2 {
        return path.first().copied();
    }
    let mut traversed = 0.0;
    for seg in path.windows(2) {
        let seg_len = point_distance(seg[0], seg[1]);
        if seg_len <= 1e-6 {
            continue;
        }
        if traversed + seg_len >= distance {
            let t = (distance - traversed) / seg_len;
            return Some(FPoint::new(
                seg[0].x + (seg[1].x - seg[0].x) * t,
                seg[0].y + (seg[1].y - seg[0].y) * t,
            ));
        }
        traversed += seg_len;
    }
    path.last().copied()
}
