//! Direct graph-in / geometry-out layout facade.
//!
//! This is a narrow, stable public surface for callers that build a
//! [`crate::graph::Graph`] in memory and want positioned node and routed edge
//! geometry back, without going through Mermaid source or MMDS JSON.
//!
//! The facade lives in `runtime` because it is the only boundary permitted to
//! reach the `engines` graph-solve contracts (see `boundaries.toml`). It hides
//! the engine-internal solve request, the `MeasurementMode` /
//! `TextMetricsProvider` measurement seam, and the full [`GraphGeometry`] hint
//! channels behind a small result type.
//!
//! Geometry is computed with the same text-metrics profile, geometry contract,
//! and routing defaults as the MMDS output path, so [`layout_graph`] returns
//! the same coordinates a caller would get from
//! `render_diagram(.., OutputFormat::Mmds, ..)` at the matching geometry level.
//!
//! Determinism: same input produces the same geometry. Nodes are returned
//! sorted by id and edges in input order, so the result is stable across runs
//! (the underlying geometry stores nodes in a `HashMap`, whose iteration order
//! is not).

use crate::engines::graph::contracts::{GraphGeometryContract, MeasurementMode};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphSolveRequest, SubgraphDirectionPolicy, solve_graph_family,
};
use crate::errors::RenderError;
use crate::graph::geometry::PositionedNode;
use crate::graph::label_wrap::prepare_wrapped_labels_with_provider;
use crate::graph::measure::{TextMetricsProfileConfig, resolve_text_metrics_profile};
use crate::graph::space::FPoint;
use crate::graph::{GeometryLevel, Graph};
use crate::runtime::config::LayoutConfig;

/// Options controlling a [`layout_graph`] call.
///
/// Construct with [`LayoutOptions::default`] for the recommended defaults
/// (the `flux-layered` engine and routed geometry) and override fields as
/// needed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LayoutOptions {
    /// Engine + algorithm used to solve the layout. Defaults to
    /// [`EngineAlgorithmId::FLUX_LAYERED`].
    pub engine: EngineAlgorithmId,
    /// Geometry detail level. [`GeometryLevel::Routed`] (the default) produces
    /// drawable edge polylines; [`GeometryLevel::Layout`] produces node
    /// positions and edge topology only, with empty edge polylines.
    pub geometry_level: GeometryLevel,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            engine: EngineAlgorithmId::FLUX_LAYERED,
            geometry_level: GeometryLevel::Routed,
        }
    }
}

impl LayoutOptions {
    /// Set the engine + algorithm used to solve the layout.
    ///
    /// Builder-style setter so callers can configure a `#[non_exhaustive]`
    /// `LayoutOptions` without struct-literal syntax.
    #[must_use]
    pub fn with_engine(mut self, engine: EngineAlgorithmId) -> Self {
        self.engine = engine;
        self
    }

    /// Set the geometry detail level.
    #[must_use]
    pub fn with_geometry_level(mut self, geometry_level: GeometryLevel) -> Self {
        self.geometry_level = geometry_level;
        self
    }
}

/// Laid-out geometry for a graph.
///
/// `nodes` are sorted by id and `edges` follow the input edge order, so the
/// whole struct is deterministic for a given input.
///
/// Coordinates ([`LaidOutNode::center`], [`LaidOutEdge::points`]) are absolute
/// layout coordinates. `width`/`height` are the bounding-box *extent* (matching
/// the MMDS `metadata.bounds`), not an offset — the bounds are not guaranteed to
/// start at `(0, 0)`, so a routed path that bows past the leftmost/topmost node
/// can place a coordinate slightly outside `0..width` / `0..height`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LaidOutGraph {
    /// Positioned nodes, sorted by [`LaidOutNode::id`].
    pub nodes: Vec<LaidOutNode>,
    /// Routed edges, in the order they were added to the input graph.
    pub edges: Vec<LaidOutEdge>,
    /// Bounding-box width (extent, matching MMDS `metadata.bounds`).
    pub width: f64,
    /// Bounding-box height (extent, matching MMDS `metadata.bounds`).
    pub height: f64,
}

/// A positioned node.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LaidOutNode {
    /// The node id, verbatim from the input graph.
    pub id: String,
    /// Center point of the node's bounding box.
    pub center: FPoint,
    /// Node width.
    pub width: f64,
    /// Node height.
    pub height: f64,
}

/// A routed edge.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LaidOutEdge {
    /// Source node id, verbatim from the input edge.
    pub from: String,
    /// Target node id, verbatim from the input edge.
    pub to: String,
    /// Routed polyline, in engine path order. Empty when
    /// [`LayoutOptions::geometry_level`] is [`GeometryLevel::Layout`]. When
    /// [`Self::is_backward`] is `false` the points run `from` → `to`; when
    /// `true` the path order may not match `from` → `to` (see `is_backward`).
    /// The path is passed through verbatim, mirroring the MMDS contract.
    pub points: Vec<FPoint>,
    /// Whether the engine reversed this edge for acyclic layout. Always
    /// `false` for acyclic input. When `true`, [`Self::points`] follow the
    /// reversed routed path rather than `from` → `to`; consumers that need a
    /// strict `from` → `to` polyline should account for this.
    pub is_backward: bool,
}

/// Solve layout for an in-memory graph and return positioned geometry.
///
/// Node ids and edge endpoints are opaque strings carried through verbatim;
/// no Mermaid grammar is involved, so ids may contain any characters.
///
/// # Errors
///
/// Returns [`RenderError`] if the requested engine is unavailable or the
/// engine fails to solve the layout.
pub fn layout_graph(graph: &Graph, options: &LayoutOptions) -> Result<LaidOutGraph, RenderError> {
    // Resolve the default proportional text metrics profile (mmdflux-sans-v1),
    // matching the MMDS output path so geometry is consistent with the JSON
    // a caller would get from `render_diagram(.., Mmds, ..)`.
    let resolved =
        resolve_text_metrics_profile(TextMetricsProfileConfig::default()).map_err(|error| {
            RenderError {
                message: error.to_string(),
            }
        })?;
    let metrics = &resolved.metrics;

    let layout_config = LayoutConfig::default();

    // Clone so the required pre-engine wrap pass can populate
    // `wrapped_label_lines` without mutating the caller's graph. The pass is a
    // no-op for label-free edges.
    let mut graph = graph.clone();
    prepare_wrapped_labels_with_provider(
        &mut graph.edges,
        metrics,
        layout_config.edge_label_max_width,
    );

    let request = GraphSolveRequest::new(
        MeasurementMode::Proportional(metrics),
        GraphGeometryContract::Canonical,
        options.geometry_level,
        None,
        SubgraphDirectionPolicy::default(),
    );
    let engine_config = EngineConfig::from(layout_config);
    let result = solve_graph_family(&graph, options.engine, &engine_config, &request)?;

    let mut nodes: Vec<LaidOutNode> = result.geometry.nodes.values().map(laid_out_node).collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // Routed edge paths, keyed by edge index for input-order assembly. Normal
    // edges live in `routed.edges`; self-loops are routed into
    // `routed.self_edges` (keyed by `edge_index`), so check both — mirroring
    // the MMDS emitter's fallback.
    let routed = result.routed.as_ref();
    let edges: Vec<LaidOutEdge> = graph
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let (points, is_backward) = routed
                .and_then(|r| {
                    r.edges
                        .iter()
                        .find(|e| e.index == index)
                        .map(|e| (e.path.clone(), e.is_backward))
                        .or_else(|| {
                            r.self_edges
                                .iter()
                                .find(|e| e.edge_index == index)
                                .map(|e| (e.path.clone(), false))
                        })
                })
                .unwrap_or_default();
            LaidOutEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
                points,
                is_backward,
            }
        })
        .collect();

    let bounds = routed.map_or(result.geometry.bounds, |r| r.bounds);

    Ok(LaidOutGraph {
        nodes,
        edges,
        width: bounds.width,
        height: bounds.height,
    })
}

fn laid_out_node(node: &PositionedNode) -> LaidOutNode {
    // `PositionedNode.rect` is a top-left-corner rect; emit the center so
    // callers never have to know the corner convention.
    LaidOutNode {
        id: node.id.clone(),
        center: node.rect.center(),
        width: node.rect.width,
        height: node.rect.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Direction, Edge, Node};

    /// Supersession-style DAG: B1 and B2 both supersede A; C supersedes B1; D
    /// is isolated. Edges point newest -> superseded.
    fn supersession_graph() -> Graph {
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("rev:sha256:A").with_label("A"));
        g.add_node(Node::new("rev:sha256:B1").with_label("B1"));
        g.add_node(Node::new("rev:sha256:B2").with_label("B2"));
        g.add_node(Node::new("rev:sha256:C").with_label("C"));
        g.add_node(Node::new("rev:sha256:D").with_label("D"));
        g.add_edge(Edge::new("rev:sha256:B1", "rev:sha256:A"));
        g.add_edge(Edge::new("rev:sha256:B2", "rev:sha256:A"));
        g.add_edge(Edge::new("rev:sha256:C", "rev:sha256:B1"));
        g
    }

    #[test]
    fn lays_out_nodes_sorted_by_id() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        let ids: Vec<&str> = laid.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "nodes must be returned sorted by id");
        assert_eq!(laid.nodes.len(), 5);
    }

    #[test]
    fn opaque_ids_round_trip_verbatim() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        assert!(laid.nodes.iter().any(|n| n.id == "rev:sha256:B1"));
        assert!(
            laid.edges
                .iter()
                .any(|e| e.from == "rev:sha256:C" && e.to == "rev:sha256:B1")
        );
    }

    #[test]
    fn edges_follow_input_order() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        let pairs: Vec<(&str, &str)> = laid
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("rev:sha256:B1", "rev:sha256:A"),
                ("rev:sha256:B2", "rev:sha256:A"),
                ("rev:sha256:C", "rev:sha256:B1"),
            ]
        );
    }

    #[test]
    fn routed_level_produces_edge_polylines() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        for edge in &laid.edges {
            assert!(
                edge.points.len() >= 2,
                "routed edge {} -> {} should have a polyline",
                edge.from,
                edge.to
            );
        }
    }

    #[test]
    fn layout_level_omits_edge_polylines() {
        let options = LayoutOptions::default().with_geometry_level(GeometryLevel::Layout);
        let laid = layout_graph(&supersession_graph(), &options).expect("layout should succeed");
        for edge in &laid.edges {
            assert!(edge.points.is_empty());
        }
        // Node geometry is still present at layout level.
        assert_eq!(laid.nodes.len(), 5);
    }

    #[test]
    fn competing_heads_are_equal_rank_peers() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        let b1 = laid.nodes.iter().find(|n| n.id == "rev:sha256:B1").unwrap();
        let b2 = laid.nodes.iter().find(|n| n.id == "rev:sha256:B2").unwrap();
        assert!(
            (b1.center.y - b2.center.y).abs() < f64::EPSILON,
            "competing heads must share a rank (y): B1={} B2={}",
            b1.center.y,
            b2.center.y
        );
    }

    #[test]
    fn newest_node_is_above_superseded_node() {
        // TopDown: source (newest) sits above target (superseded), i.e. smaller y.
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        let a = laid.nodes.iter().find(|n| n.id == "rev:sha256:A").unwrap();
        let b1 = laid.nodes.iter().find(|n| n.id == "rev:sha256:B1").unwrap();
        assert!(b1.center.y < a.center.y, "B1 should be above A");
    }

    #[test]
    fn same_input_produces_identical_geometry() {
        let g = supersession_graph();
        let first = layout_graph(&g, &LayoutOptions::default()).expect("layout should succeed");
        for _ in 0..4 {
            let next = layout_graph(&g, &LayoutOptions::default()).expect("layout should succeed");
            assert_eq!(first, next, "layout must be deterministic");
        }
    }

    #[test]
    fn does_not_mutate_the_caller_graph() {
        let g = supersession_graph();
        let edges_before = g.edges.clone();
        let _ = layout_graph(&g, &LayoutOptions::default()).expect("layout should succeed");
        assert_eq!(
            g.edges.len(),
            edges_before.len(),
            "input graph must be untouched"
        );
    }

    #[test]
    fn single_isolated_node_lays_out() {
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("only").with_label("only"));
        let laid = layout_graph(&g, &LayoutOptions::default()).expect("layout should succeed");
        assert_eq!(laid.nodes.len(), 1);
        assert!(laid.edges.is_empty());
        assert!(laid.width > 0.0 && laid.height > 0.0);
    }

    #[test]
    fn acyclic_edges_are_not_backward() {
        let laid = layout_graph(&supersession_graph(), &LayoutOptions::default())
            .expect("layout should succeed");
        assert!(laid.edges.iter().all(|e| !e.is_backward));
    }

    #[test]
    fn self_edge_gets_a_routed_polyline() {
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("a").with_label("a"));
        g.add_node(Node::new("b").with_label("b"));
        g.add_edge(Edge::new("a", "b"));
        g.add_edge(Edge::new("a", "a")); // self-loop, routed into self_edges

        let laid = layout_graph(&g, &LayoutOptions::default()).expect("layout should succeed");

        let self_edge = laid
            .edges
            .iter()
            .find(|e| e.from == "a" && e.to == "a")
            .expect("self-edge should be present in input order");
        assert!(
            self_edge.points.len() >= 2,
            "self-edge should carry its loop polyline, got {:?}",
            self_edge.points
        );
    }
}
