//! Compiler from state diagram AST to canonical `graph::Graph`.
//!
//! Maps states to graph nodes and transitions to graph edges.
//! `[*]` markers become `SmallCircle` (source) or `FramedCircle` (target) nodes.
//! Composite states become subgraphs with optional direction overrides.

use std::collections::HashSet;

use crate::graph::{Arrow, Direction, Edge, Graph, Node, Shape, Subgraph};
use crate::mermaid::state::{
    StateDecl, StateModel, StateStatement, StateStereotype, StateTransition,
};

/// Compile a [`StateModel`] into a canonical [`Graph`].
///
/// State diagrams use top-down layout by default. States become `Round`
/// nodes; `[*]` markers become `SmallCircle` (when a source) or
/// `FramedCircle` (when a target).
pub fn compile(model: &StateModel) -> Graph {
    let mut graph = Graph::new(direction_from_str(model.direction.as_deref()));
    let mut seen_nodes: HashSet<String> = HashSet::new();

    process_statements(
        &mut graph,
        &mut seen_nodes,
        &model.statements,
        None,
        "__root",
    );

    resolve_subgraph_edges(&mut graph);

    graph
}

fn direction_from_str(dir: Option<&str>) -> Direction {
    match dir {
        Some("LR") => Direction::LeftRight,
        Some("RL") => Direction::RightLeft,
        Some("BT") => Direction::BottomTop,
        Some("TB") => Direction::TopDown,
        _ => Direction::TopDown,
    }
}

/// Recursively process statements, adding nodes, edges, and subgraphs to the graph.
///
/// `scope` identifies the current [*] coalescing scope — `"__root"` at top level,
/// or the composite state ID when inside a `state { }` block.
fn process_statements(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    statements: &[StateStatement],
    parent_subgraph: Option<&str>,
    scope: &str,
) {
    for stmt in statements {
        match stmt {
            StateStatement::Transition(t) => {
                add_transition(graph, seen_nodes, t, parent_subgraph, scope);
            }
            StateStatement::State(decl) => {
                process_state_decl(graph, seen_nodes, decl, parent_subgraph, scope);
            }
            StateStatement::Direction(_) => {
                // Handled at the composite level during subgraph creation.
            }
        }
    }
}

/// Process a state declaration: create a node (with stereotype shape),
/// handle descriptions, and recurse into composite children.
fn process_state_decl(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    decl: &StateDecl,
    parent_subgraph: Option<&str>,
    _scope: &str,
) {
    let is_composite = !decl.children.is_empty();

    if is_composite {
        // Composite state → subgraph.
        // Extract direction override from children.
        let dir = decl.children.iter().find_map(|s| match s {
            StateStatement::Direction(d) => Some(direction_from_str(Some(d))),
            _ => None,
        });

        // Collect child IDs in AST order for deterministic subgraph node lists.
        let child_ids = collect_child_node_ids(&decl.children, &decl.id);
        process_statements(
            graph,
            seen_nodes,
            &decl.children,
            Some(&decl.id),
            &decl.id, // new scope for [*] coalescing
        );

        // Set parent on child nodes.
        for child_id in &child_ids {
            if let Some(node) = graph.nodes.get_mut(child_id) {
                node.parent = Some(decl.id.clone());
            }
        }

        graph.subgraphs.insert(
            decl.id.clone(),
            Subgraph {
                id: decl.id.clone(),
                title: decl.alias.as_deref().unwrap_or(&decl.id).to_string(),
                nodes: child_ids,
                parent: parent_subgraph.map(|s| s.to_string()),
                dir,
            },
        );
        graph.subgraph_order.push(decl.id.clone());
    } else {
        // Simple state node.
        ensure_state_node_with_decl(graph, seen_nodes, decl, parent_subgraph);
    }
}

/// Collect child node IDs from statements in AST order (deterministic).
/// This mirrors how the flowchart compiler's `collect_node_ids` works.
fn collect_child_node_ids(statements: &[StateStatement], scope: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for stmt in statements {
        match stmt {
            StateStatement::Transition(t) => {
                let from = if t.from == "[*]" {
                    star_node_id(scope, true)
                } else {
                    t.from.clone()
                };
                let to = if t.to == "[*]" {
                    star_node_id(scope, false)
                } else {
                    t.to.clone()
                };
                if seen.insert(from.clone()) {
                    ids.push(from);
                }
                if seen.insert(to.clone()) {
                    ids.push(to);
                }
            }
            StateStatement::State(decl) if decl.children.is_empty() => {
                if seen.insert(decl.id.clone()) {
                    ids.push(decl.id.clone());
                }
            }
            _ => {}
        }
    }
    ids
}

/// Scope-based ID for a `[*]` pseudo-state. One start and one end per scope.
fn star_node_id(scope: &str, is_source: bool) -> String {
    let suffix = if is_source { "start" } else { "end" };
    format!("{scope}_{suffix}")
}

/// Create or update a state node from a declaration.
fn ensure_state_node_with_decl(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    decl: &StateDecl,
    parent: Option<&str>,
) {
    let shape = match &decl.stereotype {
        Some(StateStereotype::Fork | StateStereotype::Join) => Shape::ForkJoin,
        Some(StateStereotype::Choice) => Shape::Diamond,
        None => Shape::Round,
    };

    let display_name = decl.alias.as_deref().unwrap_or(&decl.id);
    // Pseudo-state shapes (fork/join bars, choice diamonds) are unlabeled.
    let label = if shape == Shape::ForkJoin || shape == Shape::Diamond {
        String::new()
    } else {
        match &decl.description {
            Some(desc) => desc.clone(),
            None => display_name.to_string(),
        }
    };

    if seen_nodes.contains(&decl.id) {
        // Update existing node with stereotype/description if needed.
        if let Some(node) = graph.nodes.get_mut(&decl.id) {
            if shape != Shape::Round {
                node.shape = shape;
                node.label = label.clone();
            }
            if decl.description.is_some() {
                node.label = label;
            }
            if parent.is_some() && node.parent.is_none() {
                node.parent = parent.map(|s| s.to_string());
            }
        }
    } else {
        let mut node = Node::new(&decl.id).with_label(label).with_shape(shape);
        node.parent = parent.map(|s| s.to_string());
        graph.add_node(node);
        seen_nodes.insert(decl.id.clone());
    }
}

/// Resolve a `[*]` marker to a scope-based node ID, creating it if needed.
/// All `[*]` sources in the same scope share one start node; all targets share one end node.
fn resolve_star_node(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    is_source: bool,
    parent: Option<&str>,
    scope: &str,
) -> String {
    let id = star_node_id(scope, is_source);
    let shape = if is_source {
        Shape::SmallCircle
    } else {
        Shape::FramedCircle
    };
    if !seen_nodes.contains(&id) {
        let mut node = Node::new(&id).with_label("").with_shape(shape);
        node.parent = parent.map(|s| s.to_string());
        graph.add_node(node);
        seen_nodes.insert(id.clone());
    }
    id
}

/// Ensure a basic Round state node exists for implicit state creation.
fn ensure_implicit_node(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    id: &str,
    parent: Option<&str>,
) {
    if !seen_nodes.contains(id) {
        let mut node = Node::new(id).with_shape(Shape::Round);
        node.parent = parent.map(|s| s.to_string());
        graph.add_node(node);
        seen_nodes.insert(id.to_string());
    }
}

fn add_transition(
    graph: &mut Graph,
    seen_nodes: &mut HashSet<String>,
    t: &StateTransition,
    parent: Option<&str>,
    scope: &str,
) {
    let from_id = if t.from == "[*]" {
        resolve_star_node(graph, seen_nodes, true, parent, scope)
    } else {
        ensure_implicit_node(graph, seen_nodes, &t.from, parent);
        t.from.clone()
    };

    let to_id = if t.to == "[*]" {
        resolve_star_node(graph, seen_nodes, false, parent, scope)
    } else {
        ensure_implicit_node(graph, seen_nodes, &t.to, parent);
        t.to.clone()
    };

    let mut edge = Edge::new(&from_id, &to_id).with_arrows(Arrow::None, Arrow::Normal);
    if let Some(label) = &t.label {
        edge = edge.with_label(label);
    }
    graph.add_edge(edge);
}

/// Replace edge endpoints that reference composite state (subgraph) IDs with
/// representative child nodes, and remove spurious regular nodes for subgraph IDs.
///
/// This mirrors `flowchart::compiler::resolve_subgraph_edges`.
fn resolve_subgraph_edges(graph: &mut Graph) {
    let mut resolved_edges = Vec::new();

    for edge in &graph.edges {
        let (from, from_subgraph) = if graph.is_subgraph(&edge.from) {
            match graph.find_subgraph_sink(&edge.from) {
                Some(child) => (child, Some(edge.from.clone())),
                None => continue,
            }
        } else {
            (edge.from.clone(), None)
        };

        let (to, to_subgraph) = if graph.is_subgraph(&edge.to) {
            match graph.find_non_cluster_child(&edge.to) {
                Some(child) => (child, Some(edge.to.clone())),
                None => continue,
            }
        } else {
            (edge.to.clone(), None)
        };

        resolved_edges.push(Edge {
            from,
            to,
            from_subgraph,
            to_subgraph,
            stroke: edge.stroke,
            arrow_start: edge.arrow_start,
            arrow_end: edge.arrow_end,
            label: edge.label.clone(),
            head_label: edge.head_label.clone(),
            tail_label: edge.tail_label.clone(),
            minlen: edge.minlen,
            index: edge.index,
        });
    }

    graph.edges = resolved_edges;

    // Remove spurious regular nodes created for subgraph IDs by implicit creation.
    let subgraph_ids: Vec<String> = graph.subgraphs.keys().cloned().collect();
    for sg_id in &subgraph_ids {
        if graph.nodes.contains_key(sg_id) {
            graph.nodes.remove(sg_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::state::parse_state_diagram;

    fn compile_state(input: &str) -> Graph {
        let model = parse_state_diagram(input).unwrap();
        compile(&model)
    }

    #[test]
    fn compiler_basic_transition_creates_nodes_and_edge() {
        let graph = compile_state("stateDiagram-v2\n    A --> B");
        assert!(graph.nodes.contains_key("A"));
        assert!(graph.nodes.contains_key("B"));
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "A");
        assert_eq!(graph.edges[0].to, "B");
    }

    #[test]
    fn compiler_state_nodes_are_round() {
        let graph = compile_state("stateDiagram-v2\n    A --> B");
        assert_eq!(graph.nodes["A"].shape, Shape::Round);
        assert_eq!(graph.nodes["B"].shape, Shape::Round);
    }

    #[test]
    fn compiler_star_source_becomes_small_circle() {
        let graph = compile_state("stateDiagram-v2\n    [*] --> Idle");
        let start_node = graph.nodes.values().find(|n| n.shape == Shape::SmallCircle);
        assert!(start_node.is_some());
        assert_eq!(graph.edges[0].to, "Idle");
    }

    #[test]
    fn compiler_star_target_becomes_framed_circle() {
        let graph = compile_state("stateDiagram-v2\n    Done --> [*]");
        let end_node = graph
            .nodes
            .values()
            .find(|n| n.shape == Shape::FramedCircle);
        assert!(end_node.is_some());
        assert_eq!(graph.edges[0].from, "Done");
    }

    #[test]
    fn compiler_transition_label_preserved() {
        let graph = compile_state("stateDiagram-v2\n    A --> B : submit");
        assert_eq!(graph.edges[0].label, Some("submit".to_string()));
    }

    #[test]
    fn compiler_default_direction_is_top_down() {
        let graph = compile_state("stateDiagram-v2\n    A --> B");
        assert_eq!(graph.direction, Direction::TopDown);
    }

    #[test]
    fn compiler_direction_lr() {
        let graph = compile_state("stateDiagram-v2\n    direction LR\n    A --> B");
        assert_eq!(graph.direction, Direction::LeftRight);
    }

    #[test]
    fn compiler_fork_stereotype_uses_fork_join_shape() {
        let graph =
            compile_state("stateDiagram-v2\n    state forkNode <<fork>>\n    A --> forkNode");
        assert_eq!(graph.nodes["forkNode"].shape, Shape::ForkJoin);
    }

    #[test]
    fn compiler_choice_stereotype_uses_diamond_shape() {
        let graph =
            compile_state("stateDiagram-v2\n    state choiceNode <<choice>>\n    A --> choiceNode");
        assert_eq!(graph.nodes["choiceNode"].shape, Shape::Diamond);
    }

    #[test]
    fn compiler_star_markers_coalesce_per_scope() {
        let input = "\
stateDiagram-v2
    [*] --> A
    [*] --> B
    A --> [*]
    B --> [*]";
        let graph = compile_state(input);
        // All [*] sources coalesce to one start node; all targets to one end node.
        let start_nodes: Vec<_> = graph
            .nodes
            .values()
            .filter(|n| n.shape == Shape::SmallCircle)
            .collect();
        let end_nodes: Vec<_> = graph
            .nodes
            .values()
            .filter(|n| n.shape == Shape::FramedCircle)
            .collect();
        assert_eq!(start_nodes.len(), 1);
        assert_eq!(end_nodes.len(), 1);
        // Two edges from start, two edges to end.
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|e| e.from == start_nodes[0].id)
                .count(),
            2
        );
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|e| e.to == end_nodes[0].id)
                .count(),
            2
        );
    }

    #[test]
    fn compiler_composite_gets_own_star_scope() {
        let input = "\
stateDiagram-v2
    [*] --> Active
    state Active {
        [*] --> Running
        Running --> [*]
    }
    Active --> [*]";
        let graph = compile_state(input);
        // Root scope: 1 start + 1 end. Active scope: 1 start + 1 end. Total: 2 + 2.
        let start_nodes: Vec<_> = graph
            .nodes
            .values()
            .filter(|n| n.shape == Shape::SmallCircle)
            .collect();
        let end_nodes: Vec<_> = graph
            .nodes
            .values()
            .filter(|n| n.shape == Shape::FramedCircle)
            .collect();
        assert_eq!(start_nodes.len(), 2);
        assert_eq!(end_nodes.len(), 2);
    }

    #[test]
    fn compiler_composite_state_creates_subgraph() {
        let input = "\
stateDiagram-v2
    [*] --> Active
    state Active {
        [*] --> Running
        Running --> [*]
    }
    Active --> [*]";
        let graph = compile_state(input);
        assert!(graph.subgraphs.contains_key("Active"));
        let sg = &graph.subgraphs["Active"];
        assert_eq!(sg.title, "Active");
        assert!(sg.parent.is_none());
        // Child nodes should have parent set.
        assert!(
            graph
                .nodes
                .values()
                .any(|n| n.parent.as_deref() == Some("Active"))
        );
    }

    #[test]
    fn compiler_composite_direction_override() {
        let input = "\
stateDiagram-v2
    state Processing {
        direction LR
        [*] --> Validating
        Validating --> [*]
    }";
        let graph = compile_state(input);
        let sg = &graph.subgraphs["Processing"];
        assert_eq!(sg.dir, Some(Direction::LeftRight));
    }

    #[test]
    fn compiler_state_description_replaces_label() {
        let input = "\
stateDiagram-v2
    Active : The system is active
    [*] --> Active";
        let graph = compile_state(input);
        assert_eq!(graph.nodes["Active"].label, "The system is active");
    }

    #[test]
    fn compiler_stereotype_overrides_implicit_shape() {
        // Transition creates the node as Round first, then stereotype upgrades it.
        let input = "\
stateDiagram-v2
    A --> forkNode
    state forkNode <<fork>>";
        let graph = compile_state(input);
        assert_eq!(graph.nodes["forkNode"].shape, Shape::ForkJoin);
    }

    #[test]
    fn compiler_full_example() {
        let input = "\
stateDiagram-v2
    [*] --> Idle
    Idle --> Processing : submit
    Processing --> Done : complete
    Done --> [*]";
        let graph = compile_state(input);
        // 3 named states + 1 start + 1 end = 5 nodes
        assert_eq!(graph.nodes.len(), 5);
        assert_eq!(graph.edges.len(), 4);
    }
}
