//! Graph-family geometry IR contracts.
//!
//! Two-layer float-coordinate geometry produced by layout engines and
//! consumed by routing and downstream output stages. Engine-agnostic core with optional
//! engine-specific hint channels.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::errors::RenderError;
use crate::format::normalize_enum_token;
pub use crate::graph::attachment::EdgePort;
use crate::graph::projection::GridProjection;
pub use crate::graph::space::{FPoint, FRect};
use crate::graph::{Direction, Shape};

/// Requested graph-geometry detail level for downstream emitters and exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryLevel {
    /// Node geometry + edge topology only (no edge paths).
    #[default]
    Layout,
    /// Full geometry including routed edge paths.
    Routed,
}

impl std::fmt::Display for GeometryLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeometryLevel::Layout => write!(f, "layout"),
            GeometryLevel::Routed => write!(f, "routed"),
        }
    }
}

impl GeometryLevel {
    /// Parse a geometry level from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "layout" => Ok(GeometryLevel::Layout),
            "routed" => Ok(GeometryLevel::Routed),
            _ => Err(RenderError {
                message: format!("unknown geometry level: {s:?}"),
            }),
        }
    }
}

impl std::str::FromStr for GeometryLevel {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

// ---------------------------------------------------------------------------
// Layer 1: GraphGeometry (layout output → routing input)
// ---------------------------------------------------------------------------

/// Positioned graph geometry in float coordinate space.
///
/// Produced by layout engines via normalization adapters,
/// consumed by routing and shared policy functions.
#[derive(Debug, Clone)]
pub struct GraphGeometry {
    /// Positioned nodes with bounding rects.
    pub nodes: HashMap<String, PositionedNode>,
    /// Edge routing hints from layout (waypoints, label positions).
    pub edges: Vec<LayoutEdge>,
    /// Subgraph bounding boxes.
    pub subgraphs: HashMap<String, SubgraphGeometry>,
    /// Self-edge loop geometry.
    pub self_edges: Vec<SelfEdgeGeometry>,
    /// Root layout direction.
    pub direction: Direction,
    /// Per-node effective direction (accounting for subgraph overrides).
    pub node_directions: HashMap<String, Direction>,
    /// Total layout bounding box.
    pub bounds: FRect,
    /// Which edge indices were reversed for cycle removal.
    pub reversed_edges: Vec<usize>,
    /// Optional engine-specific metadata for grid-snap and rank-aware routing.
    pub engine_hints: Option<EngineHints>,
    /// Optional graph-owned replay metadata for projecting float geometry onto a discrete grid.
    pub grid_projection: Option<GridProjection>,
    /// Edge indices rerouted by the layout engine (e.g., direction-override subgraph edges).
    /// Populated by engines that perform float-space subgraph post-processing.
    /// Used by downstream emitters to preserve explicit endpoint geometry.
    pub rerouted_edges: HashSet<usize>,
    /// Whether enhanced backward edge routing should be applied.
    /// Set by engines that use layout quality enhancements (e.g., flux-layered).
    pub enhanced_backward_routing: bool,
}

/// A positioned node with its bounding rect and shape.
#[derive(Debug, Clone)]
pub struct PositionedNode {
    pub id: String,
    /// Bounding rect in layout float space (x,y = center).
    pub rect: FRect,
    pub shape: Shape,
    pub label: String,
    pub parent: Option<String>,
}

/// An edge with layout-computed routing hints.
#[derive(Debug, Clone)]
pub struct LayoutEdge {
    /// Index into the diagram's edge list (for metadata lookup).
    pub index: usize,
    pub from: String,
    pub to: String,
    /// Waypoint positions from layout engine.
    pub waypoints: Vec<FPoint>,
    /// Label position computed by layout engine.
    pub label_position: Option<FPoint>,
    /// Label side (Above/Below/Center) from side selection.
    pub label_side: Option<EdgeLabelSide>,
    /// If source is a subgraph-as-node, the subgraph ID.
    pub from_subgraph: Option<String>,
    /// If target is a subgraph-as-node, the subgraph ID.
    pub to_subgraph: Option<String>,
    /// Optional complete path from engines that provide full routing (e.g. ELK).
    pub layout_path_hint: Option<Vec<FPoint>>,
    /// Preserve the explicit orthogonal topology instead of simplifying it away.
    /// Used when routing introduced a deliberate de-overlap corridor.
    pub preserve_orthogonal_topology: bool,
    /// Shared label-rectangle geometry (populated by the routing label-lane
    /// pass and copied back onto `LayoutEdge` so `Visual` SVG solve paths
    /// still see authoritative label rectangles). `None` before routing.
    pub label_geometry: Option<EdgeLabelGeometry>,
    /// Plan 0149 (#237): lane-aware re-wrap output, forwarded from
    /// `RoutedEdgeGeometry::effective_wrapped_lines` through
    /// `geometry_for_routed_svg` so the SVG renderer (which consumes
    /// `LayoutEdge`, not `RoutedEdgeGeometry`) can emit text matching
    /// the post-rewrap rect. `None` means "use the pre-engine wrap
    /// artifact from `diagram.edges[idx].wrapped_label_lines`".
    ///
    /// Only populated on the routed-SVG downgrade path; the pre-routing
    /// `LayoutEdge` produced by the kernel always carries `None` here.
    pub effective_wrapped_lines: Option<Vec<String>>,
}

/// Subgraph bounding box in layout float space.
#[derive(Debug, Clone)]
pub struct SubgraphGeometry {
    pub id: String,
    /// Bounding rect (x,y = center for layered-style, or top-left for others).
    pub rect: FRect,
    pub title: String,
    pub depth: usize,
}

/// Self-edge loop geometry.
#[derive(Debug, Clone)]
pub struct SelfEdgeGeometry {
    pub node_id: String,
    pub edge_index: usize,
    pub points: Vec<FPoint>,
}

/// Label side for positioned and routed edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeLabelSide {
    Above,
    Below,
    #[default]
    Center,
}

/// Shared label-rectangle geometry carried on both `LayoutEdge` (for the
/// `Visual` SVG solve path) and `RoutedEdgeGeometry` (for MMDS/SVG/bounds
/// consumers after routing). Populated exactly once by the label-lane pass
/// and read-only downstream — every consumer sees the same rectangle and
/// center. See plan 0145 architecture design §3.1 for invariants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeLabelGeometry {
    /// Midpoint of `rect`. Consumers may use either; no drift.
    pub center: FPoint,
    /// Padded label rectangle (includes `padding.0` on each side in X and
    /// `padding.1` on each side in Y).
    pub rect: FRect,
    /// Snapshot of `(metrics.label_padding_x, metrics.label_padding_y)` at
    /// construction. Consumers that need unpadded dimensions can subtract.
    pub padding: (f64, f64),
    /// Side of the edge this label is placed on.
    pub side: EdgeLabelSide,
    /// Signed lane-assignment track. `0` means no lane displacement was
    /// applied; `±1, ±2, ...` encode lane rank (sign encodes direction).
    pub track: i32,
}

impl Default for EdgeLabelGeometry {
    fn default() -> Self {
        Self {
            center: FPoint::new(0.0, 0.0),
            rect: FRect::new(0.0, 0.0, 0.0, 0.0),
            padding: (0.0, 0.0),
            side: EdgeLabelSide::Center,
            track: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Engine hints (optional engine-specific metadata for grid-snap and routing)
// ---------------------------------------------------------------------------

/// Engine-specific metadata channel.
///
/// Layout engines attach algorithm-specific hints that downstream stages
/// (grid projection, routing, rendering) can consume.  The outer enum
/// discriminates by engine family; the inner struct carries the payload.
#[derive(Debug, Clone)]
pub enum EngineHints {
    /// Hints produced by layered (Sugiyama-family) engines.
    Layered(LayeredHints),
}

/// Layered-layout-specific metadata needed during migration.
///
/// Preserves rank-annotated data from the layered layout that grid replay
/// needs for coordinate transformation. Other engines won't populate this.
#[derive(Debug, Clone)]
pub struct LayeredHints {
    /// Per-node rank assignments (node_id → rank).
    pub node_ranks: HashMap<String, i32>,
    /// Rank → (primary_start, primary_end) coordinates in layout float space.
    /// Primary axis is Y for TD/BT, X for LR/RL.
    pub rank_to_position: HashMap<i32, (f64, f64)>,
    /// Waypoints with rank info for grid-snap transformation.
    /// Key: edge index, Value: list of (position, rank) pairs.
    pub edge_waypoints: HashMap<usize, Vec<(FPoint, i32)>>,
    /// Label positions with rank info for grid-snap transformation.
    /// Key: edge index, Value: (position, rank).
    pub label_positions: HashMap<usize, (FPoint, i32)>,
}

// ---------------------------------------------------------------------------
// Layer 2: RoutedGraphGeometry (routing output → downstream emitter input)
// ---------------------------------------------------------------------------

/// Graph geometry with fully-routed edge paths.
///
/// Produced by the routing stage, consumed by downstream emitters.
#[derive(Debug, Clone)]
pub struct RoutedGraphGeometry {
    /// Same positioned nodes as input.
    pub nodes: HashMap<String, PositionedNode>,
    /// Fully-routed edges with polyline paths.
    pub edges: Vec<RoutedEdgeGeometry>,
    /// Subgraph bounds (may differ from layout bounds after routing adjustments).
    pub subgraphs: HashMap<String, SubgraphGeometry>,
    /// Routed self-edge paths.
    pub self_edges: Vec<RoutedSelfEdge>,
    /// Root direction.
    pub direction: Direction,
    /// Total bounds.
    pub bounds: FRect,
    /// Edges whose label rect could not fit in the gap between source and
    /// target node faces (after marker avoidance). Populated by the routing
    /// label-clamp pass in `route_graph_geometry`. **Empty in all healthy
    /// layouts.**
    ///
    /// Plan 0146 Task 2.1: this field is populated unconditionally in **all**
    /// builds (no `cfg(test)`, no env-var gating). Consumers may inspect it
    /// to surface a warning. Not serialized in MMDS layout output (it is a
    /// routing diagnostic, not part of the layout contract); MMDS adds it
    /// under a separate optional `diagnostics` object — see
    /// `mmds::output::serialize_routed_graph_with_diagnostics` (Task 2.3).
    pub unfit_label_overlaps: Vec<UnfitOverlap>,
}

/// One label that the clamp pass could not fit in the available gap
/// between source and target node faces (after marker avoidance).
///
/// Populated on `RoutedGraphGeometry::unfit_label_overlaps`. Read-only
/// after routing.
#[derive(Debug, Clone, PartialEq)]
pub struct UnfitOverlap {
    /// Index of the offending edge in the original `Graph::edges` list.
    pub edge_index: usize,
    /// Authored label text. Empty string for edges that have a label
    /// position but no label string (should not normally happen).
    pub label: String,
    /// Available gap between visual source's far face and visual target's
    /// near face along the edge-parallel axis, after marker avoidance.
    /// Negative values indicate marker avoidance + spacing already exceeds
    /// the inter-node distance.
    pub gap_pixels: f64,
    /// Label rect dimension along the edge-parallel axis.
    pub label_span_pixels: f64,
    /// The side the engine attempted to place the label on.
    pub attempted_side: EdgeLabelSide,
}

/// A fully-routed edge with polyline path.
#[derive(Debug, Clone)]
pub struct RoutedEdgeGeometry {
    pub index: usize,
    pub from: String,
    pub to: String,
    /// Polyline path in float coordinates.
    pub path: Vec<FPoint>,
    /// Label center position.
    pub label_position: Option<FPoint>,
    /// Label side (Above/Below/Center) from side selection.
    pub label_side: Option<EdgeLabelSide>,
    /// Label position near the target endpoint (head).
    pub head_label_position: Option<FPoint>,
    /// Label position near the source endpoint (tail).
    pub tail_label_position: Option<FPoint>,
    /// Whether this edge flows backward in the layout direction.
    pub is_backward: bool,
    /// If source is a subgraph-as-node, the subgraph ID.
    pub from_subgraph: Option<String>,
    /// If target is a subgraph-as-node, the subgraph ID.
    pub to_subgraph: Option<String>,
    /// Port attachment at the source node.
    pub source_port: Option<EdgePort>,
    /// Port attachment at the target node.
    pub target_port: Option<EdgePort>,
    /// Preserve the explicit orthogonal topology instead of simplifying it away.
    /// Set when routing introduced a deliberate de-overlap corridor.
    pub preserve_orthogonal_topology: bool,
    /// Shared label-rectangle geometry populated by the routing label-lane
    /// pass. Read by SVG/MMDS/bounds consumers; one rectangle, no divergence.
    pub label_geometry: Option<EdgeLabelGeometry>,
    /// Plan 0149 (#237): lane-aware re-wrap output. `Some(lines)` when the
    /// post-lane `label_rewrap` pass decided to narrow this edge's label
    /// below the pre-engine wrap width so it fits the compartment's
    /// `label_step` budget. Renderers (SVG `labels.rs`, text `edge.rs`,
    /// MMDS replay) must prefer this over `diagram.edges[idx].
    /// wrapped_label_lines` when present so the rect geometry and the
    /// emitted text stay in sync.
    ///
    /// Intentionally NOT propagated back to `Graph::edges.wrapped_label_lines`
    /// so the kernel's Grid measurement (which ran pre-routing) stays on the
    /// original pre-engine wrap. That's what keeps Text snapshots
    /// byte-stable — Grid-measured layer heights were committed before this
    /// field existed, and Text consumers that honour a routed override need
    /// to opt in explicitly. See `findings/01-spike-result.md` §1.2.
    pub effective_wrapped_lines: Option<Vec<String>>,
}

/// A routed self-edge loop.
#[derive(Debug, Clone)]
pub struct RoutedSelfEdge {
    pub node_id: String,
    pub edge_index: usize,
    pub path: Vec<FPoint>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_geometry_default_construction() {
        let geo = GraphGeometry {
            nodes: HashMap::new(),
            edges: Vec::new(),
            subgraphs: HashMap::new(),
            self_edges: Vec::new(),
            direction: Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 0.0, 0.0),
            reversed_edges: Vec::new(),
            engine_hints: None,
            grid_projection: None,
            rerouted_edges: HashSet::new(),
            enhanced_backward_routing: false,
        };
        assert!(geo.nodes.is_empty());
        assert!(geo.edges.is_empty());
        assert!(geo.engine_hints.is_none());
        assert!(geo.grid_projection.is_none());
    }

    #[test]
    fn engine_hints_layered_construction() {
        let hints = EngineHints::Layered(LayeredHints {
            node_ranks: HashMap::new(),
            rank_to_position: HashMap::new(),
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
        });
        let EngineHints::Layered(inner) = &hints;
        assert!(inner.node_ranks.is_empty());
    }

    #[test]
    fn layout_edge_path_hint_optional() {
        let edge = LayoutEdge {
            index: 0,
            from: "A".into(),
            to: "B".into(),
            waypoints: vec![FPoint::new(1.0, 2.0)],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: None,
            preserve_orthogonal_topology: false,
            label_geometry: None,
            effective_wrapped_lines: None,
        };
        assert!(edge.layout_path_hint.is_none());
        assert_eq!(edge.waypoints.len(), 1);
    }

    #[test]
    fn edge_label_geometry_constructs_with_center_rect_padding_side_track() {
        let g = EdgeLabelGeometry {
            center: FPoint::new(10.0, 20.0),
            rect: FRect::new(0.0, 10.0, 20.0, 20.0),
            padding: (4.0, 2.0),
            side: EdgeLabelSide::Above,
            track: 1,
        };
        assert_eq!(g.center.x, 10.0);
        assert_eq!(g.center.y, 20.0);
        assert_eq!(g.rect.width, 20.0);
        assert_eq!(g.track, 1);
    }

    #[test]
    fn layout_edge_carries_label_geometry_none_by_default() {
        let edge = LayoutEdge {
            index: 0,
            from: "A".into(),
            to: "B".into(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: None,
            preserve_orthogonal_topology: false,
            label_geometry: None,
            effective_wrapped_lines: None,
        };
        assert!(edge.label_geometry.is_none());
    }

    #[test]
    fn routed_edge_geometry_carries_label_geometry_none_by_default() {
        let edge = RoutedEdgeGeometry {
            index: 0,
            from: "A".into(),
            to: "B".into(),
            path: vec![],
            label_position: None,
            label_side: None,
            head_label_position: None,
            tail_label_position: None,
            is_backward: false,
            from_subgraph: None,
            to_subgraph: None,
            source_port: None,
            target_port: None,
            preserve_orthogonal_topology: false,
            label_geometry: None,
            effective_wrapped_lines: None,
        };
        assert!(edge.label_geometry.is_none());
    }

    #[test]
    fn routed_edge_geometry_with_ports() {
        let edge = RoutedEdgeGeometry {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            path: vec![],
            label_position: None,
            label_side: None,
            head_label_position: None,
            tail_label_position: None,
            is_backward: false,
            from_subgraph: None,
            to_subgraph: None,
            source_port: Some(EdgePort {
                face: crate::graph::attachment::PortFace::Bottom,
                fraction: 0.5,
                position: FPoint::new(50.0, 35.0),
                group_size: 1,
            }),
            target_port: None,
            preserve_orthogonal_topology: false,
            label_geometry: None,
            effective_wrapped_lines: None,
        };
        assert!(edge.source_port.is_some());
        assert!(edge.target_port.is_none());
    }
}
