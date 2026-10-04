//! Graph transforms the text width fit applies before re-solving a layout.
//!
//! Each mutating transform returns `true` iff it changed the graph, so the
//! fit can skip a rung whose transforms leave the graph as it was. Wrapping
//! is lossless (it never splits a word); truncation and member elision drop
//! text and are only used when the caller opts in.

use crate::format::display_width;
use crate::graph::{Direction, Graph, Node};

/// Greedily wrap `line` on single spaces so each line is at most `cells`
/// wide, never splitting a word. A word wider than `cells` gets its own line.
pub(crate) fn wrap_words_to_cells(line: &str, cells: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in line.split(' ').filter(|word| !word.is_empty()) {
        if current.is_empty() {
            current = word.to_string();
        } else if display_width(&current) + 1 + display_width(word) <= cells {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// Keep the longest prefix of `line` whose width plus one fits in `cells`
/// and append `…`; a line that already fits is returned unchanged.
pub(crate) fn truncate_to_cells(line: &str, cells: usize) -> String {
    if display_width(line) <= cells {
        return line.to_string();
    }
    let mut kept = String::new();
    for ch in line.chars() {
        kept.push(ch);
        if display_width(&kept) + 1 > cells {
            kept.pop();
            break;
        }
    }
    kept.push('…');
    kept
}

/// Swap the layout axis: TD↔LR and BT↔RL.
pub(crate) fn transpose_direction(direction: Direction) -> Direction {
    match direction {
        Direction::TopDown => Direction::LeftRight,
        Direction::LeftRight => Direction::TopDown,
        Direction::BottomTop => Direction::RightLeft,
        Direction::RightLeft => Direction::BottomTop,
    }
}

/// Transpose the root direction and every subgraph override, then clear
/// overrides that now equal their parent's direction.
pub(crate) fn flip_graph_direction(graph: &mut Graph) -> bool {
    graph.direction = transpose_direction(graph.direction);
    for subgraph in graph.subgraphs.values_mut() {
        subgraph.dir = subgraph.dir.map(transpose_direction);
    }
    clear_redundant_subgraph_directions(graph);
    true
}

/// Under a vertical root, drop horizontal (LR/RL) subgraph overrides, then
/// clear overrides that now equal their parent's direction.
pub(crate) fn strip_horizontal_subgraph_overrides(graph: &mut Graph) -> bool {
    if !is_vertical(graph.direction) {
        return false;
    }
    let mut changed = false;
    for subgraph in graph.subgraphs.values_mut() {
        if subgraph.dir.is_some_and(|dir| !is_vertical(dir)) {
            subgraph.dir = None;
            changed = true;
        }
    }
    clear_redundant_subgraph_directions(graph) || changed
}

/// Top-down over the subgraph tree, clear an override equal to the effective
/// parent direction (the nearest ancestor's remaining override, else the
/// root direction).
pub(crate) fn clear_redundant_subgraph_directions(graph: &mut Graph) -> bool {
    let mut ids: Vec<String> = graph.subgraphs.keys().cloned().collect();
    ids.sort_by(|a, b| {
        graph
            .subgraph_depth(a)
            .cmp(&graph.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });
    let mut changed = false;
    for id in ids {
        let parent_direction = effective_parent_direction(graph, &id);
        if let Some(subgraph) = graph.subgraphs.get_mut(&id)
            && subgraph.dir == Some(parent_direction)
        {
            subgraph.dir = None;
            changed = true;
        }
    }
    changed
}

fn effective_parent_direction(graph: &Graph, id: &str) -> Direction {
    let mut current = graph.subgraphs.get(id).and_then(|sg| sg.parent.clone());
    while let Some(parent_id) = current {
        let Some(parent) = graph.subgraphs.get(&parent_id) else {
            break;
        };
        if let Some(dir) = parent.dir {
            return dir;
        }
        current = parent.parent.clone();
    }
    graph.direction
}

fn is_vertical(direction: Direction) -> bool {
    matches!(direction, Direction::TopDown | Direction::BottomTop)
}

/// Split a label into its head (lines up to and including the first
/// separator) and member lines. Labels without a separator have no members.
fn member_split(label: &str) -> Option<(Vec<&str>, Vec<&str>)> {
    let lines: Vec<&str> = label.split('\n').collect();
    let separator = lines.iter().position(|line| *line == Node::SEPARATOR)?;
    Some((
        lines[..=separator].to_vec(),
        lines[separator + 1..].to_vec(),
    ))
}

fn has_member_sections(node: &Node) -> bool {
    node.label.split('\n').any(|line| line == Node::SEPARATOR)
}

fn replace_label(target: &mut String, lines: Vec<String>) -> bool {
    let joined = lines.join("\n");
    if *target == joined {
        return false;
    }
    *target = joined;
    true
}

/// Truncate node label lines (separators untouched) and edge label lines to
/// `cells`.
pub(crate) fn truncate_labels(graph: &mut Graph, cells: usize) -> bool {
    let mut changed = false;
    for node in graph.nodes.values_mut() {
        let lines = node
            .label
            .split('\n')
            .map(|line| {
                if line == Node::SEPARATOR {
                    line.to_string()
                } else {
                    truncate_to_cells(line, cells)
                }
            })
            .collect();
        changed |= replace_label(&mut node.label, lines);
    }
    for edge in &mut graph.edges {
        if let Some(label) = edge.label.as_mut() {
            let lines = label
                .split('\n')
                .map(|line| truncate_to_cells(line, cells))
                .collect();
            changed |= replace_label(label, lines);
        }
    }
    changed
}

/// Keep the first `keep` member lines of each class member section and
/// append a `…` line to sections that lost members.
pub(crate) fn elide_class_members(graph: &mut Graph, keep: usize) -> bool {
    let mut changed = false;
    for node in graph.nodes.values_mut() {
        let Some((head, _)) = member_split(&node.label) else {
            continue;
        };
        let mut out: Vec<String> = head.iter().map(|line| line.to_string()).collect();
        let mut section: Vec<&str> = Vec::new();
        let flush = |section: &mut Vec<&str>, out: &mut Vec<String>| {
            out.extend(section.iter().take(keep).map(|line| line.to_string()));
            if section.len() > keep {
                out.push("…".to_string());
            }
            section.clear();
        };
        for line in node.label.split('\n').skip(head.len()) {
            if line == Node::SEPARATOR {
                flush(&mut section, &mut out);
                out.push(line.to_string());
            } else {
                section.push(line);
            }
        }
        flush(&mut section, &mut out);
        changed |= replace_label(&mut node.label, out);
    }
    changed
}

/// Wrap class member lines (after the first separator) to `cells`,
/// indenting continuation lines by two spaces.
pub(crate) fn wrap_class_members(graph: &mut Graph, cells: usize) -> bool {
    let mut changed = false;
    for node in graph.nodes.values_mut() {
        let Some((head, members)) = member_split(&node.label) else {
            continue;
        };
        let mut out: Vec<String> = head.iter().map(|line| line.to_string()).collect();
        for line in members {
            if line == Node::SEPARATOR {
                out.push(line.to_string());
                continue;
            }
            for (i, wrapped) in wrap_words_to_cells(line, cells).into_iter().enumerate() {
                out.push(if i == 0 {
                    wrapped
                } else {
                    format!("  {wrapped}")
                });
            }
        }
        changed |= replace_label(&mut node.label, out);
    }
    changed
}

/// Wrap node label lines wider than `cells`, skipping nodes with class
/// member sections.
pub(crate) fn wrap_node_labels(graph: &mut Graph, cells: usize) -> bool {
    let mut changed = false;
    for node in graph.nodes.values_mut() {
        if has_member_sections(node) {
            continue;
        }
        let lines = node
            .label
            .split('\n')
            .flat_map(|line| {
                if display_width(line) > cells {
                    wrap_words_to_cells(line, cells)
                } else {
                    vec![line.to_string()]
                }
            })
            .collect();
        changed |= replace_label(&mut node.label, lines);
    }
    changed
}

/// Set `wrapped_label_lines` on edges whose label has a line wider than
/// `cells`. Other edges keep `wrapped_label_lines == None` so the engine's
/// pixel wrap pass treats them exactly as in the as-authored render.
pub(crate) fn wrap_edge_labels(graph: &mut Graph, cells: usize) -> bool {
    let mut changed = false;
    for edge in &mut graph.edges {
        let Some(label) = edge.label.as_deref() else {
            continue;
        };
        if !label.split('\n').any(|line| display_width(line) > cells) {
            continue;
        }
        edge.wrapped_label_lines = Some(
            label
                .split('\n')
                .flat_map(|line| wrap_words_to_cells(line, cells))
                .collect(),
        );
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Subgraph};

    #[test]
    fn fit_levers_wrap_words_greedily_without_splitting() {
        assert_eq!(
            wrap_words_to_cells("load the config file", 10),
            vec!["load the", "config", "file"]
        );
        assert_eq!(
            wrap_words_to_cells("internationalization now", 8),
            vec!["internationalization", "now"]
        );
        assert_eq!(wrap_words_to_cells("", 8), vec![""]);
        assert_eq!(
            wrap_words_to_cells("日本語 テキスト 表示", 9),
            vec!["日本語", "テキスト", "表示"]
        );
    }

    #[test]
    fn fit_levers_truncate_keeps_prefix_and_ellipsis() {
        assert_eq!(truncate_to_cells("authentication", 8), "authent…");
        assert_eq!(truncate_to_cells("short", 8), "short");
        assert_eq!(truncate_to_cells("日本語テキスト", 6), "日本…");
    }

    fn graph_with_edges(labels: &[Option<&str>]) -> Graph {
        let mut graph = Graph::new(Direction::LeftRight);
        for (i, label) in labels.iter().enumerate() {
            let (from, to) = (format!("N{i}"), format!("N{}", i + 1));
            graph.add_node(Node::new(&from));
            graph.add_node(Node::new(&to));
            let mut edge = Edge::new(from, to);
            edge.label = label.map(str::to_string);
            graph.add_edge(edge);
        }
        graph
    }

    #[test]
    fn fit_levers_wrap_edge_labels_only_touches_wide_labels() {
        let mut graph =
            graph_with_edges(&[Some("deploy to the staging cluster"), Some("ok"), None]);
        assert!(wrap_edge_labels(&mut graph, 16));
        assert_eq!(
            graph.edges[0].wrapped_label_lines,
            Some(vec![
                "deploy to the".to_string(),
                "staging cluster".to_string()
            ])
        );
        assert_eq!(graph.edges[1].wrapped_label_lines, None);
        assert_eq!(graph.edges[2].wrapped_label_lines, None);

        let mut narrow = graph_with_edges(&[Some("ok"), None]);
        let before = narrow.edges.clone();
        assert!(!wrap_edge_labels(&mut narrow, 16));
        assert_eq!(narrow.edges, before);
    }

    fn class_node(id: &str, label: &str) -> Node {
        let mut node = Node::new(id);
        node.label = label.to_string();
        node
    }

    #[test]
    fn fit_levers_wrap_node_labels_skips_class_nodes() {
        let mut graph = Graph::new(Direction::TopDown);
        graph.add_node(class_node("A", "Validate the incoming request"));
        graph.add_node(class_node("C", "Account\n---\n+String owner name here"));
        assert!(wrap_node_labels(&mut graph, 12));
        assert_eq!(graph.nodes["A"].label, "Validate the\nincoming\nrequest");
        assert_eq!(
            graph.nodes["C"].label,
            "Account\n---\n+String owner name here"
        );
        assert!(!wrap_node_labels(&mut graph, 40));
    }

    #[test]
    fn fit_levers_wrap_class_members_indents_continuations() {
        let mut graph = Graph::new(Direction::TopDown);
        graph.add_node(class_node(
            "C",
            "Account Holder Name\n---\n+String owner name here\n---\n+deposit(amount)",
        ));
        assert!(wrap_class_members(&mut graph, 13));
        assert_eq!(
            graph.nodes["C"].label,
            "Account Holder Name\n---\n+String owner\n  name here\n---\n+deposit(amount)"
        );
    }

    #[test]
    fn fit_levers_truncate_labels_covers_nodes_and_edges() {
        let mut graph = graph_with_edges(&[Some("authentication")]);
        graph.add_node(class_node("C", "Authenticator\n---\n+authenticate()"));
        assert!(truncate_labels(&mut graph, 8));
        assert_eq!(graph.nodes["C"].label, "Authent…\n---\n+authen…");
        assert_eq!(graph.edges[0].label.as_deref(), Some("authent…"));
        assert!(!truncate_labels(&mut graph, 8));
    }

    #[test]
    fn fit_levers_elide_class_members_keeps_first_members() {
        let mut graph = Graph::new(Direction::TopDown);
        graph.add_node(class_node("C", "Shape\n---\n+a\n+b\n+c\n+d\n---\n+area()"));
        assert!(elide_class_members(&mut graph, 3));
        assert_eq!(
            graph.nodes["C"].label,
            "Shape\n---\n+a\n+b\n+c\n…\n---\n+area()"
        );
        assert!(!elide_class_members(&mut graph, 3));
    }

    #[test]
    fn fit_levers_transpose_swaps_axes() {
        assert_eq!(
            transpose_direction(Direction::TopDown),
            Direction::LeftRight
        );
        assert_eq!(
            transpose_direction(Direction::LeftRight),
            Direction::TopDown
        );
        assert_eq!(
            transpose_direction(Direction::BottomTop),
            Direction::RightLeft
        );
        assert_eq!(
            transpose_direction(Direction::RightLeft),
            Direction::BottomTop
        );
    }

    fn subgraph(id: &str, parent: Option<&str>, dir: Option<Direction>) -> Subgraph {
        Subgraph {
            id: id.to_string(),
            title: id.to_string(),
            parent: parent.map(str::to_string),
            dir,
            ..Subgraph::default()
        }
    }

    fn graph_with_subgraphs(root: Direction, subgraphs: Vec<Subgraph>) -> Graph {
        let mut graph = Graph::new(root);
        for sg in subgraphs {
            graph.subgraph_order.push(sg.id.clone());
            graph.subgraphs.insert(sg.id.clone(), sg);
        }
        graph
    }

    fn dir_of(graph: &Graph, id: &str) -> Option<Direction> {
        graph.subgraphs[id].dir
    }

    #[test]
    fn fit_levers_flip_rotates_overrides_and_clears_redundant() {
        let mut graph = graph_with_subgraphs(
            Direction::LeftRight,
            vec![
                subgraph("a", None, Some(Direction::LeftRight)),
                subgraph("b", None, Some(Direction::TopDown)),
                subgraph("c", None, None),
            ],
        );
        assert!(flip_graph_direction(&mut graph));
        assert_eq!(graph.direction, Direction::TopDown);
        assert_eq!(dir_of(&graph, "a"), None);
        assert_eq!(dir_of(&graph, "b"), Some(Direction::LeftRight));
        assert_eq!(dir_of(&graph, "c"), None);
    }

    #[test]
    fn fit_levers_flip_nested_overrides() {
        let mut graph = graph_with_subgraphs(
            Direction::TopDown,
            vec![
                subgraph("outer", None, Some(Direction::LeftRight)),
                subgraph("inner", Some("outer"), Some(Direction::TopDown)),
            ],
        );
        flip_graph_direction(&mut graph);
        assert_eq!(graph.direction, Direction::LeftRight);
        assert_eq!(dir_of(&graph, "outer"), Some(Direction::TopDown));
        assert_eq!(dir_of(&graph, "inner"), Some(Direction::LeftRight));
    }

    #[test]
    fn fit_levers_clear_redundant_uses_nearest_override() {
        let mut graph = graph_with_subgraphs(
            Direction::TopDown,
            vec![
                subgraph("outer", None, Some(Direction::LeftRight)),
                subgraph("inner", Some("outer"), Some(Direction::LeftRight)),
            ],
        );
        assert!(clear_redundant_subgraph_directions(&mut graph));
        assert_eq!(dir_of(&graph, "outer"), Some(Direction::LeftRight));
        assert_eq!(dir_of(&graph, "inner"), None);

        let mut inherited = graph_with_subgraphs(
            Direction::LeftRight,
            vec![
                subgraph("outer", None, None),
                subgraph("inner", Some("outer"), Some(Direction::LeftRight)),
            ],
        );
        assert!(clear_redundant_subgraph_directions(&mut inherited));
        assert_eq!(dir_of(&inherited, "inner"), None);
    }

    #[test]
    fn fit_levers_strip_horizontal_overrides_under_vertical_root() {
        let mut graph = graph_with_subgraphs(
            Direction::TopDown,
            vec![
                subgraph("a", None, Some(Direction::LeftRight)),
                subgraph("b", None, Some(Direction::BottomTop)),
                subgraph("child", Some("a"), Some(Direction::TopDown)),
            ],
        );
        assert!(strip_horizontal_subgraph_overrides(&mut graph));
        assert_eq!(dir_of(&graph, "a"), None);
        assert_eq!(dir_of(&graph, "b"), Some(Direction::BottomTop));
        assert_eq!(dir_of(&graph, "child"), None);

        let mut horizontal = graph_with_subgraphs(
            Direction::LeftRight,
            vec![subgraph("a", None, Some(Direction::RightLeft))],
        );
        assert!(!strip_horizontal_subgraph_overrides(&mut horizontal));
        assert_eq!(dir_of(&horizontal, "a"), Some(Direction::RightLeft));
    }
}
