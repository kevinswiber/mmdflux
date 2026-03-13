//! Graph-owned grid replay metadata.
//!
//! These contracts travel with `GraphGeometry` and MMDS payloads, but they are
//! not themselves grid algorithms. Keeping them separate from `grid` avoids a
//! geometry -> grid dependency while preserving the replay contract.

use std::collections::HashMap;

use crate::graph::space::{FPoint, FRect};

/// Graph-owned projection data needed to replay float geometry onto a derived grid.
#[derive(Debug, Clone, Default)]
pub struct GridProjection {
    /// Per-node rank assignments (node_id -> rank).
    pub node_ranks: HashMap<String, i32>,
    /// Waypoints with rank info for grid-snap transformation.
    /// Key: edge index, Value: list of (position, rank) pairs.
    pub edge_waypoints: HashMap<usize, Vec<(FPoint, i32)>>,
    /// Label positions with rank info for grid-snap transformation.
    /// Key: edge index, Value: (position, rank).
    pub label_positions: HashMap<usize, (FPoint, i32)>,
    /// Precomputed direction-override subgraph layouts for grid replay.
    pub override_subgraphs: HashMap<String, OverrideSubgraphProjection>,
}

/// Graph-owned replay data for a direction-override subgraph.
#[derive(Debug, Clone, Default)]
pub struct OverrideSubgraphProjection {
    /// Per-node sublayout rectangles in subgraph-local float coordinates.
    pub nodes: HashMap<String, FRect>,
}
