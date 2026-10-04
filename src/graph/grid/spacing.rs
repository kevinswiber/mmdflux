//! Crate-private grid spacing overrides for text compaction.
//!
//! These knobs let the text renderer derive a tighter grid than the default
//! heuristics without adding fields to the public [`super::GridLayoutConfig`].

/// Spacing overrides applied when deriving a grid layout for text.
///
/// The default (all off) derives exactly the same grid as
/// [`super::geometry_to_grid_layout_with_routed`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridSpacingOverrides {
    /// Size each rank gap from the edge labels that sit in it instead of the
    /// widest label in the whole graph.
    pub label_aware: bool,
    /// Rank and node gaps in cells, replacing the default grid spacing.
    pub gaps: Option<GridGaps>,
}

/// Rank and node gaps in terminal cells.
///
/// `rank_gap` runs along the root direction (rows for TD/BT, columns for
/// LR/RL); `node_gap` separates nodes within a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridGaps {
    pub rank_gap: usize,
    pub node_gap: usize,
}
