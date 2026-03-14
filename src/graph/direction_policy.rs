//! Graph-family direction-override policy.
//!
//! These helpers are shared by geometry normalization, routing, and renderers.

use std::collections::HashMap;

use crate::graph::{Direction, Graph};

/// Build per-node effective direction map.
pub fn build_node_directions(diagram: &Graph) -> HashMap<String, Direction> {
    let mut node_directions: HashMap<String, Direction> = HashMap::new();
    for node_id in diagram.nodes.keys() {
        node_directions.insert(node_id.clone(), diagram.direction);
    }

    for sg_id in override_subgraph_ids(diagram) {
        let sg = &diagram.subgraphs[sg_id];
        let override_dir = sg.dir.unwrap();
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id) {
                node_directions.insert(node_id.clone(), override_dir);
            }
        }
    }

    node_directions
}

/// Determine the effective direction for an edge.
pub fn effective_edge_direction(
    node_directions: &HashMap<String, Direction>,
    from: &str,
    to: &str,
    fallback: Direction,
) -> Direction {
    let src_dir = node_directions.get(from).copied().unwrap_or(fallback);
    let tgt_dir = node_directions.get(to).copied().unwrap_or(fallback);
    if src_dir == tgt_dir {
        src_dir
    } else {
        fallback
    }
}

/// Resolve direction policy for a cross-boundary edge.
pub fn cross_boundary_edge_direction(
    diagram: &Graph,
    node_directions: &HashMap<String, Direction>,
    from_sg: Option<&String>,
    to_sg: Option<&String>,
    from_node: &str,
    to_node: &str,
    fallback: Direction,
) -> Direction {
    if let (Some(sg_a), Some(sg_b)) = (from_sg, to_sg) {
        if is_ancestor_sg(diagram, sg_a, sg_b) {
            return diagram
                .subgraphs
                .get(sg_a.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        if is_ancestor_sg(diagram, sg_b, sg_a) {
            return diagram
                .subgraphs
                .get(sg_b.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        return fallback;
    }

    let outside_node = if from_sg.is_some() && to_sg.is_none() {
        to_node
    } else {
        from_node
    };

    node_directions
        .get(outside_node)
        .copied()
        .unwrap_or(fallback)
}

/// Build the override node map: node_id -> subgraph_id.
pub fn build_override_node_map(diagram: &Graph) -> HashMap<String, String> {
    let mut override_nodes = HashMap::new();
    for sg_id in override_subgraph_ids(diagram) {
        let sg = &diagram.subgraphs[sg_id];
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id) {
                override_nodes.insert(node_id.clone(), sg_id.clone());
            }
        }
    }
    override_nodes
}

fn override_subgraph_ids(diagram: &Graph) -> Vec<&String> {
    let mut subgraph_ids: Vec<_> = diagram
        .subgraphs
        .iter()
        .filter(|(_, subgraph)| subgraph.dir.is_some())
        .map(|(id, _)| id)
        .collect();
    subgraph_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });
    subgraph_ids
}

fn is_ancestor_sg(diagram: &Graph, ancestor: &str, descendant: &str) -> bool {
    let mut current = descendant;
    while let Some(parent) = diagram
        .subgraphs
        .get(current)
        .and_then(|sg| sg.parent.as_deref())
    {
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::graph::{Graph, Node};
    use crate::mermaid::parse_flowchart;

    #[test]
    fn build_node_directions_all_root() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A"));
        diagram.add_node(Node::new("B"));
        let dirs = build_node_directions(&diagram);
        assert_eq!(dirs.get("A"), Some(&Direction::TopDown));
        assert_eq!(dirs.get("B"), Some(&Direction::TopDown));
    }

    #[test]
    fn effective_edge_direction_same_override() {
        let mut dirs = HashMap::new();
        dirs.insert("A".into(), Direction::LeftRight);
        dirs.insert("B".into(), Direction::LeftRight);
        assert_eq!(
            effective_edge_direction(&dirs, "A", "B", Direction::TopDown),
            Direction::LeftRight
        );
    }

    #[test]
    fn effective_edge_direction_different_overrides_falls_back() {
        let mut dirs = HashMap::new();
        dirs.insert("A".into(), Direction::LeftRight);
        dirs.insert("B".into(), Direction::BottomTop);
        assert_eq!(
            effective_edge_direction(&dirs, "A", "B", Direction::TopDown),
            Direction::TopDown
        );
    }

    #[test]
    fn build_override_node_map_empty_without_overrides() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.add_node(Node::new("A"));
        let map = build_override_node_map(&diagram);
        assert!(map.is_empty());
    }

    #[test]
    fn cross_boundary_direction_uses_ancestor_override() {
        let input = "graph TD\nsubgraph outer\ndirection LR\nA\nsubgraph inner\ndirection BT\nB\nend\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        let dirs = build_node_directions(&diagram);
        let override_nodes = build_override_node_map(&diagram);

        let direction = cross_boundary_edge_direction(
            &diagram,
            &dirs,
            override_nodes.get("A"),
            override_nodes.get("B"),
            "A",
            "B",
            Direction::TopDown,
        );

        assert_eq!(direction, Direction::LeftRight);
    }
}
