//! Compiler from class diagram AST to canonical `graph::Diagram`.
//!
//! Maps classes to graph nodes and relationships to graph edges,
//! preserving class diagram semantics through edge styles and arrows.

use std::collections::HashSet;

use crate::graph::{Arrow, Direction, Edge, Graph, Node, Shape, Stroke, Subgraph};
use crate::mermaid::class::ast::{ClassModel, ClassRelationType};

/// Compile a `ClassModel` into a canonical `graph::Diagram`.
///
/// Class diagrams use top-down layout by default. Each class becomes a
/// rectangle node whose label includes member lines (if any). Relationships
/// map to edges with style/arrow metadata based on their type.
pub fn compile(model: &ClassModel) -> Graph {
    let mut diagram = Graph::new(class_direction(model.direction.as_deref()));
    let marker_only_lollipop_interfaces = lollipop_interface_nodes(model);

    for class in &model.classes {
        if marker_only_lollipop_interfaces.contains(&class.name) {
            continue;
        }

        let mut header: Vec<String> = class
            .annotations
            .iter()
            .map(|a| format!("<<{a}>>"))
            .collect();
        header.push(class.name.clone());

        let label = if class.members.is_empty() {
            if class.annotations.is_empty() {
                class.name.clone()
            } else {
                header.join("\n")
            }
        } else {
            // 3-compartment UML: name / attributes / operations.
            // Mermaid heuristic: contains ')' → method, otherwise attribute.
            let (methods, attrs): (Vec<_>, Vec<_>) =
                class.members.iter().partition(|m| m.contains(')'));
            let mut parts = header;
            parts.push(Node::SEPARATOR.to_string());
            parts.extend(attrs.into_iter().cloned());
            parts.push(Node::SEPARATOR.to_string());
            parts.extend(methods.into_iter().cloned());
            parts.join("\n")
        };

        diagram.add_node(
            Node::new(&class.name)
                .with_label(label)
                .with_shape(Shape::Rectangle),
        );
    }

    apply_namespaces(model, &mut diagram);

    for (relation_index, rel) in model.relations.iter().enumerate() {
        let (stroke, marker_arrow) = relation_style(rel.relation_type);
        let arrow_start = if rel.marker_start {
            marker_arrow
        } else {
            Arrow::None
        };
        let arrow_end = if rel.marker_end {
            marker_arrow
        } else {
            Arrow::None
        };
        let from_node_id = if rel.relation_type == ClassRelationType::Lollipop
            && rel.marker_start
            && marker_only_lollipop_interfaces.contains(&rel.from)
        {
            let id = format!("__mmdflux_lollipop_{relation_index}_start");
            diagram.add_node(
                Node::new(&id)
                    .with_label(rel.from.clone())
                    .with_shape(Shape::TextBlock),
            );
            id
        } else {
            rel.from.clone()
        };
        let to_node_id = if rel.relation_type == ClassRelationType::Lollipop
            && rel.marker_end
            && marker_only_lollipop_interfaces.contains(&rel.to)
        {
            let id = format!("__mmdflux_lollipop_{relation_index}_end");
            diagram.add_node(
                Node::new(&id)
                    .with_label(rel.to.clone())
                    .with_shape(Shape::TextBlock),
            );
            id
        } else {
            rel.to.clone()
        };

        let mut edge = Edge::new(&from_node_id, &to_node_id)
            .with_stroke(stroke)
            .with_arrows(arrow_start, arrow_end);

        if let Some(label) = &rel.label {
            edge = edge.with_label(label);
        }

        diagram.add_edge(edge);
    }

    diagram
}

fn class_direction(direction: Option<&str>) -> Direction {
    match direction {
        Some("LR") => Direction::LeftRight,
        Some("RL") => Direction::RightLeft,
        Some("BT") => Direction::BottomTop,
        Some("TB") => Direction::TopDown,
        _ => Direction::TopDown,
    }
}

fn lollipop_interface_nodes(model: &ClassModel) -> HashSet<String> {
    let mut candidates = HashSet::new();
    let mut keep_box = HashSet::new();

    for rel in &model.relations {
        if rel.relation_type == ClassRelationType::Lollipop {
            if rel.marker_start {
                candidates.insert(rel.from.clone());
            } else {
                keep_box.insert(rel.from.clone());
            }

            if rel.marker_end {
                candidates.insert(rel.to.clone());
            } else {
                keep_box.insert(rel.to.clone());
            }
        } else {
            keep_box.insert(rel.from.clone());
            keep_box.insert(rel.to.clone());
        }
    }

    for class in &model.classes {
        if !class.annotations.is_empty() || !class.members.is_empty() {
            keep_box.insert(class.name.clone());
        }
    }

    candidates.retain(|name| !keep_box.contains(name));
    candidates
}

fn apply_namespaces(model: &ClassModel, diagram: &mut Graph) {
    for namespace in &model.namespaces {
        diagram.subgraphs.insert(
            namespace.id.clone(),
            Subgraph {
                id: namespace.id.clone(),
                title: namespace.name.clone(),
                nodes: Vec::new(),
                parent: namespace.parent.clone(),
                dir: None,
            },
        );
        diagram.subgraph_order.push(namespace.id.clone());
    }

    for class in &model.classes {
        let Some(namespace_id) = class.namespace.as_deref() else {
            continue;
        };

        if let Some(node) = diagram.nodes.get_mut(&class.name) {
            node.parent = Some(namespace_id.to_string());
        }

        let mut current_namespace = Some(namespace_id.to_string());
        while let Some(ns_id) = current_namespace {
            let Some(namespace) = diagram.subgraphs.get_mut(&ns_id) else {
                break;
            };
            if !namespace.nodes.contains(&class.name) {
                namespace.nodes.push(class.name.clone());
            }
            current_namespace = namespace.parent.clone();
        }
    }
}

/// Map a class relationship type to edge style.
fn relation_style(rel: ClassRelationType) -> (Stroke, Arrow) {
    match rel {
        ClassRelationType::Association => (Stroke::Solid, Arrow::None),
        ClassRelationType::DirectedAssociation => (Stroke::Solid, Arrow::Normal),
        ClassRelationType::Inheritance => (Stroke::Solid, Arrow::OpenTriangle),
        ClassRelationType::Realization => (Stroke::Dotted, Arrow::OpenTriangle),
        ClassRelationType::Composition => (Stroke::Solid, Arrow::Diamond),
        ClassRelationType::Aggregation => (Stroke::Solid, Arrow::OpenDiamond),
        ClassRelationType::Dependency => (Stroke::Dotted, Arrow::None),
        ClassRelationType::DirectedDependency => (Stroke::Dotted, Arrow::Normal),
        ClassRelationType::Lollipop => (Stroke::Solid, Arrow::Circle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::class::parse_class_diagram;

    fn compile_class(input: &str) -> Graph {
        let model = parse_class_diagram(input).unwrap();
        compile(&model)
    }

    #[test]
    fn compiler_emits_nodes() {
        let diagram = compile_class("classDiagram\nclass A\nclass B");
        assert!(diagram.nodes.contains_key("A"));
        assert!(diagram.nodes.contains_key("B"));
        assert_eq!(diagram.nodes.len(), 2);
    }

    #[test]
    fn compiler_emits_edges() {
        let diagram = compile_class("classDiagram\nA --> B");
        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].from, "A");
        assert_eq!(diagram.edges[0].to, "B");
    }

    #[test]
    fn compiler_nodes_and_edges() {
        let diagram = compile_class("classDiagram\nclass A\nclass B\nA --> B");
        assert!(diagram.nodes.contains_key("A"));
        assert!(diagram.nodes.contains_key("B"));
        assert_eq!(diagram.edges.len(), 1);
    }

    #[test]
    fn compiler_default_direction_is_top_down() {
        let diagram = compile_class("classDiagram\nclass A");
        assert_eq!(diagram.direction, Direction::TopDown);
    }

    #[test]
    fn compiler_uses_lr_direction_from_model() {
        let diagram = compile_class("classDiagram\ndirection LR\nA --> B");
        assert_eq!(diagram.direction, Direction::LeftRight);
    }

    #[test]
    fn compiler_uses_rl_direction_from_model() {
        let diagram = compile_class("classDiagram\ndirection RL\nA --> B");
        assert_eq!(diagram.direction, Direction::RightLeft);
    }

    #[test]
    fn compiler_uses_bt_direction_from_model() {
        let diagram = compile_class("classDiagram\ndirection BT\nA --> B");
        assert_eq!(diagram.direction, Direction::BottomTop);
    }

    #[test]
    fn compiler_uses_tb_direction_from_model() {
        let diagram = compile_class("classDiagram\ndirection TB\nA --> B");
        assert_eq!(diagram.direction, Direction::TopDown);
    }

    #[test]
    fn compiler_falls_back_to_top_down_for_invalid_or_missing_direction() {
        let invalid = compile_class("classDiagram\ndirection DIAGONAL\nA --> B");
        assert_eq!(invalid.direction, Direction::TopDown);

        let missing = compile_class("classDiagram\nA --> B");
        assert_eq!(missing.direction, Direction::TopDown);
    }

    #[test]
    fn compiler_node_shape_is_rectangle() {
        let diagram = compile_class("classDiagram\nclass User");
        assert_eq!(diagram.nodes["User"].shape, Shape::Rectangle);
    }

    #[test]
    fn compiler_inheritance_edge_style() {
        let diagram = compile_class("classDiagram\nAnimal <|-- Dog");
        assert_eq!(diagram.edges[0].stroke, Stroke::Solid);
    }

    #[test]
    fn compiler_realization_edge_is_dotted() {
        let diagram = compile_class("classDiagram\nLogger <|.. ConsoleLogger");
        assert_eq!(diagram.edges[0].stroke, Stroke::Dotted);
        assert_eq!(diagram.edges[0].arrow_start, Arrow::OpenTriangle);
    }

    #[test]
    fn compiler_dependency_edge_is_dotted() {
        let diagram = compile_class("classDiagram\nA ..> B");
        assert_eq!(diagram.edges[0].stroke, Stroke::Dotted);
    }

    #[test]
    fn compiler_lollipop_edge_uses_circle_marker_at_end() {
        let diagram = compile_class("classDiagram\nClass01 --() bar");
        assert_eq!(diagram.edges[0].arrow_end, Arrow::Circle);
        assert_eq!(diagram.edges[0].arrow_start, Arrow::None);
        assert!(!diagram.nodes.contains_key("bar"));
        let endpoint = &diagram.nodes[&diagram.edges[0].to];
        assert_eq!(endpoint.shape, Shape::TextBlock);
        assert_eq!(endpoint.label, "bar");
        assert_eq!(diagram.nodes["Class01"].shape, Shape::Rectangle);
    }

    #[test]
    fn compiler_lollipop_edge_uses_circle_marker_at_start() {
        let diagram = compile_class("classDiagram\nfoo ()-- Class01");
        assert_eq!(diagram.edges[0].arrow_start, Arrow::Circle);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::None);
        assert!(!diagram.nodes.contains_key("foo"));
        let endpoint = &diagram.nodes[&diagram.edges[0].from];
        assert_eq!(endpoint.shape, Shape::TextBlock);
        assert_eq!(endpoint.label, "foo");
        assert_eq!(diagram.nodes["Class01"].shape, Shape::Rectangle);
    }

    #[test]
    fn compiler_lollipop_endpoint_in_non_lollipop_relation_stays_boxed() {
        let diagram = compile_class("classDiagram\nService --() InterfaceA\nInterfaceA --> Repo");
        assert_eq!(diagram.nodes["InterfaceA"].shape, Shape::Rectangle);
    }

    #[test]
    fn compiler_lollipop_same_name_endpoints_are_distinct_nodes() {
        let diagram =
            compile_class("classDiagram\nService --() InterfaceA\nClient --() InterfaceA");

        let interface_nodes: Vec<_> = diagram
            .nodes
            .values()
            .filter(|n| n.label == "InterfaceA")
            .collect();
        assert_eq!(interface_nodes.len(), 2);
        assert_ne!(diagram.edges[0].to, diagram.edges[1].to);
        assert!(!diagram.nodes.contains_key("InterfaceA"));
    }

    #[test]
    fn compiler_namespace_creates_subgraph_and_assigns_node_parent() {
        let input = "\
classDiagram
namespace BaseShapes {
  class Triangle
  class Rectangle
}";
        let diagram = compile_class(input);

        assert!(diagram.subgraphs.contains_key("namespace:BaseShapes"));
        let namespace = &diagram.subgraphs["namespace:BaseShapes"];
        assert_eq!(namespace.title, "BaseShapes");
        assert!(namespace.nodes.contains(&"Triangle".to_string()));
        assert!(namespace.nodes.contains(&"Rectangle".to_string()));
        assert_eq!(
            diagram.nodes["Triangle"].parent.as_deref(),
            Some("namespace:BaseShapes")
        );
    }

    #[test]
    fn compiler_cross_namespace_relation_keeps_endpoints() {
        let input = "\
classDiagram
namespace A {
  class Source
}
namespace B {
  class Target
}
Source --> Target";
        let diagram = compile_class(input);

        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].from, "Source");
        assert_eq!(diagram.edges[0].to, "Target");
        assert_eq!(
            diagram.nodes["Source"].parent.as_deref(),
            Some("namespace:A")
        );
        assert_eq!(
            diagram.nodes["Target"].parent.as_deref(),
            Some("namespace:B")
        );
    }

    #[test]
    fn compiler_edge_label_preserved() {
        let diagram = compile_class("classDiagram\nA --> B : uses");
        assert_eq!(diagram.edges[0].label, Some("uses".to_string()));
    }

    #[test]
    fn compiler_class_with_members_has_three_compartments() {
        let input = "classDiagram\nclass User {\n  +String name\n  +String email\n  +login()\n  +logout()\n}";
        let diagram = compile_class(input);
        let label = &diagram.nodes["User"].label;
        let lines: Vec<&str> = label.lines().collect();
        // name / separator / attrs... / separator / methods...
        assert_eq!(lines[0], "User");
        assert_eq!(lines[1], Node::SEPARATOR);
        assert_eq!(lines[2], "+String name");
        assert_eq!(lines[3], "+String email");
        assert_eq!(lines[4], Node::SEPARATOR);
        assert_eq!(lines[5], "+login()");
        assert_eq!(lines[6], "+logout()");
    }

    #[test]
    fn compiler_annotation_is_rendered_above_class_name() {
        let input = "classDiagram\nclass Logger {\n  <<interface>>\n  +log(message)\n}";
        let diagram = compile_class(input);
        let label = &diagram.nodes["Logger"].label;
        let lines: Vec<&str> = label.lines().collect();
        // annotation + name share top compartment, then attrs/methods sections
        assert_eq!(lines[0], "<<interface>>");
        assert_eq!(lines[1], "Logger");
        assert_eq!(lines[2], Node::SEPARATOR);
        // empty attrs compartment
        assert_eq!(lines[3], Node::SEPARATOR);
        assert_eq!(lines[4], "+log(message)");
    }

    #[test]
    fn compiler_annotation_without_members_preserves_header() {
        let input = "classDiagram\nclass Logger <<interface>>";
        let diagram = compile_class(input);
        let label = &diagram.nodes["Logger"].label;
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines, vec!["<<interface>>", "Logger"]);
    }

    #[test]
    fn compiler_methods_only_has_empty_attrs_compartment() {
        let input = "classDiagram\nclass Foo {\n  +doStuff()\n}";
        let diagram = compile_class(input);
        let label = &diagram.nodes["Foo"].label;
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines[0], "Foo");
        assert_eq!(lines[1], Node::SEPARATOR);
        // empty attrs compartment
        assert_eq!(lines[2], Node::SEPARATOR);
        assert_eq!(lines[3], "+doStuff()");
    }

    #[test]
    fn compiler_attrs_only_has_empty_methods_compartment() {
        let input = "classDiagram\nclass Foo {\n  +String name\n}";
        let diagram = compile_class(input);
        let label = &diagram.nodes["Foo"].label;
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines[0], "Foo");
        assert_eq!(lines[1], Node::SEPARATOR);
        assert_eq!(lines[2], "+String name");
        assert_eq!(lines[3], Node::SEPARATOR);
        // empty methods compartment
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn compiler_implicit_classes_from_relations() {
        let diagram = compile_class("classDiagram\nA --> B");
        assert_eq!(diagram.nodes.len(), 2);
    }

    #[test]
    fn compiler_edge_indices_sequential() {
        let diagram = compile_class("classDiagram\nA --> B\nB --> C\nC --> A");
        assert_eq!(diagram.edges[0].index, 0);
        assert_eq!(diagram.edges[1].index, 1);
        assert_eq!(diagram.edges[2].index, 2);
    }
}
