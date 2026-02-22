//! Shared layout building infrastructure used by text, SVG, and engine pipelines.
//!
//! Contains the `build_layered_layout()` entry point, sublayout computation for
//! direction-override subgraphs, and the `layered_config_for_layout()` bridge
//! from `TextLayoutConfig` to `layered::LayoutConfig`.

use std::collections::{HashMap, HashSet};

use super::text_types::TextLayoutConfig;
use crate::graph::{Diagram, Direction, Edge, Node, Stroke};
use crate::layered::{self, Direction as LayeredDirection, LayoutConfig as LayeredConfig};

/// Convert a graph-level Direction to a layered Direction.
pub(crate) fn to_layered_direction(dir: Direction) -> LayeredDirection {
    match dir {
        Direction::TopDown => LayeredDirection::TopBottom,
        Direction::BottomTop => LayeredDirection::BottomTop,
        Direction::LeftRight => LayeredDirection::LeftRight,
        Direction::RightLeft => LayeredDirection::RightLeft,
    }
}

/// Pre-computed sub-layout result for a direction-override subgraph.
pub(crate) struct SubLayoutResult {
    /// The LayoutResult with node positions in the sub-layout coordinate system.
    pub(crate) result: layered::LayoutResult,
    /// Map from sublayout edge index to original diagram edge index.
    pub(crate) edge_index_map: Vec<usize>,
}

/// Mermaid-compatible isolation check for sublayout extraction.
///
/// Treat edges that target/source the subgraph endpoint (`to_subgraph` /
/// `from_subgraph`) as cluster-endpoint links, not direction-tainting
/// node-level cross-boundary edges.
fn subgraph_has_tainting_cross_boundary_edges(diagram: &Diagram, sg_id: &str) -> bool {
    let Some(sg) = diagram.subgraphs.get(sg_id) else {
        return false;
    };
    let sg_nodes: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
    diagram.edges.iter().any(|edge| {
        let from_in = sg_nodes.contains(edge.from.as_str());
        let to_in = sg_nodes.contains(edge.to.as_str());
        if from_in == to_in {
            return false;
        }
        let via_sg_endpoint = edge.to_subgraph.as_deref() == Some(sg_id)
            || edge.from_subgraph.as_deref() == Some(sg_id);
        !via_sg_endpoint
    })
}

/// Compute sub-layouts for subgraphs with direction overrides.
///
/// For each subgraph that has a `dir` override, this creates a standalone layered
/// graph with just the subgraph's internal nodes and edges, and runs layout with
/// the overridden direction. The resulting dimensions can be injected into the
/// parent layout so the compound node is sized correctly.
pub(crate) fn compute_sublayouts<FN, FE>(
    diagram: &Diagram,
    parent_layered_config: &LayeredConfig,
    node_dims: FN,
    edge_label_dims: FE,
    skip_non_isolated_overrides: bool,
) -> HashMap<String, SubLayoutResult>
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let mut sublayouts = HashMap::new();

    for (sg_id, sg) in &diagram.subgraphs {
        let sub_dir = match sg.dir {
            Some(d) => d,
            None => continue,
        };

        if skip_non_isolated_overrides && subgraph_has_tainting_cross_boundary_edges(diagram, sg_id)
        {
            continue;
        }

        let layered_direction = to_layered_direction(sub_dir);

        let mut sub_graph: layered::DiGraph<(f64, f64)> = layered::DiGraph::new();

        // Add leaf nodes (not child subgraphs)
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id)
                && let Some(node) = diagram.nodes.get(node_id)
            {
                let (w, h) = node_dims(node);
                sub_graph.add_node(node_id.as_str(), (w, h));
            }
        }

        // Add internal edges only (both endpoints inside this subgraph)
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        let mut edge_labels: HashMap<usize, layered::normalize::EdgeLabelInfo> = HashMap::new();
        let mut edge_index_map = Vec::new();
        for (edge_idx, edge) in diagram.edges.iter().enumerate() {
            if sg_node_set.contains(edge.from.as_str()) && sg_node_set.contains(edge.to.as_str()) {
                let sub_edge_idx = edge_index_map.len();
                edge_index_map.push(edge_idx);
                sub_graph.add_edge(edge.from.as_str(), edge.to.as_str());
                if let Some((label_width, label_height)) = edge_label_dims(edge) {
                    edge_labels.insert(
                        sub_edge_idx,
                        layered::normalize::EdgeLabelInfo::new(label_width, label_height),
                    );
                }
            }
        }

        // Chain disconnected components with synthetic edges so the layout spreads
        // all nodes along the sub-layout's primary axis.  Without this, nodes
        // with no internal edges collapse into rank 0.
        //
        // Find connected components via union-find, then link the last node of
        // each component (in declaration order) to the first node of the next.
        let leaf_ids: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| !diagram.is_subgraph(n) && diagram.nodes.contains_key(n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if leaf_ids.len() > 1 {
            let id_to_idx: HashMap<&str, usize> = leaf_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect();
            let mut parent: Vec<usize> = (0..leaf_ids.len()).collect();
            let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
                while parent[x] != x {
                    parent[x] = parent[parent[x]];
                    x = parent[x];
                }
                x
            };
            for edge in &diagram.edges {
                if let (Some(&a), Some(&b)) = (
                    id_to_idx.get(edge.from.as_str()),
                    id_to_idx.get(edge.to.as_str()),
                ) {
                    let ra = find(&mut parent, a);
                    let rb = find(&mut parent, b);
                    if ra != rb {
                        parent[ra] = rb;
                    }
                }
            }
            // Walk leaf_ids in declaration order; when the component changes,
            // add a synthetic edge from the previous node to the current one.
            let mut prev_component = find(&mut parent, 0);
            for i in 1..leaf_ids.len() {
                let comp = find(&mut parent, i);
                if comp != prev_component {
                    sub_graph.add_edge(leaf_ids[i - 1], leaf_ids[i]);
                    // Merge so subsequent nodes in the same component don't
                    // create redundant edges.
                    let rc = find(&mut parent, comp);
                    let rp = find(&mut parent, prev_component);
                    parent[rc] = rp;
                }
                prev_component = find(&mut parent, i);
            }
        }

        // Use parent config but override direction
        let sub_config = LayeredConfig {
            direction: layered_direction,
            ..parent_layered_config.clone()
        };

        let result =
            layered::layout_with_labels(&sub_graph, &sub_config, |_, dims| *dims, &edge_labels);

        sublayouts.insert(
            sg_id.clone(),
            SubLayoutResult {
                result,
                edge_index_map,
            },
        );
    }

    sublayouts
}

pub(crate) fn layered_config_for_layout(
    diagram: &Diagram,
    config: &TextLayoutConfig,
) -> LayeredConfig {
    let layered_direction = to_layered_direction(diagram.direction);

    let node_sep = config.node_sep;
    let edge_sep = config.edge_sep;
    let mut rank_sep = config.rank_sep;
    if diagram.has_subgraphs() && config.cluster_rank_sep > 0.0 {
        // Mermaid increases ranksep for cluster graphs (ranksep + 25).
        // We apply the offset when subgraphs are present to approximate that behavior.
        rank_sep += config.cluster_rank_sep;
    }

    LayeredConfig {
        direction: layered_direction,
        node_sep,
        edge_sep,
        rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or_default(),
    }
}

fn build_layered_layout_with_config<FN, FE>(
    diagram: &Diagram,
    layered_config: &LayeredConfig,
    node_dims: FN,
    edge_label_dims: FE,
) -> layered::LayoutResult
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let mut dgraph = layered::DiGraph::new();

    let mut seen = std::collections::HashSet::new();
    let mut ordered_node_ids = Vec::new();
    for edge in &diagram.edges {
        for node_id in [&edge.from, &edge.to] {
            if seen.insert(node_id.clone()) {
                ordered_node_ids.push(node_id.clone());
            }
        }
    }
    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for id in node_keys {
        if seen.insert(id.clone()) {
            ordered_node_ids.push(id.clone());
        }
    }

    for id in &ordered_node_ids {
        if let Some(node) = diagram.nodes.get(id) {
            let dims = node_dims(node);
            dgraph.add_node(id.as_str(), dims);
        }
    }

    // Add subgraph compound nodes in reverse parse order (Mermaid parity).
    // subgraph_order is post-order (inner-first); reversing gives outer-first,
    // matching Mermaid's getData() insertion order.
    // Falls back to sorted keys for manually constructed Diagrams without subgraph_order.
    let subgraph_keys: Vec<&String> = if !diagram.subgraph_order.is_empty() {
        diagram.subgraph_order.iter().rev().collect()
    } else {
        let mut keys: Vec<&String> = diagram.subgraphs.keys().collect();
        keys.sort();
        keys
    };
    for sg_id in &subgraph_keys {
        let sg = &diagram.subgraphs[*sg_id];
        dgraph.add_node(sg_id.as_str(), (0.0, 0.0));
        if !sg.title.trim().is_empty() {
            dgraph.set_has_title(sg_id.as_str());
        }
    }

    // Set parent relationships for compound nodes
    let mut node_parent_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_parent_keys.sort();
    for node_id in node_parent_keys {
        let node = &diagram.nodes[node_id];
        if let Some(ref parent) = node.parent {
            dgraph.set_parent(node_id.as_str(), parent.as_str());
        }
    }

    // Set parent relationships for nested subgraphs
    for sg_id in &subgraph_keys {
        let sg = &diagram.subgraphs[*sg_id];
        if let Some(ref parent_id) = sg.parent {
            dgraph.set_parent(sg_id.as_str(), parent_id.as_str());
        }
    }

    // Edges internal to a true direction-override subgraph are handled by the
    // sub-layout, not the main compound layout. Including them here would
    // force the subgraph to span multiple ranks along the root direction,
    // producing excessive spacing that persists even after reconciliation.
    //
    // A subgraph `dir` that equals its effective parent direction is treated as
    // inherited direction, not an override.
    let dir_override_internal: HashSet<usize> = {
        fn effective_parent_direction(diagram: &Diagram, sg: &crate::graph::Subgraph) -> Direction {
            let mut current = sg.parent.as_deref();
            while let Some(parent_id) = current {
                let Some(parent) = diagram.subgraphs.get(parent_id) else {
                    break;
                };
                if let Some(dir) = parent.dir {
                    return dir;
                }
                current = parent.parent.as_deref();
            }
            diagram.direction
        }

        let mut set = HashSet::new();
        for sg in diagram.subgraphs.values() {
            let Some(sub_dir) = sg.dir else {
                continue;
            };
            if sub_dir == effective_parent_direction(diagram, sg) {
                continue;
            }
            let members: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
            for (idx, edge) in diagram.edges.iter().enumerate() {
                if members.contains(edge.from.as_str()) && members.contains(edge.to.as_str()) {
                    set.insert(idx);
                }
            }
        }
        set
    };

    let mut edge_labels: HashMap<usize, layered::normalize::EdgeLabelInfo> = HashMap::new();
    for (edge_idx, edge) in diagram.edges.iter().enumerate() {
        let weight = if edge.stroke == Stroke::Invisible {
            0.0
        } else {
            1.0
        };
        if dir_override_internal.contains(&edge_idx) {
            // Keep connectivity but don't force rank separation.
            dgraph.add_edge_full(edge.from.as_str(), edge.to.as_str(), weight, 0);
        } else {
            dgraph.add_edge_full(edge.from.as_str(), edge.to.as_str(), weight, edge.minlen);
        }
        if let Some((label_width, label_height)) = edge_label_dims(edge) {
            edge_labels.insert(
                edge_idx,
                layered::normalize::EdgeLabelInfo::new(label_width, label_height),
            );
        }
    }

    let result =
        layered::layout_with_labels(&dgraph, layered_config, |_, dims| *dims, &edge_labels);

    if std::env::var("MMDFLUX_DEBUG_NODE_POS").is_ok_and(|v| v == "1") {
        for (id, rect) in &result.nodes {
            eprintln!(
                "[layered_nodes] {} x={:.2} y={:.2} w={:.2} h={:.2}",
                id.0, rect.x, rect.y, rect.width, rect.height
            );
        }
    }

    result
}

pub(crate) fn build_layered_layout<FN, FE>(
    diagram: &Diagram,
    config: &TextLayoutConfig,
    node_dims: FN,
    edge_label_dims: FE,
) -> layered::LayoutResult
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let layered_config = layered_config_for_layout(diagram, config);
    build_layered_layout_with_config(diagram, &layered_config, node_dims, edge_label_dims)
}
