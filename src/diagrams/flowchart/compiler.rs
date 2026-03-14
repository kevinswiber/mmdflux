//! Converts AST to graph data structures.

use std::collections::{HashMap, HashSet};

use crate::graph::style::NodeStyle;
use crate::graph::{Arrow, Direction, Edge, Graph, Node, Shape, Stroke, Subgraph};
use crate::mermaid::{
    ArrowHead, ConnectorSpec, Direction as ParseDirection, EdgeSpec, Flowchart, ShapeSpec,
    Statement, StrokeSpec, Vertex,
};

/// Build a Diagram from a parsed Flowchart.
pub fn compile_to_graph(flowchart: &Flowchart) -> Graph {
    let direction = convert_direction(flowchart.direction);
    let mut diagram = Graph::new(direction);
    let mut node_styles = HashMap::new();
    process_statements(&mut diagram, &flowchart.statements, None, &mut node_styles);
    resolve_subgraph_edges(&mut diagram);
    diagram
}

fn process_statements(
    diagram: &mut Graph,
    statements: &[Statement],
    parent_subgraph: Option<&str>,
    node_styles: &mut HashMap<String, NodeStyle>,
) {
    for statement in statements {
        match statement {
            Statement::Vertex(vertex) => {
                add_vertex_to_diagram(
                    diagram,
                    vertex,
                    parent_subgraph,
                    node_styles.get(&vertex.id),
                );
            }
            Statement::Edge(edge_spec) => {
                add_vertex_to_diagram(
                    diagram,
                    &edge_spec.from,
                    parent_subgraph,
                    node_styles.get(&edge_spec.from.id),
                );
                add_vertex_to_diagram(
                    diagram,
                    &edge_spec.to,
                    parent_subgraph,
                    node_styles.get(&edge_spec.to.id),
                );
                let edge = convert_edge(edge_spec);
                diagram.add_edge(edge);
            }
            Statement::Subgraph(sg_spec) => {
                process_statements(diagram, &sg_spec.statements, Some(&sg_spec.id), node_styles);
                let node_ids = collect_node_ids(&sg_spec.statements);
                diagram.subgraphs.insert(
                    sg_spec.id.clone(),
                    Subgraph {
                        id: sg_spec.id.clone(),
                        title: sg_spec.title.clone(),
                        nodes: node_ids,
                        parent: parent_subgraph.map(|s| s.to_string()),
                        dir: sg_spec.dir.map(convert_direction),
                    },
                );
                diagram.subgraph_order.push(sg_spec.id.clone());
            }
            Statement::NodeStyle(style_stmt) => {
                merge_node_style(diagram, node_styles, &style_stmt.node_id, &style_stmt.style);
            }
        }
    }
}

fn convert_direction(dir: ParseDirection) -> Direction {
    match dir {
        ParseDirection::TopDown => Direction::TopDown,
        ParseDirection::BottomTop => Direction::BottomTop,
        ParseDirection::LeftRight => Direction::LeftRight,
        ParseDirection::RightLeft => Direction::RightLeft,
    }
}

fn add_vertex_to_diagram(
    diagram: &mut Graph,
    vertex: &Vertex,
    parent: Option<&str>,
    style: Option<&NodeStyle>,
) {
    if let Some(existing) = diagram.nodes.get_mut(&vertex.id) {
        // Update existing node if this vertex has more specific shape info
        if let Some(shape_spec) = &vertex.shape
            && existing.label == existing.id
        {
            let shape = convert_shape(shape_spec);
            existing.label = normalize_shape_label(&vertex.id, shape_spec, shape);
            existing.shape = shape;
        }
        // Set parent if provided and not already set
        if parent.is_some() && existing.parent.is_none() {
            existing.parent = parent.map(|s| s.to_string());
        }
        if let Some(style) = style {
            existing.style = style.clone();
        }
    } else {
        let mut node = convert_vertex(vertex);
        node.parent = parent.map(|s| s.to_string());
        if let Some(style) = style {
            node.style = style.clone();
        }
        diagram.add_node(node);
    }
}

fn merge_node_style(
    diagram: &mut Graph,
    node_styles: &mut HashMap<String, NodeStyle>,
    node_id: &str,
    style: &NodeStyle,
) {
    let merged_style = node_styles
        .entry(node_id.to_string())
        .and_modify(|existing| *existing = existing.merge(style))
        .or_insert_with(|| style.clone())
        .clone();

    if let Some(node) = diagram.nodes.get_mut(node_id) {
        node.style = merged_style;
    }
}

/// Replace edge endpoints that reference subgraph IDs with representative child nodes.
fn resolve_subgraph_edges(diagram: &mut Graph) {
    let mut resolved_edges = Vec::new();

    for edge in &diagram.edges {
        let (from, from_subgraph) = if diagram.is_subgraph(&edge.from) {
            match find_subgraph_sink(diagram, &edge.from) {
                Some(child) => (child, Some(edge.from.clone())),
                None => continue,
            }
        } else {
            (edge.from.clone(), None)
        };

        let (to, to_subgraph) = if diagram.is_subgraph(&edge.to) {
            match find_non_cluster_child(diagram, &edge.to) {
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

    diagram.edges = resolved_edges;

    // Remove spurious regular nodes created for subgraph IDs during edge parsing
    let subgraph_ids: Vec<String> = diagram.subgraphs.keys().cloned().collect();
    for sg_id in &subgraph_ids {
        if let Some(node) = diagram.nodes.get(sg_id)
            && node.parent.is_none()
            && node.label == *sg_id
        {
            diagram.nodes.remove(sg_id);
        }
    }
}

/// Find a non-compound child node within a subgraph.
///
/// Walks the subgraph's children, returning the first leaf node that is not
/// itself a subgraph. Returns `None` for empty subgraphs or nonexistent IDs.
///
/// This is the Rust equivalent of Mermaid's `findNonClusterChild()`.
pub fn find_non_cluster_child(diagram: &Graph, subgraph_id: &str) -> Option<String> {
    let sg = diagram.subgraphs.get(subgraph_id)?;
    sg.nodes.iter().find(|id| !diagram.is_subgraph(id)).cloned()
}

/// Find a sink node in a subgraph — a non-cluster child that has no successors
/// within the subgraph.  Used when the subgraph is the **source** of an edge
/// so the target ends up ranked after the entire subgraph, not beside internal
/// nodes.  Falls back to `find_non_cluster_child` if every node has a successor.
fn find_subgraph_sink(diagram: &Graph, subgraph_id: &str) -> Option<String> {
    let sg = diagram.subgraphs.get(subgraph_id)?;
    let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
    let non_cluster: Vec<&str> = sg
        .nodes
        .iter()
        .filter(|id| !diagram.is_subgraph(id))
        .map(|s| s.as_str())
        .collect();

    // A sink has no outgoing edges to other nodes within the subgraph.
    let sink = non_cluster.iter().find(|&&node| {
        !diagram
            .edges
            .iter()
            .any(|e| e.from == node && sg_node_set.contains(e.to.as_str()) && e.to != node)
    });

    sink.map(|s| s.to_string())
        .or_else(|| find_non_cluster_child(diagram, subgraph_id))
}

fn collect_node_ids(statements: &[Statement]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    collect_node_ids_inner(statements, &mut result, &mut seen);
    result
}

fn collect_node_ids_inner(
    statements: &[Statement],
    result: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    for stmt in statements {
        match stmt {
            Statement::Vertex(v) => {
                if seen.insert(v.id.clone()) {
                    result.push(v.id.clone());
                }
            }
            Statement::Edge(e) => {
                if seen.insert(e.from.id.clone()) {
                    result.push(e.from.id.clone());
                }
                if seen.insert(e.to.id.clone()) {
                    result.push(e.to.id.clone());
                }
            }
            Statement::Subgraph(sg) => {
                collect_node_ids_inner(&sg.statements, result, seen);
            }
            Statement::NodeStyle(_) => {}
        }
    }
}

fn convert_vertex(vertex: &Vertex) -> Node {
    match &vertex.shape {
        Some(shape_spec) => {
            let shape = convert_shape(shape_spec);
            let label = normalize_shape_label(&vertex.id, shape_spec, shape);
            Node::new(&vertex.id).with_label(label).with_shape(shape)
        }
        None => Node::new(&vertex.id),
    }
}

fn normalize_shape_label(id: &str, shape_spec: &ShapeSpec, shape: Shape) -> String {
    let text = shape_spec.text();
    if text.is_empty()
        && !matches!(
            shape,
            Shape::SmallCircle | Shape::FramedCircle | Shape::CrossedCircle | Shape::ForkJoin
        )
    {
        id.to_string()
    } else {
        normalize_br_tags(text)
    }
}

/// Replace HTML `<br>` tag variants with newline characters.
///
/// Handles `<br>`, `<br/>`, `<br />`, and case-insensitive variants,
/// matching the Mermaid convention for line breaks in labels.
fn normalize_br_tags(text: &str) -> String {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'<' {
            // Try to match <br>, <br/>, <br />, etc.
            let start = i;
            i += 1;
            // Skip optional whitespace after <
            while i < len && bytes[i] == b' ' {
                i += 1;
            }
            // Match 'b'/'B'
            if i < len && bytes[i].eq_ignore_ascii_case(&b'b') {
                i += 1;
                // Match 'r'/'R'
                if i < len && bytes[i].eq_ignore_ascii_case(&b'r') {
                    i += 1;
                    // Skip optional whitespace
                    while i < len && bytes[i] == b' ' {
                        i += 1;
                    }
                    // Skip optional '/'
                    if i < len && bytes[i] == b'/' {
                        i += 1;
                    }
                    // Skip optional whitespace
                    while i < len && bytes[i] == b' ' {
                        i += 1;
                    }
                    // Match '>'
                    if i < len && bytes[i] == b'>' {
                        i += 1;
                        result.push('\n');
                        continue;
                    }
                }
            }
            // Not a <br> tag — emit the characters we skipped
            for &b in &bytes[start..i] {
                result.push(b as char);
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

fn convert_shape(shape_spec: &ShapeSpec) -> Shape {
    match shape_spec {
        ShapeSpec::Rectangle(_) => Shape::Rectangle,
        ShapeSpec::Round(_) => Shape::Round,
        ShapeSpec::Diamond(_) => Shape::Diamond,
        ShapeSpec::Stadium(_) => Shape::Stadium,
        ShapeSpec::Subroutine(_) => Shape::Subroutine,
        ShapeSpec::Cylinder(_) => Shape::Cylinder,
        ShapeSpec::Document(_) => Shape::Document,
        ShapeSpec::Documents(_) => Shape::Documents,
        ShapeSpec::TaggedDocument(_) => Shape::TaggedDocument,
        ShapeSpec::Card(_) => Shape::Card,
        ShapeSpec::TaggedRect(_) => Shape::TaggedRect,
        ShapeSpec::Circle(_) => Shape::Circle,
        ShapeSpec::DoubleCircle(_) => Shape::DoubleCircle,
        ShapeSpec::Hexagon(_) => Shape::Hexagon,
        ShapeSpec::Parallelogram(_) => Shape::Parallelogram,
        ShapeSpec::InvParallelogram(_) => Shape::InvParallelogram,
        ShapeSpec::ManualInput(_) => Shape::ManualInput,
        ShapeSpec::Asymmetric(_) => Shape::Asymmetric,
        ShapeSpec::Trapezoid(_) => Shape::Trapezoid,
        ShapeSpec::InvTrapezoid(_) => Shape::InvTrapezoid,
        ShapeSpec::SmallCircle(_) => Shape::SmallCircle,
        ShapeSpec::FramedCircle(_) => Shape::FramedCircle,
        ShapeSpec::CrossedCircle(_) => Shape::CrossedCircle,
        ShapeSpec::TextBlock(_) => Shape::TextBlock,
        ShapeSpec::ForkJoin(_) => Shape::ForkJoin,
    }
}

fn convert_edge(edge_spec: &EdgeSpec) -> Edge {
    let (stroke, mut arrow_start, mut arrow_end, label) = convert_connector(&edge_spec.connector);
    let no_arrows =
        edge_spec.connector.left == ArrowHead::None && edge_spec.connector.right == ArrowHead::None;
    // Parser length is style-token length, not normalized minlen.
    // For solid/thick open links, baseline syntax has one extra token
    // ("---", "==="), so normalize it back to minlen=1.
    let minlen = if no_arrows
        && matches!(
            edge_spec.connector.stroke,
            StrokeSpec::Solid | StrokeSpec::Thick
        ) {
        (edge_spec.connector.length.saturating_sub(1)).max(1) as i32
    } else {
        edge_spec.connector.length as i32
    };

    let (from, to) = if arrow_start != Arrow::None && arrow_end == Arrow::None {
        // If only the left arrow is present, treat it as a reversed edge.
        std::mem::swap(&mut arrow_start, &mut arrow_end);
        (edge_spec.to.id.clone(), edge_spec.from.id.clone())
    } else {
        (edge_spec.from.id.clone(), edge_spec.to.id.clone())
    };

    let mut edge = Edge::new(from, to)
        .with_stroke(stroke)
        .with_arrows(arrow_start, arrow_end)
        .with_minlen(minlen);
    edge.label = label;
    edge
}

fn convert_connector(connector: &ConnectorSpec) -> (Stroke, Arrow, Arrow, Option<String>) {
    let stroke = match connector.stroke {
        StrokeSpec::Solid => Stroke::Solid,
        StrokeSpec::Dotted => Stroke::Dotted,
        StrokeSpec::Thick => Stroke::Thick,
        StrokeSpec::Invisible => Stroke::Invisible,
    };

    let arrow_start = map_arrow_head(connector.left);
    let arrow_end = map_arrow_head(connector.right);

    (
        stroke,
        arrow_start,
        arrow_end,
        connector.label.as_deref().map(normalize_br_tags),
    )
}

fn map_arrow_head(head: ArrowHead) -> Arrow {
    match head {
        ArrowHead::None => Arrow::None,
        ArrowHead::Normal => Arrow::Normal,
        ArrowHead::Cross => Arrow::Cross,
        ArrowHead::Circle => Arrow::Circle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::parse_flowchart;

    #[test]
    fn test_build_simple_diagram() {
        let flowchart = parse_flowchart("graph TD\nA --> B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.direction, Direction::TopDown);
        assert_eq!(diagram.nodes.len(), 2);
        assert_eq!(diagram.edges.len(), 1);

        assert!(diagram.nodes.contains_key("A"));
        assert!(diagram.nodes.contains_key("B"));
    }

    #[test]
    fn test_build_diagram_with_shapes() {
        let flowchart = parse_flowchart("graph LR\nA[Start] --> B{Decision}\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.direction, Direction::LeftRight);

        let node_a = diagram.get_node("A").unwrap();
        assert_eq!(node_a.label, "Start");
        assert_eq!(node_a.shape, Shape::Rectangle);

        let node_b = diagram.get_node("B").unwrap();
        assert_eq!(node_b.label, "Decision");
        assert_eq!(node_b.shape, Shape::Diamond);
    }

    #[test]
    fn test_build_diagram_with_edge_label() {
        let flowchart = parse_flowchart("graph TD\nA -->|yes| B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].label, Some("yes".to_string()));
    }

    #[test]
    fn test_build_diagram_deduplicates_nodes() {
        let flowchart = parse_flowchart("graph TD\nA --> B\nB --> C\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        // B appears in both edges but should only be one node
        assert_eq!(diagram.nodes.len(), 3);
        assert_eq!(diagram.edges.len(), 2);
    }

    #[test]
    fn test_build_diagram_node_update() {
        // First edge has A without shape, then A[Start] appears
        let flowchart = parse_flowchart("graph TD\nA --> B\nA[Start] --> C\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        let node_a = diagram.get_node("A").unwrap();
        // Should have the shape info from the second occurrence
        assert_eq!(node_a.label, "Start");
        assert_eq!(node_a.shape, Shape::Rectangle);
    }

    #[test]
    fn test_build_diagram_merges_style_onto_existing_node() {
        let input = "graph TD\nA[Alpha]\nstyle A fill:#ffeeaa,stroke:#333,color:#111\n";
        let chart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&chart);
        let node = diagram.get_node("A").unwrap();

        assert_eq!(node.style.fill.as_ref().unwrap().raw(), "#ffeeaa");
        assert_eq!(node.style.stroke.as_ref().unwrap().raw(), "#333");
        assert_eq!(node.style.color.as_ref().unwrap().raw(), "#111");
    }

    #[test]
    fn style_before_node_definition_is_applied_after_build() {
        let input = "graph TD\nstyle A fill:#ffeeaa\nA[Alpha]\n";
        let chart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&chart);

        assert_eq!(
            diagram
                .get_node("A")
                .unwrap()
                .style
                .fill
                .as_ref()
                .unwrap()
                .raw(),
            "#ffeeaa"
        );
    }

    #[test]
    fn repeated_style_statements_merge_by_property() {
        let input = "graph TD\nA[Alpha]\nstyle A fill:#ffeeaa,stroke:#333\nstyle A color:#111,stroke:#555\n";
        let chart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&chart);
        let style = &diagram.get_node("A").unwrap().style;

        assert_eq!(style.fill.as_ref().unwrap().raw(), "#ffeeaa");
        assert_eq!(style.stroke.as_ref().unwrap().raw(), "#555");
        assert_eq!(style.color.as_ref().unwrap().raw(), "#111");
    }

    #[test]
    fn style_after_implicit_node_from_edge_is_applied() {
        let input = "graph TD\nA --> B\nstyle A stroke:#333\n";
        let chart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&chart);

        assert_eq!(
            diagram
                .get_node("A")
                .unwrap()
                .style
                .stroke
                .as_ref()
                .unwrap()
                .raw(),
            "#333"
        );
    }

    #[test]
    fn test_build_diagram_edge_strokes() {
        let flowchart = parse_flowchart("graph TD\nA --> B\nB -.-> C\nC ==> D\nD --- E\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.edges[0].stroke, Stroke::Solid);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::Normal);

        assert_eq!(diagram.edges[1].stroke, Stroke::Dotted);
        assert_eq!(diagram.edges[1].arrow_end, Arrow::Normal);

        assert_eq!(diagram.edges[2].stroke, Stroke::Thick);
        assert_eq!(diagram.edges[2].arrow_end, Arrow::Normal);

        assert_eq!(diagram.edges[3].stroke, Stroke::Solid);
        assert_eq!(diagram.edges[3].arrow_end, Arrow::None);
    }

    #[test]
    fn test_build_diagram_from_chain() {
        let flowchart = parse_flowchart("graph TD\nA --> B --> C --> D\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.nodes.len(), 4);
        assert_eq!(diagram.edges.len(), 3);
    }

    #[test]
    fn test_build_diagram_from_ampersand() {
        let flowchart = parse_flowchart("graph TD\nA & B --> C\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.nodes.len(), 3);
        assert_eq!(diagram.edges.len(), 2);
    }

    #[test]
    fn test_nested_subgraph_outer_contains_inner_nodes() {
        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert!(diagram.subgraphs["outer"].nodes.contains(&"A".to_string()));
        assert!(diagram.subgraphs["outer"].nodes.contains(&"B".to_string()));
        assert!(diagram.subgraphs["inner"].nodes.contains(&"A".to_string()));
        assert!(diagram.subgraphs["inner"].nodes.contains(&"B".to_string()));
    }

    #[test]
    fn test_nested_subgraph_parent_set() {
        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert_eq!(diagram.subgraphs["inner"].parent, Some("outer".to_string()));
        assert_eq!(diagram.subgraphs["outer"].parent, None);
    }

    #[test]
    fn test_build_diagram_with_subgraph() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert!(diagram.has_subgraphs());
        assert!(diagram.subgraphs.contains_key("sg1"));
        let sg = &diagram.subgraphs["sg1"];
        assert_eq!(sg.title, "Group");
        assert!(sg.nodes.contains(&"A".to_string()));
        assert!(sg.nodes.contains(&"B".to_string()));
    }

    #[test]
    fn test_build_diagram_node_parent_set() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.nodes["A"].parent, Some("sg1".to_string()));
        assert_eq!(diagram.nodes["B"].parent, Some("sg1".to_string()));
        assert_eq!(diagram.nodes["C"].parent, None);
    }

    #[test]
    fn test_build_diagram_subgraph_edges_cross_boundary() {
        let input = "graph TD\nsubgraph sg1[Group]\nA\nB\nend\nA --> C\nC --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.edges.len(), 2);
        assert_eq!(diagram.nodes["A"].parent, Some("sg1".to_string()));
        assert_eq!(diagram.nodes["C"].parent, None);
    }

    #[test]
    fn test_build_diagram_invisible_edge() {
        let flowchart = parse_flowchart("graph TD\nA ~~~ B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].stroke, Stroke::Invisible);
        assert_eq!(diagram.edges[0].arrow_start, Arrow::None);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::None);
        assert_eq!(diagram.edges[0].minlen, 1);
    }

    #[test]
    fn test_build_diagram_variable_length_edge_sets_minlen() {
        let flowchart = parse_flowchart("graph TD\nA ----> B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert_eq!(diagram.edges.len(), 1);
        assert!(diagram.edges[0].minlen > 1);
    }

    #[test]
    fn test_build_diagram_open_solid_edge_default_minlen() {
        let flowchart = parse_flowchart("graph TD\nA --- B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert_eq!(diagram.edges.len(), 1);
        assert_eq!(diagram.edges[0].minlen, 1);
    }

    #[test]
    fn test_cross_arrow_preserved() {
        let fc = parse_flowchart("graph TD\nA --x B\n").unwrap();
        let diagram = compile_to_graph(&fc);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::Cross);
    }

    #[test]
    fn test_circle_arrow_preserved() {
        let fc = parse_flowchart("graph TD\nA --o B\n").unwrap();
        let diagram = compile_to_graph(&fc);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::Circle);
    }

    #[test]
    fn test_bidirectional_cross_arrows() {
        let fc = parse_flowchart("graph TD\nA x--x B\n").unwrap();
        let diagram = compile_to_graph(&fc);
        assert_eq!(diagram.edges[0].arrow_start, Arrow::Cross);
        assert_eq!(diagram.edges[0].arrow_end, Arrow::Cross);
    }

    #[test]
    fn test_build_diagram_multi_edges() {
        let flowchart = parse_flowchart("graph TD\nA -->|first| B\nA -->|second| B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.nodes.len(), 2);
        assert_eq!(
            diagram.edges.len(),
            2,
            "Both edges between A and B preserved"
        );
        assert_eq!(diagram.edges[0].label, Some("first".to_string()));
        assert_eq!(diagram.edges[1].label, Some("second".to_string()));
    }

    #[test]
    fn test_find_non_cluster_child_simple() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let child = find_non_cluster_child(&diagram, "sg1");
        assert!(child.is_some());
        let child_id = child.unwrap();
        assert!(child_id == "A" || child_id == "B");
    }

    #[test]
    fn test_find_non_cluster_child_nested() {
        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let child = find_non_cluster_child(&diagram, "outer");
        assert!(child.is_some());
        let child_id = child.unwrap();
        assert!(child_id == "A" || child_id == "B");
    }

    #[test]
    fn test_find_non_cluster_child_empty_subgraph() {
        let input = "graph TD\nsubgraph sg1[Empty]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let child = find_non_cluster_child(&diagram, "sg1");
        assert!(child.is_none());
    }

    #[test]
    fn test_find_non_cluster_child_nonexistent() {
        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let child = find_non_cluster_child(&diagram, "no_such_sg");
        assert!(child.is_none());
    }

    #[test]
    fn test_build_diagram_subgraph_dir_propagated() {
        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::LeftRight));
    }

    #[test]
    fn test_build_diagram_subgraph_no_dir() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.subgraphs["sg1"].dir, None);
    }

    #[test]
    fn test_edge_to_subgraph_resolved() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> sg1\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let c_edges: Vec<_> = diagram.edges.iter().filter(|e| e.from == "C").collect();
        assert_eq!(c_edges.len(), 1);
        assert!(
            c_edges[0].to == "A" || c_edges[0].to == "B",
            "Edge to subgraph should resolve to child, got: {}",
            c_edges[0].to
        );
    }

    #[test]
    fn test_edge_from_subgraph_resolved() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nsg1 --> C\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let c_edges: Vec<_> = diagram.edges.iter().filter(|e| e.to == "C").collect();
        assert_eq!(c_edges.len(), 1);
        assert!(
            c_edges[0].from == "A" || c_edges[0].from == "B",
            "Edge from subgraph should resolve to child, got: {}",
            c_edges[0].from
        );
    }

    #[test]
    fn test_edge_between_subgraphs_resolved() {
        let input = "graph TD\nsubgraph sg1[G1]\nA\nend\nsubgraph sg2[G2]\nB\nend\nsg1 --> sg2\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let edges: Vec<_> = diagram.edges.iter().collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, "A");
        assert_eq!(edges[0].to, "B");
    }

    #[test]
    fn test_edge_to_subgraph_no_duplicate_node() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> sg1\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert!(
            !diagram.nodes.contains_key("sg1") || diagram.subgraphs.contains_key("sg1"),
            "sg1 should be a subgraph, not a regular node"
        );
    }

    #[test]
    fn test_edge_to_empty_subgraph_dropped() {
        let input = "graph TD\nsubgraph sg1[Empty]\nend\nC --> sg1\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let c_edges: Vec<_> = diagram.edges.iter().filter(|e| e.from == "C").collect();
        assert_eq!(c_edges.len(), 0, "Edge to empty subgraph should be dropped");
    }

    #[test]
    fn test_build_diagram_shape_config_label_defaults() {
        let input = "graph TD\nA@{shape: doc}\nJ@{shape: sm-circ}\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let node_a = diagram.get_node("A").unwrap();
        assert_eq!(node_a.shape, Shape::Document);
        assert_eq!(node_a.label, "A");

        let node_j = diagram.get_node("J").unwrap();
        assert_eq!(node_j.shape, Shape::SmallCircle);
        assert_eq!(node_j.label, "");
    }

    // === normalize_br_tags tests ===

    #[test]
    fn test_br_tag_lowercase() {
        assert_eq!(normalize_br_tags("hello<br>world"), "hello\nworld");
    }

    #[test]
    fn test_br_tag_uppercase() {
        assert_eq!(normalize_br_tags("hello<BR>world"), "hello\nworld");
    }

    #[test]
    fn test_br_tag_self_closing() {
        assert_eq!(normalize_br_tags("hello<br/>world"), "hello\nworld");
    }

    #[test]
    fn test_br_tag_self_closing_with_space() {
        assert_eq!(normalize_br_tags("hello<br />world"), "hello\nworld");
    }

    #[test]
    fn test_br_tag_mixed_case() {
        assert_eq!(normalize_br_tags("hello<Br>world"), "hello\nworld");
        assert_eq!(normalize_br_tags("hello<bR/>world"), "hello\nworld");
    }

    #[test]
    fn test_br_tag_multiple() {
        assert_eq!(normalize_br_tags("a<br>b<br/>c<BR />d"), "a\nb\nc\nd");
    }

    #[test]
    fn test_br_tag_no_tags() {
        assert_eq!(normalize_br_tags("hello world"), "hello world");
    }

    #[test]
    fn test_br_tag_empty_string() {
        assert_eq!(normalize_br_tags(""), "");
    }

    #[test]
    fn test_br_tag_non_br_html_preserved() {
        assert_eq!(normalize_br_tags("a<b>bold</b>c"), "a<b>bold</b>c");
    }

    #[test]
    fn test_br_tag_incomplete_tag_preserved() {
        assert_eq!(normalize_br_tags("a<br"), "a<br");
    }

    #[test]
    fn test_node_label_with_br_tag() {
        let flowchart = parse_flowchart("graph TD\nA[Hello<br>World]\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        let node = diagram.get_node("A").unwrap();
        assert_eq!(node.label, "Hello\nWorld");
    }

    #[test]
    fn test_edge_label_with_br_tag() {
        let flowchart = parse_flowchart("graph TD\nA -->|yes<br>no| B\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        assert_eq!(diagram.edges[0].label, Some("yes\nno".to_string()));
    }

    mod owner_local_fixture_regressions {
        use super::*;

        #[test]
        fn simple_parses_correctly() {
            let diagram = compile_fixture_diagram("simple.mmd");

            assert_eq!(diagram.direction, Direction::TopDown);
            assert_eq!(diagram.nodes.len(), 2);
            assert_eq!(diagram.edges.len(), 1);

            assert!(diagram.nodes.contains_key("A"));
            assert!(diagram.nodes.contains_key("B"));
            assert_eq!(diagram.nodes["A"].label, "Start");
            assert_eq!(diagram.nodes["B"].label, "End");
        }

        #[test]
        fn decision_parses_correctly() {
            let diagram = compile_fixture_diagram("decision.mmd");

            assert_eq!(diagram.nodes.len(), 4);
            assert_eq!(diagram.edges.len(), 4);
            assert_eq!(diagram.nodes["B"].shape, Shape::Diamond);
            assert_eq!(diagram.nodes["B"].label, "Is it working?");
        }

        #[test]
        fn shapes_parses_correctly() {
            let diagram = compile_fixture_diagram("shapes.mmd");

            assert_eq!(diagram.nodes["rect"].shape, Shape::Rectangle);
            assert_eq!(diagram.nodes["round"].shape, Shape::Round);
            assert_eq!(diagram.nodes["diamond"].shape, Shape::Diamond);
        }

        #[test]
        fn shape_keywords_parse_junctions_and_specials() {
            let diagram = compile_fixture_diagram("shapes_junction.mmd");
            assert_eq!(diagram.nodes["j1"].shape, Shape::SmallCircle);
            assert_eq!(diagram.nodes["j2"].shape, Shape::FramedCircle);
            assert_eq!(diagram.nodes["j3"].shape, Shape::CrossedCircle);

            let diagram = compile_fixture_diagram("shapes_special.mmd");
            assert_eq!(diagram.nodes["fork"].shape, Shape::ForkJoin);
            assert_eq!(diagram.nodes["note"].shape, Shape::TextBlock);
        }

        #[test]
        fn shape_keywords_parse_degenerate_fallbacks() {
            let diagram = compile_fixture_diagram("shapes_degenerate.mmd");
            for id in [
                "cloud",
                "bolt",
                "bang",
                "icon",
                "hourglass",
                "tri",
                "flip",
                "notch",
            ] {
                assert_eq!(diagram.nodes[id].shape, Shape::Rectangle);
            }
        }

        #[test]
        fn left_right_direction() {
            let diagram = compile_fixture_diagram("left_right.mmd");
            assert_eq!(diagram.direction, Direction::LeftRight);
        }

        #[test]
        fn bottom_top_direction() {
            let diagram = compile_fixture_diagram("bottom_top.mmd");
            assert_eq!(diagram.direction, Direction::BottomTop);
        }

        #[test]
        fn right_left_direction() {
            let diagram = compile_fixture_diagram("right_left.mmd");
            assert_eq!(diagram.direction, Direction::RightLeft);
        }

        #[test]
        fn chain_creates_correct_edges() {
            let diagram = compile_fixture_diagram("chain.mmd");
            assert_eq!(diagram.nodes.len(), 4);
            assert_eq!(diagram.edges.len(), 3);
        }

        #[test]
        fn ampersand_expands_to_multiple_edges() {
            let diagram = compile_fixture_diagram("ampersand.mmd");
            assert_eq!(diagram.nodes.len(), 5);
            assert_eq!(diagram.edges.len(), 4);
        }

        #[test]
        fn labeled_edges_parsed() {
            let diagram = compile_fixture_diagram("labeled_edges.mmd");
            let edges_with_labels = diagram
                .edges
                .iter()
                .filter(|edge| edge.label.is_some())
                .count();
            assert!(edges_with_labels > 0, "Should have labeled edges");
        }

        #[test]
        fn inline_edge_labels_parsed() {
            let diagram = compile_fixture_diagram("inline_edge_labels.mmd");

            assert_eq!(diagram.edges.len(), 4);
            assert_eq!(diagram.edges[0].label.as_deref(), Some("yes"));
            assert_eq!(diagram.edges[1].label.as_deref(), Some("retry"));
            assert_eq!(diagram.edges[2].label.as_deref(), Some("final step"));
            assert_eq!(diagram.edges[3].label.as_deref(), Some("no"));

            assert!(!diagram.nodes.contains_key("yes"));
            assert!(!diagram.nodes.contains_key("retry"));
            assert!(!diagram.nodes.contains_key("no"));
        }

        #[test]
        fn inline_label_flowchart_parsed() {
            let diagram = compile_fixture_diagram("inline_label_flowchart.mmd");

            let mut counts: HashMap<&str, usize> = HashMap::new();
            for label in diagram
                .edges
                .iter()
                .filter_map(|edge| edge.label.as_deref())
            {
                *counts.entry(label).or_insert(0) += 1;
            }

            assert_eq!(counts.get("no"), Some(&2));
            assert_eq!(counts.get("yes"), Some(&2));
            assert_eq!(counts.get("sync"), Some(&1));
            assert_eq!(counts.get("async"), Some(&1));
            assert_eq!(counts.get("hit"), Some(&1));
            assert_eq!(counts.get("miss"), Some(&1));
            assert_eq!(counts.get("warn"), Some(&1));
            assert_eq!(counts.values().sum::<usize>(), 9);
        }

        #[test]
        fn complex_parses_all_features() {
            let diagram = compile_fixture_diagram("complex.mmd");
            assert!(diagram.nodes.len() >= 9);
            assert!(diagram.edges.len() >= 10);
        }

        #[test]
        fn test_parse_simple_subgraph_fixture() {
            let diagram = compile_fixture_diagram("simple_subgraph.mmd");

            assert!(diagram.has_subgraphs());
            assert!(diagram.subgraphs.contains_key("sg1"));
            assert_eq!(diagram.subgraphs["sg1"].title, "Process");
            assert!(diagram.subgraphs["sg1"].nodes.contains(&"A".to_string()));
            assert!(diagram.subgraphs["sg1"].nodes.contains(&"B".to_string()));
        }

        #[test]
        fn test_parse_subgraph_edges_fixture() {
            let diagram = compile_fixture_diagram("subgraph_edges.mmd");

            assert_eq!(diagram.subgraphs.len(), 2);
            assert!(diagram.subgraphs.contains_key("sg1"));
            assert!(diagram.subgraphs.contains_key("sg2"));
            assert!(
                diagram
                    .edges
                    .iter()
                    .any(|edge| edge.from == "A" && edge.to == "C")
            );
            assert!(
                diagram
                    .edges
                    .iter()
                    .any(|edge| edge.from == "B" && edge.to == "D")
            );
        }

        #[test]
        fn test_parse_multi_subgraph_fixture() {
            let diagram = compile_fixture_diagram("multi_subgraph.mmd");

            assert_eq!(diagram.subgraphs.len(), 2);
            assert!(diagram.subgraphs.contains_key("sg1"));
            assert!(diagram.subgraphs.contains_key("sg2"));
            assert_eq!(diagram.subgraphs["sg1"].title, "Frontend");
            assert_eq!(diagram.subgraphs["sg2"].title, "Backend");
            assert!(
                diagram
                    .edges
                    .iter()
                    .any(|edge| edge.from == "B" && edge.to == "C")
            );
        }
    }

    fn compile_fixture_diagram(name: &str) -> Graph {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join(name);
        let input = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error));
        let flowchart = parse_flowchart(&input).unwrap_or_else(|error| {
            panic!(
                "Failed to parse flowchart fixture {}: {}",
                path.display(),
                error
            )
        });
        compile_to_graph(&flowchart)
    }
}
