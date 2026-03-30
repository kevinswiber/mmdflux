//! Diagram container holding nodes, edges, and layout direction.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::edge::{Edge, Stroke};
use super::node::Node;

/// Direction of the diagram layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Top to bottom (vertical, downward).
    #[default]
    TopDown,
    /// Bottom to top (vertical, upward).
    BottomTop,
    /// Left to right (horizontal, rightward).
    LeftRight,
    /// Right to left (horizontal, leftward).
    RightLeft,
}

/// A subgraph grouping of nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Subgraph {
    /// Unique identifier for this subgraph.
    pub id: String,
    /// Display title (defaults to id if not specified via bracket syntax).
    pub title: String,
    /// IDs of nodes belonging to this subgraph.
    pub nodes: Vec<String>,
    /// Parent subgraph ID (None if top-level).
    pub parent: Option<String>,
    /// Direction override for this subgraph (None = inherit from parent).
    pub dir: Option<Direction>,
}

/// A complete flowchart diagram.
#[derive(Debug, Clone)]
pub struct Graph {
    /// Layout direction.
    pub direction: Direction,
    /// Nodes indexed by their ID.
    pub nodes: HashMap<String, Node>,
    /// Edges connecting nodes.
    pub edges: Vec<Edge>,
    /// Subgraphs indexed by their ID.
    pub subgraphs: HashMap<String, Subgraph>,
    /// Subgraph IDs in parse order (inner-first / post-order).
    pub subgraph_order: Vec<String>,
}

impl Graph {
    /// Create a new empty diagram.
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            nodes: HashMap::new(),
            edges: Vec::new(),
            subgraphs: HashMap::new(),
            subgraph_order: Vec::new(),
        }
    }

    /// Add a node to the diagram.
    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an edge to the diagram, auto-assigning its index.
    pub fn add_edge(&mut self, mut edge: Edge) {
        edge.index = self.edges.len();
        self.edges.push(edge);
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = &String> {
        self.nodes.keys()
    }

    /// Check if the diagram contains any subgraphs.
    pub fn has_subgraphs(&self) -> bool {
        !self.subgraphs.is_empty()
    }

    /// Check if an ID corresponds to a subgraph (compound node).
    pub fn is_subgraph(&self, id: &str) -> bool {
        self.subgraphs.contains_key(id)
    }

    /// Find the first non-compound (leaf) child node within a subgraph.
    ///
    /// Returns `None` for empty subgraphs or nonexistent IDs.
    /// This is the Rust equivalent of Mermaid's `findNonClusterChild()`.
    pub fn find_non_cluster_child(&self, subgraph_id: &str) -> Option<String> {
        let sg = self.subgraphs.get(subgraph_id)?;
        sg.nodes.iter().find(|id| !self.is_subgraph(id)).cloned()
    }

    /// Find a sink node in a subgraph — a non-cluster child with no successors
    /// within the subgraph.  Used when the subgraph is the **source** of an edge
    /// so the target ends up ranked after the entire subgraph.
    /// Falls back to `find_non_cluster_child` if every node has a successor.
    pub fn find_subgraph_sink(&self, subgraph_id: &str) -> Option<String> {
        let sg = self.subgraphs.get(subgraph_id)?;
        let sg_node_set: std::collections::HashSet<&str> =
            sg.nodes.iter().map(|s| s.as_str()).collect();
        let non_cluster: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|id| !self.is_subgraph(id))
            .map(|s| s.as_str())
            .collect();

        let sink = non_cluster.iter().find(|&&node| {
            !self
                .edges
                .iter()
                .any(|e| e.from == node && sg_node_set.contains(e.to.as_str()) && e.to != node)
        });

        sink.map(|s| s.to_string())
            .or_else(|| self.find_non_cluster_child(subgraph_id))
    }

    /// Return the IDs of subgraphs whose parent is `parent_id`.
    pub fn subgraph_children(&self, parent_id: &str) -> Vec<&String> {
        self.subgraphs
            .values()
            .filter(|sg| sg.parent.as_deref() == Some(parent_id))
            .map(|sg| &sg.id)
            .collect()
    }

    /// Add a same-rank constraint between two nodes.
    /// Creates an invisible edge with minlen=0.
    pub fn add_same_rank_constraint(&mut self, a: &str, b: &str) {
        self.add_edge(
            Edge::new(a, b)
                .with_stroke(Stroke::Invisible)
                .with_minlen(0),
        );
    }

    /// Returns true if any edge crosses the subgraph boundary
    /// (one endpoint inside, one outside).
    pub fn subgraph_has_cross_boundary_edges(&self, sg_id: &str) -> bool {
        let Some(sg) = self.subgraphs.get(sg_id) else {
            return false;
        };
        let sg_nodes: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        self.edges.iter().any(|edge| {
            let from_in = sg_nodes.contains(edge.from.as_str());
            let to_in = sg_nodes.contains(edge.to.as_str());
            from_in != to_in
        })
    }

    /// Return the nesting depth of a subgraph (0 = top-level).
    pub fn subgraph_depth(&self, sg_id: &str) -> usize {
        let mut depth = 0;
        let mut current = sg_id;
        while let Some(parent) = self
            .subgraphs
            .get(current)
            .and_then(|sg| sg.parent.as_deref())
        {
            depth += 1;
            current = parent;
        }
        depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Shape;

    #[test]
    fn test_subgraph_construction() {
        let sg = Subgraph {
            id: "sg1".to_string(),
            title: "My Group".to_string(),
            nodes: vec!["A".to_string(), "B".to_string()],
            parent: None,
            dir: None,
        };
        assert_eq!(sg.id, "sg1");
        assert_eq!(sg.title, "My Group");
        assert_eq!(sg.nodes.len(), 2);
    }

    #[test]
    fn test_subgraph_has_parent_field() {
        let sg = Subgraph {
            id: "inner".to_string(),
            title: "Inner".to_string(),
            nodes: vec!["A".to_string()],
            parent: Some("outer".to_string()),
            dir: None,
        };
        assert_eq!(sg.parent, Some("outer".to_string()));
    }

    #[test]
    fn cross_boundary_edges_nonexistent_subgraph() {
        let diagram = Graph::new(Direction::TopDown);
        assert!(!diagram.subgraph_has_cross_boundary_edges("nope"));
    }

    #[test]
    fn test_diagram_subgraphs_empty() {
        let diagram = Graph::new(Direction::TopDown);
        assert!(diagram.subgraphs.is_empty());
        assert!(!diagram.has_subgraphs());
    }

    #[test]
    fn test_diagram_has_subgraphs() {
        let mut diagram = Graph::new(Direction::TopDown);
        diagram.subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Group".to_string(),
                nodes: vec![],
                parent: None,
                dir: None,
            },
        );
        assert!(diagram.has_subgraphs());
    }

    fn graph_with_subgraph() -> Graph {
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("A").with_shape(Shape::Round));
        g.add_node(Node::new("B").with_shape(Shape::Round));
        g.add_edge(Edge::new("A", "B"));
        g.subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Group".to_string(),
                nodes: vec!["A".to_string(), "B".to_string()],
                parent: None,
                dir: None,
            },
        );
        g
    }

    #[test]
    fn find_non_cluster_child_returns_leaf_node() {
        let g = graph_with_subgraph();
        let child = g.find_non_cluster_child("sg1");
        assert!(child.is_some());
        assert!(child.as_deref() == Some("A") || child.as_deref() == Some("B"));
    }

    #[test]
    fn find_non_cluster_child_skips_nested_subgraphs() {
        let mut g = graph_with_subgraph();
        // Add a nested subgraph as a child of sg1.
        g.subgraphs.insert(
            "inner".to_string(),
            Subgraph {
                id: "inner".to_string(),
                title: "Inner".to_string(),
                nodes: vec!["A".to_string()],
                parent: Some("sg1".to_string()),
                dir: None,
            },
        );
        // "inner" is a subgraph, so it should be skipped; "A" or "B" returned.
        g.subgraphs.get_mut("sg1").unwrap().nodes = vec!["inner".to_string(), "B".to_string()];
        let child = g.find_non_cluster_child("sg1");
        assert_eq!(child.as_deref(), Some("B"));
    }

    #[test]
    fn find_non_cluster_child_empty_subgraph() {
        let mut g = Graph::new(Direction::TopDown);
        g.subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Empty".to_string(),
                nodes: vec![],
                parent: None,
                dir: None,
            },
        );
        assert!(g.find_non_cluster_child("sg1").is_none());
    }

    #[test]
    fn find_non_cluster_child_nonexistent_subgraph() {
        let g = Graph::new(Direction::TopDown);
        assert!(g.find_non_cluster_child("nope").is_none());
    }

    #[test]
    fn find_subgraph_sink_prefers_node_without_successors() {
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("A").with_shape(Shape::Round));
        g.add_node(Node::new("B").with_shape(Shape::Round));
        g.add_node(Node::new("C").with_shape(Shape::Round));
        g.add_edge(Edge::new("A", "B"));
        // A has successor B inside sg1; B has successor C inside sg1; C has no successors.
        g.add_edge(Edge::new("B", "C"));
        g.subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Group".to_string(),
                nodes: vec!["A".to_string(), "B".to_string(), "C".to_string()],
                parent: None,
                dir: None,
            },
        );
        let sink = g.find_subgraph_sink("sg1");
        assert_eq!(sink.as_deref(), Some("C"));
    }

    #[test]
    fn find_subgraph_sink_falls_back_to_first_child() {
        // All nodes have successors inside — falls back to find_non_cluster_child.
        let mut g = Graph::new(Direction::TopDown);
        g.add_node(Node::new("A").with_shape(Shape::Round));
        g.add_node(Node::new("B").with_shape(Shape::Round));
        g.add_edge(Edge::new("A", "B"));
        g.add_edge(Edge::new("B", "A"));
        g.subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Cycle".to_string(),
                nodes: vec!["A".to_string(), "B".to_string()],
                parent: None,
                dir: None,
            },
        );
        let sink = g.find_subgraph_sink("sg1");
        assert!(sink.is_some()); // falls back to first non-cluster child
    }

    #[test]
    fn find_subgraph_sink_nonexistent() {
        let g = Graph::new(Direction::TopDown);
        assert!(g.find_subgraph_sink("nope").is_none());
    }
}
