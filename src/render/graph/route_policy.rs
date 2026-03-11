//! Shared routing policy functions for flowchart text and SVG paths.
//!
//! These functions were duplicated between the text layout pipeline
//! and SVG router. Extracted here as a single source of truth.

use std::collections::HashMap;

use crate::graph::{Diagram, Direction};

/// Build per-node effective direction map.
///
/// Nodes inside a direction-override subgraph get the subgraph's direction;
/// all other nodes get the diagram's root direction.
///
/// Processes subgraphs in depth order (shallowest first) so the deepest
/// override deterministically wins for nested subgraphs.
pub fn build_node_directions(diagram: &Diagram) -> HashMap<String, Direction> {
    let mut node_directions: HashMap<String, Direction> = HashMap::new();
    for node_id in diagram.nodes.keys() {
        node_directions.insert(node_id.clone(), diagram.direction);
    }

    let mut dir_sg_ids: Vec<&String> = diagram
        .subgraphs
        .iter()
        .filter(|(_, sg)| sg.dir.is_some())
        .map(|(id, _)| id)
        .collect();
    dir_sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });
    for sg_id in dir_sg_ids {
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
///
/// If both endpoints share the same direction override, returns that direction.
/// Otherwise returns the fallback (diagram root direction).
///
/// Shared policy entry point for text/SVG routing and shared attachment planning.
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

/// Resolve the direction policy for a cross-boundary edge.
///
/// Rules:
/// - if endpoints are in different override subgraphs and one is ancestor of the
///   other, use the ancestor override direction;
/// - if endpoints are in different non-ancestor branches, use `fallback`;
/// - if only one endpoint is in an override subgraph, use the outside node's
///   effective direction.
pub fn cross_boundary_edge_direction(
    diagram: &Diagram,
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

/// Build the override node map: node_id → subgraph_id for direction-override subgraphs.
///
/// Processes subgraphs in depth order so the deepest override wins.
pub fn build_override_node_map(diagram: &Diagram) -> HashMap<String, String> {
    let mut override_nodes = HashMap::new();
    let mut sg_ids: Vec<&String> = diagram
        .subgraphs
        .iter()
        .filter(|(_, sg)| sg.dir.is_some())
        .map(|(id, _)| id)
        .collect();
    sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });
    for sg_id in sg_ids {
        let sg = &diagram.subgraphs[sg_id];
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id) {
                override_nodes.insert(node_id.clone(), sg_id.clone());
            }
        }
    }
    override_nodes
}

fn is_ancestor_sg(diagram: &Diagram, ancestor: &str, descendant: &str) -> bool {
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
    use crate::frontends::mermaid::parse_flowchart;
    use crate::graph::{Diagram, Node};

    #[test]
    fn build_node_directions_all_root() {
        let mut diagram = Diagram::new(Direction::TopDown);
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
        let mut diagram = Diagram::new(Direction::TopDown);
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
            diagram.direction,
        );

        assert_eq!(direction, Direction::LeftRight);
    }

    #[test]
    fn cross_boundary_direction_uses_outside_node_direction() {
        let input = "graph TD\nsubgraph sg1\ndirection LR\nA --> B\nend\nB --> C\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        let dirs = build_node_directions(&diagram);
        let override_nodes = build_override_node_map(&diagram);

        let direction = cross_boundary_edge_direction(
            &diagram,
            &dirs,
            override_nodes.get("B"),
            override_nodes.get("C"),
            "B",
            "C",
            diagram.direction,
        );

        assert_eq!(direction, Direction::TopDown);
    }
}
