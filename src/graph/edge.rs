//! Edge types including stroke styles and arrow heads.

use serde::Serialize;

/// Style of the edge line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stroke {
    /// Normal solid line: --
    #[default]
    Solid,
    /// Dotted line: -.
    Dotted,
    /// Thick/bold line: ==
    Thick,
    /// Invisible edge (layout-only, not rendered): ~~~
    Invisible,
}

/// Type of arrow head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrow {
    /// Arrow head pointing to target: >
    #[default]
    Normal,
    /// No arrow head (open line): -
    None,
    /// Cross arrow: x
    Cross,
    /// Circle arrow: o
    Circle,
    /// Open (hollow) triangle arrow: △ (inheritance)
    OpenTriangle,
    /// Filled diamond: ◆ (composition)
    Diamond,
    /// Open (hollow) diamond: ◇ (aggregation)
    OpenDiamond,
}

/// An edge connecting two nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edge {
    /// Source node ID.
    pub from: String,
    /// Target node ID.
    pub to: String,
    /// Original subgraph ID for the source endpoint, if the edge targeted a subgraph.
    pub from_subgraph: Option<String>,
    /// Original subgraph ID for the target endpoint, if the edge targeted a subgraph.
    pub to_subgraph: Option<String>,
    /// Optional label on the edge.
    pub label: Option<String>,
    /// Line style.
    pub stroke: Stroke,
    /// Arrow head at the start (source-side) of the edge.
    pub arrow_start: Arrow,
    /// Arrow head at the end (target-side) of the edge.
    pub arrow_end: Arrow,
    /// Label near the target endpoint (head).
    pub head_label: Option<String>,
    /// Label near the source endpoint (tail).
    pub tail_label: Option<String>,
    /// Minimum rank separation between source and target. Default 1.
    pub minlen: i32,
    /// Index of this edge in the diagram's edge list.
    /// Assigned automatically by `Diagram::add_edge()`.
    pub index: usize,
}

impl Edge {
    /// Create a new edge with default style (solid line with arrow).
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            from_subgraph: None,
            to_subgraph: None,
            label: None,
            stroke: Stroke::default(),
            arrow_start: Arrow::None,
            arrow_end: Arrow::default(),
            head_label: None,
            tail_label: None,
            minlen: 1,
            index: 0,
        }
    }

    /// Set the label for this edge.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the stroke style.
    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = stroke;
        self
    }

    /// Set the arrow type for the end (target-side) of the edge.
    pub fn with_arrow(mut self, arrow: Arrow) -> Self {
        self.arrow_end = arrow;
        self
    }

    /// Set arrow types for both start and end.
    pub fn with_arrows(mut self, start: Arrow, end: Arrow) -> Self {
        self.arrow_start = start;
        self.arrow_end = end;
        self
    }

    /// Set minimum rank separation (default 1). Use 0 for same-rank placement.
    pub fn with_minlen(mut self, minlen: i32) -> Self {
        self.minlen = minlen;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invisible_stroke_variant() {
        let edge = Edge::new("A", "B").with_stroke(Stroke::Invisible);
        assert_eq!(edge.stroke, Stroke::Invisible);
    }

    #[test]
    fn test_arrow_cross_and_circle_variants() {
        let edge = Edge::new("A", "B").with_arrows(Arrow::Cross, Arrow::Circle);
        assert_eq!(edge.arrow_start, Arrow::Cross);
        assert_eq!(edge.arrow_end, Arrow::Circle);
    }

    #[test]
    fn edge_head_tail_labels_default_none() {
        let edge = Edge::new("A", "B");
        assert_eq!(edge.head_label, None);
        assert_eq!(edge.tail_label, None);
    }

    #[test]
    fn edge_with_head_label() {
        let mut edge = Edge::new("A", "B");
        edge.head_label = Some("1..*".to_string());
        assert_eq!(edge.head_label.as_deref(), Some("1..*"));
    }

    #[test]
    fn test_minlen_default_and_builder() {
        let edge = Edge::new("A", "B");
        assert_eq!(edge.minlen, 1);

        let edge = Edge::new("A", "B").with_minlen(0);
        assert_eq!(edge.minlen, 0);
    }
}
