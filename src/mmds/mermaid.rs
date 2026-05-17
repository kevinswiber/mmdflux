//! Mermaid syntax regeneration from MMDS interchange payloads.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use super::{Document, Edge, Node, Subgraph, parse_input};
use crate::graph::{Arrow, Shape, Stroke};
use crate::mmds::MmdsToken;

/// Error produced when MMDS-to-Mermaid regeneration fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationError {
    message: String,
}

impl GenerationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MMDS generation error: {}", self.message)
    }
}

impl Error for GenerationError {}

struct IdentifierMaps {
    node_ids: HashMap<String, String>,
    subgraph_ids: HashMap<String, String>,
}

/// Generate canonical Mermaid text from parsed MMDS output.
pub fn generate_mermaid(output: &Document) -> Result<String, GenerationError> {
    validate_mermaid_generation_scope(output)?;

    let identifiers = build_identifier_maps(output);

    let mut lines = Vec::with_capacity(1 + output.nodes.len() + output.edges.len());
    lines.push(format!(
        "flowchart {}",
        output.metadata.direction.as_mmds_str()
    ));

    emit_nodes(output, &identifiers, &mut lines)?;
    emit_edges(output, &identifiers, &mut lines)?;

    Ok(lines.join("\n") + "\n")
}

/// Parse MMDS JSON and generate canonical Mermaid text.
pub fn generate_mermaid_from_str(input: &str) -> Result<String, GenerationError> {
    let output = parse_input(input)
        .map_err(|err| GenerationError::new(format!("failed to parse MMDS input: {err}")))?;
    generate_mermaid(&output)
}

fn build_identifier_maps(output: &Document) -> IdentifierMaps {
    IdentifierMaps {
        node_ids: build_identifier_map(output.nodes.iter().map(|node| node.id.as_str()), "node"),
        subgraph_ids: build_identifier_map(
            output.subgraphs.iter().map(|subgraph| subgraph.id.as_str()),
            "subgraph",
        ),
    }
}

fn validate_mermaid_generation_scope(output: &Document) -> Result<(), GenerationError> {
    match output.metadata.diagram_type.as_str() {
        "flowchart" | "class" => Ok(()),
        value => Err(GenerationError::new(format!(
            "unsupported MMDS diagram_type '{value}'; expected flowchart or class"
        ))),
    }
}

fn build_identifier_map<'a, I>(ids: I, fallback_prefix: &str) -> HashMap<String, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut sorted_ids: Vec<&str> = ids.into_iter().collect();
    sorted_ids.sort_unstable();

    let mut mapped = HashMap::new();
    let mut used = HashSet::new();

    for raw in sorted_ids {
        let base = normalize_identifier(raw, fallback_prefix);
        let mut candidate = base.clone();
        let mut suffix = 2;
        while !used.insert(candidate.clone()) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        mapped.insert(raw.to_string(), candidate);
    }

    mapped
}

fn normalize_identifier(raw: &str, fallback_prefix: &str) -> String {
    // Interior ASCII dashes are preserved because Mermaid's flowchart lexer
    // accepts them inside node, subgraph, and class identifiers (the
    // NODE_STRING / idString tokens include MINUS). Keeping them through
    // MMDS → Mermaid emission also keeps SVG `id` attributes byte-stable
    // across the round-trip: see `xml_safe_id` in
    // src/render/graph/svg/nodes.rs, which already passes dashes through
    // unchanged. Consecutive dashes are collapsed and leading/trailing
    // dashes are trimmed because `--` is the edge operator in Mermaid and
    // would otherwise re-tokenize the identifier.
    let mut normalized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();

    while normalized.contains("__") {
        normalized = normalized.replace("__", "_");
    }
    while normalized.contains("--") {
        normalized = normalized.replace("--", "-");
    }

    normalized = normalized
        .trim_matches(|ch: char| ch == '_' || ch == '-')
        .to_string();
    if normalized.is_empty() {
        return fallback_prefix.to_string();
    }

    let starts_with_digit = normalized
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false);
    if starts_with_digit {
        format!("{fallback_prefix}_{normalized}")
    } else {
        normalized
    }
}

fn emit_nodes(
    output: &Document,
    identifiers: &IdentifierMaps,
    lines: &mut Vec<String>,
) -> Result<(), GenerationError> {
    let mut root_subgraphs: Vec<&Subgraph> = Vec::new();
    let mut subgraphs_by_parent: HashMap<&str, Vec<&Subgraph>> = HashMap::new();

    for subgraph in &output.subgraphs {
        if let Some(parent) = subgraph.parent.as_deref() {
            subgraphs_by_parent
                .entry(parent)
                .or_default()
                .push(subgraph);
        } else {
            root_subgraphs.push(subgraph);
        }
    }
    root_subgraphs.sort_by(|left, right| left.id.cmp(&right.id));
    for children in subgraphs_by_parent.values_mut() {
        children.sort_by(|left, right| left.id.cmp(&right.id));
    }

    let mut standalone_nodes: Vec<&Node> = Vec::new();
    let mut nodes_by_parent: HashMap<&str, Vec<&Node>> = HashMap::new();
    for node in &output.nodes {
        if let Some(parent) = node.parent.as_deref() {
            nodes_by_parent.entry(parent).or_default().push(node);
        } else {
            standalone_nodes.push(node);
        }
    }
    standalone_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    for child_nodes in nodes_by_parent.values_mut() {
        child_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    }

    for subgraph in root_subgraphs {
        emit_subgraph(
            subgraph,
            identifiers,
            0,
            &subgraphs_by_parent,
            &nodes_by_parent,
            lines,
        )?;
    }

    for node in standalone_nodes {
        let node_id = map_node_id(identifiers, node.id.as_str())?;
        lines.push(render_node(node_id, node)?);
    }

    Ok(())
}

fn emit_subgraph(
    subgraph: &Subgraph,
    identifiers: &IdentifierMaps,
    indent: usize,
    subgraphs_by_parent: &HashMap<&str, Vec<&Subgraph>>,
    nodes_by_parent: &HashMap<&str, Vec<&Node>>,
    lines: &mut Vec<String>,
) -> Result<(), GenerationError> {
    let padding = " ".repeat(indent);
    lines.push(format!(
        "{padding}{}",
        render_subgraph_header(subgraph, identifiers)?
    ));

    if let Some(direction) = &subgraph.direction {
        let child_padding = " ".repeat(indent + 4);
        lines.push(format!(
            "{child_padding}direction {}",
            direction.as_mmds_str()
        ));
    }

    if let Some(children) = subgraphs_by_parent.get(subgraph.id.as_str()) {
        for child_subgraph in children {
            emit_subgraph(
                child_subgraph,
                identifiers,
                indent + 4,
                subgraphs_by_parent,
                nodes_by_parent,
                lines,
            )?;
        }
    }

    if let Some(children) = nodes_by_parent.get(subgraph.id.as_str()) {
        let child_padding = " ".repeat(indent + 4);
        for node in children {
            let node_id = map_node_id(identifiers, node.id.as_str())?;
            lines.push(format!("{child_padding}{}", render_node(node_id, node)?));
        }
    }

    lines.push(format!("{padding}end"));
    Ok(())
}

fn render_subgraph_header(
    subgraph: &Subgraph,
    identifiers: &IdentifierMaps,
) -> Result<String, GenerationError> {
    let id = map_subgraph_id(identifiers, subgraph.id.as_str())?;
    if subgraph.title == subgraph.id {
        Ok(format!("subgraph {id}"))
    } else {
        Ok(format!(
            "subgraph {id}[{}]",
            format_structural_label(&subgraph.title)
        ))
    }
}

fn emit_edges(
    output: &Document,
    identifiers: &IdentifierMaps,
    lines: &mut Vec<String>,
) -> Result<(), GenerationError> {
    let mut edges: Vec<(usize, &Edge)> = output.edges.iter().enumerate().collect();
    edges.sort_by(|(left_index, left), (right_index, right)| {
        compare_edge_ids(&left.id, &right.id).then(left_index.cmp(right_index))
    });

    for (_, edge) in edges {
        lines.push(render_edge(edge, identifiers)?);
    }

    Ok(())
}

fn render_node(node_id: &str, node: &Node) -> Result<String, GenerationError> {
    let label = format_structural_label(&node.label);

    match node.shape {
        Shape::Rectangle => Ok(format!("{node_id}[{label}]")),
        Shape::Round => Ok(format!("{node_id}({label})")),
        Shape::Diamond => Ok(format!("{node_id}{{{label}}}")),
        Shape::Stadium => Ok(format!("{node_id}([{label}])")),
        Shape::Subroutine => Ok(format!("{node_id}[[{label}]]")),
        Shape::Cylinder => Ok(format!("{node_id}[({label})]")),
        Shape::Circle => Ok(format!("{node_id}(({label}))")),
        Shape::DoubleCircle => Ok(format!("{node_id}((({label})))")),
        Shape::Hexagon => Ok(format!("{node_id}{{{{{label}}}}}")),
        Shape::Trapezoid => Ok(format!(r#"{node_id}[/{label}\]"#)),
        Shape::InvTrapezoid => Ok(format!(r#"{node_id}[\{label}/]"#)),
        Shape::Asymmetric => Ok(format!("{node_id}>{label}]")),
        Shape::Document => Ok(render_at_shape(node_id, "doc", &node.label)),
        Shape::Documents => Ok(render_at_shape(node_id, "docs", &node.label)),
        Shape::TaggedDocument => Ok(render_at_shape(node_id, "tag-doc", &node.label)),
        Shape::Card => Ok(render_at_shape(node_id, "card", &node.label)),
        Shape::TaggedRect => Ok(render_at_shape(node_id, "tag-rect", &node.label)),
        Shape::Parallelogram => Ok(render_at_shape(node_id, "lean-r", &node.label)),
        Shape::InvParallelogram => Ok(render_at_shape(node_id, "lean-l", &node.label)),
        Shape::ManualInput => Ok(render_at_shape(node_id, "sl-rect", &node.label)),
        Shape::SmallCircle => Ok(render_at_shape(node_id, "sm-circ", &node.label)),
        Shape::FramedCircle => Ok(render_at_shape(node_id, "fr-circ", &node.label)),
        Shape::CrossedCircle => Ok(render_at_shape(node_id, "cross-circ", &node.label)),
        Shape::TextBlock => Ok(render_at_shape(node_id, "text", &node.label)),
        Shape::ForkJoin => Ok(render_at_shape(node_id, "fork", &node.label)),
        Shape::NoteRect => Err(GenerationError::new(format!(
            "unsupported node shape '{value}' for node '{}'",
            node.id,
            value = node.shape.as_mmds_str()
        ))),
    }
}

fn render_at_shape(node_id: &str, keyword: &str, label: &str) -> String {
    let escaped = escape_quoted_label(label);
    format!(r#"{node_id}@{{shape: {keyword}, label: "{escaped}"}}"#)
}

fn render_edge(edge: &Edge, identifiers: &IdentifierMaps) -> Result<String, GenerationError> {
    let connector = connector_for(edge)?;
    let source_id = map_node_id(identifiers, edge.source.as_str())?;
    let target_id = map_node_id(identifiers, edge.target.as_str())?;

    if let Some(label) = edge.label.as_ref()
        && !label.is_empty()
    {
        return Ok(format!(
            "{source_id} {connector}|{}| {target_id}",
            format_edge_label(label)
        ));
    }

    Ok(format!("{source_id} {connector} {target_id}"))
}

fn map_node_id<'a>(
    identifiers: &'a IdentifierMaps,
    raw_id: &str,
) -> Result<&'a str, GenerationError> {
    identifiers
        .node_ids
        .get(raw_id)
        .map(String::as_str)
        .ok_or_else(|| GenerationError::new(format!("unknown node id '{raw_id}'")))
}

fn map_subgraph_id<'a>(
    identifiers: &'a IdentifierMaps,
    raw_id: &str,
) -> Result<&'a str, GenerationError> {
    identifiers
        .subgraph_ids
        .get(raw_id)
        .map(String::as_str)
        .ok_or_else(|| GenerationError::new(format!("unknown subgraph id '{raw_id}'")))
}

fn format_structural_label(label: &str) -> String {
    if needs_quotes(label) {
        format!("\"{}\"", escape_quoted_label(label))
    } else {
        label.to_string()
    }
}

fn needs_quotes(label: &str) -> bool {
    if label.is_empty() {
        return true;
    }

    label
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
}

fn escape_quoted_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_edge_label(label: &str) -> String {
    escape_quoted_label(label).replace('|', "&#124;")
}

fn connector_for(edge: &Edge) -> Result<String, GenerationError> {
    let minlen = edge.minlen.max(1) as usize;
    let connector = match (edge.stroke, edge.arrow_start, edge.arrow_end) {
        (Stroke::Solid, Arrow::None, Arrow::Normal) => format!("{}>", "-".repeat(minlen + 1)),
        (Stroke::Solid, Arrow::None, Arrow::None) => "-".repeat(minlen + 2),
        (Stroke::Solid, Arrow::Normal, Arrow::Normal) => format!("<{}>", "-".repeat(minlen + 1)),
        (Stroke::Solid, Arrow::None, Arrow::Cross) => format!("{}x", "-".repeat(minlen + 1)),
        (Stroke::Solid, Arrow::Cross, Arrow::Cross) => format!("x{}x", "-".repeat(minlen + 1)),
        (Stroke::Solid, Arrow::None, Arrow::Circle) => format!("{}o", "-".repeat(minlen + 1)),
        (Stroke::Solid, Arrow::Circle, Arrow::Circle) => format!("o{}o", "-".repeat(minlen + 1)),
        (Stroke::Dotted, Arrow::None, Arrow::Normal) => format!("-{}->", ".".repeat(minlen)),
        (Stroke::Dotted, Arrow::None, Arrow::None) => format!("-{}-", ".".repeat(minlen)),
        (Stroke::Dotted, Arrow::Normal, Arrow::Normal) => format!("<-{}->", ".".repeat(minlen)),
        (Stroke::Dotted, Arrow::None, Arrow::Cross) => format!("-{}-x", ".".repeat(minlen)),
        (Stroke::Dotted, Arrow::None, Arrow::Circle) => format!("-{}-o", ".".repeat(minlen)),
        (Stroke::Thick, Arrow::None, Arrow::Normal) => format!("{}>", "=".repeat(minlen + 1)),
        (Stroke::Thick, Arrow::None, Arrow::None) => "=".repeat(minlen + 2),
        (Stroke::Thick, Arrow::Normal, Arrow::Normal) => format!("<{}>", "=".repeat(minlen + 1)),
        (Stroke::Thick, Arrow::None, Arrow::Cross) => format!("{}x", "=".repeat(minlen + 1)),
        (Stroke::Thick, Arrow::None, Arrow::Circle) => format!("{}o", "=".repeat(minlen + 1)),
        (Stroke::Invisible, Arrow::None, Arrow::None) => "~".repeat(minlen + 2),
        _ => {
            return Err(GenerationError::new(format!(
                "unsupported edge connector combination stroke='{}' arrow_start='{}' arrow_end='{}' on edge '{}'",
                edge.stroke.as_mmds_str(),
                edge.arrow_start.as_mmds_str(),
                edge.arrow_end.as_mmds_str(),
                edge.id
            )));
        }
    };

    Ok(connector)
}

fn compare_edge_ids(left: &str, right: &str) -> Ordering {
    let left_number = parse_edge_index(left);
    let right_number = parse_edge_index(right);

    match (left_number, right_number) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

fn parse_edge_index(value: &str) -> Option<u64> {
    value.strip_prefix('e')?.parse::<u64>().ok()
}
