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
}
