//! Marker geometry helpers shared between routing-time clamps and SVG render.
//!
//! `marker_envelope` returns the full marker bounding-box envelope at a path
//! endpoint: the `length` along the path direction (away from the anchor
//! node) and the `width` perpendicular to it. The clamp pass uses just the
//! `length` (via `marker_avoidance_distance`); the corpus boundary
//! assertion test uses both dimensions to model the real marker envelope at
//! the path endpoint.
//!
//! See `marker_offset_for_arrow` in `render::graph::svg::edges::markers` for
//! the path-endpoint pullback half (which is for path geometry, not avoidance).
//! The two helpers coexist with non-overlapping purposes and are intentionally
//! not unified — the pullback values are tuned for visual marker placement,
//! and the envelope values are the worst-case bounding box for collision tests.
//!
//! ## Derivation
//!
//! `length = pullback + effective_refX`, where:
//!  - `pullback` is the path-endpoint shortening from `marker_offset_for_arrow`
//!  - `effective_refX = refX_viewbox * (markerWidth_world / viewBox_width)`
//!
//! Geometrically: with `marker-end` and `orient="auto"`, the marker is
//! rotated so its `+X` axis aligns with the path tangent. The marker spans
//! from `anchor - effective_refX` (back toward the path interior) to
//! `anchor + (markerWidth - effective_refX)` (forward into the node).
//! With pullback, anchor sits `pullback` away from the node face, so the
//! marker's farthest extent into the gap is `pullback + effective_refX`.
//!
//! See `tests/svg-snapshots/class/relationships.svg` for an empirical check:
//! a `Diamond` marker-start on a TD path with `pullback=5` and
//! `effective_refX=6` produces a marker bbox that ends 11 px past the source
//! node face — matching this module's `length` value of 11 for `Diamond`.

use crate::graph::Arrow;

// Consumers land in Task 1.1 (assertion test) and Task 2.1 (clamp pass) of
// plan 0146. Until those land, the helpers are exercised only by the unit
// tests in this module.

/// Full envelope of an edge marker bounding box at the path endpoint.
///
/// Used by the routing label-clamp pass and the boundary-assertion test
/// to keep edge labels clear of marker geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct MarkerEnvelope {
    /// Distance from the node face along the path direction past which
    /// any subsequent geometry (e.g., edge labels) must sit to avoid
    /// overlapping the marker bounding box.
    ///
    /// Equals `pullback + effective_refX` (see module docs).
    pub length: f64,

    /// Full perpendicular extent of the marker bbox (sum across both sides
    /// of the path centerline). For markers with symmetric refY at the
    /// vertical midpoint this is `markerHeight` in world coordinates
    /// (post viewBox→markerHeight scaling).
    pub width: f64,
}

/// Return the marker envelope for a given arrow type, or `None` for arrows
/// that have no marker (`Arrow::None`).
///
/// `Arrow::Cross` returns an envelope because the cross visual still has
/// extent that labels must clear, even though the path is not pulled back.
#[allow(dead_code)]
pub(crate) fn marker_envelope(arrow: Arrow) -> Option<MarkerEnvelope> {
    match arrow {
        // viewBox 10×10, markerWidth=8, refX=5 (center).
        // effective_refX = 5 * (8/10) = 4. pullback = 4. length = 4 + 4 = 8.
        // markerHeight = 8 (perpendicular world extent).
        Arrow::Normal => Some(MarkerEnvelope {
            length: 8.0,
            width: 8.0,
        }),

        // Same viewBox/markerWidth as Normal, different pullback.
        // effective_refX = 4. pullback = 5. length = 5 + 4 = 9.
        Arrow::OpenTriangle => Some(MarkerEnvelope {
            length: 9.0,
            width: 8.0,
        }),

        // viewBox 12×12, markerWidth=12, refX=6 (center).
        // effective_refX = 6 * (12/12) = 6. pullback = 5. length = 5 + 6 = 11.
        // Verified empirically against tests/svg-snapshots/class/relationships.svg
        // (Order→Product diamond marker spans y=[375, 387], source bottom y=376,
        // so marker extends 11 px past source face).
        Arrow::Diamond => Some(MarkerEnvelope {
            length: 11.0,
            width: 12.0,
        }),

        // Same viewBox as Diamond, larger pullback.
        // effective_refX = 6. pullback = 6. length = 6 + 6 = 12.
        Arrow::OpenDiamond => Some(MarkerEnvelope {
            length: 12.0,
            width: 12.0,
        }),

        // viewBox 12×12, markerWidth=12, refX=11 (near right edge).
        // effective_refX = 11 * (12/12) = 11. pullback = 10. length = 10 + 11 = 21.
        // Conservative: actual circle (cx=6, r=5) only spans 10 of 12 viewBox
        // width. We use the full viewBox extent to bias toward false positives
        // (safer for an assertion target).
        Arrow::Circle => Some(MarkerEnvelope {
            length: 21.0,
            width: 12.0,
        }),

        // viewBox 11×11, markerWidth=11, refX=12 (one past viewBox right edge).
        // effective_refX = 12 * (11/11) = 12. pullback = 0 (no path shortening
        // for a stop marker). length = 0 + 12 = 12.
        Arrow::Cross => Some(MarkerEnvelope {
            length: 12.0,
            width: 11.0,
        }),

        Arrow::None => None,
    }
}

/// Convenience wrapper for the routing label-clamp pass — only the
/// along-path length is needed there.
#[allow(dead_code)]
pub(crate) fn marker_avoidance_distance(arrow: Arrow) -> f64 {
    marker_envelope(arrow).map(|e| e.length).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_envelope_returns_none_for_arrow_none() {
        assert_eq!(marker_envelope(Arrow::None), None);
    }

    #[test]
    fn marker_envelope_returns_some_for_every_other_arrow() {
        for arrow in [
            Arrow::Normal,
            Arrow::OpenTriangle,
            Arrow::Diamond,
            Arrow::OpenDiamond,
            Arrow::Circle,
            Arrow::Cross,
        ] {
            assert!(
                marker_envelope(arrow).is_some(),
                "arrow {arrow:?} should have an envelope"
            );
        }
    }

    #[test]
    fn marker_avoidance_distance_matches_envelope_length() {
        // Locked invariant: the wrapper must stay in sync with the source.
        // If either drifts, both must drift together.
        for arrow in [
            Arrow::Normal,
            Arrow::OpenTriangle,
            Arrow::Diamond,
            Arrow::OpenDiamond,
            Arrow::Circle,
            Arrow::Cross,
            Arrow::None,
        ] {
            let len = marker_envelope(arrow).map(|e| e.length).unwrap_or(0.0);
            assert_eq!(
                marker_avoidance_distance(arrow),
                len,
                "arrow {arrow:?}: avoidance and envelope.length must agree"
            );
        }
    }

    #[test]
    fn diamond_envelope_matches_class_relationships_svg() {
        // The `Order o-- Product : contains` edge in
        // tests/fixtures/class/relationships.mmd produces a diamond marker
        // that empirically spans y=[375, 387] in world coordinates, with
        // the source node (Order) bottom at y=376. The marker extends
        // 11 px past the source face into the gap. Pin that here so any
        // future change to the diamond marker definition or pullback
        // surfaces immediately.
        let env = marker_envelope(Arrow::Diamond).expect("diamond has envelope");
        assert_eq!(env.length, 11.0);
        assert_eq!(env.width, 12.0);
    }

    #[test]
    fn arrowhead_envelope_matches_normal_path_pullback() {
        // viewBox 10×10, markerWidth=8, refX=5, pullback=4.
        // length = 4 + (5 * 8/10) = 4 + 4 = 8.
        let env = marker_envelope(Arrow::Normal).expect("normal arrow has envelope");
        assert_eq!(env.length, 8.0);
        assert_eq!(env.width, 8.0);
    }
}
