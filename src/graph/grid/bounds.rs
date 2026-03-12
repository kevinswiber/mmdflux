use std::collections::HashMap;

use super::intersect::{NodeFace, classify_face};
use super::{GridLayout, NodeBounds, SubgraphBounds};
use crate::graph::{Direction, Edge, Shape};

pub(crate) type NodeContainingSubgraphMap<'a> = HashMap<&'a str, &'a str>;

pub(crate) fn subgraph_edge_face(
    bounds: &NodeBounds,
    other: &NodeBounds,
    direction: Direction,
) -> NodeFace {
    let bounds_right = bounds.x + bounds.width.saturating_sub(1);
    let bounds_bottom = bounds.y + bounds.height.saturating_sub(1);
    let other_right = other.x + other.width.saturating_sub(1);
    let other_bottom = other.y + other.height.saturating_sub(1);

    match direction {
        Direction::TopDown | Direction::BottomTop => {
            if other_bottom < bounds.y {
                return NodeFace::Top;
            }
            if other.y > bounds_bottom {
                return NodeFace::Bottom;
            }
            if other_right < bounds.x {
                return NodeFace::Left;
            }
            if other.x > bounds_right {
                return NodeFace::Right;
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            if other_right < bounds.x {
                return NodeFace::Left;
            }
            if other.x > bounds_right {
                return NodeFace::Right;
            }
            if other_bottom < bounds.y {
                return NodeFace::Top;
            }
            if other.y > bounds_bottom {
                return NodeFace::Bottom;
            }
        }
    }

    classify_face(
        bounds,
        (other.center_x(), other.center_y()),
        Shape::Rectangle,
    )
}

pub(crate) fn subgraph_bounds_as_node(bounds: &SubgraphBounds) -> NodeBounds {
    NodeBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

pub(crate) fn resolve_edge_bounds(
    layout: &GridLayout,
    edge: &Edge,
) -> Option<(NodeBounds, NodeBounds)> {
    let from_bounds = if let Some(sg_id) = edge.from_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.from)?
    };
    let to_bounds = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        layout
            .subgraph_bounds
            .get(sg_id)
            .map(subgraph_bounds_as_node)?
    } else {
        *layout.get_bounds(&edge.to)?
    };
    Some((from_bounds, to_bounds))
}

pub(crate) fn bounds_for_node_id(layout: &GridLayout, node_id: &str) -> Option<NodeBounds> {
    if let Some(bounds) = layout.get_bounds(node_id) {
        return Some(*bounds);
    }
    layout
        .subgraph_bounds
        .get(node_id)
        .map(subgraph_bounds_as_node)
}

pub(crate) fn node_inside_subgraph(bounds: &NodeBounds, sg: &SubgraphBounds) -> bool {
    let node_right = bounds.x + bounds.width;
    let node_bottom = bounds.y + bounds.height;
    let sg_right = sg.x + sg.width;
    let sg_bottom = sg.y + sg.height;
    bounds.x >= sg.x && bounds.y >= sg.y && node_right <= sg_right && node_bottom <= sg_bottom
}

pub(crate) fn containing_subgraph_id_uncached<'a>(
    layout: &'a GridLayout,
    node_id: &str,
) -> Option<&'a str> {
    let bounds = layout.node_bounds.get(node_id)?;
    layout
        .subgraph_bounds
        .iter()
        .filter(|(_, sg)| node_inside_subgraph(bounds, sg))
        .max_by_key(|(_, sg)| (sg.depth, usize::MAX - (sg.width * sg.height)))
        .map(|(id, _)| id.as_str())
}

pub(crate) fn containing_subgraph_id<'a>(
    layout: &'a GridLayout,
    node_id: &str,
    node_containing_subgraph: Option<&NodeContainingSubgraphMap<'a>>,
) -> Option<&'a str> {
    node_containing_subgraph
        .and_then(|map| map.get(node_id).copied())
        .or_else(|| containing_subgraph_id_uncached(layout, node_id))
}

pub(crate) fn build_node_containing_subgraph_map<'a>(
    layout: &'a GridLayout,
) -> NodeContainingSubgraphMap<'a> {
    layout
        .node_bounds
        .keys()
        .filter_map(|node_id| {
            containing_subgraph_id_uncached(layout, node_id.as_str())
                .map(|subgraph_id| (node_id.as_str(), subgraph_id))
        })
        .collect()
}
