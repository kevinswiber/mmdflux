//! Compiler from ER diagram AST to canonical `graph::Graph`.
//!
//! Maps entities to graph nodes and relationships to graph edges,
//! preserving ER diagram semantics through cardinality arrows and
//! identifying/non-identifying stroke styles.

use crate::graph::{Arrow, Direction, Edge, Graph, Node, Shape, Stroke};
use crate::mermaid::er::{Cardinality, ErModel};

/// Compile an `ErModel` into a canonical `graph::Graph`.
///
/// ER diagrams use top-down layout by default. Each entity becomes a
/// rectangle node whose label uses the 2-compartment separator pattern
/// (name + attribute rows). Relationships map to edges with cardinality
/// arrows and stroke style.
pub fn compile(model: &ErModel) -> Graph {
    let mut diagram = Graph::new(Direction::TopDown);

    for entity in &model.entities {
        let label = if entity.attributes.is_empty() {
            entity.name.clone()
        } else {
            let mut parts = vec![entity.name.clone()];
            parts.push(Node::SEPARATOR.to_string());
            for attr in &entity.attributes {
                parts.push(format_attribute(attr));
            }
            parts.join("\n")
        };

        diagram.add_node(
            Node::new(&entity.name)
                .with_label(label)
                .with_shape(Shape::Rectangle),
        );
    }

    for rel in &model.relationships {
        let stroke = if rel.identifying {
            Stroke::Solid
        } else {
            Stroke::Dotted
        };

        let arrow_start = cardinality_to_arrow(rel.cardinality_a);
        let arrow_end = cardinality_to_arrow(rel.cardinality_b);

        let mut edge = Edge::new(&rel.entity_a, &rel.entity_b)
            .with_stroke(stroke)
            .with_arrows(arrow_start, arrow_end);

        if let Some(ref role) = rel.role {
            edge = edge.with_label(role.clone());
        }

        // Text-mode cardinality labels.
        edge.tail_label = Some(cardinality_text(rel.cardinality_a));
        edge.head_label = Some(cardinality_text(rel.cardinality_b));

        diagram.add_edge(edge);
    }

    diagram
}

fn cardinality_to_arrow(card: Cardinality) -> Arrow {
    match card {
        Cardinality::OnlyOne => Arrow::OnlyOne,
        Cardinality::ZeroOrOne => Arrow::ZeroOrOne,
        Cardinality::OneOrMore => Arrow::OneOrMore,
        Cardinality::ZeroOrMore => Arrow::ZeroOrMore,
    }
}

fn cardinality_text(card: Cardinality) -> String {
    match card {
        Cardinality::OnlyOne => "||".to_string(),
        Cardinality::ZeroOrOne => "o|".to_string(),
        Cardinality::OneOrMore => "}|".to_string(),
        Cardinality::ZeroOrMore => "}o".to_string(),
    }
}

fn format_attribute(attr: &crate::mermaid::er::ErAttribute) -> String {
    let mut parts = vec![attr.attr_type.clone(), attr.name.clone()];
    if !attr.keys.is_empty() {
        parts.push(attr.keys.join(","));
    }
    if let Some(ref comment) = attr.comment {
        parts.push(format!("\"{comment}\""));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::er::parse_er_diagram;

    #[test]
    fn compiler_basic_entities_create_nodes() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : rel").unwrap();
        let graph = compile(&model);
        assert!(graph.nodes.contains_key("A"));
        assert!(graph.nodes.contains_key("B"));
        assert_eq!(graph.nodes["A"].shape, Shape::Rectangle);
    }

    #[test]
    fn compiler_entity_with_attributes_has_separator() {
        let input = "\
erDiagram
    CUSTOMER {
        string name PK
        int age
    }";
        let model = parse_er_diagram(input).unwrap();
        let graph = compile(&model);
        let label = &graph.nodes["CUSTOMER"].label;
        assert!(label.contains(Node::SEPARATOR));
        assert!(label.contains("string name PK"));
        assert!(label.contains("int age"));
    }

    #[test]
    fn compiler_entity_without_attributes_is_plain() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.nodes["A"].label, "A");
        assert!(!graph.nodes["A"].label.contains(Node::SEPARATOR));
    }

    #[test]
    fn compiler_relationship_creates_edge() {
        let model = parse_er_diagram("erDiagram\n    A ||--o{ B : places").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "A");
        assert_eq!(graph.edges[0].to, "B");
    }

    #[test]
    fn compiler_identifying_relationship_solid() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges[0].stroke, Stroke::Solid);
    }

    #[test]
    fn compiler_non_identifying_relationship_dotted() {
        let model = parse_er_diagram("erDiagram\n    A ||..|| B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges[0].stroke, Stroke::Dotted);
    }

    #[test]
    fn compiler_cardinality_mapped_to_arrows() {
        let model = parse_er_diagram("erDiagram\n    A ||--o{ B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges[0].arrow_start, Arrow::OnlyOne);
        assert_eq!(graph.edges[0].arrow_end, Arrow::ZeroOrMore);
    }

    #[test]
    fn compiler_role_label_on_edge() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : places").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges[0].label.as_deref(), Some("places"));
    }

    #[test]
    fn compiler_head_tail_labels_for_cardinality() {
        let model = parse_er_diagram("erDiagram\n    A }o--o| B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.edges[0].tail_label.as_deref(), Some("}o"));
        assert_eq!(graph.edges[0].head_label.as_deref(), Some("o|"));
    }

    #[test]
    fn compiler_default_direction_is_top_down() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : rel").unwrap();
        let graph = compile(&model);
        assert_eq!(graph.direction, Direction::TopDown);
    }
}
