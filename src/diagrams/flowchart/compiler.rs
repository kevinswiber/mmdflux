//! Converts AST to graph data structures.

use std::collections::{HashMap, HashSet};

use crate::graph::style::{LinkStyleTarget, NodeStyle};
use crate::graph::{Arrow, Direction, Edge, Graph, Node, Shape, Stroke, Subgraph};
use crate::mermaid::{
    ArrowHead, ConnectorSpec, Direction as ParseDirection, EdgeSpec, Flowchart, LinkStyleStatement,
    ShapeSpec, Statement, StrokeSpec, Vertex,
};

type ClassDefRegistry = HashMap<String, NodeStyle>;

struct StyleTargets<'a> {
    node_styles: &'a mut HashMap<String, NodeStyle>,
    subgraph_styles: &'a mut HashMap<String, NodeStyle>,
    subgraph_ids: &'a HashSet<String>,
}

/// Build a Diagram from a parsed Flowchart.
pub fn compile_to_graph(flowchart: &Flowchart) -> Graph {
    let direction = convert_direction(flowchart.direction);
    let mut diagram = Graph::new(direction);
    let mut node_styles = HashMap::new();
    let mut subgraph_styles = HashMap::new();
    let mut edge_styles = Vec::new();
    let class_defs = collect_class_defs(&flowchart.statements);
    let subgraph_ids = collect_subgraph_ids(&flowchart.statements);
    let mut collision_nesting: Vec<(String, String)> = Vec::new();
    {
        let mut style_targets = StyleTargets {
            node_styles: &mut node_styles,
            subgraph_styles: &mut subgraph_styles,
            subgraph_ids: &subgraph_ids,
        };
        process_statements(
            &mut diagram,
            &flowchart.statements,
            None,
            &mut style_targets,
            &mut edge_styles,
            &class_defs,
            &mut collision_nesting,
        );
    }
    apply_default_class(&mut diagram, &node_styles, &class_defs);
    resolve_subgraph_edges(&mut diagram);
    apply_collision_nesting(&mut diagram, &collision_nesting);
    apply_edge_styles(&mut diagram, &edge_styles);
    diagram
}

/// Collect all classDef definitions (recursively into subgraphs) before processing.
fn collect_class_defs(statements: &[Statement]) -> ClassDefRegistry {
    let mut defs = HashMap::new();
    collect_class_defs_recursive(statements, &mut defs);
    defs
}

fn collect_class_defs_recursive(statements: &[Statement], defs: &mut ClassDefRegistry) {
    for stmt in statements {
        match stmt {
            Statement::ClassDef(cd) => {
                defs.entry(cd.class_name.clone())
                    .and_modify(|existing| *existing = existing.merge(&cd.style))
                    .or_insert_with(|| cd.style.clone());
            }
            Statement::Subgraph(sg) => {
                collect_class_defs_recursive(&sg.statements, defs);
            }
            _ => {}
        }
    }
}

fn process_statements(
    diagram: &mut Graph,
    statements: &[Statement],
    parent_subgraph: Option<&str>,
    style_targets: &mut StyleTargets<'_>,
    edge_styles: &mut Vec<LinkStyleStatement>,
    class_defs: &ClassDefRegistry,
    collision_nesting: &mut Vec<(String, String)>,
) {
    for statement in statements {
        match statement {
            Statement::Vertex(vertex) => {
                resolve_class_annotation(
                    diagram,
                    style_targets,
                    class_defs,
                    &vertex.id,
                    &vertex.class_name,
                );
                add_vertex_to_diagram(
                    diagram,
                    vertex,
                    parent_subgraph,
                    style_targets.node_styles.get(&vertex.id),
                    style_targets.subgraph_ids,
                    collision_nesting,
                );
            }
            Statement::Edge(edge_spec) => {
                resolve_class_annotation(
                    diagram,
                    style_targets,
                    class_defs,
                    &edge_spec.from.id,
                    &edge_spec.from.class_name,
                );
                add_vertex_to_diagram(
                    diagram,
                    &edge_spec.from,
                    parent_subgraph,
                    style_targets.node_styles.get(&edge_spec.from.id),
                    style_targets.subgraph_ids,
                    collision_nesting,
                );
                resolve_class_annotation(
                    diagram,
                    style_targets,
                    class_defs,
                    &edge_spec.to.id,
                    &edge_spec.to.class_name,
                );
                add_vertex_to_diagram(
                    diagram,
                    &edge_spec.to,
                    parent_subgraph,
                    style_targets.node_styles.get(&edge_spec.to.id),
                    style_targets.subgraph_ids,
                    collision_nesting,
                );
                let edge = convert_edge(edge_spec);
                diagram.add_edge(edge);
            }
            Statement::Subgraph(sg_spec) => {
                process_statements(
                    diagram,
                    &sg_spec.statements,
                    Some(&sg_spec.id),
                    style_targets,
                    edge_styles,
                    class_defs,
                    collision_nesting,
                );
                let node_ids = collect_node_ids(&sg_spec.statements);
                diagram.subgraphs.insert(
                    sg_spec.id.clone(),
                    Subgraph {
                        id: sg_spec.id.clone(),
                        title: sg_spec.title.clone(),
                        nodes: node_ids,
                        parent: parent_subgraph.map(|s| s.to_string()),
                        dir: sg_spec.dir.map(convert_direction),
                        invisible: false,
                        concurrent_regions: Vec::new(),
                        style: style_targets
                            .subgraph_styles
                            .get(&sg_spec.id)
                            .cloned()
                            .unwrap_or_default(),
                        class_names: Vec::new(),
                    },
                );
                diagram.subgraph_order.push(sg_spec.id.clone());
            }
            Statement::NodeStyle(style_stmt) => {
                merge_target_style(
                    diagram,
                    style_targets,
                    &style_stmt.node_id,
                    &style_stmt.style,
                    None,
                );
            }
            Statement::ClassDef(_) => {
                // Already collected in pass 1.
            }
            Statement::ClassApply(apply) => {
                if let Some(style) = class_defs.get(&apply.class_name) {
                    for node_id in &apply.node_ids {
                        merge_target_style(
                            diagram,
                            style_targets,
                            node_id,
                            style,
                            Some(&apply.class_name),
                        );
                    }
                }
            }
            Statement::LinkStyle(link_style) => edge_styles.push(link_style.clone()),
        }
    }
}

fn collect_subgraph_ids(statements: &[Statement]) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_subgraph_ids_recursive(statements, &mut ids);
    ids
}

fn collect_subgraph_ids_recursive(statements: &[Statement], ids: &mut HashSet<String>) {
    for stmt in statements {
        if let Statement::Subgraph(sg) = stmt {
            ids.insert(sg.id.clone());
            collect_subgraph_ids_recursive(&sg.statements, ids);
        }
    }
}

/// A single explicit-shape id collision found in the AST.
///
/// Sourced by walking the parsed `Flowchart`. Bare references (`A --> B`
/// where `A` is a subgraph and the reference carries no shape) are
/// intentionally excluded — they are edge endpoints handled by
/// `resolve_subgraph_edges`, not collisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IdCollision {
    pub id: String,
    pub parent: Option<String>,
}

/// Walk the AST and return every explicit-shape collision occurrence in
/// document order. Multiple references to the same id produce multiple
/// entries.
pub(super) fn collect_id_collisions(flowchart: &Flowchart) -> Vec<IdCollision> {
    let subgraph_ids = collect_subgraph_ids(&flowchart.statements);
    if subgraph_ids.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    walk_collisions(&flowchart.statements, None, &subgraph_ids, &mut out);
    out
}

fn walk_collisions(
    statements: &[Statement],
    parent: Option<&str>,
    subgraph_ids: &HashSet<String>,
    out: &mut Vec<IdCollision>,
) {
    for stmt in statements {
        match stmt {
            Statement::Vertex(v) => record_collision_occurrence(v, parent, subgraph_ids, out),
            Statement::Edge(e) => {
                record_collision_occurrence(&e.from, parent, subgraph_ids, out);
                record_collision_occurrence(&e.to, parent, subgraph_ids, out);
            }
            Statement::Subgraph(sg) => {
                walk_collisions(&sg.statements, Some(&sg.id), subgraph_ids, out);
            }
            Statement::NodeStyle(_)
            | Statement::ClassDef(_)
            | Statement::ClassApply(_)
            | Statement::LinkStyle(_) => {}
        }
    }
}

fn record_collision_occurrence(
    vertex: &Vertex,
    parent: Option<&str>,
    subgraph_ids: &HashSet<String>,
    out: &mut Vec<IdCollision>,
) {
    if vertex.shape.is_some() && subgraph_ids.contains(&vertex.id) {
        out.push(IdCollision {
            id: vertex.id.clone(),
            parent: parent.map(|s| s.to_string()),
        });
    }
}

/// Resolve a `:::className` annotation by looking up the class in the registry
/// and merging its style into the target's accumulated styles. When the target
/// id matches a declared subgraph, the style routes to the subgraph style map
/// (the same rule used by `class A foo` and `style A …` statements) rather
/// than being silently absorbed by a soon-to-be-pruned spurious node.
fn resolve_class_annotation(
    diagram: &mut Graph,
    style_targets: &mut StyleTargets<'_>,
    class_defs: &ClassDefRegistry,
    target_id: &str,
    class_name: &Option<String>,
) {
    if let Some(cn) = class_name
        && let Some(style) = class_defs.get(cn)
    {
        merge_target_style(diagram, style_targets, target_id, style, Some(cn.as_str()));
    }
}

/// Apply `classDef default` to every node that has no explicit class styling.
fn apply_default_class(
    diagram: &mut Graph,
    node_styles: &HashMap<String, NodeStyle>,
    class_defs: &ClassDefRegistry,
) {
    if let Some(default_style) = class_defs.get("default") {
        let unstyled: Vec<String> = diagram
            .nodes
            .keys()
            .filter(|id| !node_styles.contains_key(*id))
            .cloned()
            .collect();
        for node_id in unstyled {
            if let Some(node) = diagram.nodes.get_mut(&node_id) {
                node.style = default_style.merge(&node.style);
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

/// Insert (or update) the explicit-node entry for `vertex`.
///
/// When `vertex.id` is already declared as a subgraph, the explicit-node
/// reference is dropped: the subgraph wins on identifier collision. The
/// unified-namespace policy keeps `Graph.nodes` and `Graph.subgraphs`
/// disjoint and aligns with the existing style-routing rule in
/// `merge_target_style`.
///
/// Reference-site nesting: when an *explicit-shape* reference (e.g.
/// `A[NodeBox]`) collides with a declared subgraph and sits inside another
/// subgraph's body, a `(child_sg_id, parent_sg_id)` pair is recorded so
/// `apply_collision_nesting` can reparent the child subgraph. Bare
/// references (`A` with no shape) are NOT recorded — they are edge
/// endpoints, not nesting declarations.
fn add_vertex_to_diagram(
    diagram: &mut Graph,
    vertex: &Vertex,
    parent: Option<&str>,
    style: Option<&NodeStyle>,
    subgraph_ids: &HashSet<String>,
    collision_nesting: &mut Vec<(String, String)>,
) {
    if subgraph_ids.contains(&vertex.id) {
        if vertex.shape.is_some()
            && let Some(parent_sg) = parent
            && vertex.id != parent_sg
        {
            collision_nesting.push((vertex.id.clone(), parent_sg.to_string()));
        }
        return;
    }
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

fn merge_subgraph_style(
    diagram: &mut Graph,
    subgraph_styles: &mut HashMap<String, NodeStyle>,
    subgraph_id: &str,
    style: &NodeStyle,
) {
    let merged_style = subgraph_styles
        .entry(subgraph_id.to_string())
        .and_modify(|existing| *existing = existing.merge(style))
        .or_insert_with(|| style.clone());

    if let Some(subgraph) = diagram.subgraphs.get_mut(subgraph_id) {
        subgraph.style = merged_style.clone();
    }
}

/// Route a style declaration to the appropriate style map.
///
/// When `target_id` matches a declared subgraph id, the style merges into the
/// subgraph style map; otherwise it merges into the node style map. This rule
/// is shared by `style A …`, `class A foo`, and inline `A:::foo`. The two
/// style maps are kept independent, so MMDS `styleMap` and `subgraphStyleMap`
/// never collide on a shared id even when the input declares both.
///
/// When `class_name` is `Some`, the identifier is recorded on
/// `Subgraph.class_names` (preserving application order and de-duplicating)
/// so the SVG renderer can surface it as a CSS hook. `None` covers `style A …`,
/// which carries no class identity.
fn merge_target_style(
    diagram: &mut Graph,
    style_targets: &mut StyleTargets<'_>,
    target_id: &str,
    style: &NodeStyle,
    class_name: Option<&str>,
) {
    if style_targets.subgraph_ids.contains(target_id) {
        merge_subgraph_style(diagram, style_targets.subgraph_styles, target_id, style);
        if let Some(name) = class_name
            && let Some(subgraph) = diagram.subgraphs.get_mut(target_id)
            && !subgraph.class_names.iter().any(|existing| existing == name)
        {
            subgraph.class_names.push(name.to_string());
        }
    } else {
        merge_node_style(diagram, style_targets.node_styles, target_id, style);
    }
}

fn apply_edge_styles(diagram: &mut Graph, styles: &[LinkStyleStatement]) {
    for style_stmt in styles {
        match &style_stmt.target {
            LinkStyleTarget::Default => {
                for edge in &mut diagram.edges {
                    edge.style = edge.style.merge(&style_stmt.style);
                }
            }
            LinkStyleTarget::Indices(indices) => {
                for index in indices {
                    if let Some(edge) = diagram.edges.iter_mut().find(|edge| edge.index == *index) {
                        edge.style = edge.style.merge(&style_stmt.style);
                    }
                }
            }
        }
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
            style: edge.style.clone(),
            arrow_start: edge.arrow_start,
            arrow_end: edge.arrow_end,
            label: edge.label.clone(),
            head_label: edge.head_label.clone(),
            tail_label: edge.tail_label.clone(),
            minlen: edge.minlen,
            index: edge.index,
            wrapped_label_lines: edge.wrapped_label_lines.clone(),
        });
    }

    diagram.edges = resolved_edges;

    // Identifiers occupy a single namespace; when an id is both a node and a
    // subgraph, the subgraph wins. The insertion path normally prevents this;
    // this pass is a safety net for synthesized nodes that bypass it.
    let subgraph_ids: Vec<String> = diagram.subgraphs.keys().cloned().collect();
    for sg_id in &subgraph_ids {
        diagram.nodes.remove(sg_id);
    }
}

/// Reparent declared subgraphs whose ids were referenced with an explicit
/// shape inside another subgraph's body.
///
/// Each `(child_id, parent_id)` pair was recorded by `add_vertex_to_diagram`
/// when it skipped an explicit-shape collision. Existing parents are not
/// clobbered — a subgraph already nested by syntax keeps its declared
/// parent.
fn apply_collision_nesting(diagram: &mut Graph, pairs: &[(String, String)]) {
    for (child_id, parent_id) in pairs {
        if let Some(child_sg) = diagram.subgraphs.get_mut(child_id)
            && child_sg.parent.is_none()
        {
            child_sg.parent = Some(parent_id.clone());
        }
    }
}

fn find_non_cluster_child(diagram: &Graph, subgraph_id: &str) -> Option<String> {
    diagram.find_non_cluster_child(subgraph_id)
}

fn find_subgraph_sink(diagram: &Graph, subgraph_id: &str) -> Option<String> {
    diagram.find_subgraph_sink(subgraph_id)
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
            Statement::NodeStyle(_)
            | Statement::ClassDef(_)
            | Statement::ClassApply(_)
            | Statement::LinkStyle(_) => {}
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
pub(crate) fn normalize_br_tags(text: &str) -> String {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut cursor = 0;

    while let Some(offset) = text[cursor..].find('<') {
        let start = cursor + offset;
        result.push_str(&text[cursor..start]);

        if let Some(end) = match_br_tag(bytes, start) {
            result.push('\n');
            cursor = end;
        } else {
            result.push('<');
            cursor = start + 1;
        }
    }

    result.push_str(&text[cursor..]);
    result
}

fn match_br_tag(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start + 1;

    // Skip optional whitespace after <
    while i < len && bytes[i] == b' ' {
        i += 1;
    }

    // Match 'b'/'B' and 'r'/'R'
    if i >= len || !bytes[i].eq_ignore_ascii_case(&b'b') {
        return None;
    }
    i += 1;
    if i >= len || !bytes[i].eq_ignore_ascii_case(&b'r') {
        return None;
    }
    i += 1;

    // Skip optional whitespace and optional '/'
    while i < len && bytes[i] == b' ' {
        i += 1;
    }
    if i < len && bytes[i] == b'/' {
        i += 1;
    }
    while i < len && bytes[i] == b' ' {
        i += 1;
    }

    (i < len && bytes[i] == b'>').then_some(i + 1)
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
    fn flowchart_with_colliding_subgraph_and_node_ids_yields_clean_ir() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        let node_ids: HashSet<&str> = graph.nodes.keys().map(String::as_str).collect();
        let sg_ids: HashSet<&str> = graph.subgraphs.keys().map(String::as_str).collect();

        assert_eq!(node_ids, HashSet::from(["a1", "B"]));
        assert_eq!(sg_ids, HashSet::from(["A", "C"]));
        assert!(
            node_ids.is_disjoint(&sg_ids),
            "node and subgraph id sets must be disjoint",
        );

        assert_eq!(graph.edges.len(), 1);
        let edge = &graph.edges[0];
        assert_eq!(edge.from, "a1");
        assert_eq!(edge.to, "B");
        assert_eq!(edge.from_subgraph.as_deref(), Some("A"));
        assert_eq!(edge.to_subgraph, None);

        let sg_a = &graph.subgraphs["A"];
        let sg_c = &graph.subgraphs["C"];
        assert_eq!(
            sg_a.parent.as_deref(),
            Some("C"),
            "subgraph A should nest under C because its id was referenced \
             inside C's body",
        );
        assert_eq!(sg_c.parent, None, "subgraph C is top-level");
        assert!(
            sg_a.nodes.iter().any(|id| id == "a1"),
            "subgraph A should still contain a1: {:?}",
            sg_a.nodes,
        );
        let sg_c_children: HashSet<&str> = sg_c.nodes.iter().map(String::as_str).collect();
        assert_eq!(
            sg_c_children,
            HashSet::from(["A", "B"]),
            "subgraph C should list both 'A' (the nested subgraph) and 'B'",
        );
    }

    #[test]
    fn explicit_shape_collision_inside_outer_subgraph_nests_inner_subgraph() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        let sg_a = graph.subgraphs.get("A").expect("subgraph A should survive");
        assert_eq!(
            sg_a.parent.as_deref(),
            Some("C"),
            "subgraph A should nest under C because its id was referenced \
             WITH SHAPE inside C's body",
        );

        let sg_c = graph.subgraphs.get("C").expect("subgraph C should exist");
        assert!(sg_c.nodes.iter().any(|id| id == "A"));
        assert!(sg_c.nodes.iter().any(|id| id == "B"));
        assert_eq!(sg_c.parent, None);
    }

    #[test]
    fn bare_reference_to_subgraph_does_not_reparent() {
        // `A --> B` inside subgraph C, where A is a top-level subgraph.
        // Mermaid treats bare A as an edge endpoint, not as a nesting
        // declaration. The existing resolve_subgraph_edges rewrite handles
        // the source/target.
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A --> B
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        let sg_a = graph.subgraphs.get("A").expect("subgraph A");
        assert_eq!(
            sg_a.parent, None,
            "bare A inside C must NOT reparent subgraph A; it is an edge \
             endpoint, not a nesting declaration. Got parent={:?}",
            sg_a.parent,
        );

        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "a1");
        assert_eq!(graph.edges[0].from_subgraph.as_deref(), Some("A"));
    }

    #[test]
    fn nesting_pass_does_not_overwrite_existing_parents() {
        // If A is syntactically nested inside C already, its parent must
        // remain "C" and the nesting pass must not touch it.
        let input = "\
flowchart LR

subgraph C
  subgraph A
    a1
  end
  B
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);
        let sg_a = graph.subgraphs.get("A").expect("subgraph A");
        assert_eq!(sg_a.parent.as_deref(), Some("C"));
    }

    #[test]
    fn explicit_shape_collision_at_top_level_does_not_reparent() {
        // `A[NodeBox]` outside any other subgraph — the spurious node is
        // dropped by collision suppression, but there is no enclosing
        // subgraph to reparent A under, so subgraph A keeps parent=None.
        let input = "\
flowchart LR

subgraph A
a1
end

A[NodeBox]
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);
        let sg_a = graph.subgraphs.get("A").expect("subgraph A");
        assert_eq!(sg_a.parent, None);
        assert!(!graph.nodes.contains_key("A"));
    }

    #[test]
    fn resolve_subgraph_edges_prunes_collision_node_with_parent() {
        let mut graph = Graph::new(Direction::LeftRight);

        graph.subgraphs.insert(
            "A".to_string(),
            Subgraph {
                id: "A".into(),
                title: "A".into(),
                nodes: vec!["a1".into()],
                ..Default::default()
            },
        );
        graph.subgraphs.insert(
            "C".to_string(),
            Subgraph {
                id: "C".into(),
                title: "C".into(),
                nodes: vec!["A".into(), "B".into()],
                ..Default::default()
            },
        );
        graph
            .subgraph_order
            .extend(["A".to_string(), "C".to_string()]);

        let mut a1 = Node::new("a1");
        a1.parent = Some("A".into());
        graph.add_node(a1);
        let mut b = Node::new("B");
        b.label = "NodeBox2".into();
        b.parent = Some("C".into());
        graph.add_node(b);
        // Spurious collision node — same id as subgraph A, has a parent, label differs.
        let mut spurious = Node::new("A");
        spurious.label = "NodeBox".into();
        spurious.parent = Some("C".into());
        graph.add_node(spurious);
        graph.add_edge(Edge::new("A", "B"));

        resolve_subgraph_edges(&mut graph);

        assert!(
            !graph.nodes.contains_key("A"),
            "collision node A must be pruned even when label!=id and parent is set",
        );
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "a1");
        assert_eq!(graph.edges[0].from_subgraph.as_deref(), Some("A"));
    }

    #[test]
    fn add_vertex_skips_when_id_is_subgraph() {
        let input = "\
flowchart LR

subgraph A
a1
end

A[NodeBox]
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        assert!(
            !graph.nodes.contains_key("A"),
            "explicit node A should be collapsed into subgraph A; got {:?}",
            graph.nodes.get("A"),
        );
        assert!(
            graph.subgraphs.contains_key("A"),
            "subgraph A should survive"
        );
        assert!(
            graph.nodes.contains_key("a1"),
            "child node a1 should survive"
        );
    }

    #[test]
    fn edge_from_subgraph_id_resolves_to_real_node_with_from_subgraph_set() {
        let input = "\
flowchart LR

subgraph A
a1
end

A --> B
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        assert_eq!(graph.edges.len(), 1);
        let edge = &graph.edges[0];
        assert_eq!(
            edge.from, "a1",
            "from must be the real node, not subgraph A"
        );
        assert_eq!(edge.from_subgraph.as_deref(), Some("A"));
        assert_eq!(edge.to, "B");
        assert!(graph.nodes.contains_key("a1"));
        assert!(graph.nodes.contains_key("B"));
        assert!(graph.subgraphs.contains_key("A"));
        assert!(!graph.nodes.contains_key("A"));
    }

    #[test]
    fn edge_to_nested_subgraph_resolves_through_non_cluster_child() {
        let input = "\
flowchart LR

top

subgraph Outer
  subgraph Inner
    leaf
  end
end

top --> Outer
";
        let flowchart = parse_flowchart(input).expect("parses");
        let graph = compile_to_graph(&flowchart);

        assert_eq!(graph.edges.len(), 1);
        let edge = &graph.edges[0];
        assert_eq!(edge.from, "top");
        assert_eq!(edge.to, "leaf", "to must drill through to the real node");
        assert_eq!(edge.to_subgraph.as_deref(), Some("Outer"));
        assert!(graph.nodes.contains_key("leaf"));
        assert!(!graph.nodes.contains_key("Outer"));
        assert!(!graph.nodes.contains_key("Inner"));
    }

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
    fn subgraph_ir_carries_default_style_storage() {
        let input = "graph TD\nsubgraph A[Source]\na1\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert!(diagram.subgraphs["A"].style.is_empty());
    }

    #[test]
    fn class_subgraph_applies_classdef_style_to_subgraph() {
        let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef blue fill:#e1f5fe,stroke:#01579b,stroke-width:2px\nclass A blue\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let style = &diagram.subgraphs["A"].style;
        assert_eq!(style.fill.as_ref().map(|c| c.raw()), Some("#e1f5fe"));
        assert_eq!(style.stroke.as_ref().map(|c| c.raw()), Some("#01579b"));
        assert_eq!(style.stroke_width.as_deref(), Some("2px"));
        assert!(diagram.nodes["a1"].style.fill.is_none());
    }

    #[test]
    fn direct_style_statement_can_target_subgraph() {
        let input =
            "flowchart LR\nsubgraph A[Source]\na1\nend\nstyle A fill:#ffebee,stroke:#b71c1c\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let style = &diagram.subgraphs["A"].style;
        assert_eq!(style.fill.as_ref().map(|c| c.raw()), Some("#ffebee"));
        assert_eq!(style.stroke.as_ref().map(|c| c.raw()), Some("#b71c1c"));
        assert!(diagram.nodes["a1"].style.fill.is_none());
    }

    #[test]
    fn class_subgraph_before_subgraph_declaration_is_applied() {
        let input =
            "flowchart LR\nclassDef blue fill:#e1f5fe\nclass A blue\nsubgraph A[Source]\na1\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(
            diagram.subgraphs["A"].style.fill.as_ref().map(|c| c.raw()),
            Some("#e1f5fe")
        );
        assert!(diagram.nodes["a1"].style.fill.is_none());
    }

    #[test]
    fn subgraph_class_and_direct_style_merge_by_property() {
        let input = "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef blue fill:#e1f5fe,stroke:#01579b\nclass A blue\nstyle A stroke:#b71c1c,stroke-width:2px\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let style = &diagram.subgraphs["A"].style;
        assert_eq!(style.fill.as_ref().map(|c| c.raw()), Some("#e1f5fe"));
        assert_eq!(style.stroke.as_ref().map(|c| c.raw()), Some("#b71c1c"));
        assert_eq!(style.stroke_width.as_deref(), Some("2px"));
    }

    #[test]
    fn inline_triple_colon_class_on_subgraph_id_applies_to_subgraph() {
        // `A:::blue` after `subgraph A …` previously routed through the node
        // style map and was silently discarded by the spurious-node cleanup.
        // It now routes the same way as `class A blue` and `style A …`, which
        // matches Mermaid's observed behavior — see
        // docs/development/mermaid-style-routing-parity.md.
        let input =
            "flowchart LR\nsubgraph A[Source]\na1\nend\nclassDef blue fill:#e1f5fe\nA:::blue\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(
            diagram.subgraphs["A"].style.fill.as_ref().map(|c| c.raw()),
            Some("#e1f5fe"),
        );
        assert!(!diagram.nodes.contains_key("A"));
    }

    #[test]
    fn explicit_node_with_same_id_as_subgraph_keeps_style_maps_independent() {
        // When an explicit `A[Box]` shares its id with `subgraph A`, the
        // subgraph wins: the explicit-node entry is dropped from `Graph.nodes`
        // and every style/class declaration targeting the shared id routes to
        // the subgraph style map. This test pins both the style routing rule
        // and the disjoint-ids invariant between `nodes` and `subgraphs`.
        // See docs/development/mermaid-style-routing-parity.md.
        let input = concat!(
            "flowchart LR\n",
            "A[NodeBox]\n",
            "subgraph A\n",
            "a1\n",
            "end\n",
            "classDef blue fill:#e1f5fe\n",
            "class A blue\n",
            "style A stroke:#b71c1c\n",
        );
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        let sg_style = &diagram.subgraphs["A"].style;
        assert_eq!(
            sg_style.fill.as_ref().map(|c| c.raw()),
            Some("#e1f5fe"),
            "subgraph receives classDef fill",
        );
        assert_eq!(
            sg_style.stroke.as_ref().map(|c| c.raw()),
            Some("#b71c1c"),
            "subgraph receives direct style stroke",
        );

        assert!(
            !diagram.nodes.contains_key("A"),
            "explicit node A collapses into subgraph A on id collision",
        );
    }

    #[test]
    fn classdef_default_remains_node_only_not_subgraph_style() {
        let input = "flowchart LR\nclassDef default fill:#f00\nsubgraph A[Source]\na1\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert!(diagram.subgraphs["A"].style.fill.is_none());
        assert_eq!(
            diagram.nodes["a1"].style.fill.as_ref().map(|c| c.raw()),
            Some("#f00")
        );
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
    fn test_br_tag_preserves_utf8_text() {
        assert_eq!(normalize_br_tags("開始<br>処理"), "開始\n処理");
        assert_eq!(normalize_br_tags("開始<タグ>処理"), "開始<タグ>処理");
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

    #[test]
    fn test_node_label_with_utf8_text() {
        let flowchart = parse_flowchart("graph TD\nA[開始]\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        let node = diagram.get_node("A").unwrap();
        assert_eq!(node.label, "開始");
    }

    #[test]
    fn test_node_label_with_utf8_text_and_br_tag() {
        let flowchart = parse_flowchart("graph TD\nA[開始<br>処理]\n").unwrap();
        let diagram = compile_to_graph(&flowchart);
        let node = diagram.get_node("A").unwrap();
        assert_eq!(node.label, "開始\n処理");
    }

    #[test]
    fn test_edge_label_with_utf8_text() {
        for input in ["graph TD\nA -- 確認 --> B\n", "graph TD\nA -->|確認| B\n"] {
            let flowchart = parse_flowchart(input).unwrap();
            let diagram = compile_to_graph(&flowchart);
            assert_eq!(diagram.edges[0].label, Some("確認".to_string()));
        }
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

    #[test]
    fn classdef_default_applies_to_unclassified_nodes() {
        let flowchart =
            parse_flowchart("graph TD\nclassDef default fill:#f00\nA --> B\nB:::custom --> C\nclassDef custom fill:#0f0\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        // A and C have no explicit class — should get the default fill.
        assert_eq!(
            diagram.nodes["A"].style.fill.as_ref().unwrap().raw(),
            "#f00"
        );
        assert_eq!(
            diagram.nodes["C"].style.fill.as_ref().unwrap().raw(),
            "#f00"
        );
        // B has explicit :::custom — should NOT get the default.
        assert_eq!(
            diagram.nodes["B"].style.fill.as_ref().unwrap().raw(),
            "#0f0"
        );
    }

    #[test]
    fn compiler_resolves_distinct_node_and_edge_label_font_styles() {
        let input = r#"graph TD
            A[Regular] -->|link| B(Styled Node)
            style A font-family:Verdana,font-size:8px
            style B font-family:Courier New,font-size:20px
            linkStyle 0 font-family:Times New Roman,font-size:32px
        "#;

        let flowchart = parse_flowchart(input).unwrap();
        let graph = compile_to_graph(&flowchart);
        let a = graph.nodes.get("A").unwrap();
        let b = graph.nodes.get("B").unwrap();
        let edge = &graph.edges[0];

        assert_eq!(a.style.font_family.as_deref(), Some("Verdana"));
        assert_eq!(a.style.font_size.as_deref(), Some("8px"));
        assert_eq!(b.style.font_family.as_deref(), Some("Courier New"));
        assert_eq!(b.style.font_size.as_deref(), Some("20px"));
        assert_eq!(edge.style.font_family.as_deref(), Some("Times New Roman"));
        assert_eq!(edge.style.font_size.as_deref(), Some("32px"));
    }

    #[test]
    fn classdef_font_properties_apply_and_direct_style_overrides_property_by_property() {
        let input = r#"graph TD
            classDef default font-family:Arial,font-size:16px,font-style:normal,font-weight:400
            classDef compact font-family:Verdana,font-size:8px,font-style:italic,font-weight:700
            A:::compact --> B
            class B compact
            style A font-size:12px,font-weight:500
        "#;

        let flowchart = parse_flowchart(input).unwrap();
        let graph = compile_to_graph(&flowchart);
        let a = graph.nodes.get("A").unwrap();
        let b = graph.nodes.get("B").unwrap();

        assert_eq!(a.style.font_family.as_deref(), Some("Verdana"));
        assert_eq!(a.style.font_size.as_deref(), Some("12px"));
        assert_eq!(a.style.font_style.as_deref(), Some("italic"));
        assert_eq!(a.style.font_weight.as_deref(), Some("500"));
        assert_eq!(b.style.font_family.as_deref(), Some("Verdana"));
        assert_eq!(b.style.font_size.as_deref(), Some("8px"));
        assert_eq!(b.style.font_style.as_deref(), Some("italic"));
        assert_eq!(b.style.font_weight.as_deref(), Some("700"));
    }

    #[test]
    fn classdef_default_font_properties_apply_to_unclassified_nodes() {
        let flowchart = parse_flowchart(
            "graph TD\nclassDef default font-family:Arial,font-size:16px\nA --> B:::custom\nclassDef custom font-family:Verdana\n",
        )
        .unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(
            diagram.nodes["A"].style.font_family.as_deref(),
            Some("Arial")
        );
        assert_eq!(diagram.nodes["A"].style.font_size.as_deref(), Some("16px"));
        assert_eq!(
            diagram.nodes["B"].style.font_family.as_deref(),
            Some("Verdana")
        );
        assert_eq!(diagram.nodes["B"].style.font_size.as_deref(), None);
    }

    #[test]
    fn classdef_multi_class_names() {
        let flowchart =
            parse_flowchart("graph TD\nclassDef a,b fill:#f00\nA:::a --> B:::b --> C\n").unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(
            diagram.nodes["A"].style.fill.as_ref().unwrap().raw(),
            "#f00"
        );
        assert_eq!(
            diagram.nodes["B"].style.fill.as_ref().unwrap().raw(),
            "#f00"
        );
        assert!(diagram.nodes["C"].style.fill.is_none());
    }

    #[test]
    fn linkstyle_default_and_index_styles_merge_in_statement_order() {
        let flowchart = parse_flowchart(
            "graph TD\nA --> B\nB --> C\nlinkStyle default stroke:#999,stroke-width:2px\nlinkStyle 1 stroke:#0f0\n",
        )
        .unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(
            diagram.edges[0].style.stroke.as_ref().unwrap().raw(),
            "#999"
        );
        assert_eq!(diagram.edges[0].style.stroke_width.as_deref(), Some("2px"));
        assert_eq!(
            diagram.edges[1].style.stroke.as_ref().unwrap().raw(),
            "#0f0"
        );
        assert_eq!(diagram.edges[1].style.stroke_width.as_deref(), Some("2px"));
    }

    #[test]
    fn linkstyle_default_and_index_font_styles_merge_property_by_property() {
        let flowchart = parse_flowchart(
            "graph TD\nA -->|first| B\nB -->|second| C\nlinkStyle default font-family:Arial,font-size:16px,font-style:normal,font-weight:400\nlinkStyle 1 font-size:24px,font-weight:700\n",
        )
        .unwrap();
        let diagram = compile_to_graph(&flowchart);

        assert_eq!(diagram.edges[0].style.font_family.as_deref(), Some("Arial"));
        assert_eq!(diagram.edges[0].style.font_size.as_deref(), Some("16px"));
        assert_eq!(diagram.edges[0].style.font_style.as_deref(), Some("normal"));
        assert_eq!(diagram.edges[0].style.font_weight.as_deref(), Some("400"));
        assert_eq!(diagram.edges[1].style.font_family.as_deref(), Some("Arial"));
        assert_eq!(diagram.edges[1].style.font_size.as_deref(), Some("24px"));
        assert_eq!(diagram.edges[1].style.font_style.as_deref(), Some("normal"));
        assert_eq!(diagram.edges[1].style.font_weight.as_deref(), Some("700"));
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

    #[test]
    fn collect_id_collisions_returns_explicit_shape_collisions_only() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let collisions = collect_id_collisions(&flowchart);

        assert_eq!(collisions.len(), 1, "got {collisions:?}");
        assert_eq!(collisions[0].id, "A");
        assert_eq!(collisions[0].parent.as_deref(), Some("C"));
    }

    #[test]
    fn reverse_declaration_order_yields_same_ir_as_forward_order() {
        let forward = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";

        let reverse = "\
flowchart LR

subgraph C
A[NodeBox] --> B[NodeBox2]
end

subgraph A
a1
end
";

        let forward_graph = compile_to_graph(&parse_flowchart(forward).expect("parses"));
        let reverse_graph = compile_to_graph(&parse_flowchart(reverse).expect("parses"));

        let forward_nodes: std::collections::BTreeSet<&String> =
            forward_graph.nodes.keys().collect();
        let reverse_nodes: std::collections::BTreeSet<&String> =
            reverse_graph.nodes.keys().collect();
        assert_eq!(
            forward_nodes, reverse_nodes,
            "explicit-node set must be order-independent",
        );

        let forward_sgs: std::collections::BTreeSet<&String> =
            forward_graph.subgraphs.keys().collect();
        let reverse_sgs: std::collections::BTreeSet<&String> =
            reverse_graph.subgraphs.keys().collect();
        assert_eq!(
            forward_sgs, reverse_sgs,
            "subgraph set must be order-independent",
        );

        assert_eq!(
            forward_graph.subgraphs["A"].parent.as_deref(),
            Some("C"),
            "forward: A nested under C",
        );
        assert_eq!(
            reverse_graph.subgraphs["A"].parent.as_deref(),
            Some("C"),
            "reverse: A nested under C (same as forward)",
        );

        assert!(!forward_graph.nodes.contains_key("A"));
        assert!(!reverse_graph.nodes.contains_key("A"));
    }

    #[test]
    fn collect_id_collisions_skips_bare_references() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A --> B
end
";
        let flowchart = parse_flowchart(input).expect("parses");
        let collisions = collect_id_collisions(&flowchart);
        assert!(
            collisions.is_empty(),
            "bare reference must not register as a collision: {collisions:?}",
        );
    }

    fn compile_input(input: &str) -> Graph {
        let flowchart = parse_flowchart(input).expect("parses");
        compile_to_graph(&flowchart)
    }

    #[test]
    fn class_statement_captures_class_names_on_subgraph() {
        let input = "\
flowchart TD
    subgraph lr [Left to Right]
        A --> B
    end
    classDef blueFill fill:#00f
    class lr blueFill
";
        let diagram = compile_input(input);
        let sg = diagram.subgraphs.get("lr").expect("subgraph lr present");
        assert_eq!(sg.class_names, vec!["blueFill".to_string()]);
    }

    #[test]
    fn multiple_class_applications_accumulate_on_subgraph_in_order() {
        let input = "\
flowchart TD
    subgraph lr [Left to Right]
        A --> B
    end
    classDef blueFill fill:#00f
    classDef thickBorder stroke-width:3
    class lr blueFill
    class lr thickBorder
";
        let diagram = compile_input(input);
        let sg = diagram.subgraphs.get("lr").expect("subgraph lr present");
        assert_eq!(
            sg.class_names,
            vec!["blueFill".to_string(), "thickBorder".to_string()]
        );
    }

    #[test]
    fn duplicate_class_application_does_not_duplicate_on_subgraph() {
        let input = "\
flowchart TD
    subgraph lr [Left to Right]
        A --> B
    end
    classDef blueFill fill:#00f
    class lr blueFill
    class lr blueFill
";
        let diagram = compile_input(input);
        let sg = diagram.subgraphs.get("lr").expect("subgraph lr present");
        assert_eq!(sg.class_names, vec!["blueFill".to_string()]);
    }

    #[test]
    fn class_application_to_node_does_not_populate_subgraph_class_names() {
        let input = "\
flowchart TD
    subgraph lr [Left to Right]
        A --> B
    end
    classDef blueFill fill:#00f
    class A blueFill
";
        let diagram = compile_input(input);
        let sg = diagram.subgraphs.get("lr").expect("subgraph lr present");
        assert!(sg.class_names.is_empty());
    }
}
