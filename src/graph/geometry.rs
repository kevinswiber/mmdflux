//! Graph-family geometry IR contracts.
//!
//! Two-layer float-coordinate geometry produced by layout engines and
//! consumed by routing and rendering. Engine-agnostic core with optional
//! engine-specific hint channels.

use std::collections::{HashMap, HashSet};

use crate::graph::grid_projection::GridProjection;
use crate::graph::{Direction, Shape};

// ---------------------------------------------------------------------------
// Float coordinate primitives
// ---------------------------------------------------------------------------

/// Float-precision rectangle (layout coordinate space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl FRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
}

/// Float-precision point (layout coordinate space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FPoint {
    pub x: f64,
    pub y: f64,
}

impl FPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
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
    /// Optional engine-specific metadata for migration-sensitive behavior.
    pub engine_hints: Option<EngineHints>,
    /// Optional graph-owned replay metadata for projecting float geometry onto a discrete grid.
    pub grid_projection: Option<GridProjection>,
    /// Edge indices rerouted by the layout engine (e.g., direction-override subgraph edges).
    /// Populated by engines that perform SVG-specific subgraph post-processing.
    /// Used by the SVG renderer to skip shape-clipping on explicitly routed edges.
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

// ---------------------------------------------------------------------------
// Engine hints (optional, typed per engine)
// ---------------------------------------------------------------------------

/// Engine-specific metadata that does not belong in the core geometry contract.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EngineHints {
    Layered(LayeredHints),
}

/// Layered-layout-specific metadata needed during migration.
///
/// Preserves rank-annotated data from the layered layout that the text pipeline
/// needs for grid-snap coordinate transformation. Other engines won't populate this.
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
// Layer 2: RoutedGraphGeometry (routing output → renderer input)
// ---------------------------------------------------------------------------

/// Graph geometry with fully-routed edge paths.
///
/// Produced by the routing stage, consumed by renderers.
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
}

/// A routed self-edge loop.
#[derive(Debug, Clone)]
pub struct RoutedSelfEdge {
    pub node_id: String,
    pub edge_index: usize,
    pub path: Vec<FPoint>,
}

// ---------------------------------------------------------------------------
// Port attachment types
// ---------------------------------------------------------------------------

/// Which face of a node boundary an edge port attaches to.
///
/// Separate from `text_routing_core::Face` to avoid coupling the
/// geometry IR to the text rendering module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortFace {
    Top,
    Bottom,
    Left,
    Right,
}

impl PortFace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl std::str::FromStr for PortFace {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(()),
        }
    }
}

/// Port attachment information for one end of a routed edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePort {
    /// Face on the node boundary where the edge attaches.
    pub face: PortFace,
    /// Fractional position along the face (0.0 = start, 1.0 = end).
    /// For top/bottom: 0.0 is left, 1.0 is right.
    /// For left/right: 0.0 is top, 1.0 is bottom.
    pub fraction: f64,
    /// Computed position on the node boundary in layout coordinate space.
    pub position: FPoint,
    /// Number of edges attached to this face of this node.
    pub group_size: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagram;
    use crate::engines::graph::algorithms::layered;
    use crate::engines::graph::algorithms::layered::adapter::from_layered_layout;
    use crate::engines::graph::algorithms::layered::normalize::WaypointWithRank;
    use crate::engines::graph::algorithms::layered::{
        EdgeLayout, NodeId, Point, Rect, SelfEdgeLayout,
    };
    use crate::graph::{Edge, Node};

    /// Build a simple LayoutResult with two nodes and one edge.
    fn sample_layout_result() -> layered::LayoutResult {
        let mut nodes = HashMap::new();
        nodes.insert(
            NodeId::from("A"),
            Rect {
                x: 50.0,
                y: 25.0,
                width: 40.0,
                height: 20.0,
            },
        );
        nodes.insert(
            NodeId::from("B"),
            Rect {
                x: 50.0,
                y: 75.0,
                width: 40.0,
                height: 20.0,
            },
        );

        let edges = vec![EdgeLayout {
            from: NodeId::from("A"),
            to: NodeId::from("B"),
            points: vec![Point { x: 50.0, y: 35.0 }, Point { x: 50.0, y: 65.0 }],
            index: 0,
        }];

        let mut node_ranks = HashMap::new();
        node_ranks.insert(NodeId::from("A"), 0);
        node_ranks.insert(NodeId::from("B"), 2);

        let mut rank_to_position = HashMap::new();
        rank_to_position.insert(0, (15.0, 35.0));
        rank_to_position.insert(2, (65.0, 85.0));

        layered::LayoutResult {
            nodes,
            edges,
            reversed_edges: vec![],
            width: 100.0,
            height: 100.0,
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
            label_sides: HashMap::new(),
            subgraph_bounds: HashMap::new(),
            self_edges: vec![],
            rank_to_position,
            node_ranks,
        }
    }

    /// Build a matching Diagram for the sample layout result.
    fn sample_diagram() -> Diagram {
        let mut diagram = Diagram::new(Direction::TopDown);
        diagram.add_node(Node::new("A"));
        diagram.add_node(Node::new("B"));
        diagram.add_edge(Edge::new("A", "B"));
        diagram
    }

    #[test]
    fn layered_adapter_produces_nodes_and_edges() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        assert_eq!(geom.nodes.len(), 2);
        assert_eq!(geom.edges.len(), 1);
        assert!(geom.engine_hints.is_some());
        assert!(geom.grid_projection.is_some());
        assert_eq!(geom.direction, Direction::TopDown);
    }

    #[test]
    fn layered_adapter_maps_node_rect() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        let node_a = &geom.nodes["A"];
        assert_eq!(node_a.rect.x, 50.0);
        assert_eq!(node_a.rect.y, 25.0);
        assert_eq!(node_a.rect.width, 40.0);
        assert_eq!(node_a.rect.height, 20.0);
        assert_eq!(node_a.label, "A");
        assert_eq!(node_a.shape, Shape::Rectangle);
        assert!(node_a.parent.is_none());
    }

    #[test]
    fn layered_adapter_maps_edge() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        let edge = &geom.edges[0];
        assert_eq!(edge.index, 0);
        assert_eq!(edge.from, "A");
        assert_eq!(edge.to, "B");
        assert!(edge.waypoints.is_empty()); // short edge, no waypoints
        assert!(edge.label_position.is_none());
        assert!(edge.from_subgraph.is_none());
        assert!(edge.to_subgraph.is_none());
        // layout_path_hint is populated from layout edge points
        let path = edge.layout_path_hint.as_ref().unwrap();
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].x, 50.0);
        assert_eq!(path[0].y, 35.0);
    }

    #[test]
    fn layered_adapter_maps_waypoints_with_ranks() {
        let mut result = sample_layout_result();
        result.edge_waypoints.insert(
            0,
            vec![WaypointWithRank {
                point: Point { x: 50.0, y: 50.0 },
                rank: 1,
            }],
        );
        result.label_positions.insert(
            0,
            WaypointWithRank {
                point: Point { x: 50.0, y: 48.0 },
                rank: 1,
            },
        );

        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        // Check geometry IR waypoints (positions only)
        assert_eq!(geom.edges[0].waypoints.len(), 1);
        assert_eq!(geom.edges[0].waypoints[0].x, 50.0);
        assert_eq!(geom.edges[0].waypoints[0].y, 50.0);
        assert!(geom.edges[0].label_position.is_some());

        // Check layered hints preserve rank info
        let hints = match &geom.engine_hints {
            Some(EngineHints::Layered(h)) => h,
            _ => panic!("expected layered hints"),
        };
        let projection = geom.grid_projection.as_ref().expect("grid projection");
        let wp_ranks = &hints.edge_waypoints[&0];
        assert_eq!(wp_ranks.len(), 1);
        assert_eq!(wp_ranks[0].1, 1); // rank = 1
        let (lp, lr) = &hints.label_positions[&0];
        assert_eq!(lp.x, 50.0);
        assert_eq!(*lr, 1);
        assert_eq!(projection.edge_waypoints[&0][0].1, 1);
        assert_eq!(projection.label_positions[&0].1, 1);
    }

    #[test]
    fn layered_adapter_maps_layout_hints() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        let hints = match &geom.engine_hints {
            Some(EngineHints::Layered(h)) => h,
            _ => panic!("expected layered hints"),
        };
        assert_eq!(hints.node_ranks["A"], 0);
        assert_eq!(hints.node_ranks["B"], 2);
        assert_eq!(hints.rank_to_position[&0], (15.0, 35.0));
        assert_eq!(hints.rank_to_position[&2], (65.0, 85.0));
    }

    #[test]
    fn layered_adapter_maps_reversed_edges() {
        let mut result = sample_layout_result();
        result.reversed_edges = vec![0];
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);
        assert_eq!(geom.reversed_edges, vec![0]);
    }

    #[test]
    fn layered_adapter_maps_bounds() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);
        assert_eq!(geom.bounds.width, 100.0);
        assert_eq!(geom.bounds.height, 100.0);
    }

    #[test]
    fn layered_adapter_maps_self_edges() {
        let mut result = sample_layout_result();
        result.self_edges.push(SelfEdgeLayout {
            node: NodeId::from("A"),
            edge_index: 1,
            points: vec![
                Point { x: 70.0, y: 15.0 },
                Point { x: 80.0, y: 15.0 },
                Point { x: 80.0, y: 35.0 },
                Point { x: 70.0, y: 35.0 },
            ],
        });
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        assert_eq!(geom.self_edges.len(), 1);
        assert_eq!(geom.self_edges[0].node_id, "A");
        assert_eq!(geom.self_edges[0].edge_index, 1);
        assert_eq!(geom.self_edges[0].points.len(), 4);
    }

    #[test]
    fn layered_adapter_maps_subgraph_bounds() {
        let mut result = sample_layout_result();
        result.subgraph_bounds.insert(
            "sg1".into(),
            Rect {
                x: 10.0,
                y: 5.0,
                width: 80.0,
                height: 90.0,
            },
        );

        let mut diagram = sample_diagram();
        diagram.subgraphs.insert(
            "sg1".into(),
            crate::graph::Subgraph {
                id: "sg1".into(),
                title: "Group 1".into(),
                nodes: vec!["A".into(), "B".into()],
                parent: None,
                dir: None,
            },
        );

        let geom = from_layered_layout(&result, &diagram);
        assert_eq!(geom.subgraphs.len(), 1);
        let sg = &geom.subgraphs["sg1"];
        assert_eq!(sg.title, "Group 1");
        assert_eq!(sg.rect.x, 10.0);
        assert_eq!(sg.depth, 0);
    }

    #[test]
    fn layered_adapter_node_directions() {
        let result = sample_layout_result();
        let diagram = sample_diagram();
        let geom = from_layered_layout(&result, &diagram);

        // Both nodes should have root direction (no subgraph overrides)
        assert_eq!(geom.node_directions["A"], Direction::TopDown);
        assert_eq!(geom.node_directions["B"], Direction::TopDown);
    }

    #[test]
    fn layered_adapter_skips_compound_nodes() {
        // If layout result has a subgraph as a node entry (compound graph),
        // it should not appear in geometry nodes (only in subgraphs).
        let mut result = sample_layout_result();
        result.nodes.insert(
            NodeId::from("sg1"),
            Rect {
                x: 50.0,
                y: 50.0,
                width: 100.0,
                height: 100.0,
            },
        );
        result.subgraph_bounds.insert(
            "sg1".into(),
            Rect {
                x: 10.0,
                y: 5.0,
                width: 80.0,
                height: 90.0,
            },
        );

        let mut diagram = sample_diagram();
        diagram.subgraphs.insert(
            "sg1".into(),
            crate::graph::Subgraph {
                id: "sg1".into(),
                title: "SG".into(),
                nodes: vec!["A".into()],
                parent: None,
                dir: None,
            },
        );

        let geom = from_layered_layout(&result, &diagram);
        // sg1 should NOT appear in nodes (not in diagram.nodes)
        assert!(!geom.nodes.contains_key("sg1"));
        // sg1 SHOULD appear in subgraphs
        assert!(geom.subgraphs.contains_key("sg1"));
    }

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
        assert!(matches!(hints, EngineHints::Layered(_)));
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
        };
        assert!(edge.layout_path_hint.is_none());
        assert_eq!(edge.waypoints.len(), 1);
    }

    #[test]
    fn port_face_as_str() {
        assert_eq!(PortFace::Top.as_str(), "top");
        assert_eq!(PortFace::Bottom.as_str(), "bottom");
        assert_eq!(PortFace::Left.as_str(), "left");
        assert_eq!(PortFace::Right.as_str(), "right");
    }

    #[test]
    fn port_face_from_str() {
        assert_eq!("top".parse::<PortFace>(), Ok(PortFace::Top));
        assert_eq!("bottom".parse::<PortFace>(), Ok(PortFace::Bottom));
        assert_eq!("left".parse::<PortFace>(), Ok(PortFace::Left));
        assert_eq!("right".parse::<PortFace>(), Ok(PortFace::Right));
        assert_eq!("invalid".parse::<PortFace>(), Err(()));
    }

    #[test]
    fn edge_port_construction() {
        let port = EdgePort {
            face: PortFace::Top,
            fraction: 0.5,
            position: FPoint { x: 50.0, y: 10.0 },
            group_size: 1,
        };
        assert_eq!(port.face, PortFace::Top);
        assert!((port.fraction - 0.5).abs() < f64::EPSILON);
        assert!((port.position.x - 50.0).abs() < f64::EPSILON);
        assert_eq!(port.group_size, 1);
    }

    #[test]
    fn routed_edge_geometry_with_ports() {
        let port = EdgePort {
            face: PortFace::Bottom,
            fraction: 0.5,
            position: FPoint { x: 50.0, y: 35.0 },
            group_size: 1,
        };
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
            source_port: Some(port),
            target_port: None,
            preserve_orthogonal_topology: false,
        };
        assert!(edge.source_port.is_some());
        assert!(edge.target_port.is_none());
    }

    #[test]
    fn frect_center() {
        let r = FRect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.center_x(), 60.0);
        assert_eq!(r.center_y(), 45.0);
    }
}
