//! Shared label-geometry helpers used by both the legacy body-label branches
//! in `edge.rs` and the render-time placer in `label_placement.rs`.
//!
//! Splitting these out breaks the `edge ↔ label_placement` dependency cycle
//! the architecture checker's `no-boundary-cycles` rule enforces.

use crate::graph::grid::{Point, Segment};

/// Calculate the label position at the midpoint of a routed path.
///
/// Walks the segments by Manhattan distance and returns the point at 50%
/// of the total path length. Returns `None` if the path has no segments.
pub(super) fn calc_label_position(segments: &[Segment]) -> Option<Point> {
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

/// A label split into lines with precomputed dimensions.
#[derive(Debug)]
pub(super) struct LabelBlock<'a> {
    pub(super) lines: Vec<&'a str>,
    pub(super) width: usize,
    pub(super) height: usize,
}

pub(super) fn label_block(label: &str) -> LabelBlock<'_> {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| crate::format::display_width(line))
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    LabelBlock {
        lines,
        width,
        height,
    }
}

/// Resolve the effective label string for text rendering, honoring the
/// pre-engine wrap artifact. When `wrapped_label_lines` is
/// populated, returns the `'\n'`-joined line vector so existing
/// `'\n'`-splitting call sites (`label_block`, `draw_label_direct`) pick
/// up the wrapped shape without threading an extra parameter.
pub(super) fn effective_edge_label<'a>(
    edge: &'a crate::graph::Edge,
) -> Option<std::borrow::Cow<'a, str>> {
    if let Some(lines) = edge.wrapped_label_lines.as_deref() {
        return Some(std::borrow::Cow::Owned(lines.join("\n")));
    }
    edge.label.as_deref().map(std::borrow::Cow::Borrowed)
}

pub(super) fn label_top_for_center(center_y: usize, height: usize) -> usize {
    center_y.saturating_sub(height / 2)
}

pub(super) fn clamp_label_x(
    label_x: usize,
    label_width: usize,
    containment: Option<(usize, usize)>,
) -> usize {
    let Some((c_min, c_max)) = containment else {
        return label_x;
    };
    let avail = c_max.saturating_sub(c_min);
    if label_width <= avail {
        label_x.max(c_min).min(c_max.saturating_sub(label_width))
    } else {
        c_min + avail.saturating_sub(label_width) / 2
    }
}
