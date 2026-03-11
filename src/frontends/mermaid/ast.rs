//! Abstract Syntax Tree types for parsed Mermaid flowcharts.

use crate::style::NodeStyle;

/// Shape specification from parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeSpec {
    // === Box-style shapes ===
    /// Rectangle: [text]
    Rectangle(String),
    /// Rounded: (text)
    Round(String),
    /// Stadium: ([text])
    Stadium(String),
    /// Subroutine: [[text]]
    Subroutine(String),
    /// Cylinder: [(text)]
    Cylinder(String),
    /// Document (wavy bottom): @{shape: doc}
    Document(String),
    /// Stacked documents: @{shape: docs}
    Documents(String),
    /// Tagged document (folded corner + wavy bottom): @{shape: tag-doc}
    TaggedDocument(String),
    /// Card (folded corner): @{shape: card}
    Card(String),
    /// Tagged rectangle: @{shape: tag-rect}
    TaggedRect(String),

    // === Angular shapes ===
    /// Diamond: {text}
    Diamond(String),
    /// Hexagon: {{text}}
    Hexagon(String),
    /// Trapezoid: [/text\]
    Trapezoid(String),
    /// Inverse trapezoid: [\text/]
    InvTrapezoid(String),
    /// Parallelogram: @{shape: lean-r} or @{shape: parallelogram}
    Parallelogram(String),
    /// Inverted parallelogram: @{shape: lean-l} or @{shape: inv-parallelogram}
    InvParallelogram(String),
    /// Manual input: @{shape: manual}
    ManualInput(String),
    /// Asymmetric/flag: >text]
    Asymmetric(String),

    // === Circular shapes ===
    /// Circle: ((text))
    Circle(String),
    /// Double circle: (((text)))
    DoubleCircle(String),
    /// Small circle (junction): @{shape: sm-circ}
    SmallCircle(String),
    /// Framed circle (junction): @{shape: fr-circ}
    FramedCircle(String),
    /// Crossed circle (junction): @{shape: cross-circ}
    CrossedCircle(String),

    // === Special shapes ===
    /// Text block with no border: @{shape: text}
    TextBlock(String),
    /// Fork/join bar: @{shape: fork}
    ForkJoin(String),
}

impl ShapeSpec {
    /// Get the text content of the shape.
    pub fn text(&self) -> &str {
        match self {
            ShapeSpec::Rectangle(s)
            | ShapeSpec::Round(s)
            | ShapeSpec::Stadium(s)
            | ShapeSpec::Subroutine(s)
            | ShapeSpec::Cylinder(s)
            | ShapeSpec::Document(s)
            | ShapeSpec::Documents(s)
            | ShapeSpec::TaggedDocument(s)
            | ShapeSpec::Card(s)
            | ShapeSpec::TaggedRect(s)
            | ShapeSpec::Diamond(s)
            | ShapeSpec::Hexagon(s)
            | ShapeSpec::Trapezoid(s)
            | ShapeSpec::InvTrapezoid(s)
            | ShapeSpec::Parallelogram(s)
            | ShapeSpec::InvParallelogram(s)
            | ShapeSpec::ManualInput(s)
            | ShapeSpec::Asymmetric(s)
            | ShapeSpec::Circle(s)
            | ShapeSpec::DoubleCircle(s)
            | ShapeSpec::SmallCircle(s)
            | ShapeSpec::FramedCircle(s)
            | ShapeSpec::CrossedCircle(s)
            | ShapeSpec::TextBlock(s)
            | ShapeSpec::ForkJoin(s) => s,
        }
    }
}

/// A vertex (node definition) in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vertex {
    /// The node identifier.
    pub id: String,
    /// Optional shape with label text.
    pub shape: Option<ShapeSpec>,
}

impl Vertex {
    /// Create a new vertex with just an ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            shape: None,
        }
    }

    /// Create a vertex with a shape.
    pub fn with_shape(id: impl Into<String>, shape: ShapeSpec) -> Self {
        Self {
            id: id.into(),
            shape: Some(shape),
        }
    }
}

/// Stroke style of an edge connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeSpec {
    /// Solid line: --
    Solid,
    /// Dotted line: -.
    Dotted,
    /// Thick line: ==
    Thick,
    /// Invisible edge: ~~~
    Invisible,
}

/// Arrow head type on one end of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowHead {
    /// No arrow head
    None,
    /// Normal arrow: > or <
    Normal,
    /// Cross arrow: x
    Cross,
    /// Circle arrow: o
    Circle,
}

/// Edge connector parsed from Mermaid syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorSpec {
    /// Line stroke style.
    pub stroke: StrokeSpec,
    /// Left (source-side) arrow head.
    pub left: ArrowHead,
    /// Right (target-side) arrow head.
    pub right: ArrowHead,
    /// Edge length (number of repeated characters, minimum 1).
    pub length: usize,
    /// Optional label text.
    pub label: Option<String>,
}

impl ConnectorSpec {
    /// Get the label if present.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Check if this connector has any arrow head (on either side).
    pub fn has_arrow(&self) -> bool {
        self.left != ArrowHead::None || self.right != ArrowHead::None
    }
}

/// An edge statement in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeSpec {
    /// Source vertex.
    pub from: Vertex,
    /// Edge connector type.
    pub connector: ConnectorSpec,
    /// Target vertex.
    pub to: Vertex,
}

/// A subgraph block in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubgraphSpec {
    /// The subgraph identifier.
    pub id: String,
    /// The display title (defaults to id if not specified via bracket syntax).
    pub title: String,
    /// Statements contained within the subgraph.
    pub statements: Vec<Statement>,
    /// Optional direction override for this subgraph.
    pub dir: Option<super::flowchart::Direction>,
}

/// A Mermaid `style NODE ...` statement for flowchart nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeStyleStatement {
    /// The target node identifier.
    pub node_id: String,
    /// Supported node style properties extracted from the declaration list.
    pub style: NodeStyle,
}

/// A statement in the flowchart AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// A standalone vertex definition.
    Vertex(Vertex),
    /// An edge connecting two vertices.
    Edge(EdgeSpec),
    /// A subgraph block.
    Subgraph(SubgraphSpec),
    /// A flowchart node style declaration.
    NodeStyle(NodeStyleStatement),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subgraph_spec_construction() {
        let sg = SubgraphSpec {
            id: "sg1".to_string(),
            title: "My Group".to_string(),
            statements: vec![],
            dir: None,
        };
        assert_eq!(sg.id, "sg1");
        assert_eq!(sg.title, "My Group");
        assert!(sg.statements.is_empty());
    }

    #[test]
    fn test_statement_subgraph_variant() {
        let sg = SubgraphSpec {
            id: "sg1".to_string(),
            title: "Title".to_string(),
            statements: vec![Statement::Vertex(Vertex {
                id: "A".to_string(),
                shape: None,
            })],
            dir: None,
        };
        let stmt = Statement::Subgraph(sg);
        match &stmt {
            Statement::Subgraph(s) => {
                assert_eq!(s.id, "sg1");
                assert_eq!(s.statements.len(), 1);
            }
            _ => panic!("Expected Subgraph variant"),
        }
    }
}
