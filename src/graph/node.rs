//! Node types and shape definitions.

use serde::{Deserialize, Serialize};

use super::style::NodeStyle;

/// Shape of a node in the diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    // === Box-style shapes ===
    /// Rectangle shape: `[text]`
    #[default]
    Rectangle,
    /// Rounded rectangle shape: `(text)`
    Round,
    /// Stadium shape: `([text])` (renders as Round)
    Stadium,
    /// Subroutine shape: `[[text]]` (double vertical borders)
    Subroutine,
    /// Cylinder/database shape: [(text)] (curved sides)
    Cylinder,

    /// Document shape (wavy bottom): @{shape: doc}
    Document,
    /// Stacked documents (fallbacks to Document): @{shape: docs}
    Documents,
    /// Tagged document (folded corner + wavy bottom): @{shape: tag-doc}
    TaggedDocument,
    /// Card with folded corner: @{shape: card}
    Card,
    /// Tagged rectangle (fallbacks to Card): @{shape: tag-rect}
    TaggedRect,

    // === Angular shapes ===
    /// Diamond/decision shape: {text}
    Diamond,
    /// Hexagon shape: {{text}} (renders as Diamond)
    Hexagon,
    /// Trapezoid shape: [/text\] (fallbacks to Rectangle)
    Trapezoid,
    /// Inverse trapezoid shape: [\text/] (fallbacks to Rectangle)
    InvTrapezoid,
    /// Parallelogram (lean right): @{shape: sl-rect} (fallbacks to Rectangle)
    Parallelogram,
    /// Inverted parallelogram (lean left): @{shape: inv-parallelogram} (fallbacks to Rectangle)
    InvParallelogram,
    /// Manual input (sloped top): @{shape: manual} (fallbacks to Rectangle)
    ManualInput,
    /// Asymmetric/flag shape: >text] (fallbacks to Rectangle)
    Asymmetric,

    // === Circular shapes ===
    /// Circle shape: ((text)) (renders as Round)
    Circle,
    /// Double circle shape: (((text))) (renders as Round)
    DoubleCircle,
    /// Small circle (junction point): @{shape: sm-circ} (glyph when unlabeled)
    SmallCircle,
    /// Framed circle (junction point): @{shape: fr-circ} (glyph when unlabeled)
    FramedCircle,
    /// Crossed circle (inhibit): @{shape: cross-circ} (glyph when unlabeled)
    CrossedCircle,

    // === Special shapes ===
    /// Text block with no border: @{shape: text}
    TextBlock,
    /// Fork/join bar: @{shape: fork}
    ForkJoin,
    /// Note rectangle (for state diagram notes)
    NoteRect,
}

/// A node in the flowchart diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Node {
    /// Unique identifier for this node.
    pub id: String,
    /// Display label (defaults to id if not specified).
    pub label: String,
    /// Shape of the node.
    pub shape: Shape,
    /// Parent subgraph ID, if this node belongs to a subgraph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Optional style hints carried with the node.
    #[serde(skip_serializing_if = "NodeStyle::is_empty", default)]
    pub style: NodeStyle,
    /// Original Mermaid class names applied to this node, preserved so the
    /// SVG renderer can emit them on the `<g class="node default ...">`
    /// wrapper. Order is preserved in application order; duplicates are not
    /// stored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_names: Vec<String>,
}

impl Node {
    /// Separator marker for multi-line labels (e.g., between class name and members).
    /// Rendered as a horizontal rule inside box shapes.
    pub const SEPARATOR: &'static str = "---";

    /// Create a new node with just an ID (label defaults to ID, shape to Rectangle).
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            label: id.clone(),
            id,
            shape: Shape::default(),
            parent: None,
            style: NodeStyle::default(),
            class_names: Vec::new(),
        }
    }

    /// Set the label for this node.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Set the shape for this node.
    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_parent_default_none() {
        let node = Node::new("A");
        assert_eq!(node.parent, None);
    }

    #[test]
    fn test_node_parent_set() {
        let mut node = Node::new("A");
        node.parent = Some("sg1".to_string());
        assert_eq!(node.parent, Some("sg1".to_string()));
    }

    #[test]
    fn node_default_has_empty_class_names() {
        let node = Node::new("A");
        assert!(node.class_names.is_empty());
    }

    #[test]
    fn node_class_names_serialize_when_present() {
        let mut node = Node::new("A");
        node.class_names = vec!["highlight".to_string(), "thickBorder".to_string()];
        let json = serde_json::to_string(&node).unwrap();
        assert!(
            json.contains(r#""class_names":["highlight","thickBorder"]"#),
            "{json}"
        );
    }

    #[test]
    fn node_class_names_omitted_when_empty() {
        let node = Node::new("A");
        let json = serde_json::to_string(&node).unwrap();
        assert!(!json.contains("class_names"), "{json}");
    }
}
