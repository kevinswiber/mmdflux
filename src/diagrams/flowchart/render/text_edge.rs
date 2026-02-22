//! Edge rendering on the canvas.

use std::collections::HashMap;

use super::text_router::{AttachDirection, Point, RoutedEdge, Segment};
use crate::graph::{Arrow, Direction, Stroke};
use crate::render::canvas::{Canvas, Connections};
use crate::render::chars::CharSet;

/// Calculate the label position at the midpoint of a routed path.
///
/// Walks the segments by Manhattan distance and returns the point at 50%
/// of the total path length. Returns `None` if the path has no segments.
pub fn calc_label_position(segments: &[Segment]) -> Option<Point> {
    let first = segments.first()?;

    let total_length: usize = segments.iter().map(Segment::length).sum();
    if total_length == 0 {
        return Some(first.start_point());
    }

    let target = total_length / 2;
    let mut accumulated = 0usize;

    for seg in segments {
        let seg_len = seg.length();
        if accumulated + seg_len >= target {
            return Some(seg.point_at_offset(target - accumulated));
        }
        accumulated += seg_len;
    }

    segments.last().map(Segment::end_point)
}

const PRECOMPUTED_LABEL_MAX_DRIFT: f64 = 2.0;
const LABEL_POINT_EPS: f64 = 0.000_001;

fn point_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn distance_point_to_segment(point: (f64, f64), segment: &Segment) -> f64 {
    let start = segment.start_point();
    let end = segment.end_point();
    let a = (start.x as f64, start.y as f64);
    let b = (end.x as f64, end.y as f64);
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= LABEL_POINT_EPS {
        return point_distance(point, a);
    }
    let projection = ((point.0 - a.0) * dx + (point.1 - a.1) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = (a.0 + t * dx, a.1 + t * dy);
    point_distance(point, closest)
}

fn distance_point_to_path(point: (usize, usize), segments: &[Segment]) -> f64 {
    if segments.is_empty() {
        return f64::INFINITY;
    }
    let p = (point.0 as f64, point.1 as f64);
    segments
        .iter()
        .map(|segment| distance_point_to_segment(p, segment))
        .fold(f64::INFINITY, f64::min)
}

/// A label split into lines with precomputed dimensions.
#[derive(Debug)]
struct LabelBlock<'a> {
    lines: Vec<&'a str>,
    width: usize,
    height: usize,
}

fn label_block(label: &str) -> LabelBlock<'_> {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    LabelBlock {
        lines,
        width,
        height,
    }
}

fn label_top_for_center(center_y: usize, height: usize) -> usize {
    center_y.saturating_sub(height / 2)
}

fn label_center_from_top(top_y: usize, height: usize) -> usize {
    top_y + height / 2
}

fn exit_direction_from_segments(segments: &[Segment]) -> AttachDirection {
    match segments.first() {
        Some(Segment::Vertical { y_start, y_end, .. }) if *y_end > *y_start => {
            AttachDirection::Bottom
        }
        Some(Segment::Vertical { .. }) => AttachDirection::Top,
        Some(Segment::Horizontal { x_start, x_end, .. }) if *x_end > *x_start => {
            AttachDirection::Right
        }
        Some(Segment::Horizontal { .. }) => AttachDirection::Left,
        None => AttachDirection::Bottom,
    }
}

/// Render a routed edge onto the canvas.
pub fn render_edge(
    canvas: &mut Canvas,
    routed: &RoutedEdge,
    charset: &CharSet,
    diagram_direction: Direction,
) {
    if routed.edge.stroke == Stroke::Invisible {
        return;
    }

    let stroke = routed.edge.stroke;

    // Draw each segment
    for segment in &routed.segments {
        draw_segment(canvas, segment, stroke, charset);
    }

    // Draw arrow at the end point using entry direction
    if routed.edge.arrow_end != Arrow::None {
        draw_arrow_with_entry(
            canvas,
            &routed.end,
            routed.entry_direction,
            charset,
            routed.edge.arrow_end,
        );
    }

    // Draw arrow at the start point using exit direction (if not a self-edge)
    if routed.edge.arrow_start != Arrow::None && !routed.is_self_edge {
        let exit_direction = exit_direction_from_segments(&routed.segments);
        draw_arrow_with_entry(
            canvas,
            &routed.start,
            exit_direction,
            charset,
            routed.edge.arrow_start,
        );
    }

    // Draw label if present
    if let Some(label) = &routed.edge.label {
        draw_edge_label_with_tracking(canvas, routed, label, diagram_direction, &[], charset);
    }
}

/// Draw a label on an edge at an appropriate position along the edge path.
///
/// For forward edges, places the label at the midpoint between start and end.
/// For backward edges (routed around perimeter), places the label along the
/// actual routed path (typically on the longest waypoint segment).
/// If the label would collide with a node or another label, tries alternative positions.
///
/// Returns the placed label's bounding box if successfully placed.
fn draw_edge_label_with_tracking(
    canvas: &mut Canvas,
    routed: &RoutedEdge,
    label: &str,
    direction: Direction,
    placed_labels: &[PlacedLabel],
    charset: &CharSet,
) -> Option<PlacedLabel> {
    let block = label_block(label);
    let label_width = block.width;
    let label_height = block.height;

    // Calculate base position for label.
    // `on_h_seg` tracks whether we placed above a horizontal segment,
    // which means edge cell collisions should be ignored (the label
    // intentionally overwrites the jog line).
    let mut on_h_seg = false;
    let (base_x, base_center_y) = {
        match direction {
            Direction::TopDown | Direction::BottomTop => {
                // For vertical layouts with Z-shaped paths (3+ segments),
                // place label on the best segment available.
                if routed.segments.len() >= 3 {
                    let is_long_path = routed.segments.len() >= 6;

                    // For short forward paths, prefer placing the label centered
                    // above a horizontal segment when it's wide enough. This keeps
                    // labels on the horizontal "jog" of Z-paths rather than beside
                    // short vertical stubs where they can crowd adjacent edges.
                    let h_seg = if !is_long_path {
                        routed
                            .segments
                            .iter()
                            .filter(|s| match s {
                                Segment::Horizontal { x_start, x_end, .. } => {
                                    // Require padding so the label doesn't touch the
                                    // turn characters at segment endpoints.
                                    x_start.abs_diff(*x_end) >= label_width + 2
                                }
                                _ => false,
                            })
                            .max_by_key(|s| match s {
                                Segment::Horizontal { x_start, x_end, .. } => {
                                    x_start.abs_diff(*x_end)
                                }
                                _ => 0,
                            })
                    } else {
                        None
                    };

                    if let Some(Segment::Horizontal { y, x_start, x_end }) = h_seg {
                        let seg_min_x = (*x_start).min(*x_end);
                        let seg_max_x = (*x_start).max(*x_end);
                        let seg_len = seg_max_x - seg_min_x;
                        let label_x = seg_min_x + (seg_len - label_width) / 2;
                        on_h_seg = true;
                        (label_x, *y)
                    } else {
                        // Fall back to vertical segment placement.
                        // For backward edges, prefer the longest inner vertical segment.
                        // For forward edges, prefer the longest vertical near the source.
                        let chosen_seg = select_label_segment(&routed.segments);

                        if let Some(seg) = chosen_seg {
                            // Determine which side to place the label based on target position
                            let mut place_right = routed.end.x > routed.start.x;

                            // Check if the proposed position would place the label between
                            // two attachment ports. If an edge cell exists on the far side
                            // of the label, flip sides.
                            let (trial_x, trial_y) = find_label_position_on_segment_with_side(
                                seg,
                                label_width,
                                place_right,
                            );
                            if label_adjacent_to_edge_on_far_side(
                                canvas,
                                trial_x,
                                trial_y,
                                label_width,
                                label_height,
                                place_right,
                            ) {
                                place_right = !place_right;
                            }

                            find_label_position_on_segment_with_side(seg, label_width, place_right)
                        } else {
                            // Fallback to midpoint
                            let center_y = (routed.start.y + routed.end.y) / 2;
                            (routed.end.x.saturating_sub(label_width / 2), center_y)
                        }
                    }
                } else {
                    // Simple straight path - place label beside the edge line
                    let center_y = (routed.start.y + routed.end.y) / 2;
                    // Place label to the left of the edge, not centered on it
                    // This avoids collision with the edge line
                    let label_x = routed.end.x.saturating_sub(label_width + 1);
                    (label_x, center_y)
                }
            }
            Direction::LeftRight => {
                if routed.segments.len() >= 3 {
                    label_on_horizontal_segment(routed, label_width)
                } else {
                    // Short/straight path — keep existing inline placement
                    let center_y = (routed.start.y + routed.end.y) / 2;
                    let max_label_end = routed.end.x.saturating_sub(1);
                    let min_x = routed.start.x.saturating_add(1);
                    let available = max_label_end.saturating_sub(routed.start.x);
                    let label_x = if available >= label_width {
                        let centered = routed.start.x + (available - label_width) / 2;
                        let max_x = max_label_end.saturating_sub(label_width);
                        centered.max(min_x).min(max_x)
                    } else {
                        min_x
                    };
                    (label_x, center_y)
                }
            }
            Direction::RightLeft => {
                if routed.segments.len() >= 3 {
                    label_on_horizontal_segment(routed, label_width)
                } else {
                    // Short/straight path — keep existing inline placement
                    let center_y = (routed.start.y + routed.end.y) / 2;
                    let mid_x = (routed.start.x + routed.end.x) / 2;
                    let label_x = mid_x.saturating_sub(label_width / 2);
                    let max_x = routed.start.x.saturating_sub(label_width + 1);
                    let min_x = routed.end.x.saturating_add(2);
                    let label_x = if max_x < min_x {
                        let available = routed.start.x.saturating_sub(routed.end.x);
                        if available >= label_width {
                            routed.end.x + (available - label_width) / 2
                        } else {
                            routed.end.x
                        }
                    } else {
                        label_x.max(min_x).min(max_x)
                    };
                    (label_x, center_y)
                }
            }
        }
    };
    let base_y = label_top_for_center(base_center_y, label_height);

    // Try to find a position that doesn't collide with nodes or other labels.
    // When placed above a horizontal segment, skip edge collision checks since
    // the label intentionally overwrites edge cells on the jog line.
    let check_edge = !on_h_seg;
    let (label_x, label_y) = find_safe_label_position(
        canvas,
        (base_x, base_y),
        (label_width, label_height),
        direction,
        placed_labels,
        check_edge,
        charset,
    );

    // If collision avoidance displaced the label far from its base (more
    // than 2 rows), retry at the overall edge midpoint, centered on the
    // edge line. The segment-level midpoint can land on a node-border row;
    // the edge midpoint sits between the two node rows where there is room.
    // Edge collision is ignored since the label intentionally overwrites the
    // edge path character at this position.
    let (label_x, label_y) =
        if base_center_y.abs_diff(label_center_from_top(label_y, label_height)) > 2 {
            let alt_center_y = (routed.start.y + routed.end.y) / 2;
            let alt_x = routed.end.x.saturating_sub(label_width / 2);
            let alt_y = label_top_for_center(alt_center_y, label_height);
            find_safe_label_position(
                canvas,
                (alt_x, alt_y),
                (label_width, label_height),
                direction,
                placed_labels,
                false,
                charset,
            )
        } else {
            (label_x, label_y)
        };
    // Expand canvas if the label would extend past the right edge
    let needed_width = label_x + label_width;
    if needed_width > canvas.width() {
        canvas.expand_width(needed_width);
    }

    // Write the label block only to non-node cells, avoiding the arrow positions.
    let arrow_pos = (routed.end.x, routed.end.y);
    let arrow_start_pos = (routed.start.x, routed.start.y);
    write_label_block(
        canvas,
        &block.lines,
        label_x,
        label_y,
        label_width,
        charset,
        &[arrow_pos, arrow_start_pos],
    );

    Some(PlacedLabel {
        x: label_x,
        y: label_y,
        width: label_width,
        height: label_height,
    })
}

/// Position a label above the best horizontal segment for LR/RL multi-segment edges.
///
/// Shared by both LeftRight and RightLeft layout branches. Centers the label on
/// the widest horizontal segment when possible, otherwise falls back to the
/// midpoint between source and target anchored to the source y.
fn label_on_horizontal_segment(routed: &RoutedEdge, label_len: usize) -> (usize, usize) {
    if let Some(Segment::Horizontal { y, x_start, x_end }) =
        select_label_segment_horizontal(&routed.segments)
    {
        let seg_min_x = (*x_start).min(*x_end);
        let seg_max_x = (*x_start).max(*x_end);
        let seg_len = seg_max_x - seg_min_x;
        let label_x = if seg_len >= label_len {
            seg_min_x + (seg_len - label_len) / 2
        } else {
            seg_min_x
        };
        (label_x, y.saturating_sub(1))
    } else {
        // Anchor y to source exit point, not averaged midpoint
        let anchor_y = routed.start.y.saturating_sub(1);
        let mid_x = (routed.start.x + routed.end.x) / 2;
        (mid_x.saturating_sub(label_len / 2), anchor_y)
    }
}

/// Find the label position on a segment, with control over which side to place it.
///
/// Only used for TD/BT layouts where edges have Z-shaped paths. LR/RL layouts
/// use inline label positioning with collision avoidance via `find_safe_label_position`.
///
/// For vertical segments (the typical case in TD/BT):
/// - `place_right = false`: label goes to the left of the segment
/// - `place_right = true`: label goes to the right of the segment
///
/// For horizontal segments (middle of Z-paths): label is placed above the segment.
fn find_label_position_on_segment_with_side(
    segment: &Segment,
    label_len: usize,
    place_right: bool,
) -> (usize, usize) {
    match segment {
        Segment::Vertical { x, y_start, y_end } => {
            let mid_y = (*y_start + *y_end) / 2;
            if place_right {
                // Place label to the right of the vertical line (1-space gap)
                (*x + 2, mid_y)
            } else {
                // Place label to the left of the vertical line
                // Prefer 1-space gap if there's room, otherwise place adjacent
                let needed_with_gap = label_len + 1;
                let label_x = if *x >= needed_with_gap {
                    x - needed_with_gap // 1-space gap
                } else {
                    x.saturating_sub(label_len) // no gap, tight fit
                };
                (label_x, mid_y)
            }
        }
        Segment::Horizontal { y, x_start, x_end } => {
            // For horizontal segments, place label above
            let mid_x = (*x_start + *x_end) / 2;
            let label_x = mid_x.saturating_sub(label_len / 2);
            (label_x, y.saturating_sub(1))
        }
    }
}

/// Find a safe position for an edge label that doesn't collide with nodes or other labels.
///
/// Tries the base position first, then shifts in the appropriate direction
/// based on the diagram layout until a collision-free position is found.
///
/// When `check_edge_collision` is false, labels can be placed over edge cells
/// (useful when intentionally centering above a horizontal segment where the
/// label is expected to overwrite the jog line).
fn find_safe_label_position(
    canvas: &Canvas,
    base: (usize, usize),
    label_size: (usize, usize),
    direction: Direction,
    placed_labels: &[PlacedLabel],
    check_edge_collision: bool,
    charset: &CharSet,
) -> (usize, usize) {
    let (base_x, base_y) = base;
    let (label_width, label_height) = label_size;
    let has_collision = |x, y| {
        label_collides_with_node(canvas, x, y, label_width, label_height)
            || (check_edge_collision
                && label_collides_with_edge(canvas, x, y, label_width, label_height))
            || label_collides_with_arrow(canvas, x, y, label_width, label_height, charset)
            || placed_labels
                .iter()
                .any(|p| p.overlaps(x, y, label_width, label_height))
    };

    // Check if the base position has any collision
    if !has_collision(base_x, base_y) {
        return (base_x, base_y);
    }

    // Try shifting positions based on diagram direction
    const VERTICAL_SHIFTS: &[(isize, isize)] = &[
        (0, -1),
        (0, 1),
        (0, -2),
        (0, 2),
        (-1, 0),
        (1, 0),
        (-2, 0),
        (2, 0),
        (0, -3),
        (0, 3),
        (-3, 0),
        (3, 0),
    ];
    const HORIZONTAL_SHIFTS: &[(isize, isize)] = &[
        (0, -1),
        (0, 1),
        (0, -2),
        (0, 2),
        (-1, 0),
        (1, 0),
        (0, -3),
        (0, 3),
    ];
    let shifts = match direction {
        Direction::TopDown | Direction::BottomTop => VERTICAL_SHIFTS,
        Direction::LeftRight | Direction::RightLeft => HORIZONTAL_SHIFTS,
    };

    // Try each shift until we find a collision-free position
    for (dx, dy) in shifts {
        let new_x = (base_x as isize + dx).max(0) as usize;
        let new_y = (base_y as isize + dy).max(0) as usize;

        if !has_collision(new_x, new_y) {
            return (new_x, new_y);
        }
    }

    // If all shifts fail, return the base position (will skip node cells when writing)
    (base_x, base_y)
}

/// Check if placing a label at the given position would collide with any node cells.
fn label_collides_with_node(
    canvas: &Canvas,
    x: usize,
    y: usize,
    label_width: usize,
    label_height: usize,
) -> bool {
    (0..label_height).any(|dy| {
        (0..label_width).any(|dx| canvas.get(x + dx, y + dy).is_some_and(|cell| cell.is_node))
    })
}

/// Check if placing a label at the given position would collide with any edge cells.
fn label_collides_with_edge(
    canvas: &Canvas,
    x: usize,
    y: usize,
    label_width: usize,
    label_height: usize,
) -> bool {
    (0..label_height).any(|dy| {
        (0..label_width).any(|dx| canvas.get(x + dx, y + dy).is_some_and(|cell| cell.is_edge))
    })
}

/// Check if placing a label at the given position would collide with any arrow characters.
fn label_collides_with_arrow(
    canvas: &Canvas,
    x: usize,
    y: usize,
    label_width: usize,
    label_height: usize,
    charset: &CharSet,
) -> bool {
    (0..label_height).any(|dy| {
        (0..label_width).any(|dx| {
            canvas
                .get(x + dx, y + dy)
                .is_some_and(|cell| charset.is_arrow(cell.ch))
        })
    })
}

/// Check if an edge cell exists on the far side of a proposed label position.
///
/// When a label is placed next to a vertical segment, this detects whether
/// there's another edge nearby on the opposite side, which would mean the
/// label is sandwiched between two attachment ports (visually ambiguous).
///
/// `place_right` indicates the side the label was placed on relative to its segment.
/// We check the far side (right edge of label if place_right, left edge if !place_right).
fn label_adjacent_to_edge_on_far_side(
    canvas: &Canvas,
    label_x: usize,
    label_y: usize,
    label_width: usize,
    label_height: usize,
    place_right: bool,
) -> bool {
    if place_right {
        // Label is to the right of its segment; check cells just after the label end
        let check_x = label_x + label_width;
        (0..label_height).any(|dy| {
            let y = label_y + dy;
            (0..=1).any(|offset| {
                canvas
                    .get(check_x + offset, y)
                    .is_some_and(|cell| cell.is_edge)
            })
        })
    } else {
        // Label is to the left of its segment; check cells just before the label start
        (0..label_height).any(|dy| {
            let y = label_y + dy;
            (1..=2).any(|offset| {
                label_x
                    .checked_sub(offset)
                    .and_then(|x| canvas.get(x, y))
                    .is_some_and(|cell| cell.is_edge)
            })
        })
    }
}

/// Return the inner segments of an edge path, excluding the first and last
/// stub segments near the source and target nodes. Falls back to the full
/// slice when there are 2 or fewer segments.
fn inner_segments(segments: &[Segment]) -> &[Segment] {
    match segments.len() {
        0..=2 => segments,
        n => &segments[1..n - 1],
    }
}

/// Select the best segment for placing a label on a multi-segment edge.
///
/// For forward edges (few segments), returns the last vertical segment
/// approaching the target — labels near the target are clear for short paths.
///
/// For backward edges (many segments routed via layout waypoints), returns the
/// longest vertical segment. This is typically the long waypoint path spanning
/// multiple ranks, which is isolated from other edges and avoids crowding near
/// the target node.
fn select_label_segment(segments: &[Segment]) -> Option<&Segment> {
    fn vertical_length(s: &Segment) -> usize {
        match s {
            Segment::Vertical { y_start, y_end, .. } => y_start.abs_diff(*y_end),
            _ => 0,
        }
    }

    fn longest_vertical<'a>(segs: impl Iterator<Item = &'a Segment>) -> Option<&'a Segment> {
        segs.filter(|s| matches!(s, Segment::Vertical { .. }))
            .max_by_key(|s| vertical_length(s))
    }

    // Backward edges routed through layout waypoints typically have 6+ segments.
    // Forward Z-paths typically have 3-4 segments.
    let is_long_path = segments.len() >= 6;

    if is_long_path {
        // For long paths (backward edges), find the longest vertical segment
        // in the inner portion, falling back to the last vertical segment.
        longest_vertical(inner_segments(segments).iter()).or_else(|| {
            segments
                .iter()
                .rev()
                .find(|s| matches!(s, Segment::Vertical { .. }))
        })
    } else {
        // For short paths, prefer the longest vertical segment nearest to source.
        // Iterating in reverse makes max_by_key's last-wins tie-breaking favor
        // earlier segments.
        longest_vertical(segments.iter().rev())
    }
}

/// Select the best horizontal segment for label placement on LR/RL edges.
///
/// Analogous to `select_label_segment()` for TD/BT vertical segments.
/// For long paths (backward edges, 6+ segments), returns the longest inner horizontal segment.
/// For shorter paths, returns the last horizontal segment.
fn select_label_segment_horizontal(segments: &[Segment]) -> Option<&Segment> {
    fn horizontal_length(s: &Segment) -> usize {
        match s {
            Segment::Horizontal { x_start, x_end, .. } => x_start.abs_diff(*x_end),
            _ => 0,
        }
    }

    fn longest_horizontal<'a>(segs: impl Iterator<Item = &'a Segment>) -> Option<&'a Segment> {
        segs.filter(|s| matches!(s, Segment::Horizontal { .. }))
            .max_by_key(|s| horizontal_length(s))
    }

    let is_long_path = segments.len() >= 6;

    if is_long_path {
        longest_horizontal(inner_segments(segments).iter()).or_else(|| {
            segments
                .iter()
                .rev()
                .find(|s| matches!(s, Segment::Horizontal { .. }))
        })
    } else {
        // For LR/RL short paths, the last horizontal segment approaches the
        // target at a unique Y position, so labels on sibling edges naturally
        // separate vertically.
        segments
            .iter()
            .rev()
            .find(|s| matches!(s, Segment::Horizontal { .. }))
    }
}

/// Draw a single segment on the canvas, honoring stroke style.
fn draw_segment(canvas: &mut Canvas, segment: &Segment, stroke: Stroke, charset: &CharSet) {
    match segment {
        Segment::Vertical { x, y_start, y_end } => {
            let (y_min, y_max) = if y_start < y_end {
                (*y_start, *y_end)
            } else {
                (*y_end, *y_start)
            };

            for y in y_min..=y_max {
                let connections = Connections {
                    up: y > y_min,
                    down: y < y_max,
                    left: false,
                    right: false,
                };
                canvas.set_with_connection(*x, y, connections, charset, stroke);
            }
        }
        Segment::Horizontal { y, x_start, x_end } => {
            let (x_min, x_max) = if x_start < x_end {
                (*x_start, *x_end)
            } else {
                (*x_end, *x_start)
            };

            for x in x_min..=x_max {
                let connections = Connections {
                    up: false,
                    down: false,
                    left: x > x_min,
                    right: x < x_max,
                };
                canvas.set_with_connection(x, *y, connections, charset, stroke);
            }
        }
    }
}

/// Draw an arrow at the given point based on entry direction.
///
/// The arrow points in the direction the edge is coming from (into the target).
fn draw_arrow_with_entry(
    canvas: &mut Canvas,
    point: &Point,
    entry_direction: AttachDirection,
    charset: &CharSet,
    arrow_type: Arrow,
) {
    // Protect node content from being overwritten by arrows
    if canvas
        .get(point.x, point.y)
        .is_some_and(|cell| cell.is_node)
    {
        return;
    }

    // Select arrow character based on type and direction
    let arrow_char = match (arrow_type, entry_direction) {
        (Arrow::Normal, AttachDirection::Top) => charset.arrow_down,
        (Arrow::Normal, AttachDirection::Bottom) => charset.arrow_up,
        (Arrow::Normal, AttachDirection::Left) => charset.arrow_right,
        (Arrow::Normal, AttachDirection::Right) => charset.arrow_left,
        (Arrow::Cross, AttachDirection::Top) => charset.arrow_cross_down,
        (Arrow::Cross, AttachDirection::Bottom) => charset.arrow_cross_up,
        (Arrow::Cross, AttachDirection::Left) => charset.arrow_cross_right,
        (Arrow::Cross, AttachDirection::Right) => charset.arrow_cross_left,
        (Arrow::Circle, AttachDirection::Top) => charset.arrow_circle_down,
        (Arrow::Circle, AttachDirection::Bottom) => charset.arrow_circle_up,
        (Arrow::Circle, AttachDirection::Left) => charset.arrow_circle_right,
        (Arrow::Circle, AttachDirection::Right) => charset.arrow_circle_left,
        (Arrow::OpenTriangle, AttachDirection::Top) => charset.arrow_open_down,
        (Arrow::OpenTriangle, AttachDirection::Bottom) => charset.arrow_open_up,
        (Arrow::OpenTriangle, AttachDirection::Left) => charset.arrow_open_right,
        (Arrow::OpenTriangle, AttachDirection::Right) => charset.arrow_open_left,
        (Arrow::Diamond, _) => charset.arrow_diamond,
        (Arrow::OpenDiamond, _) => charset.arrow_open_diamond,
        (Arrow::None, _) => return,
    };

    // If the arrow position is a subgraph title or border cell, nudge it one cell inward
    // (in the direction the edge is traveling). This keeps arrowheads inside boxes.
    let (ax, ay) = match canvas.get(point.x, point.y) {
        Some(cell) if cell.is_subgraph_title || cell.is_subgraph_border => {
            let (nx, ny) = match entry_direction {
                AttachDirection::Top => (point.x, point.y + 1),
                AttachDirection::Bottom => (point.x, point.y.saturating_sub(1)),
                AttachDirection::Left => (point.x + 1, point.y),
                AttachDirection::Right => (point.x.saturating_sub(1), point.y),
            };
            // Don't nudge into a node cell
            if canvas.get(nx, ny).is_some_and(|inner| inner.is_node) {
                (point.x, point.y)
            } else {
                (nx, ny)
            }
        }
        _ => (point.x, point.y),
    };

    canvas.set(ax, ay, arrow_char);
}

/// Draw an arrow at the given point (legacy function for tests).
#[cfg(test)]
fn draw_arrow(canvas: &mut Canvas, point: &Point, direction: Direction, charset: &CharSet) {
    let arrow_char = match direction {
        Direction::TopDown => charset.arrow_down,
        Direction::BottomTop => charset.arrow_up,
        Direction::LeftRight => charset.arrow_right,
        Direction::RightLeft => charset.arrow_left,
    };

    canvas.set(point.x, point.y, arrow_char);
}

/// A placed label's bounding box for collision detection.
#[derive(Debug, Clone)]
struct PlacedLabel {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl PlacedLabel {
    /// Check if this label overlaps with a proposed label position.
    fn overlaps(&self, x: usize, y: usize, width: usize, height: usize) -> bool {
        let self_end_x = self.x + self.width;
        let self_end_y = self.y + self.height;
        let other_end_x = x + width;
        let other_end_y = y + height;
        x < self_end_x && self.x < other_end_x && y < self_end_y && self.y < other_end_y
    }
}

/// Render all edges onto the canvas.
///
/// Draws all segments and arrows first, then all labels, ensuring labels
/// are not overwritten by later edge segments.
///
/// # Arguments
/// * `canvas` - The canvas to draw on
/// * `routed_edges` - The edges to render
/// * `charset` - Character set for drawing
/// * `diagram_direction` - Layout direction for label positioning
/// * `label_positions` - Optional pre-computed label positions from normalization
pub fn render_all_edges(
    canvas: &mut Canvas,
    routed_edges: &[RoutedEdge],
    charset: &CharSet,
    diagram_direction: Direction,
) {
    render_all_edges_with_labels(
        canvas,
        routed_edges,
        charset,
        diagram_direction,
        &HashMap::new(),
    )
}

/// Render all edges with optional pre-computed label positions.
pub fn render_all_edges_with_labels(
    canvas: &mut Canvas,
    routed_edges: &[RoutedEdge],
    charset: &CharSet,
    diagram_direction: Direction,
    label_positions: &HashMap<usize, (usize, usize)>,
) {
    // First pass: draw all segments and arrows
    for routed in routed_edges {
        if routed.edge.stroke == Stroke::Invisible {
            continue;
        }
        for segment in &routed.segments {
            draw_segment(canvas, segment, routed.edge.stroke, charset);
        }
        if routed.edge.arrow_end != Arrow::None {
            draw_arrow_with_entry(
                canvas,
                &routed.end,
                routed.entry_direction,
                charset,
                routed.edge.arrow_end,
            );
        }
        if routed.edge.arrow_start != Arrow::None && !routed.is_self_edge {
            let exit_direction = exit_direction_from_segments(&routed.segments);
            draw_arrow_with_entry(
                canvas,
                &routed.start,
                exit_direction,
                charset,
                routed.edge.arrow_start,
            );
        }
    }

    // Second pass: draw all labels (so they appear on top of segments)
    // Track placed labels to avoid collisions
    let mut placed_labels: Vec<PlacedLabel> = Vec::new();
    for routed in routed_edges {
        if let Some(label) = &routed.edge.label {
            // Check for pre-computed label position from normalization
            let block = label_block(label);
            let label_width = block.width;
            let label_height = block.height;

            // Use precomputed position if available and within canvas bounds,
            // otherwise fall back to heuristic placement.
            let allow_precomputed =
                routed.edge.from_subgraph.is_none() && routed.edge.to_subgraph.is_none();
            let mut stale_precomputed_anchor = false;
            let precomputed = if allow_precomputed {
                label_positions
                    .get(&routed.edge.index)
                    .and_then(|&(px, py)| {
                        let in_bounds = px < canvas.width()
                            && py < canvas.height()
                            && px.saturating_add(label_width) <= canvas.width();
                        if !in_bounds {
                            return None;
                        }
                        let drift = distance_point_to_path((px, py), &routed.segments);
                        if drift <= PRECOMPUTED_LABEL_MAX_DRIFT {
                            Some((px, py))
                        } else {
                            stale_precomputed_anchor = true;
                            None
                        }
                    })
            } else {
                None
            };

            let placed = if routed.is_self_edge || routed.is_backward {
                // For backward edges, compute label position from actual routed path
                // Center on midpoint, then run collision avoidance like forward edges
                if let Some(midpoint) = calc_label_position(&routed.segments) {
                    let base_x = midpoint.x.saturating_sub(label_width / 2);
                    let base_y = label_top_for_center(midpoint.y, label_height);
                    let (safe_x, safe_y) = find_safe_label_position(
                        canvas,
                        (base_x, base_y),
                        (label_width, label_height),
                        diagram_direction,
                        &placed_labels,
                        false,
                        charset,
                    );
                    draw_label_direct(canvas, label, safe_x, safe_y, charset)
                } else {
                    draw_edge_label_with_tracking(
                        canvas,
                        routed,
                        label,
                        diagram_direction,
                        &placed_labels,
                        charset,
                    )
                }
            } else if let Some((pre_x, pre_y)) = precomputed {
                // Defensive safety net: route precomputed position through
                // collision avoidance. When the midpoint formula is correct,
                // find_safe_label_position returns the base position unchanged.
                let base_x = pre_x.saturating_sub(label_width / 2);
                let base_y = label_top_for_center(pre_y, label_height);
                let (safe_x, safe_y) = find_safe_label_position(
                    canvas,
                    (base_x, base_y),
                    (label_width, label_height),
                    diagram_direction,
                    &placed_labels,
                    false,
                    charset,
                );
                draw_label_direct(canvas, label, safe_x, safe_y, charset)
            } else if stale_precomputed_anchor {
                if let Some(midpoint) = calc_label_position(&routed.segments) {
                    let base_x = midpoint.x.saturating_sub(label_width / 2);
                    let base_y = label_top_for_center(midpoint.y, label_height);
                    let (safe_x, safe_y) = find_safe_label_position(
                        canvas,
                        (base_x, base_y),
                        (label_width, label_height),
                        diagram_direction,
                        &placed_labels,
                        false,
                        charset,
                    );
                    draw_label_direct(canvas, label, safe_x, safe_y, charset)
                } else {
                    draw_edge_label_with_tracking(
                        canvas,
                        routed,
                        label,
                        diagram_direction,
                        &placed_labels,
                        charset,
                    )
                }
            } else {
                draw_edge_label_with_tracking(
                    canvas,
                    routed,
                    label,
                    diagram_direction,
                    &placed_labels,
                    charset,
                )
            };

            if let Some(p) = placed {
                placed_labels.push(p);
            }
        }
    }
}

/// Draw a label at an exact position (no centering adjustment).
///
/// Used for backward edge labels where the position is already computed
/// relative to the routed path. Expands the canvas if the label would
/// extend beyond the current bounds.
fn write_label_block(
    canvas: &mut Canvas,
    lines: &[&str],
    x: usize,
    y: usize,
    block_width: usize,
    charset: &CharSet,
    blocked_points: &[(usize, usize)],
) {
    for (line_idx, line) in lines.iter().enumerate() {
        let row_y = y + line_idx;
        let line_width = line.chars().count();
        let line_x = x + (block_width.saturating_sub(line_width) / 2);
        for (ch_idx, ch) in line.chars().enumerate() {
            let cell_x = line_x + ch_idx;
            if blocked_points
                .iter()
                .any(|&(bx, by)| bx == cell_x && by == row_y)
            {
                continue;
            }
            if canvas
                .get(cell_x, row_y)
                .is_some_and(|cell| !cell.is_node && !charset.is_arrow(cell.ch))
            {
                canvas.set(cell_x, row_y, ch);
            }
        }
    }
}

fn draw_label_direct(
    canvas: &mut Canvas,
    label: &str,
    x: usize,
    y: usize,
    charset: &CharSet,
) -> Option<PlacedLabel> {
    let block = label_block(label);
    let label_width = block.width;
    let label_height = block.height;

    // Expand canvas if label extends beyond current width
    let needed_width = x + label_width;
    if needed_width > canvas.width() {
        canvas.expand_width(needed_width);
    }

    write_label_block(canvas, &block.lines, x, y, label_width, charset, &[]);

    Some(PlacedLabel {
        x,
        y,
        width: label_width,
        height: label_height,
    })
}

#[cfg(test)]
mod tests {
    use super::super::text_adapter::compute_layout;
    use super::super::text_layout::TextLayoutConfig;
    use super::super::text_router::route_edge;
    use super::*;
    use crate::graph::{Diagram, Edge, Node};

    fn simple_diagram() -> Diagram {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B"));
        diagram
    }

    #[test]
    fn test_render_vertical_edge() {
        let diagram = simple_diagram();
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        // Should contain vertical line character or arrow
        assert!(output.contains('│') || output.contains('▼'));
    }

    #[test]
    fn test_render_edge_with_arrow() {
        let diagram = simple_diagram();
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        // Should contain down arrow for TD direction
        assert!(output.contains('▼'));
    }

    #[test]
    fn test_render_dotted_edge() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_stroke(Stroke::Dotted));

        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        // Dotted edge should be drawn (may or may not be visible depending on layout)
        // Just verify it doesn't crash
        let _output = canvas.to_string();
    }

    #[test]
    fn test_render_edge_without_arrow() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrow(Arrow::None));

        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::TopDown,
            None,
            None,
            false,
        )
        .unwrap();
        render_edge(&mut canvas, &routed, &charset, Direction::TopDown);

        let output = canvas.to_string();
        // Should NOT contain arrow for Arrow::None
        assert!(!output.contains('▼'));
    }

    #[test]
    fn test_draw_arrow_directions() {
        let charset = CharSet::unicode();

        // Test each direction
        let mut canvas = Canvas::new(10, 10);
        draw_arrow(&mut canvas, &Point::new(1, 1), Direction::TopDown, &charset);
        assert_eq!(canvas.get(1, 1).unwrap().ch, '▼');

        let mut canvas = Canvas::new(10, 10);
        draw_arrow(
            &mut canvas,
            &Point::new(1, 1),
            Direction::BottomTop,
            &charset,
        );
        assert_eq!(canvas.get(1, 1).unwrap().ch, '▲');

        let mut canvas = Canvas::new(10, 10);
        draw_arrow(
            &mut canvas,
            &Point::new(1, 1),
            Direction::LeftRight,
            &charset,
        );
        assert_eq!(canvas.get(1, 1).unwrap().ch, '►');

        let mut canvas = Canvas::new(10, 10);
        draw_arrow(
            &mut canvas,
            &Point::new(1, 1),
            Direction::RightLeft,
            &charset,
        );
        assert_eq!(canvas.get(1, 1).unwrap().ch, '◄');
    }

    #[test]
    fn test_render_all_edges() {
        let diagram = simple_diagram();
        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        let mut canvas = Canvas::new(layout.width, layout.height);
        let charset = CharSet::unicode();

        let routed_edges: Vec<_> = diagram
            .edges
            .iter()
            .filter_map(|e| route_edge(e, &layout, Direction::TopDown, None, None, false))
            .collect();

        render_all_edges(&mut canvas, &routed_edges, &charset, Direction::TopDown);

        // Should have rendered something
        let output = canvas.to_string();
        assert!(!output.trim().is_empty());
    }

    #[test]
    fn test_render_edge_with_label() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B").with_label("Yes"));

        let output = crate::render::render(&diagram, &crate::render::RenderOptions::default());
        // Should contain the label
        assert!(output.contains("Yes"));
    }

    #[test]
    fn test_render_multiline_edge_label_as_centered_block() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_label("yes\nno"));

        let output = crate::render::render(&diagram, &crate::render::RenderOptions::default());
        let lines: Vec<&str> = output.lines().collect();

        let yes_line = lines
            .iter()
            .position(|l| l.contains("yes"))
            .expect("missing first line of multiline label");
        let no_line = lines
            .iter()
            .position(|l| l.contains("no"))
            .expect("missing second line of multiline label");
        assert_eq!(
            no_line,
            yes_line + 1,
            "multiline label lines should render on consecutive rows:\n{output}"
        );

        let yes_col = lines[yes_line]
            .find("yes")
            .expect("could not locate 'yes' column");
        let no_col = lines[no_line]
            .find("no")
            .expect("could not locate 'no' column");
        assert!(
            yes_col.abs_diff(no_col) <= 1,
            "multiline label lines should stay horizontally aligned:\n{output}"
        );

        let a_line = lines
            .iter()
            .position(|l| l.contains(" A "))
            .expect("missing node A row");
        let b_line = lines
            .iter()
            .rposition(|l| l.contains(" B "))
            .expect("missing node B row");
        let edge_mid = (a_line + b_line) / 2;
        let label_mid = (yes_line + no_line) / 2;
        assert!(
            label_mid.abs_diff(edge_mid) <= 1,
            "multiline label should stay near the edge midpoint:\n{output}"
        );
    }

    #[test]
    fn test_label_rendered_at_precomputed_position() {
        let output = crate::render::render(
            &{
                let mut d = Diagram::new(Direction::TopDown);
                d.add_node(Node::new("A").with_label("A"));
                d.add_node(Node::new("B").with_label("B"));
                d.add_edge(Edge::new("A", "B").with_label("yes"));
                d
            },
            &crate::render::RenderOptions::default(),
        );

        assert!(output.contains("yes"), "Label 'yes' should be rendered");

        // Label should appear between A and B rows
        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines.iter().position(|l| l.contains('A')).unwrap();
        let b_line = lines.iter().rposition(|l| l.contains('B')).unwrap();
        let yes_line = lines.iter().position(|l| l.contains("yes")).unwrap();
        assert!(
            yes_line > a_line && yes_line < b_line,
            "Label at line {} should be between A (line {}) and B (line {})\n{}",
            yes_line,
            a_line,
            b_line,
            output
        );
    }

    #[test]
    fn precomputed_label_avoids_node_overlap() {
        // Build a LR diagram where nodes are wide enough that
        // a precomputed label position could land on a node boundary.
        // After rendering, verify the label text doesn't collide with node cells.
        let output = crate::render::render(
            &{
                let mut d = Diagram::new(Direction::LeftRight);
                d.add_node(Node::new("A").with_label("Working Dir"));
                d.add_node(Node::new("B").with_label("Staging Area"));
                d.add_node(Node::new("C").with_label("Local Repo"));
                d.add_edge(Edge::new("A", "B").with_label("git add"));
                d.add_edge(Edge::new("B", "C").with_label("git commit"));
                d
            },
            &crate::render::RenderOptions::default(),
        );

        // Both labels should be fully visible (not clipped by node boundaries)
        assert!(
            output.contains("git add"),
            "Label 'git add' should be fully visible:\n{output}"
        );
        assert!(
            output.contains("git commit"),
            "Label 'git commit' should be fully visible:\n{output}"
        );
    }

    #[test]
    fn test_labeled_edge_has_waypoints() {
        // Verify a labeled short edge (A->B) now produces waypoints
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        diagram.add_edge(Edge::new("A", "B").with_label("yes"));

        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);

        // The A->B edge should have waypoints from the label dummy
        let ab_edge_idx = diagram
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == "B")
            .expect("Should have an A→B edge")
            .index;
        assert!(
            layout.edge_waypoints.contains_key(&ab_edge_idx),
            "Labeled short edge should have waypoints from label dummy"
        );
    }

    #[test]
    fn test_segment_length() {
        let vertical = Segment::Vertical {
            x: 5,
            y_start: 10,
            y_end: 20,
        };
        assert_eq!(vertical.length(), 10);

        let horizontal = Segment::Horizontal {
            y: 5,
            x_start: 20,
            x_end: 10,
        };
        assert_eq!(horizontal.length(), 10);
    }

    #[test]
    fn test_label_collides_with_edge() {
        let mut canvas = Canvas::new(20, 10);
        let charset = CharSet::unicode();

        // Draw a horizontal edge segment
        let connections = Connections {
            up: false,
            down: false,
            left: true,
            right: true,
        };
        for x in 5..15 {
            canvas.set_with_connection(x, 5, connections, &charset, Stroke::Solid);
        }

        // Label at y=5 should collide with edge
        assert!(label_collides_with_edge(&canvas, 7, 5, 5, 1));

        // Label at y=4 should not collide
        assert!(!label_collides_with_edge(&canvas, 7, 4, 5, 1));

        // Label at y=6 should not collide
        assert!(!label_collides_with_edge(&canvas, 7, 6, 5, 1));

        // Partial overlap still collides
        assert!(label_collides_with_edge(&canvas, 3, 5, 5, 1)); // ends at x=7, overlapping edge at x=5-7
    }

    #[test]
    fn test_select_label_segment_horizontal_short_path() {
        // 3-segment H-V-H forward path
        let segments = vec![
            Segment::Horizontal {
                y: 5,
                x_start: 10,
                x_end: 20,
            },
            Segment::Vertical {
                x: 20,
                y_start: 5,
                y_end: 10,
            },
            Segment::Horizontal {
                y: 10,
                x_start: 20,
                x_end: 30,
            },
        ];
        let chosen = select_label_segment_horizontal(&segments);
        // For short paths, should return the last horizontal segment
        match chosen {
            Some(Segment::Horizontal { y, .. }) => assert_eq!(*y, 10),
            _ => panic!("Expected last horizontal segment at y=10"),
        }
    }

    #[test]
    fn test_select_label_segment_horizontal_long_path() {
        // 7-segment backward edge path
        let segments = vec![
            Segment::Horizontal {
                y: 3,
                x_start: 50,
                x_end: 55,
            }, // short exit stub
            Segment::Vertical {
                x: 55,
                y_start: 3,
                y_end: 12,
            },
            Segment::Horizontal {
                y: 12,
                x_start: 55,
                x_end: 5,
            }, // long bottom run (50 chars)
            Segment::Vertical {
                x: 5,
                y_start: 12,
                y_end: 3,
            },
            Segment::Horizontal {
                y: 3,
                x_start: 5,
                x_end: 10,
            }, // short entry stub
            Segment::Vertical {
                x: 10,
                y_start: 3,
                y_end: 5,
            },
            Segment::Horizontal {
                y: 5,
                x_start: 10,
                x_end: 15,
            },
        ];
        let chosen = select_label_segment_horizontal(&segments);
        // For long paths, should return the longest inner horizontal segment (50 chars at y=12)
        match chosen {
            Some(Segment::Horizontal { y, .. }) => assert_eq!(*y, 12),
            _ => panic!("Expected longest inner horizontal segment at y=12"),
        }
    }

    #[test]
    fn test_lr_label_placement_near_edge_segment() {
        use crate::graph::Direction;

        let mut diagram = Diagram::new(Direction::LeftRight);
        diagram.add_node(Node::new("A").with_label("Start"));
        diagram.add_node(Node::new("B").with_label("End"));
        let mut edge = Edge::new("A", "B");
        edge.label = Some("test".to_string());
        diagram.add_edge(edge);

        let config = TextLayoutConfig::default();
        let layout = compute_layout(&diagram, &config);
        let charset = CharSet::unicode();

        let routed = route_edge(
            &diagram.edges[0],
            &layout,
            Direction::LeftRight,
            None,
            None,
            false,
        )
        .unwrap();

        // Check that the routed edge has segments
        assert!(
            !routed.segments.is_empty(),
            "Routed edge should have segments"
        );

        // Render the edge with label
        let mut canvas = Canvas::new(layout.width, layout.height);
        render_edge(&mut canvas, &routed, &charset, Direction::LeftRight);

        let output = canvas.to_string();
        // The label "test" should appear in the output
        assert!(
            output.contains("test"),
            "Label 'test' should appear in output:\n{}",
            output
        );

        // Find where "test" appears and where edge chars appear in the output
        let lines: Vec<&str> = output.lines().collect();
        let label_line = lines
            .iter()
            .position(|l| l.contains("test"))
            .expect("Label should be on some line");

        // Find lines with edge characters (horizontal segments appear as ─ or -)
        let edge_lines: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains('─') || l.contains('►') || l.contains('-'))
            .map(|(i, _)| i)
            .collect();

        // The label should be within 1 row of an actual edge line
        let near_edge = edge_lines.iter().any(|&ey| ey.abs_diff(label_line) <= 1);
        assert!(
            near_edge,
            "Label at line {} should be within 1 row of an edge line (edge lines at {:?})",
            label_line, edge_lines
        );
    }

    #[test]
    fn test_select_label_segment_horizontal_no_horizontal() {
        // Edge case: only vertical segments
        let segments = vec![Segment::Vertical {
            x: 5,
            y_start: 0,
            y_end: 10,
        }];
        let chosen = select_label_segment_horizontal(&segments);
        assert!(
            chosen.is_none(),
            "Should return None when no horizontal segments exist"
        );
    }

    #[test]
    fn draw_arrow_does_not_overwrite_node_content() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(10, 10);

        // Mark a cell as node content
        canvas.set(5, 5, 'X');
        canvas.mark_as_node(5, 5);

        // Try to draw an arrow at the same position
        let point = Point { x: 5, y: 5 };
        draw_arrow_with_entry(
            &mut canvas,
            &point,
            AttachDirection::Top,
            &charset,
            Arrow::Normal,
        );

        // The cell should still contain 'X', not an arrow
        let cell = canvas.get(5, 5).unwrap();
        assert_eq!(cell.ch, 'X', "Arrow should not overwrite node content");
        assert!(cell.is_node, "Cell should still be marked as node");
    }

    #[test]
    fn draw_arrow_writes_on_non_node_cell() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(10, 10);

        // Draw an arrow on an empty cell (no node)
        let point = Point { x: 5, y: 5 };
        draw_arrow_with_entry(
            &mut canvas,
            &point,
            AttachDirection::Top,
            &charset,
            Arrow::Normal,
        );

        // Should succeed — arrow should be drawn
        let cell = canvas.get(5, 5).unwrap();
        assert_eq!(
            cell.ch, charset.arrow_down,
            "Arrow should be drawn on empty cell"
        );
    }

    #[test]
    fn test_cross_arrow_renders_x_character() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(10, 10);
        let point = Point { x: 5, y: 5 };
        draw_arrow_with_entry(
            &mut canvas,
            &point,
            AttachDirection::Top,
            &charset,
            Arrow::Cross,
        );
        let cell = canvas.get(5, 5).unwrap();
        assert_eq!(cell.ch, 'x', "Cross arrow should render as 'x'");
    }

    #[test]
    fn test_circle_arrow_renders_o_character() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(10, 10);
        let point = Point { x: 5, y: 5 };
        draw_arrow_with_entry(
            &mut canvas,
            &point,
            AttachDirection::Top,
            &charset,
            Arrow::Circle,
        );
        let cell = canvas.get(5, 5).unwrap();
        assert_eq!(cell.ch, 'o', "Circle arrow should render as 'o'");
    }

    #[test]
    fn test_cross_arrow_all_directions() {
        let charset = CharSet::unicode();

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Top,
            &charset,
            Arrow::Cross,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_cross_down);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Bottom,
            &charset,
            Arrow::Cross,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_cross_up);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Left,
            &charset,
            Arrow::Cross,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_cross_right);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Right,
            &charset,
            Arrow::Cross,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_cross_left);
    }

    #[test]
    fn test_circle_arrow_all_directions() {
        let charset = CharSet::unicode();

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Top,
            &charset,
            Arrow::Circle,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_circle_down);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Bottom,
            &charset,
            Arrow::Circle,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_circle_up);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Left,
            &charset,
            Arrow::Circle,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_circle_right);

        let mut canvas = Canvas::new(10, 10);
        draw_arrow_with_entry(
            &mut canvas,
            &Point::new(5, 5),
            AttachDirection::Right,
            &charset,
            Arrow::Circle,
        );
        assert_eq!(canvas.get(5, 5).unwrap().ch, charset.arrow_circle_left);
    }

    #[test]
    fn test_cross_arrow_end_to_end() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrows(Arrow::None, Arrow::Cross));

        let output = crate::render::render(&diagram, &crate::render::RenderOptions::default());
        assert!(
            output.contains('x'),
            "Output should contain 'x' for cross arrow:\n{output}"
        );
        assert!(
            !output.contains('\u{25BC}'),
            "Output should NOT contain normal down arrow for cross edge"
        );
    }

    #[test]
    fn test_circle_arrow_end_to_end() {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A").with_label("A"));
        diagram.add_node(Node::new("B").with_label("B"));
        diagram.add_edge(Edge::new("A", "B").with_arrows(Arrow::None, Arrow::Circle));

        let output = crate::render::render(&diagram, &crate::render::RenderOptions::default());
        assert!(
            output.contains('o'),
            "Output should contain 'o' for circle arrow:\n{output}"
        );
        assert!(
            !output.contains('\u{25BC}'),
            "Output should NOT contain normal down arrow for circle edge"
        );
    }

    // === calc_label_position tests (Task 2.1) ===

    #[test]
    fn calc_label_empty_segments_returns_none() {
        assert_eq!(calc_label_position(&[]), None);
    }

    #[test]
    fn calc_label_single_vertical_segment_returns_midpoint() {
        let segments = vec![Segment::Vertical {
            x: 5,
            y_start: 10,
            y_end: 20,
        }];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 5, y: 15 }));
    }

    #[test]
    fn calc_label_single_horizontal_segment_returns_midpoint() {
        let segments = vec![Segment::Horizontal {
            y: 3,
            x_start: 0,
            x_end: 10,
        }];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 5, y: 3 }));
    }

    #[test]
    fn calc_label_l_path_midpoint_at_corner() {
        // V(x=5, y 0->6) + H(y=6, x 5->11) = total 12, midpoint at 6
        let segments = vec![
            Segment::Vertical {
                x: 5,
                y_start: 0,
                y_end: 6,
            },
            Segment::Horizontal {
                y: 6,
                x_start: 5,
                x_end: 11,
            },
        ];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 5, y: 6 }));
    }

    #[test]
    fn calc_label_z_path_midpoint_on_middle_segment() {
        // V(4) + H(10) + V(4) = 18, midpoint at 9 -> 4 into first, 5 into H -> (10, 4)
        let segments = vec![
            Segment::Vertical {
                x: 5,
                y_start: 0,
                y_end: 4,
            },
            Segment::Horizontal {
                y: 4,
                x_start: 5,
                x_end: 15,
            },
            Segment::Vertical {
                x: 15,
                y_start: 4,
                y_end: 8,
            },
        ];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 10, y: 4 }));
    }

    #[test]
    fn calc_label_zero_length_path_returns_start() {
        let segments = vec![Segment::Vertical {
            x: 5,
            y_start: 10,
            y_end: 10,
        }];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 5, y: 10 }));
    }

    #[test]
    fn calc_label_odd_total_length_rounds_down() {
        // Length 7, midpoint at offset 3
        let segments = vec![Segment::Vertical {
            x: 5,
            y_start: 0,
            y_end: 7,
        }];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 5, y: 3 }));
    }

    #[test]
    fn calc_label_backward_edge_typical_shape() {
        // H(5) + V(12) + H(5) = 22, midpoint at 11 -> 5 into H, 6 into V -> (25, 9)
        let segments = vec![
            Segment::Horizontal {
                y: 3,
                x_start: 20,
                x_end: 25,
            },
            Segment::Vertical {
                x: 25,
                y_start: 3,
                y_end: 15,
            },
            Segment::Horizontal {
                y: 15,
                x_start: 25,
                x_end: 20,
            },
        ];
        assert_eq!(calc_label_position(&segments), Some(Point { x: 25, y: 9 }));
    }

    // === Rendering integration tests for backward edge labels (Task 4.1) ===

    #[test]
    fn backward_edge_label_near_routed_path_td() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;
        use crate::render::{RenderOptions, render};

        let flowchart = parse_flowchart("graph TD\n    A --> B\n    B -->|retry| A").unwrap();
        let diagram = build_diagram(&flowchart);
        let output = render(&diagram, &RenderOptions::default());

        assert!(
            output.contains("retry"),
            "Label should appear in output:\n{output}"
        );

        // In TD layout, backward edge label should appear on the routed connector
        // between the source and target node rows.
        let lines: Vec<&str> = output.lines().collect();
        let a_line = lines
            .iter()
            .position(|l| l.contains(" A "))
            .expect("missing node A row");
        let b_line = lines
            .iter()
            .rposition(|l| l.contains(" B "))
            .expect("missing node B row");
        let retry_line = lines
            .iter()
            .position(|l| l.contains("retry"))
            .expect("missing retry label row");

        assert!(
            retry_line > a_line && retry_line < b_line,
            "Label row {} should be between A row {} and B row {}\n{}",
            retry_line,
            a_line,
            b_line,
            output
        );
    }
}
