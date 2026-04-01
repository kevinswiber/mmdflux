//! Compiler from state diagram AST to canonical `graph::Graph`.
//!
//! Maps states to graph nodes and transitions to graph edges.
//! `[*]` markers become `SmallCircle` (source) or `FramedCircle` (target) nodes.
//! Composite states become subgraphs with optional direction overrides.

use std::collections::HashSet;

use crate::graph::{
    Arrow, Direction, Edge, Graph, GraphNote, Node, NotePosition, Shape, Stroke, Subgraph,
};
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

    create_note_groups(&mut graph);
    resolve_subgraph_edges(&mut graph);

    graph
}

/// Create invisible compound subgraphs for note annotations.
///
/// For each note in `graph.notes`, wraps the target state + a new `NoteRect`
/// node in an invisible subgraph with a dotted edge. The subgraph has no
/// direction override (`dir: None`) so it inherits the parent direction.
/// Engines that want perpendicular note positioning (e.g. flux-layered) can
/// inject direction overrides as a pre-processing step.
///
/// If a state already has a note group (from a previous note), the new note
/// node is added to the existing group instead of creating a nested one.
fn create_note_groups(graph: &mut Graph) {
    let notes: Vec<GraphNote> = std::mem::take(&mut graph.notes);

    for (i, note) in notes.iter().enumerate() {
        let state_id = &note.target;

        // Skip if target state doesn't exist.
        if !graph.nodes.contains_key(state_id) {
            continue;
        }

        let note_node_id = format!("{state_id}____note_{i}");

        // Check if this state already has a note group.
        let existing_group_id = graph
            .subgraphs
            .values()
            .find(|sg| sg.invisible && sg.nodes.contains(&state_id.to_string()))
            .map(|sg| sg.id.clone());

        // Create the note node.
        let parent_for_note = existing_group_id
            .clone()
            .unwrap_or_else(|| format!("{state_id}____note_group_{i}"));
        let mut note_node = Node::new(&note_node_id)
            .with_label(&note.text)
            .with_shape(Shape::NoteRect);
        note_node.parent = Some(parent_for_note.clone());
        graph.add_node(note_node);

        let group_id = if let Some(ref existing_id) = existing_group_id {
            // Add note node to existing group.
            if let Some(sg) = graph.subgraphs.get_mut(existing_id) {
                sg.nodes.push(note_node_id.clone());
            }
            existing_id.clone()
        } else {
            let group_id = parent_for_note;

            // Get state's current parent before re-parenting.
            let state_parent = graph.nodes.get(state_id).and_then(|n| n.parent.clone());

            // Re-parent state into the note group.
            if let Some(node) = graph.nodes.get_mut(state_id) {
                node.parent = Some(group_id.clone());
            }

            // Update parent subgraph's children list: replace state with note group.
            if let Some(ref parent_id) = state_parent
                && let Some(parent_sg) = graph.subgraphs.get_mut(parent_id)
                && let Some(pos) = parent_sg.nodes.iter().position(|n| n == state_id)
            {
                parent_sg.nodes[pos] = group_id.clone();
            }

            graph.subgraphs.insert(
                group_id.clone(),
                Subgraph {
                    id: group_id.clone(),
                    title: String::new(),
                    nodes: vec![state_id.to_string(), note_node_id.clone()],
                    parent: state_parent,
                    dir: None,
                    invisible: true,
                },
            );
            graph.subgraph_order.push(group_id.clone());
            group_id
        };

        // Edge direction for positioning within the note group.
        // TD/BT main: note group is LR → edge controls left/right placement.
        // LR/RL main: note group is TD → edge controls top/bottom placement.
        let is_horizontal = matches!(graph.direction, Direction::LeftRight | Direction::RightLeft);
        let (from, to) = match (note.position, is_horizontal) {
            // TD: right of → state left, note right
            (NotePosition::Right, false) => (state_id.to_string(), note_node_id.clone()),
            // TD: left of → note left, state right
            (NotePosition::Left, false) => (note_node_id.clone(), state_id.to_string()),
            // LR: right of (= above) → note first rank, state second rank
            (NotePosition::Right, true) => (note_node_id.clone(), state_id.to_string()),
            // LR: left of (= below) → state first rank, note second rank
            (NotePosition::Left, true) => (state_id.to_string(), note_node_id.clone()),
        };

        graph.add_edge(
            Edge::new(&from, &to)
                .with_stroke(Stroke::Dotted)
                .with_arrows(Arrow::None, Arrow::None),
        );

        // Ensure note-group edges are not resolved as cross-subgraph edges.
        // The note group's internal edge connects siblings, not boundary-crossing nodes.
        let _ = &group_id;
    }
}

fn direction_from_str(dir: Option<&str>) -> Direction {
    match dir {
        Some("LR") => Direction::LeftRight,
        Some("RL") => Direction::RightLeft,
        Some("BT") => Direction::BottomTop,
        Some("TB") | Some("TD") => Direction::TopDown,
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
            StateStatement::Note(note) => {
                // Ensure target state exists, then store as annotation.
                ensure_implicit_node(graph, seen_nodes, &note.state_id, parent_subgraph);
                graph.notes.push(GraphNote {
                    target: note.state_id.clone(),
                    position: match note.position {
                        crate::mermaid::state::NotePosition::Right => NotePosition::Right,
                        crate::mermaid::state::NotePosition::Left => NotePosition::Left,
                    },
                    text: note.text.clone(),
                });
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
                invisible: false,
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
    let is_unlabeled_shape = shape == Shape::ForkJoin || shape == Shape::Diamond;

    if seen_nodes.contains(&decl.id) {
        // Update existing node with stereotype/description if needed.
        if let Some(node) = graph.nodes.get_mut(&decl.id) {
            if shape != Shape::Round {
                node.shape = shape;
                node.label = String::new();
            }
            if !decl.descriptions.is_empty() && !is_unlabeled_shape {
                // Accumulate descriptions for multi-line rendering.
                append_descriptions(&mut node.label, &decl.descriptions, display_name);
            }
            if parent.is_some() && node.parent.is_none() {
                node.parent = parent.map(|s| s.to_string());
            }
        }
    } else {
        let label = if is_unlabeled_shape {
            String::new()
        } else {
            build_description_label(&decl.descriptions, display_name)
        };
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

/// Build a label from descriptions for a new node.
///
/// - No descriptions → use display name
/// - One description → use that description (simple box, matching Mermaid)
/// - Two+ descriptions → first description as title, `---` separator, rest as body
fn build_description_label(descriptions: &[String], display_name: &str) -> String {
    match descriptions.len() {
        0 => display_name.to_string(),
        1 => descriptions[0].clone(),
        _ => {
            let mut parts = vec![descriptions[0].clone(), Node::SEPARATOR.to_string()];
            parts.extend(descriptions[1..].iter().cloned());
            parts.join("\n")
        }
    }
}

/// Append new descriptions to an existing node's label, adding a separator
/// when this creates a multi-section box.
fn append_descriptions(label: &mut String, new_descs: &[String], display_name: &str) {
    if new_descs.is_empty() {
        return;
    }
    // If the label was just the display name (no prior descriptions), replace it.
    if *label == display_name {
        *label = build_description_label(new_descs, display_name);
        return;
    }
    // Already has content — check if it already has a separator (multi-section).
    let has_separator = label.contains(Node::SEPARATOR);
    if has_separator {
        // Append to the body section.
        for desc in new_descs {
            label.push('\n');
            label.push_str(desc);
        }
    } else {
        // Current label is a single description. Adding more triggers two-section box.
        let existing = label.clone();
        let mut all = vec![existing];
        all.extend(new_descs.iter().cloned());
        *label = build_description_label(&all, display_name);
    }
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

    #[test]
    fn compiler_multiline_descriptions_create_separator() {
        let input = "\
stateDiagram-v2
    Server : Listening on port 8080
    Server : Accepts TCP connections
    [*] --> Server";
        let graph = compile_state(input);
        let label = &graph.nodes["Server"].label;
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines[0], "Listening on port 8080");
        assert_eq!(lines[1], Node::SEPARATOR);
        assert_eq!(lines[2], "Accepts TCP connections");
    }

    #[test]
    fn compiler_single_description_no_separator() {
        let input = "\
stateDiagram-v2
    Active : The system is active
    [*] --> Active";
        let graph = compile_state(input);
        assert!(!graph.nodes["Active"].label.contains(Node::SEPARATOR));
        assert_eq!(graph.nodes["Active"].label, "The system is active");
    }

    #[test]
    fn compiler_three_descriptions() {
        let input = "\
stateDiagram-v2
    Server : Line 1
    Server : Line 2
    Server : Line 3";
        let graph = compile_state(input);
        let label = &graph.nodes["Server"].label;
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines.len(), 4); // Line1, ---, Line2, Line3
        assert_eq!(lines[0], "Line 1");
        assert_eq!(lines[1], Node::SEPARATOR);
        assert_eq!(lines[2], "Line 2");
        assert_eq!(lines[3], "Line 3");
    }

    #[test]
    fn compiler_description_after_implicit_creation() {
        // Transition creates node implicitly, then description arrives.
        let input = "\
stateDiagram-v2
    [*] --> Server
    Server : Listening
    Server : Accepting";
        let graph = compile_state(input);
        let label = &graph.nodes["Server"].label;
        assert!(label.contains(Node::SEPARATOR));
        let lines: Vec<&str> = label.lines().collect();
        assert_eq!(lines[0], "Listening");
        assert_eq!(lines[1], Node::SEPARATOR);
        assert_eq!(lines[2], "Accepting");
    }

    #[test]
    fn compiler_note_creates_compound_group() {
        let input = "\
stateDiagram-v2
    [*] --> Active
    note right of Active : This is a note";
        let graph = compile_state(input);

        // Note becomes a NoteRect graph node.
        let note_node = graph
            .nodes
            .values()
            .find(|n| n.shape == Shape::NoteRect)
            .expect("note node should exist");
        assert_eq!(note_node.label, "This is a note");

        // Invisible note group wraps state + note.
        let group = graph
            .subgraphs
            .values()
            .find(|sg| sg.invisible)
            .expect("invisible note group should exist");
        assert!(group.nodes.contains(&"Active".to_string()));
        assert!(group.nodes.contains(&note_node.id));
        assert_eq!(group.dir, None); // no override; engines inject if needed

        // Active is re-parented into the note group.
        assert_eq!(
            graph.nodes["Active"].parent.as_deref(),
            Some(group.id.as_str())
        );

        // Dotted edge connects state and note (no arrowheads).
        let dotted_edge = graph
            .edges
            .iter()
            .find(|e| e.stroke == Stroke::Dotted)
            .expect("dotted edge should exist");
        assert_eq!(dotted_edge.arrow_start, Arrow::None);
        assert_eq!(dotted_edge.arrow_end, Arrow::None);
        // Right of in TD → state is "from", note is "to".
        assert_eq!(dotted_edge.from, "Active");
        assert_eq!(dotted_edge.to, note_node.id);

        // Notes are consumed (no longer in graph.notes).
        assert!(graph.notes.is_empty());
    }

    #[test]
    fn compiler_note_multiline() {
        let input = "\
stateDiagram-v2
    Active --> [*]
    note right of Active
        Line one
        Line two
    end note";
        let graph = compile_state(input);
        let note_node = graph
            .nodes
            .values()
            .find(|n| n.shape == Shape::NoteRect)
            .expect("note node should exist");
        assert_eq!(note_node.label, "Line one\nLine two");
    }
}
