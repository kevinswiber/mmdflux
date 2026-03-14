//! Mermaid syntax regeneration from MMDS interchange payloads.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use super::{MmdsEdge, MmdsNode, MmdsOutput, MmdsSubgraph, parse_mmds_input};

/// Error produced when MMDS-to-Mermaid regeneration fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmdsGenerationError {
    message: String,
}

impl MmdsGenerationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MmdsGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MMDS generation error: {}", self.message)
    }
}

impl Error for MmdsGenerationError {}

struct IdentifierMaps {
    node_ids: HashMap<String, String>,
    subgraph_ids: HashMap<String, String>,
}

/// Generate canonical Mermaid text from parsed MMDS output.
pub fn generate_mermaid_from_mmds(output: &MmdsOutput) -> Result<String, MmdsGenerationError> {
    validate_mermaid_generation_scope(output)?;

    let identifiers = build_identifier_maps(output);

    let mut lines = Vec::with_capacity(1 + output.nodes.len() + output.edges.len());
    lines.push(format!("flowchart {}", output.metadata.direction));

    emit_nodes(output, &identifiers, &mut lines)?;
    emit_edges(output, &identifiers, &mut lines)?;

    Ok(lines.join("\n") + "\n")
}

/// Parse MMDS JSON and generate canonical Mermaid text.
pub fn generate_mermaid_from_mmds_str(input: &str) -> Result<String, MmdsGenerationError> {
    let output = parse_mmds_input(input)
        .map_err(|err| MmdsGenerationError::new(format!("failed to parse MMDS input: {err}")))?;
    generate_mermaid_from_mmds(&output)
}

fn build_identifier_maps(output: &MmdsOutput) -> IdentifierMaps {
    IdentifierMaps {
        node_ids: build_identifier_map(output.nodes.iter().map(|node| node.id.as_str()), "node"),
        subgraph_ids: build_identifier_map(
            output.subgraphs.iter().map(|subgraph| subgraph.id.as_str()),
            "subgraph",
        ),
    }
}

fn validate_mermaid_generation_scope(output: &MmdsOutput) -> Result<(), MmdsGenerationError> {
    match output.metadata.diagram_type.as_str() {
        "flowchart" | "class" => Ok(()),
        value => Err(MmdsGenerationError::new(format!(
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
    let mut normalized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();

    while normalized.contains("__") {
        normalized = normalized.replace("__", "_");
    }

    normalized = normalized.trim_matches('_').to_string();
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
    output: &MmdsOutput,
    identifiers: &IdentifierMaps,
    lines: &mut Vec<String>,
) -> Result<(), MmdsGenerationError> {
    let mut root_subgraphs: Vec<&MmdsSubgraph> = Vec::new();
    let mut subgraphs_by_parent: HashMap<&str, Vec<&MmdsSubgraph>> = HashMap::new();

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

    let mut standalone_nodes: Vec<&MmdsNode> = Vec::new();
    let mut nodes_by_parent: HashMap<&str, Vec<&MmdsNode>> = HashMap::new();
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
    subgraph: &MmdsSubgraph,
    identifiers: &IdentifierMaps,
    indent: usize,
    subgraphs_by_parent: &HashMap<&str, Vec<&MmdsSubgraph>>,
    nodes_by_parent: &HashMap<&str, Vec<&MmdsNode>>,
    lines: &mut Vec<String>,
) -> Result<(), MmdsGenerationError> {
    let padding = " ".repeat(indent);
    lines.push(format!(
        "{padding}{}",
        render_subgraph_header(subgraph, identifiers)?
    ));

    if let Some(direction) = &subgraph.direction {
        let child_padding = " ".repeat(indent + 4);
        lines.push(format!("{child_padding}direction {direction}"));
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
    subgraph: &MmdsSubgraph,
    identifiers: &IdentifierMaps,
) -> Result<String, MmdsGenerationError> {
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
    output: &MmdsOutput,
    identifiers: &IdentifierMaps,
    lines: &mut Vec<String>,
) -> Result<(), MmdsGenerationError> {
    let mut edges: Vec<(usize, &MmdsEdge)> = output.edges.iter().enumerate().collect();
    edges.sort_by(|(left_index, left), (right_index, right)| {
        compare_edge_ids(&left.id, &right.id).then(left_index.cmp(right_index))
    });

    for (_, edge) in edges {
        lines.push(render_edge(edge, identifiers)?);
    }

    Ok(())
}

fn render_node(node_id: &str, node: &MmdsNode) -> Result<String, MmdsGenerationError> {
    let label = format_structural_label(&node.label);

    match node.shape.as_str() {
        "rectangle" => Ok(format!("{node_id}[{label}]")),
        "round" => Ok(format!("{node_id}({label})")),
        "diamond" => Ok(format!("{node_id}{{{label}}}")),
        "stadium" => Ok(format!("{node_id}([{label}])")),
        "subroutine" => Ok(format!("{node_id}[[{label}]]")),
        "cylinder" => Ok(format!("{node_id}[({label})]")),
        "circle" => Ok(format!("{node_id}(({label}))")),
        "double_circle" => Ok(format!("{node_id}((({label})))")),
        "hexagon" => Ok(format!("{node_id}{{{{{label}}}}}")),
        "trapezoid" => Ok(format!(r#"{node_id}[/{label}\]"#)),
        "inv_trapezoid" => Ok(format!(r#"{node_id}[\{label}/]"#)),
        "asymmetric" => Ok(format!("{node_id}>{label}]")),
        "document" => Ok(render_at_shape(node_id, "doc", &node.label)),
        "documents" => Ok(render_at_shape(node_id, "docs", &node.label)),
        "tagged_document" => Ok(render_at_shape(node_id, "tag-doc", &node.label)),
        "card" => Ok(render_at_shape(node_id, "card", &node.label)),
        "tagged_rect" => Ok(render_at_shape(node_id, "tag-rect", &node.label)),
        "parallelogram" => Ok(render_at_shape(node_id, "lean-r", &node.label)),
        "inv_parallelogram" => Ok(render_at_shape(node_id, "lean-l", &node.label)),
        "manual_input" => Ok(render_at_shape(node_id, "sl-rect", &node.label)),
        "small_circle" => Ok(render_at_shape(node_id, "sm-circ", &node.label)),
        "framed_circle" => Ok(render_at_shape(node_id, "fr-circ", &node.label)),
        "crossed_circle" => Ok(render_at_shape(node_id, "cross-circ", &node.label)),
        "text_block" => Ok(render_at_shape(node_id, "text", &node.label)),
        "fork_join" => Ok(render_at_shape(node_id, "fork", &node.label)),
        value => Err(MmdsGenerationError::new(format!(
            "unsupported node shape '{value}' for node '{}'",
            node.id
        ))),
    }
}

fn render_at_shape(node_id: &str, keyword: &str, label: &str) -> String {
    let escaped = escape_quoted_label(label);
    format!(r#"{node_id}@{{shape: {keyword}, label: "{escaped}"}}"#)
}

fn render_edge(
    edge: &MmdsEdge,
    identifiers: &IdentifierMaps,
) -> Result<String, MmdsGenerationError> {
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
) -> Result<&'a str, MmdsGenerationError> {
    identifiers
        .node_ids
        .get(raw_id)
        .map(String::as_str)
        .ok_or_else(|| MmdsGenerationError::new(format!("unknown node id '{raw_id}'")))
}

fn map_subgraph_id<'a>(
    identifiers: &'a IdentifierMaps,
    raw_id: &str,
) -> Result<&'a str, MmdsGenerationError> {
    identifiers
        .subgraph_ids
        .get(raw_id)
        .map(String::as_str)
        .ok_or_else(|| MmdsGenerationError::new(format!("unknown subgraph id '{raw_id}'")))
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

fn connector_for(edge: &MmdsEdge) -> Result<String, MmdsGenerationError> {
    let minlen = edge.minlen.max(1) as usize;
    let connector = match (
        edge.stroke.as_str(),
        edge.arrow_start.as_str(),
        edge.arrow_end.as_str(),
    ) {
        ("solid", "none", "normal") => format!("{}>", "-".repeat(minlen + 1)),
        ("solid", "none", "none") => "-".repeat(minlen + 2),
        ("solid", "normal", "normal") => format!("<{}>", "-".repeat(minlen + 1)),
        ("solid", "none", "cross") => format!("{}x", "-".repeat(minlen + 1)),
        ("solid", "cross", "cross") => format!("x{}x", "-".repeat(minlen + 1)),
        ("solid", "none", "circle") => format!("{}o", "-".repeat(minlen + 1)),
        ("solid", "circle", "circle") => format!("o{}o", "-".repeat(minlen + 1)),
        ("dotted", "none", "normal") => format!("-{}->", ".".repeat(minlen)),
        ("dotted", "none", "none") => format!("-{}-", ".".repeat(minlen)),
        ("dotted", "normal", "normal") => format!("<-{}->", ".".repeat(minlen)),
        ("dotted", "none", "cross") => format!("-{}-x", ".".repeat(minlen)),
        ("dotted", "none", "circle") => format!("-{}-o", ".".repeat(minlen)),
        ("thick", "none", "normal") => format!("{}>", "=".repeat(minlen + 1)),
        ("thick", "none", "none") => "=".repeat(minlen + 2),
        ("thick", "normal", "normal") => format!("<{}>", "=".repeat(minlen + 1)),
        ("thick", "none", "cross") => format!("{}x", "=".repeat(minlen + 1)),
        ("thick", "none", "circle") => format!("{}o", "=".repeat(minlen + 1)),
        ("invisible", "none", "none") => "~".repeat(minlen + 2),
        _ => {
            return Err(MmdsGenerationError::new(format!(
                "unsupported edge connector combination stroke='{}' arrow_start='{}' arrow_end='{}' on edge '{}'",
                edge.stroke, edge.arrow_start, edge.arrow_end, edge.id
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
