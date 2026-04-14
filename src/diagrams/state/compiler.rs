//! Compiler from state diagram AST to canonical `graph::Graph`.
//!
//! Maps states to graph nodes and transitions to graph edges.
//! `[*]` markers become `SmallCircle` (source) or `FramedCircle` (target) nodes.
//! Composite states become subgraphs with optional direction overrides.

use std::collections::{HashMap, HashSet};

use crate::graph::style::{ColorToken, NodeStyle};
use crate::graph::{Arrow, Direction, Edge, Graph, Node, NotePosition, Shape, Stroke, Subgraph};
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
    let class_defs = collect_class_defs(&model.statements);
    let mut state = CompileState {
        seen_nodes: HashSet::new(),
        note_counter: 0,
        node_styles: HashMap::new(),
        class_defs: &class_defs,
    };

    process_statements(&mut graph, &mut state, &model.statements, None, "__root");

    apply_default_class(&mut graph, &state.node_styles, state.class_defs);

    resolve_subgraph_edges(&mut graph);

    graph
}

/// First pass: collect all classDef definitions (including inside composites).
fn collect_class_defs(statements: &[StateStatement]) -> HashMap<String, NodeStyle> {
    let mut defs = HashMap::new();
    collect_class_defs_recursive(statements, &mut defs);
    defs
}

fn collect_class_defs_recursive(
    statements: &[StateStatement],
    defs: &mut HashMap<String, NodeStyle>,
) {
    for stmt in statements {
        match stmt {
            StateStatement::ClassDef(cd) => {
                defs.entry(cd.class_name.clone())
                    .and_modify(|e| *e = e.merge(&cd.style))
                    .or_insert_with(|| cd.style.clone());
            }
            StateStatement::State(decl) if !decl.children.is_empty() => {
                collect_class_defs_recursive(&decl.children, defs);
            }
            StateStatement::State(decl) if !decl.regions.is_empty() => {
                for region in &decl.regions {
                    collect_class_defs_recursive(region, defs);
                }
            }
            _ => {}
        }
    }
}

/// Merge a style into a node, creating or updating the node's style.
fn merge_node_style(
    graph: &mut Graph,
    node_styles: &mut HashMap<String, NodeStyle>,
    node_id: &str,
    style: &NodeStyle,
) {
    let merged = node_styles
        .entry(node_id.to_string())
        .and_modify(|existing| *existing = existing.merge(style))
        .or_insert_with(|| style.clone())
        .clone();

    if let Some(node) = graph.nodes.get_mut(node_id) {
        node.style = merged;
    }
}

/// Apply `classDef default` to every node that has no explicit class styling.
fn apply_default_class(
    graph: &mut Graph,
    node_styles: &HashMap<String, NodeStyle>,
    class_defs: &HashMap<String, NodeStyle>,
) {
    if let Some(default_style) = class_defs.get("default") {
        let unstyled: Vec<String> = graph
            .nodes
            .keys()
            .filter(|id| !node_styles.contains_key(*id))
            .cloned()
            .collect();
        for node_id in unstyled {
            if let Some(node) = graph.nodes.get_mut(&node_id) {
                node.style = default_style.merge(&node.style);
            }
        }
    }
}

/// Mutable compilation state threaded through recursive statement processing.
struct CompileState<'a> {
    seen_nodes: HashSet<String>,
    note_counter: usize,
    node_styles: HashMap<String, NodeStyle>,
    class_defs: &'a HashMap<String, NodeStyle>,
}

/// Create a standalone note node with a constraint edge to its target state.
///
/// Called inline during statement processing so the edge appears at the same
/// position in the edge list as in the source, preserving the layout engine's
/// edge-order-based ranking.
fn add_note_node(
    graph: &mut Graph,
    state_id: &str,
    text: &str,
    position: NotePosition,
    index: usize,
) {
    let note_node_id = format!("{state_id}____note_{index}");

    // Place note at the same hierarchy level as the target state.
    let state_parent = graph.nodes.get(state_id).and_then(|n| n.parent.clone());
    let mut note_node = Node::new(&note_node_id)
        .with_label(text)
        .with_shape(Shape::NoteRect);
    note_node.parent = state_parent.clone();
    note_node.style = note_style();
    graph.add_node(note_node);

    // Add note to the same parent subgraph's children list.
    if let Some(ref parent_id) = state_parent
        && let Some(parent_sg) = graph.subgraphs.get_mut(parent_id)
    {
        parent_sg.nodes.push(note_node_id.clone());
    }

    // Constraint edge: "right of" = downstream, "left of" = upstream.
    let (from, to) = match position {
        NotePosition::Right => (state_id.to_string(), note_node_id.clone()),
        NotePosition::Left => (note_node_id.clone(), state_id.to_string()),
    };

    graph.add_edge(
        Edge::new(&from, &to)
            .with_stroke(Stroke::Dashed)
            .with_arrows(Arrow::None, Arrow::None),
    );
}

fn note_style() -> NodeStyle {
    NodeStyle::default()
        .with_fill(ColorToken::parse("#fff5ad").unwrap())
        .with_stroke(ColorToken::parse("#aaaa33").unwrap())
        .with_color(ColorToken::parse("#333").unwrap())
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
    state: &mut CompileState,
    statements: &[StateStatement],
    parent_subgraph: Option<&str>,
    scope: &str,
) {
    for stmt in statements {
        match stmt {
            StateStatement::Transition(t) => {
                add_transition(graph, &mut state.seen_nodes, t, parent_subgraph, scope);
                // Resolve ::: class annotations on endpoints.
                if let Some(class_name) = &t.from_class
                    && t.from != "[*]"
                    && let Some(style) = state.class_defs.get(class_name.as_str())
                {
                    let style = style.clone();
                    merge_node_style(graph, &mut state.node_styles, &t.from, &style);
                }
                if let Some(class_name) = &t.to_class
                    && t.to != "[*]"
                    && let Some(style) = state.class_defs.get(class_name.as_str())
                {
                    let style = style.clone();
                    merge_node_style(graph, &mut state.node_styles, &t.to, &style);
                }
            }
            StateStatement::State(decl) => {
                process_state_decl(graph, state, decl, parent_subgraph, scope);
            }
            StateStatement::Direction(_) => {
                // Handled at the composite level during subgraph creation.
            }
            StateStatement::Note(note) => {
                ensure_implicit_node(
                    graph,
                    &mut state.seen_nodes,
                    &note.state_id,
                    parent_subgraph,
                );
                let position = match note.position {
                    crate::mermaid::state::NotePosition::Right => NotePosition::Right,
                    crate::mermaid::state::NotePosition::Left => NotePosition::Left,
                };
                add_note_node(
                    graph,
                    &note.state_id,
                    &note.text,
                    position,
                    state.note_counter,
                );
                state.note_counter += 1;
            }
            StateStatement::ClassDef(_) => {
                // Collected in first pass.
            }
            StateStatement::Style(style_stmt) => {
                merge_node_style(
                    graph,
                    &mut state.node_styles,
                    &style_stmt.node_id,
                    &style_stmt.style,
                );
            }
            StateStatement::ClassApply(apply) => {
                if let Some(style) = state.class_defs.get(&apply.class_name) {
                    let style = style.clone();
                    for node_id in &apply.node_ids {
                        merge_node_style(graph, &mut state.node_styles, node_id, &style);
                    }
                }
            }
            StateStatement::RegionDivider => {
                // Consumed during parsing; should never appear in processed statements.
            }
        }
    }
}

/// Process a state declaration: create a node (with stereotype shape),
/// handle descriptions, and recurse into composite children.
fn process_state_decl(
    graph: &mut Graph,
    state: &mut CompileState,
    decl: &StateDecl,
    parent_subgraph: Option<&str>,
    _scope: &str,
) {
    let is_composite = !decl.children.is_empty();
    let has_regions = !decl.regions.is_empty();

    if has_regions {
        // Concurrent composite state → parent subgraph with region child subgraphs.
        let mut all_child_ids = Vec::new();
        let mut region_sg_ids = Vec::new();

        // A direction directive in the first region (before the first `--`)
        // applies as the composite-level default for all regions.
        let composite_dir = decl.regions.first().and_then(|r| {
            r.iter().find_map(|s| match s {
                StateStatement::Direction(d) => Some(direction_from_str(Some(d))),
                _ => None,
            })
        });

        for (i, region) in decl.regions.iter().enumerate() {
            let region_sg_id = format!("{}__region_{}", decl.id, i);
            let region_scope = &region_sg_id;

            // Extract per-region direction, falling back to composite-level direction.
            let region_dir = region
                .iter()
                .find_map(|s| match s {
                    StateStatement::Direction(d) => Some(direction_from_str(Some(d))),
                    _ => None,
                })
                .or(composite_dir);

            // Collect child IDs scoped to this region.
            let child_ids = collect_child_node_ids(region, region_scope);

            process_statements(graph, state, region, Some(&region_sg_id), region_scope);

            // Set parent on region child nodes to the region subgraph.
            for child_id in &child_ids {
                if let Some(node) = graph.nodes.get_mut(child_id) {
                    node.parent = Some(region_sg_id.clone());
                }
            }

            // Region subgraph: invisible border (parent draws the outer border).
            graph.subgraphs.insert(
                region_sg_id.clone(),
                Subgraph {
                    id: region_sg_id.clone(),
                    title: String::new(),
                    nodes: child_ids.clone(),
                    parent: Some(decl.id.clone()),
                    dir: region_dir,
                    invisible: true,
                    concurrent_regions: Vec::new(),
                },
            );
            graph.subgraph_order.push(region_sg_id.clone());

            all_child_ids.push(region_sg_id.clone());
            // Also add leaf nodes so the parent subgraph contains everything.
            all_child_ids.extend(child_ids);
            region_sg_ids.push(region_sg_id);
        }

        // Add invisible edges between adjacent regions to force vertical stacking
        // in the initial compound layout. A post-layout rearrangement step
        // transforms this into LR (side-by-side) arrangement.
        for pair in region_sg_ids.windows(2) {
            let upper_sg = graph.subgraphs.get(&pair[0]);
            let lower_sg = graph.subgraphs.get(&pair[1]);
            if let (Some(upper), Some(lower)) = (upper_sg, lower_sg) {
                let upper_last = upper.nodes.last().cloned();
                let lower_first = lower.nodes.first().cloned();
                if let (Some(from), Some(to)) = (upper_last, lower_first) {
                    graph.add_edge(
                        Edge::new(&from, &to)
                            .with_stroke(Stroke::Invisible)
                            .with_arrows(Arrow::None, Arrow::None)
                            .with_minlen(1),
                    );
                }
            }
        }

        // Parent composite subgraph.
        graph.subgraphs.insert(
            decl.id.clone(),
            Subgraph {
                id: decl.id.clone(),
                title: decl.alias.as_deref().unwrap_or(&decl.id).to_string(),
                nodes: all_child_ids,
                parent: parent_subgraph.map(|s| s.to_string()),
                dir: None,
                invisible: false,
                concurrent_regions: region_sg_ids,
            },
        );
        graph.subgraph_order.push(decl.id.clone());
    } else if is_composite {
        // Composite state → subgraph (single region, no dividers).
        // Extract direction override from children.
        let dir = decl.children.iter().find_map(|s| match s {
            StateStatement::Direction(d) => Some(direction_from_str(Some(d))),
            _ => None,
        });

        // Collect child IDs in AST order for deterministic subgraph node lists.
        let child_ids = collect_child_node_ids(&decl.children, &decl.id);
        process_statements(
            graph,
            state,
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
                concurrent_regions: Vec::new(),
            },
        );
        graph.subgraph_order.push(decl.id.clone());
    } else {
        // Simple state node.
        ensure_state_node_with_decl(graph, &mut state.seen_nodes, decl, parent_subgraph);
    }

    // Apply :::className from declaration (e.g. `state Running:::active`).
    if let Some(class_name) = &decl.class_name
        && let Some(style) = state.class_defs.get(class_name.as_str())
    {
        let style = style.clone();
        merge_node_style(graph, &mut state.node_styles, &decl.id, &style);
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
            StateStatement::State(decl) => {
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
            style: edge.style.clone(),
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
        let result = parse_state_diagram(input).unwrap();
        compile(&result.model)
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
    fn compiler_note_creates_standalone_node_with_constraint_edge() {
        let input = "\
stateDiagram-v2
    [*] --> Active
    note right of Active : This is a note";
        let graph = compile_state(input);

        // Note becomes a standalone NoteRect graph node.
        let note_node = graph
            .nodes
            .values()
            .find(|n| n.shape == Shape::NoteRect)
            .expect("note node should exist");
        assert_eq!(note_node.label, "This is a note");

        // No invisible subgroup — note is a standalone node.
        assert!(
            !graph.subgraphs.values().any(|sg| sg.invisible),
            "should not create invisible subgraphs"
        );

        // Note is at the same hierarchy level as Active (both top-level).
        assert_eq!(note_node.parent, graph.nodes["Active"].parent);

        // Dotted constraint edge connects state and note (no arrowheads).
        let dotted_edge = graph
            .edges
            .iter()
            .find(|e| e.stroke == Stroke::Dashed)
            .expect("dotted edge should exist");
        assert_eq!(dotted_edge.arrow_start, Arrow::None);
        assert_eq!(dotted_edge.arrow_end, Arrow::None);
        // Right of in TD → state is "from", note is "to" (downstream).
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

    #[test]
    fn compiler_classdef_applied_via_class_statement() {
        let input = "\
stateDiagram-v2
    classDef active fill:#bfb,stroke:#0a0
    [*] --> Idle
    class Idle active";
        let graph = compile_state(input);
        let node = &graph.nodes["Idle"];
        assert_eq!(node.style.fill.as_ref().unwrap().raw(), "#bfb");
        assert_eq!(node.style.stroke.as_ref().unwrap().raw(), "#0a0");
    }

    #[test]
    fn compiler_classdef_applied_via_triple_colon() {
        let input = "\
stateDiagram-v2
    classDef active fill:#bfb
    [*] --> Running:::active";
        let graph = compile_state(input);
        let node = &graph.nodes["Running"];
        assert_eq!(node.style.fill.as_ref().unwrap().raw(), "#bfb");
    }

    #[test]
    fn compiler_style_overrides_classdef() {
        let input = "\
stateDiagram-v2
    classDef foo fill:#f00
    [*] --> A
    class A foo
    style A fill:#0f0";
        let graph = compile_state(input);
        assert_eq!(graph.nodes["A"].style.fill.as_ref().unwrap().raw(), "#0f0");
    }

    #[test]
    fn compiler_style_statement_direct() {
        let input = "\
stateDiagram-v2
    [*] --> Idle
    style Idle fill:#bfb";
        let graph = compile_state(input);
        assert_eq!(
            graph.nodes["Idle"].style.fill.as_ref().unwrap().raw(),
            "#bfb"
        );
    }

    #[test]
    fn compiler_undefined_class_silently_ignored() {
        let input = "stateDiagram-v2\n    [*] --> Idle:::nonexistent";
        let graph = compile_state(input);
        assert!(graph.nodes["Idle"].style.is_empty());
    }

    #[test]
    fn classdef_default_applies_to_unclassified_nodes() {
        let input = "stateDiagram-v2\n    classDef default fill:#f00\n    classDef active fill:#0f0\n    [*] --> Idle\n    Idle --> Running:::active";
        let graph = compile_state(input);

        // Idle has no explicit class — should get the default fill.
        assert_eq!(
            graph.nodes["Idle"].style.fill.as_ref().unwrap().raw(),
            "#f00"
        );
        // Running has explicit :::active — should NOT get the default.
        assert_eq!(
            graph.nodes["Running"].style.fill.as_ref().unwrap().raw(),
            "#0f0"
        );
    }

    #[test]
    fn state_decl_class_annotation_applied() {
        let input = "stateDiagram-v2\n    classDef active fill:#0f0\n    state Running:::active";
        let graph = compile_state(input);
        assert_eq!(
            graph.nodes["Running"].style.fill.as_ref().unwrap().raw(),
            "#0f0"
        );
    }

    #[test]
    fn state_decl_alias_class_annotation_applied() {
        let input =
            "stateDiagram-v2\n    classDef active fill:#0f0\n    state \"Running\" as R:::active";
        let graph = compile_state(input);
        assert_eq!(graph.nodes["R"].style.fill.as_ref().unwrap().raw(), "#0f0");
    }

    #[test]
    fn compiler_concurrent_regions_create_child_subgraphs() {
        let input = "\
stateDiagram-v2
    state Active {
        [*] --> A1
        A1 --> A2
        --
        [*] --> B1
        B1 --> B2
    }";
        let graph = compile_state(input);
        // Parent composite subgraph exists.
        assert!(graph.subgraphs.contains_key("Active"));
        let parent = &graph.subgraphs["Active"];
        assert_eq!(parent.concurrent_regions.len(), 2);
        assert_eq!(parent.concurrent_regions[0], "Active__region_0");
        assert_eq!(parent.concurrent_regions[1], "Active__region_1");

        // Region subgraphs exist and are children of Active.
        let r0 = &graph.subgraphs["Active__region_0"];
        assert_eq!(r0.parent.as_deref(), Some("Active"));
        assert!(r0.invisible);

        let r1 = &graph.subgraphs["Active__region_1"];
        assert_eq!(r1.parent.as_deref(), Some("Active"));
        assert!(r1.invisible);
    }

    #[test]
    fn compiler_concurrent_regions_independent_star_scopes() {
        let input = "\
stateDiagram-v2
    state Active {
        [*] --> A1
        A1 --> [*]
        --
        [*] --> B1
        B1 --> [*]
    }";
        let graph = compile_state(input);
        // Each region should have its own start and end pseudo-states.
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
        // Two start nodes (one per region) and two end nodes.
        assert_eq!(start_nodes.len(), 2);
        assert_eq!(end_nodes.len(), 2);
    }

    #[test]
    fn compiler_concurrent_regions_three_regions() {
        let input = "\
stateDiagram-v2
    state Active {
        [*] --> A
        --
        [*] --> B
        --
        [*] --> C
    }";
        let graph = compile_state(input);
        let parent = &graph.subgraphs["Active"];
        assert_eq!(parent.concurrent_regions.len(), 3);
    }

    #[test]
    fn compiler_no_regions_without_divider() {
        let input = "\
stateDiagram-v2
    state Active {
        [*] --> Running
        Running --> [*]
    }";
        let graph = compile_state(input);
        let sg = &graph.subgraphs["Active"];
        assert!(sg.concurrent_regions.is_empty());
        // No region child subgraphs.
        assert!(!graph.subgraphs.contains_key("Active__region_0"));
    }
}
