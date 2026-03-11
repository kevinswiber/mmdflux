//! Shared graph-family grid projection contracts.
//!
//! These types sit between engine-owned layout results and render-owned grid
//! emission. They are graph-owned so text replay can happen directly from
//! hydrated geometry without depending on engine hint enums or layered-owned
//! config names.

use std::collections::HashMap;

use crate::graph::geometry::{FPoint, FRect};

/// Grid-layout rank assignment strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridRanker {
    #[default]
    NetworkSimplex,
    LongestPath,
}

/// Configuration for discrete grid layout computation.
///
/// Controls integer character-grid spacing, padding, and the underlying
/// layout-derived parameters used by the grid projection pipeline.
#[derive(Debug, Clone)]
pub struct GridLayoutConfig {
    /// Horizontal spacing between nodes.
    pub h_spacing: usize,
    /// Vertical spacing between nodes.
    pub v_spacing: usize,
    /// Padding around the entire diagram.
    pub padding: usize,
    /// Extra left margin for edge labels on left branches.
    pub left_label_margin: usize,
    /// Extra right margin for edge labels on right branches.
    pub right_label_margin: usize,
    /// Ranking algorithm override.
    pub ranker: Option<GridRanker>,
    /// Node spacing (nodesep).
    pub node_sep: f64,
    /// Edge segment spacing (edgesep).
    pub edge_sep: f64,
    /// Rank spacing (ranksep).
    pub rank_sep: f64,
    /// Layout margin (applied in translateGraph).
    pub margin: f64,
    /// Additional ranksep applied when subgraphs are present (Mermaid clusters).
    pub cluster_rank_sep: f64,
}

impl Default for GridLayoutConfig {
    fn default() -> Self {
        Self {
            h_spacing: 4,
            v_spacing: 3,
            padding: 1,
            left_label_margin: 0,
            right_label_margin: 0,
            ranker: None,
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            margin: 8.0,
            cluster_rank_sep: 25.0,
        }
    }
}

/// Graph-owned projection data needed to replay float geometry onto a discrete grid.
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
