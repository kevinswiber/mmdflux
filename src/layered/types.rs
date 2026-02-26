//! Shared types for the layered layout module.

use std::collections::HashMap;

use super::normalize::WaypointWithRank;

/// Unique identifier for a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub String);

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        NodeId(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        NodeId(s)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Ranking algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ranker {
    #[default]
    NetworkSimplex,
    LongestPath,
}

/// Direction of the hierarchical layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    TopBottom, // TB/TD
    BottomTop, // BT
    LeftRight, // LR
    RightLeft, // RL
}

impl Direction {
    /// Is this a vertical (TB/BT) layout?
    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::TopBottom | Direction::BottomTop)
    }

    /// Is this a horizontal (LR/RL) layout?
    pub fn is_horizontal(self) -> bool {
        !self.is_vertical()
    }

    /// Is this a reversed direction (BT or RL)?
    pub fn is_reversed(self) -> bool {
        matches!(self, Direction::BottomTop | Direction::RightLeft)
    }
}

/// Strategy for placing a label dummy within a long edge's dummy chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelDummyStrategy {
    /// Place the label at the midpoint rank of the edge (current dagre behavior).
    #[default]
    Midpoint,
    /// Move the label to the widest layer in the chain to minimize width increase.
    WidestLayer,
}

/// A 2D point with floating-point coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle (bounding box).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }
}

/// Configuration options for the layout algorithm.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Layout direction.
    pub direction: Direction,

    /// Horizontal spacing between nodes (or vertical for LR/RL).
    pub node_sep: f64,

    /// Spacing between dummy nodes (edge segments). Matches dagre.js `edgesep`.
    pub edge_sep: f64,

    /// Vertical spacing between ranks (or horizontal for LR/RL).
    pub rank_sep: f64,

    /// Per-rank-gap spacing overrides. Key is the rank number; the gap after
    /// this rank uses the override value instead of `rank_sep`.
    /// Empty map means all gaps use `rank_sep` (backward compatible).
    pub rank_sep_overrides: HashMap<i32, f64>,

    /// Padding around the entire diagram.
    pub margin: f64,

    /// Whether to apply layout optimization for acyclic graphs.
    pub acyclic: bool,

    /// Ranking algorithm to use.
    pub ranker: Ranker,

    /// Enable greedy switch crossing reduction post-pass (flux-layered only).
    pub greedy_switch: bool,

    /// Enable model order tie-breaking in barycenter sort (flux-layered only).
    pub model_order_tiebreak: bool,

    /// Enable variable per-gap rank spacing from edge density (flux-layered only).
    pub variable_rank_spacing: bool,

    /// Use compound-style crossing reduction sweeps even for flat graphs.
    ///
    /// Mermaid-layered keeps this enabled for dagre parity.
    /// Flux-layered defaults this to false.
    pub always_compound_ordering: bool,

    /// Track reversed chain edges in `reversed_edges` after normalization.
    /// When true, chain edges created from reversed long edges are marked as
    /// reversed, affecting effective edge direction in ordering and positioning.
    /// Dagre v0.8.5 does not track these, so mermaid-layered should leave this false.
    pub track_reversed_chains: bool,

    /// When true, only labeled edges get extra rank span for label dummies.
    /// When false (default), all edges get doubled minlen (dagre-compatible).
    pub per_edge_label_spacing: bool,

    /// When true, assign Above/Below sides to label dummies sharing a layer
    /// to reduce label-label overlaps.
    pub label_side_selection: bool,

    /// Strategy for placing label dummies within long edge chains.
    pub label_dummy_strategy: LabelDummyStrategy,

    /// Gap between edge stroke and label text (in layout units).
    pub edge_label_spacing: f64,
}

impl LayoutConfig {
    /// Get the rank separation for the gap after the given rank.
    ///
    /// Returns the override value if one exists for this rank,
    /// otherwise returns the base `rank_sep`.
    pub fn rank_sep_for_gap(&self, rank: i32) -> f64 {
        self.rank_sep_overrides
            .get(&rank)
            .copied()
            .unwrap_or(self.rank_sep)
    }
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            direction: Direction::default(),
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            rank_sep_overrides: HashMap::new(),
            margin: 8.0,
            acyclic: true,
            ranker: Ranker::default(),
            greedy_switch: false,
            model_order_tiebreak: false,
            variable_rank_spacing: false,
            always_compound_ordering: false,
            track_reversed_chains: false,
            per_edge_label_spacing: false,
            label_side_selection: false,
            label_dummy_strategy: LabelDummyStrategy::default(),
            edge_label_spacing: 2.0,
        }
    }
}

/// Result of the layout computation.
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// Bounding boxes for each node (positioned).
    pub nodes: HashMap<NodeId, Rect>,

    /// Edge paths as sequences of points.
    pub edges: Vec<EdgeLayout>,

    /// Set of edge indices that were reversed for cycle removal.
    pub reversed_edges: Vec<usize>,

    /// Total width of the layout.
    pub width: f64,

    /// Total height of the layout.
    pub height: f64,

    /// Waypoints for each edge derived from dummy node positions during normalization.
    /// Key: original edge index, Value: list of waypoints with rank information.
    /// Empty for short edges (span 1 rank), populated for long edges.
    /// The rank information is needed to transform waypoints from layout coordinates to draw coordinates.
    pub edge_waypoints: HashMap<usize, Vec<WaypointWithRank>>,

    /// Pre-computed label positions for edges with labels.
    /// Key: original edge index, Value: label center position with rank.
    /// Only populated for edges that have labels.
    /// The rank information is needed to snap the primary axis to `layer_starts`.
    pub label_positions: HashMap<usize, WaypointWithRank>,

    /// Label side assignments for edges with labels.
    /// Key: original edge index, Value: Above/Below/Center.
    pub label_sides: HashMap<usize, super::normalize::LabelSide>,

    /// Bounding boxes for subgraphs (compound nodes).
    /// Key: subgraph node ID string, Value: bounding rectangle.
    /// Empty for graphs without subgraphs.
    pub subgraph_bounds: HashMap<String, Rect>,

    /// Self-edge layout data (loops where source == target).
    pub self_edges: Vec<SelfEdgeLayout>,

    /// Layout rank to position mapping for waypoint transformation.
    /// Key: layout rank, Value: (primary_start, primary_end) coordinates in layout space.
    /// The primary axis is Y for TD/BT layouts and X for LR/RL layouts.
    /// Includes all position nodes (user nodes + border nodes), used to convert
    /// waypoint ranks to draw coordinates.
    pub rank_to_position: HashMap<i32, (f64, f64)>,

    /// Layout rank for each user node.
    /// Key: node ID, Value: layout rank.
    /// Used to compute layer_starts from actual node bounds in render layer.
    pub node_ranks: HashMap<NodeId, i32>,
}

/// A self-edge (A → A) stashed before layout, reinserted after ordering.
#[derive(Debug, Clone)]
pub struct SelfEdge {
    /// Index of the node in LayoutGraph.
    pub node_index: usize,
    /// Original edge index in the input graph.
    pub orig_edge_index: usize,
    /// Index of the dummy node (set during Phase 3).
    pub dummy_index: Option<usize>,
}

/// Layout result data for a self-edge after positioning.
#[derive(Debug, Clone)]
pub struct SelfEdgeLayout {
    /// Node ID the self-edge loops on.
    pub node: NodeId,
    /// Original edge index.
    pub edge_index: usize,
    /// Points defining the orthogonal loop path.
    pub points: Vec<Point>,
}

/// Layout information for a single edge.
#[derive(Debug, Clone)]
pub struct EdgeLayout {
    /// Source node.
    pub from: NodeId,
    /// Target node.
    pub to: NodeId,
    /// Path points (for rendering as polyline or spline).
    pub points: Vec<Point>,
    /// Original edge index (for preserving metadata).
    pub index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ranker_default_is_network_simplex() {
        let config = LayoutConfig::default();
        assert_eq!(config.ranker, Ranker::NetworkSimplex);
    }

    #[test]
    fn layout_config_default_has_empty_overrides() {
        let config = LayoutConfig::default();
        assert!(config.rank_sep_overrides.is_empty());
    }

    #[test]
    fn layout_config_rank_sep_for_gap_uses_override() {
        let mut config = LayoutConfig::default();
        config.rank_sep_overrides.insert(2, 100.0);
        assert_eq!(config.rank_sep_for_gap(2), 100.0);
        assert_eq!(config.rank_sep_for_gap(0), config.rank_sep); // no override, uses base
    }

    #[test]
    fn layout_config_rank_sep_for_gap_falls_back_to_base() {
        let config = LayoutConfig::default();
        assert_eq!(config.rank_sep_for_gap(42), config.rank_sep);
    }

    #[test]
    fn per_edge_label_spacing_defaults_to_false() {
        let config = LayoutConfig::default();
        assert!(!config.per_edge_label_spacing);
    }

    #[test]
    fn label_dummy_strategy_defaults_to_midpoint() {
        let config = LayoutConfig::default();
        assert_eq!(config.label_dummy_strategy, LabelDummyStrategy::Midpoint);
    }

    #[test]
    fn edge_label_spacing_defaults_to_2() {
        let config = LayoutConfig::default();
        assert!((config.edge_label_spacing - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn label_dummy_strategy_variants() {
        let _ = LabelDummyStrategy::Midpoint;
        let _ = LabelDummyStrategy::WidestLayer;
    }

    #[test]
    fn test_self_edge_struct() {
        let se = SelfEdge {
            node_index: 0,
            orig_edge_index: 2,
            dummy_index: None,
        };
        assert_eq!(se.node_index, 0);
        assert_eq!(se.orig_edge_index, 2);
        assert!(se.dummy_index.is_none());
    }

    #[test]
    fn test_self_edge_layout_struct() {
        let sel = SelfEdgeLayout {
            node: "A".into(),
            edge_index: 0,
            points: vec![Point { x: 1.0, y: 2.0 }, Point { x: 3.0, y: 4.0 }],
        };
        assert_eq!(sel.node, "A".into());
        assert_eq!(sel.points.len(), 2);
    }

    #[test]
    fn test_layout_result_self_edges_field() {
        let result = LayoutResult {
            nodes: HashMap::new(),
            edges: vec![],
            reversed_edges: vec![],
            width: 0.0,
            height: 0.0,
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
            label_sides: HashMap::new(),
            subgraph_bounds: HashMap::new(),
            self_edges: vec![],
            rank_to_position: HashMap::new(),
            node_ranks: HashMap::new(),
        };
        assert!(result.self_edges.is_empty());
    }
}
