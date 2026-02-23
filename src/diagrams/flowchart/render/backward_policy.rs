//! Shared backward-edge routing policy helpers.
//!
//! Keeps backward channel/parity gates centralized so text and orthogonal
//! routing paths can reuse the same decision rules.

use crate::diagrams::flowchart::geometry::FRect;
use crate::graph::Direction;

/// Long backward edges (3+ user-visible rank gaps, normalized rank_span >= 6)
/// use side-face channel routing.
pub(crate) const BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN: usize = 6;

const TD_BT_PARITY_MIN_RECT_SPAN: f64 = 20.0;

/// Whether a backward edge should prefer the canonical backward side channel.
///
/// Shared policy:
/// - backward edges with no layout waypoints use the canonical channel
/// - long backward edges (`rank_span >= 6`) use canonical channel even with hints
pub(crate) fn prefer_backward_side_channel(
    is_backward: bool,
    has_layout_waypoints: bool,
    rank_span: Option<usize>,
) -> bool {
    if !is_backward {
        return false;
    }
    if rank_span.is_some_and(|span| span >= BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN) {
        return true;
    }
    !has_layout_waypoints
}

/// Whether TD/BT backward hint-parity overrides can be applied safely.
///
/// This keeps existing guardrails centralized:
/// - only TD/BT backward edges
/// - no subgraph-as-node endpoints
/// - not a long backward span (canonical channel takes priority)
/// - endpoint rectangles are large enough to make hint-face inference stable
/// - source is not fully to the right of target (avoids forward-edge crossing)
pub(crate) fn can_apply_td_bt_backward_hint_parity(
    direction: Direction,
    is_backward: bool,
    has_subgraph_endpoint: bool,
    rank_span: usize,
    source_rect: FRect,
    target_rect: FRect,
    source_center_x: f64,
) -> bool {
    if !matches!(direction, Direction::TopDown | Direction::BottomTop) {
        return false;
    }
    if has_subgraph_endpoint {
        return false;
    }
    if prefer_backward_side_channel(is_backward, true, Some(rank_span)) {
        return false;
    }
    if source_rect.width < TD_BT_PARITY_MIN_RECT_SPAN
        || source_rect.height < TD_BT_PARITY_MIN_RECT_SPAN
        || target_rect.width < TD_BT_PARITY_MIN_RECT_SPAN
        || target_rect.height < TD_BT_PARITY_MIN_RECT_SPAN
    {
        return false;
    }

    let target_right = target_rect.x + target_rect.width;
    source_center_x <= target_right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_backward_side_channel_uses_no_waypoint_fallback() {
        assert!(prefer_backward_side_channel(true, false, None));
        assert!(!prefer_backward_side_channel(true, true, None));
    }

    #[test]
    fn prefer_backward_side_channel_uses_long_span_override() {
        assert!(prefer_backward_side_channel(
            true,
            true,
            Some(BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN)
        ));
        assert!(!prefer_backward_side_channel(
            true,
            true,
            Some(BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN - 1)
        ));
    }

    #[test]
    fn prefer_backward_side_channel_ignores_forward_edges() {
        assert!(!prefer_backward_side_channel(false, false, Some(10)));
    }

    #[test]
    fn td_bt_backward_hint_parity_requires_safe_geometry() {
        let source_rect = FRect::new(10.0, 10.0, 40.0, 40.0);
        let target_rect = FRect::new(20.0, 0.0, 40.0, 40.0);
        let source_center_x = source_rect.x + source_rect.width / 2.0;

        assert!(can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            2,
            source_rect,
            target_rect,
            source_center_x
        ));
    }

    #[test]
    fn td_bt_backward_hint_parity_rejects_long_span_and_crossing_topology() {
        let source_rect = FRect::new(80.0, 10.0, 40.0, 40.0);
        let target_rect = FRect::new(10.0, 0.0, 40.0, 40.0);
        let source_center_x = source_rect.x + source_rect.width / 2.0;

        assert!(!can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            BACKWARD_SIDE_CHANNEL_LONG_RANK_SPAN,
            source_rect,
            target_rect,
            source_center_x
        ));
        assert!(!can_apply_td_bt_backward_hint_parity(
            Direction::TopDown,
            true,
            false,
            2,
            source_rect,
            target_rect,
            source_center_x
        ));
    }
}
