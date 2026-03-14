//! Adapter from layered kernel `LayoutResult` to graph-family `GraphGeometry` IR.

use std::collections::{HashMap, HashSet};

use super::kernel::types::LabelSide;
use super::kernel::{LayoutResult, Point, Rect};
use crate::graph::Graph;
use crate::graph::direction_policy::build_node_directions;
use crate::graph::geometry::{
    EdgeLabelSide, EngineHints, GraphGeometry, LayeredHints, LayoutEdge, PositionedNode,
    SelfEdgeGeometry, SubgraphGeometry,
};
use crate::graph::projection::GridProjection;
use crate::graph::space::{FPoint, FRect};

impl From<FPoint> for Point {
    fn from(p: FPoint) -> Self {
        Point { x: p.x, y: p.y }
    }
}

impl From<Point> for FPoint {
    fn from(p: Point) -> Self {
        FPoint::new(p.x, p.y)
    }
}

impl From<FRect> for Rect {
    fn from(r: FRect) -> Self {
        Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl From<Rect> for FRect {
    fn from(r: Rect) -> Self {
        FRect::new(r.x, r.y, r.width, r.height)
    }
}

fn map_label_side(side: LabelSide) -> EdgeLabelSide {
    match side {
        LabelSide::Above => EdgeLabelSide::Above,
        LabelSide::Below => EdgeLabelSide::Below,
        LabelSide::Center => EdgeLabelSide::Center,
    }
}

/// Convert layered `LayoutResult` + `Diagram` into engine-agnostic `GraphGeometry`.
pub fn from_layered_layout(result: &LayoutResult, diagram: &Graph) -> GraphGeometry {
    let nodes: HashMap<String, PositionedNode> = result
        .nodes
        .iter()
        .filter_map(|(node_id, rect)| {
            let node = diagram.nodes.get(&node_id.0)?;
            Some((
                node_id.0.clone(),
                PositionedNode {
                    id: node_id.0.clone(),
                    rect: FRect::new(rect.x, rect.y, rect.width, rect.height),
                    shape: node.shape,
                    label: node.label.clone(),
                    parent: node.parent.clone(),
                },
            ))
        })
        .collect();

    let edges: Vec<LayoutEdge> = result
        .edges
        .iter()
        .map(|el| {
            let diagram_edge = diagram.edges.get(el.index);
            let waypoints: Vec<FPoint> = result
                .edge_waypoints
                .get(&el.index)
                .map(|wps| {
                    wps.iter()
                        .map(|wp| FPoint::new(wp.point.x, wp.point.y))
                        .collect()
                })
                .unwrap_or_default();

            let label_position = result
                .label_positions
                .get(&el.index)
                .map(|wp| FPoint::new(wp.point.x, wp.point.y));

            let label_side = result
                .label_sides
                .get(&el.index)
                .copied()
                .map(map_label_side);

            let (from_subgraph, to_subgraph) = if let Some(edge) = diagram_edge {
                (edge.from_subgraph.clone(), edge.to_subgraph.clone())
            } else {
                let from_subgraph = if diagram.is_subgraph(&el.from.0) {
                    Some(el.from.0.clone())
                } else {
                    None
                };
                let to_subgraph = if diagram.is_subgraph(&el.to.0) {
                    Some(el.to.0.clone())
                } else {
                    None
                };
                (from_subgraph, to_subgraph)
            };

            LayoutEdge {
                index: el.index,
                from: el.from.0.clone(),
                to: el.to.0.clone(),
                waypoints,
                label_position,
                label_side,
                from_subgraph,
                to_subgraph,
                layout_path_hint: if el.points.is_empty() {
                    None
                } else {
                    Some(el.points.iter().map(|p| FPoint::new(p.x, p.y)).collect())
                },
                preserve_orthogonal_topology: false,
            }
        })
        .collect();

    let subgraphs: HashMap<String, SubgraphGeometry> = result
        .subgraph_bounds
        .iter()
        .filter_map(|(sg_id, rect)| {
            let sg = diagram.subgraphs.get(sg_id)?;
            Some((
                sg_id.clone(),
                SubgraphGeometry {
                    id: sg_id.clone(),
                    rect: FRect::new(rect.x, rect.y, rect.width, rect.height),
                    title: sg.title.clone(),
                    depth: diagram.subgraph_depth(sg_id),
                },
            ))
        })
        .collect();

    let self_edges: Vec<SelfEdgeGeometry> = result
        .self_edges
        .iter()
        .map(|sel| SelfEdgeGeometry {
            node_id: sel.node.0.clone(),
            edge_index: sel.edge_index,
            points: sel.points.iter().map(|p| FPoint::new(p.x, p.y)).collect(),
        })
        .collect();

    let hint_node_ranks: HashMap<String, i32> = result
        .node_ranks
        .iter()
        .map(|(id, &rank)| (id.0.clone(), rank))
        .collect();

    let hint_edge_waypoints: HashMap<usize, Vec<(FPoint, i32)>> = result
        .edge_waypoints
        .iter()
        .map(|(&idx, wps)| {
            (
                idx,
                wps.iter()
                    .map(|wp| (FPoint::new(wp.point.x, wp.point.y), wp.rank))
                    .collect(),
            )
        })
        .collect();

    let hint_label_positions: HashMap<usize, (FPoint, i32)> = result
        .label_positions
        .iter()
        .map(|(&idx, wp)| (idx, (FPoint::new(wp.point.x, wp.point.y), wp.rank)))
        .collect();

    GraphGeometry {
        nodes,
        edges,
        subgraphs,
        self_edges,
        direction: diagram.direction,
        node_directions: build_node_directions(diagram),
        bounds: FRect::new(0.0, 0.0, result.width, result.height),
        reversed_edges: result.reversed_edges.clone(),
        engine_hints: Some(EngineHints::Layered(LayeredHints {
            node_ranks: hint_node_ranks,
            rank_to_position: result.rank_to_position.clone(),
            edge_waypoints: hint_edge_waypoints,
            label_positions: hint_label_positions,
        })),
        grid_projection: Some(GridProjection {
            node_ranks: result
                .node_ranks
                .iter()
                .map(|(id, &rank)| (id.0.clone(), rank))
                .collect(),
            edge_waypoints: result
                .edge_waypoints
                .iter()
                .map(|(&idx, wps)| {
                    (
                        idx,
                        wps.iter()
                            .map(|wp| (FPoint::new(wp.point.x, wp.point.y), wp.rank))
                            .collect(),
                    )
                })
                .collect(),
            label_positions: result
                .label_positions
                .iter()
                .map(|(&idx, wp)| (idx, (FPoint::new(wp.point.x, wp.point.y), wp.rank)))
                .collect(),
            override_subgraphs: HashMap::new(),
        }),
        rerouted_edges: HashSet::new(),
        enhanced_backward_routing: false,
    }
}
