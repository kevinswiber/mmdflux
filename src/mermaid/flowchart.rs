//! Pest parser implementation for Mermaid flowcharts.

#![allow(dead_code)]

use pest::Parser;
use pest_derive::Parser;

use super::ast::{
    ArrowHead, ConnectorSpec, Direction, EdgeSpec, NodeStyleStatement, ShapeSpec, Statement,
    StrokeSpec, SubgraphSpec, Vertex,
};
use super::error::ParseError;
use crate::graph::style::parse_node_style_statement;

#[derive(Parser)]
#[grammar = "mermaid/grammar.pest"]
pub struct FlowchartParser;

/// Parsed flowchart containing direction and statements.
#[derive(Debug, Clone)]
pub struct Flowchart {
    pub direction: Direction,
    pub statements: Vec<Statement>,
}

impl Flowchart {
    /// Get all vertices (from both standalone vertex statements and edges).
    pub fn vertices(&self) -> Vec<&Vertex> {
        let mut result = Vec::new();
        for stmt in &self.statements {
            match stmt {
                Statement::Vertex(v) => result.push(v),
                Statement::Edge(e) => {
                    result.push(&e.from);
                    result.push(&e.to);
                }
                Statement::Subgraph(_) | Statement::NodeStyle(_) => {}
            }
        }
        result
    }

    /// Get all edges.
    pub fn edges(&self) -> Vec<&EdgeSpec> {
        self.statements
            .iter()
            .filter_map(|s| match s {
                Statement::Edge(e) => Some(e),
                _ => None,
            })
            .collect()
    }
}

/// Strip YAML frontmatter delimited by `---` at the start of input.
pub fn strip_frontmatter(input: &str) -> &str {
    let trimmed = input.trim_start();
    if !trimmed.starts_with("---") {
        return input;
    }
    let after_open = &trimmed[3..];
    let after_first_newline = match after_open.find('\n') {
        Some(pos) => &after_open[pos + 1..],
        None => return input,
    };
    for (i, line) in after_first_newline.lines().enumerate() {
        if line.trim() == "---" {
            // Sum byte lengths of lines up to and including the closing ---.
            // The +1 accounts for the newline delimiter stripped by .lines().
            // Clamp to input length in case the final line has no trailing newline.
            let consumed: usize = after_first_newline
                .lines()
                .take(i + 1)
                .map(|l| l.len() + 1)
                .sum();
            let consumed = consumed.min(after_first_newline.len());
            return &after_first_newline[consumed..];
        }
    }
    input
}

/// Pre-process input to strip frontmatter, directives, and unrecognized lines.
fn preprocess(input: &str) -> String {
    let input = strip_frontmatter(input);
    let mut result = String::with_capacity(input.len());
    let mut header_seen = false;

    for line in input.lines() {
        let trimmed = line.trim();

        // Always strip directives
        if trimmed.starts_with("%%{") && trimmed.ends_with("}%%") {
            continue;
        }

        // Header line
        if !header_seen {
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            if first_word.eq_ignore_ascii_case("graph")
                || first_word.eq_ignore_ascii_case("flowchart")
            {
                header_seen = true;
                push_line(&mut result, line);
                continue;
            }
            // Strip non-header lines before the header (e.g. accTitle, comments, blanks)
            continue;
        }

        // Pass through comments
        if trimmed.starts_with("%%") {
            push_line(&mut result, line);
            continue;
        }

        // Pass through empty/whitespace lines
        if trimmed.is_empty() {
            push_line(&mut result, line);
            continue;
        }

        // Known passthrough keywords
        if is_known_passthrough(trimmed) {
            push_line(&mut result, line);
            continue;
        }

        // A flowchart statement starts with an identifier-like char
        if looks_like_flowchart_statement(trimmed) {
            push_line(&mut result, line);
            continue;
        }

        // Unknown line -- strip it
    }

    if input.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn push_line(result: &mut String, line: &str) {
    if !result.is_empty() {
        result.push('\n');
    }
    result.push_str(line);
}

/// Check if `line` starts with `prefix` using ASCII case-insensitive comparison.
fn starts_with_ignore_ascii_case(line: &str, prefix: &str) -> bool {
    line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn is_known_passthrough(line: &str) -> bool {
    starts_with_ignore_ascii_case(line, "style ")
        || starts_with_ignore_ascii_case(line, "classdef ")
        || starts_with_ignore_ascii_case(line, "class ")
        || starts_with_ignore_ascii_case(line, "click ")
        || starts_with_ignore_ascii_case(line, "linkstyle ")
        || starts_with_ignore_ascii_case(line, "direction ")
        || starts_with_ignore_ascii_case(line, "subgraph ")
        || line.eq_ignore_ascii_case("subgraph")
        || line.eq_ignore_ascii_case("end")
        || starts_with_ignore_ascii_case(line, "end ")
        || starts_with_ignore_ascii_case(line, "end;")
}

fn looks_like_flowchart_statement(line: &str) -> bool {
    if starts_with_ignore_ascii_case(line, "acctitle")
        || starts_with_ignore_ascii_case(line, "accdescr")
    {
        return false;
    }
    let first_char = line.chars().next().unwrap_or(' ');
    first_char.is_ascii_alphanumeric() || first_char == '_' || first_char == ';'
}

/// Options for the flowchart parser.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// When true, the parser rejects any syntax it doesn't understand.
    /// When false (default), unrecognized syntax is silently stripped.
    pub strict: bool,
}

/// Parse a flowchart string (permissive mode, the default).
pub fn parse_flowchart(input: &str) -> Result<Flowchart, ParseError> {
    parse_flowchart_with_options(input, &ParseOptions::default())
}

/// Parse a flowchart string with options.
pub fn parse_flowchart_with_options(
    input: &str,
    options: &ParseOptions,
) -> Result<Flowchart, ParseError> {
    let input = if options.strict {
        input.to_string()
    } else {
        preprocess(input)
    };
    let pairs =
        FlowchartParser::parse(Rule::flowchart, &input).map_err(ParseError::from_pest_error)?;

    let mut direction = Direction::TopDown;
    let mut statements = Vec::new();
    let mut subgraph_counter = 0usize;

    for pair in pairs.filter(|p| p.as_rule() == Rule::flowchart) {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::header => {
                    direction = inner
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::direction)
                        .and_then(|p| Direction::from_str(p.as_str()))
                        .unwrap_or(Direction::TopDown);
                }
                Rule::statement => {
                    statements.extend(parse_statement(inner, &mut subgraph_counter));
                }
                _ => {}
            }
        }
    }

    Ok(Flowchart {
        direction,
        statements,
    })
}

fn parse_statement(
    pair: pest::iterators::Pair<Rule>,
    subgraph_counter: &mut usize,
) -> Vec<Statement> {
    if let Some(style_statement) = parse_node_style_statement(pair.as_str()) {
        return vec![Statement::NodeStyle(NodeStyleStatement {
            node_id: style_statement.node_id,
            style: style_statement.style,
        })];
    }

    pair.into_inner()
        .flat_map(|inner| match inner.as_rule() {
            Rule::vertex_statement => parse_vertex_statement(inner),
            Rule::subgraph_stmt => {
                vec![Statement::Subgraph(parse_subgraph(inner, subgraph_counter))]
            }
            Rule::direction_stmt => vec![],
            _ => vec![],
        })
        .collect()
}

/// Strip surrounding double quotes from text (Mermaid convention).
fn strip_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

/// Strip surrounding single or double quotes from text.
fn strip_quotes_any(s: &str) -> &str {
    if let Some(stripped) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return stripped;
    }
    if let Some(stripped) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return stripped;
    }
    s
}

fn parse_subgraph(pair: pest::iterators::Pair<Rule>, counter: &mut usize) -> SubgraphSpec {
    let mut id = String::new();
    let mut title = None;
    let mut has_explicit_id = false;
    let mut body_statements = Vec::new();
    let mut dir = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::subgraph_spec => {
                for spec_inner in inner.into_inner() {
                    match spec_inner.as_rule() {
                        Rule::subgraph_id => {
                            id = spec_inner.as_str().to_string();
                            has_explicit_id = true;
                        }
                        Rule::subgraph_title_bracket => {
                            title = spec_inner
                                .into_inner()
                                .find(|t| t.as_rule() == Rule::subgraph_title_text)
                                .map(|t| strip_quotes(t.as_str()).to_string());
                        }
                        Rule::subgraph_quoted_title => {
                            title = spec_inner
                                .into_inner()
                                .find(|t| t.as_rule() == Rule::subgraph_quoted_title_text)
                                .map(|t| t.as_str().to_string());
                        }
                        _ => {}
                    }
                }
            }
            Rule::subgraph_body_line => {
                for body_inner in inner.into_inner() {
                    if body_inner.as_rule() == Rule::statement {
                        if let Some(style_statement) =
                            parse_node_style_statement(body_inner.as_str())
                        {
                            body_statements.push(Statement::NodeStyle(NodeStyleStatement {
                                node_id: style_statement.node_id,
                                style: style_statement.style,
                            }));
                            continue;
                        }

                        for stmt_inner in body_inner.into_inner() {
                            match stmt_inner.as_rule() {
                                Rule::direction_stmt => {
                                    dir = stmt_inner
                                        .into_inner()
                                        .find(|p| p.as_rule() == Rule::direction_value)
                                        .and_then(|p| Direction::from_str(p.as_str()));
                                }
                                Rule::vertex_statement => {
                                    body_statements.extend(parse_vertex_statement(stmt_inner));
                                }
                                Rule::subgraph_stmt => {
                                    body_statements.push(Statement::Subgraph(parse_subgraph(
                                        stmt_inner, counter,
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if !has_explicit_id {
        id = format!("subGraph{}", counter);
        *counter += 1;
    }

    SubgraphSpec {
        title: title.unwrap_or_else(|| id.clone()),
        id,
        statements: body_statements,
        dir,
    }
}

fn parse_vertex_statement(pair: pest::iterators::Pair<Rule>) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut current_nodes: Vec<Vertex> = Vec::new();
    let mut segments: Vec<(ConnectorSpec, Vec<Vertex>)> = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::node_group => {
                if segments.is_empty() {
                    // This is the first node group (source nodes)
                    current_nodes = parse_node_group(inner);
                }
            }
            Rule::edge_segment => {
                let (connector, nodes) = parse_edge_segment(inner);
                segments.push((connector, nodes));
            }
            _ => {}
        }
    }

    if segments.is_empty() {
        // No edges, just standalone node(s)
        statements.extend(current_nodes.into_iter().map(Statement::Vertex));
    } else {
        // Process chain of edges
        let mut source_nodes = current_nodes;

        for (connector, target_nodes) in segments {
            // Create edges from each source to each target (cartesian product for &)
            for source in &source_nodes {
                for target in &target_nodes {
                    statements.push(Statement::Edge(EdgeSpec {
                        from: source.clone(),
                        connector: connector.clone(),
                        to: target.clone(),
                    }));
                }
            }
            // For chains, the targets become the sources for the next segment
            source_nodes = target_nodes;
        }
    }

    statements
}

fn parse_node_group(pair: pest::iterators::Pair<Rule>) -> Vec<Vertex> {
    pair.into_inner()
        .filter(|inner| inner.as_rule() == Rule::node)
        .map(parse_node)
        .collect()
}

fn parse_edge_segment(pair: pest::iterators::Pair<Rule>) -> (ConnectorSpec, Vec<Vertex>) {
    let mut connector = None;
    let mut nodes = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::edge_connector => connector = Some(parse_connector(inner)),
            Rule::node_group => nodes = parse_node_group(inner),
            _ => {}
        }
    }

    let connector = connector.unwrap_or(ConnectorSpec {
        stroke: StrokeSpec::Solid,
        left: ArrowHead::None,
        right: ArrowHead::Normal,
        length: 1,
        label: None,
    });

    (connector, nodes)
}

fn parse_connector(pair: pest::iterators::Pair<Rule>) -> ConnectorSpec {
    let mut stroke = StrokeSpec::Solid;
    let mut left = ArrowHead::None;
    let mut right = ArrowHead::None;
    let mut length: usize = 1;
    let mut label = None;

    for inner in pair.into_inner() {
        let (link_stroke, length_rule) = match inner.as_rule() {
            Rule::link_invisible => {
                stroke = StrokeSpec::Invisible;
                left = ArrowHead::None;
                right = ArrowHead::None;
                length = 1;
                continue;
            }
            Rule::link_solid => (StrokeSpec::Solid, Rule::solid_dashes),
            Rule::link_dotted => (StrokeSpec::Dotted, Rule::dotted_dots),
            Rule::link_thick => (StrokeSpec::Thick, Rule::thick_equals),
            Rule::link_solid_labeled => {
                stroke = StrokeSpec::Solid;
                let (l, r, len, lbl) = parse_labeled_link(
                    inner,
                    Rule::solid_dashes,
                    Rule::edge_label_inline_text_solid,
                );
                left = l;
                right = r;
                length = len;
                label = lbl.or(label);
                continue;
            }
            Rule::link_dotted_labeled => {
                stroke = StrokeSpec::Dotted;
                let (l, r, len, lbl) = parse_labeled_link(
                    inner,
                    Rule::dotted_dots,
                    Rule::edge_label_inline_text_dotted,
                );
                left = l;
                right = r;
                length = len;
                label = lbl.or(label);
                continue;
            }
            Rule::link_thick_labeled => {
                stroke = StrokeSpec::Thick;
                let (l, r, len, lbl) = parse_labeled_link(
                    inner,
                    Rule::thick_equals,
                    Rule::edge_label_inline_text_thick,
                );
                left = l;
                right = r;
                length = len;
                label = lbl.or(label);
                continue;
            }
            Rule::edge_label => {
                label = inner
                    .into_inner()
                    .find(|t| t.as_rule() == Rule::edge_label_text)
                    .and_then(|t| normalize_edge_label(t.as_str()));
                continue;
            }
            _ => continue,
        };
        stroke = link_stroke;
        (left, right, length) = parse_link_parts(inner, length_rule);
    }

    ConnectorSpec {
        stroke,
        left,
        right,
        length,
        label,
    }
}

fn parse_labeled_link(
    link: pest::iterators::Pair<Rule>,
    length_rule: Rule,
    label_rule: Rule,
) -> (ArrowHead, ArrowHead, usize, Option<String>) {
    let mut left = ArrowHead::None;
    let mut right = ArrowHead::None;
    let mut length = 1;
    let mut label = None;

    for part in link.into_inner() {
        match part.as_rule() {
            Rule::link_solid_start | Rule::link_dotted_start | Rule::link_thick_start => {
                left = parse_start_arrow_head(part.as_str());
            }
            Rule::link_solid_end | Rule::link_dotted_end | Rule::link_thick_end => {
                let (_, r, len) = parse_link_parts(part, length_rule);
                right = r;
                length = len;
            }
            rule if rule == label_rule => {
                label = normalize_edge_label(part.as_str());
            }
            _ => {}
        }
    }

    (left, right, length, label)
}

/// Parse common link parts: arrow heads and length from the line character rule.
fn parse_link_parts(
    link: pest::iterators::Pair<Rule>,
    length_rule: Rule,
) -> (ArrowHead, ArrowHead, usize) {
    let mut left = ArrowHead::None;
    let mut right = ArrowHead::None;
    let mut length = 1;

    for part in link.into_inner() {
        match part.as_rule() {
            Rule::arrow_left => left = parse_arrow_head(part.as_str()),
            Rule::arrow_right => right = parse_arrow_head(part.as_str()),
            rule if rule == length_rule => length = part.as_str().len(),
            _ => {}
        }
    }

    (left, right, length)
}

fn parse_arrow_head(s: &str) -> ArrowHead {
    match s {
        ">" | "<" => ArrowHead::Normal,
        "x" => ArrowHead::Cross,
        "o" => ArrowHead::Circle,
        _ => ArrowHead::None,
    }
}

fn parse_start_arrow_head(s: &str) -> ArrowHead {
    match s.chars().next() {
        Some('<') => ArrowHead::Normal,
        Some('x') => ArrowHead::Cross,
        Some('o') => ArrowHead::Circle,
        _ => ArrowHead::None,
    }
}

fn normalize_edge_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(strip_quotes(trimmed).to_string())
}

fn parse_node(pair: pest::iterators::Pair<Rule>) -> Vertex {
    let mut id = String::new();
    let mut shape = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                id = inner.as_str().to_string();
            }
            Rule::shape => {
                shape = parse_shape(inner);
            }
            _ => {}
        }
    }

    Vertex { id, shape }
}

fn parse_shape(pair: pest::iterators::Pair<Rule>) -> Option<ShapeSpec> {
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::shape_config {
            return parse_shape_config(inner);
        }
        let (text_rule, constructor): (Rule, fn(String) -> ShapeSpec) = match inner.as_rule() {
            Rule::shape_rect => (Rule::text_rect, ShapeSpec::Rectangle),
            Rule::shape_round => (Rule::text_round, ShapeSpec::Round),
            Rule::shape_diamond => (Rule::text_diamond, ShapeSpec::Diamond),
            Rule::shape_stadium => (Rule::text_stadium, ShapeSpec::Stadium),
            Rule::shape_subroutine => (Rule::text_subroutine, ShapeSpec::Subroutine),
            Rule::shape_cylinder => (Rule::text_cylinder, ShapeSpec::Cylinder),
            Rule::shape_circle => (Rule::text_circle, ShapeSpec::Circle),
            Rule::shape_double_circle => (Rule::text_double_circle, ShapeSpec::DoubleCircle),
            Rule::shape_hexagon => (Rule::text_hexagon, ShapeSpec::Hexagon),
            Rule::shape_asymmetric => (Rule::text_asymmetric, ShapeSpec::Asymmetric),
            Rule::shape_trapezoid => (Rule::text_trapezoid, ShapeSpec::Trapezoid),
            Rule::shape_inv_trapezoid => (Rule::text_inv_trapezoid, ShapeSpec::InvTrapezoid),
            _ => continue,
        };
        for text in inner.into_inner() {
            if text.as_rule() == text_rule {
                return Some(constructor(strip_quotes(text.as_str()).to_string()));
            }
        }
    }
    None
}

fn parse_shape_config(pair: pest::iterators::Pair<Rule>) -> Option<ShapeSpec> {
    let raw = pair
        .into_inner()
        .find(|p| p.as_rule() == Rule::shape_config_body)
        .map(|p| p.as_str())
        .unwrap_or("");
    let mut shape_keyword = None;
    let mut label_value = None;

    let mut token = String::new();
    let mut quote = None;
    let flush_token = |token: &mut String,
                       shape_keyword: &mut Option<String>,
                       label_value: &mut Option<String>| {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            token.clear();
            return;
        }
        let (key, value) = trimmed
            .split_once(':')
            .or_else(|| trimmed.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()))
            .unwrap_or((trimmed, ""));
        if key.is_empty() {
            token.clear();
            return;
        }
        let key = key.to_lowercase();
        let value = strip_quotes_any(value).trim().to_string();
        match key.as_str() {
            "shape" => {
                if !value.is_empty() {
                    *shape_keyword = Some(value);
                }
            }
            "label" | "text" => {
                *label_value = Some(value);
            }
            _ => {}
        }
        token.clear();
    };

    for ch in raw.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
                token.push(ch);
            }
            None => match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    token.push(ch);
                }
                ',' | ';' => {
                    flush_token(&mut token, &mut shape_keyword, &mut label_value);
                }
                _ => token.push(ch),
            },
        }
    }
    flush_token(&mut token, &mut shape_keyword, &mut label_value);

    let label = label_value.unwrap_or_default();
    let shape = shape_keyword.unwrap_or_else(|| "rect".to_string());
    Some(shape_from_keyword(&shape, label))
}

fn shape_from_keyword(keyword: &str, label: String) -> ShapeSpec {
    let key = keyword.trim().to_lowercase();
    match key.as_str() {
        "rect" | "rectangle" | "lin-rect" | "st-rect" | "div-rect" | "win-pane" => {
            ShapeSpec::Rectangle(label)
        }
        "round" | "rounded" => ShapeSpec::Round(label),
        "stadium" | "pill" => ShapeSpec::Stadium(label),
        "sub" | "subroutine" => ShapeSpec::Subroutine(label),
        "cyl" | "cylinder" | "h-cyl" | "lin-cyl" | "bow-rect" => ShapeSpec::Cylinder(label),
        "circle" => ShapeSpec::Circle(label),
        "double-circ" | "double-circle" | "doublecirc" | "dbl-circ" => {
            ShapeSpec::DoubleCircle(label)
        }
        "diamond" | "rhombus" | "decision" => ShapeSpec::Diamond(label),
        "hex" | "hexagon" => ShapeSpec::Hexagon(label),
        "trap" | "trapezoid" | "trap-t" | "curv-trap" => ShapeSpec::Trapezoid(label),
        "inv-trap" | "inv-trapezoid" | "trap-b" => ShapeSpec::InvTrapezoid(label),
        "sl-rect" | "manual" | "manual-input" => ShapeSpec::ManualInput(label),
        "parallelogram" | "lean-r" | "lean-right" | "in-out" => ShapeSpec::Parallelogram(label),
        "inv-parallelogram" | "inv-sl-rect" | "lean-l" | "lean-left" | "out-in" => {
            ShapeSpec::InvParallelogram(label)
        }
        "flag" | "asymmetric" => ShapeSpec::Asymmetric(label),
        "doc" | "document" | "lin-doc" => ShapeSpec::Document(label),
        "docs" => ShapeSpec::Documents(label),
        "tag-doc" => ShapeSpec::TaggedDocument(label),
        "card" => ShapeSpec::Card(label),
        "tag-rect" => ShapeSpec::TaggedRect(label),
        "text" => ShapeSpec::TextBlock(label),
        "fork" | "join" => ShapeSpec::ForkJoin(label),
        "sm-circ" => ShapeSpec::SmallCircle(label),
        "fr-circ" => ShapeSpec::FramedCircle(label),
        "cross-circ" => ShapeSpec::CrossedCircle(label),
        "f-circ" => ShapeSpec::SmallCircle(label),
        // Degenerate / unsupported shapes: fall back to rectangle with label
        "cloud" | "bolt" | "bang" | "icon" | "image" | "hourglass" | "tri" | "flip-tri"
        | "notch-pent" | "delay" | "display" => ShapeSpec::Rectangle(label),
        _ => ShapeSpec::Rectangle(label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Strict mode tests (Task 3.2)
    #[test]
    fn test_strict_mode_rejects_directive() {
        let input = "%%{init: {}}%%\ngraph TD\nA --> B\n";
        let opts = ParseOptions { strict: true };
        let result = parse_flowchart_with_options(input, &opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_permissive_mode_accepts_directive() {
        let input = "%%{init: {}}%%\ngraph TD\nA --> B\n";
        let opts = ParseOptions { strict: false };
        let result = parse_flowchart_with_options(input, &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_is_permissive() {
        let input = "%%{init: {}}%%\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input);
        assert!(result.is_ok());
    }

    // Permissive preprocessing tests (Task 3.1)
    #[test]
    fn test_permissive_strips_acc_title() {
        let input = "graph TD\naccTitle: My Diagram\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_permissive_strips_acc_descr() {
        let input = "graph TD\naccDescr: Description here\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_permissive_strips_unknown_line() {
        let input = "graph TD\n@startuml\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_permissive_preserves_all_known_syntax() {
        let input = concat!(
            "graph TD\n",
            "A[Start] --> B{Decision}\n",
            "B -->|yes| C(Process)\n",
            "B -->|no| D\n",
            "style A fill:#f9f\n",
            "classDef warning fill:#ff0\n",
            "class B warning\n",
            "%% comment\n",
            "subgraph sg1[Group]\n",
            "E --> F\n",
            "end\n",
        );
        let result = parse_flowchart(input).unwrap();
        let edge_count = count_edges_recursive(&result.statements);
        assert_eq!(edge_count, 4);
    }

    fn count_edges_recursive(stmts: &[Statement]) -> usize {
        stmts
            .iter()
            .map(|s| match s {
                Statement::Edge(_) => 1,
                Statement::Subgraph(sg) => count_edges_recursive(&sg.statements),
                _ => 0,
            })
            .sum()
    }

    // Directive stripping tests (Task 1.1)
    #[test]
    fn test_strip_single_line_directive() {
        let input = "%%{init: {\"theme\": \"dark\"}}%%\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_strip_directive_with_spaces() {
        let input = "  %%{ init: { 'theme': 'forest' } }%%  \ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_strip_multiple_directives() {
        let input = "%%{init: {}}%%\n%%{init: {\"flowchart\": {}}}%%\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_regular_comments_preserved() {
        let input = "graph TD\n%% This is a comment\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    // Frontmatter stripping tests (Task 1.2)
    #[test]
    fn test_strip_yaml_frontmatter() {
        let input = "---\nconfig:\n  theme: dark\n---\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_strip_empty_frontmatter() {
        let input = "---\n---\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_strip_frontmatter_with_directive() {
        let input = "---\nconfig:\n  theme: dark\n---\n%%{init: {}}%%\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_no_frontmatter_still_works() {
        let input = "graph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_strip_frontmatter_no_trailing_newline() {
        // Closing --- without trailing newline should not panic
        let input = "---\ntitle: x\n---";
        let result = strip_frontmatter(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_leading_comment_before_header() {
        let input = "%% this is a comment\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_leading_blank_lines_before_header() {
        let input = "\n\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_leading_comment_and_blank_before_header() {
        let input = "\n%% comment\n\ngraph TD\nA --> B\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    // Shape mapping fix tests (Task 4.1)
    #[test]
    fn test_sl_rect_maps_to_manual_input() {
        let result = parse_flowchart("graph TD\nA@{shape: sl-rect, label: \"Test\"}\n").unwrap();
        assert_eq!(
            result.vertices()[0].shape,
            Some(ShapeSpec::ManualInput("Test".to_string()))
        );
    }

    #[test]
    fn test_lean_right_maps_to_parallelogram() {
        let result = parse_flowchart("graph TD\nA@{shape: lean-r, label: \"Test\"}\n").unwrap();
        assert_eq!(
            result.vertices()[0].shape,
            Some(ShapeSpec::Parallelogram("Test".to_string()))
        );
    }

    #[test]
    fn test_lean_left_maps_to_inv_parallelogram() {
        let result = parse_flowchart("graph TD\nA@{shape: lean-l, label: \"Test\"}\n").unwrap();
        assert_eq!(
            result.vertices()[0].shape,
            Some(ShapeSpec::InvParallelogram("Test".to_string()))
        );
    }

    // Invisible edge tests (Task 2.4)
    #[test]
    fn test_parse_invisible_edge() {
        let result = parse_flowchart("graph TD\nA ~~~ B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Invisible);
    }

    #[test]
    fn test_parse_invisible_edge_no_spaces() {
        let result = parse_flowchart("graph TD\nA~~~B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Invisible);
    }

    #[test]
    fn test_parse_invisible_edge_in_chain() {
        let result = parse_flowchart("graph TD\nA --> B ~~~ C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Solid);
        assert_eq!(edges[1].connector.stroke, StrokeSpec::Invisible);
    }

    // Class annotation tests (Task 2.3)
    #[test]
    fn test_parse_node_with_class_annotation() {
        let result = parse_flowchart("graph TD\nA:::highlight --> B\n").unwrap();
        assert_eq!(result.edges().len(), 1);
        assert_eq!(result.edges()[0].from.id, "A");
        assert_eq!(result.edges()[0].to.id, "B");
    }

    #[test]
    fn test_parse_node_shape_with_class_annotation() {
        let result = parse_flowchart("graph TD\nA[Start]:::highlight --> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(
            edges[0].from.shape,
            Some(ShapeSpec::Rectangle("Start".to_string()))
        );
    }

    #[test]
    fn test_parse_multiple_class_annotations() {
        let result = parse_flowchart("graph TD\nA:::cls1 --> B:::cls2\n").unwrap();
        assert_eq!(result.edges().len(), 1);
        assert_eq!(result.edges()[0].from.id, "A");
        assert_eq!(result.edges()[0].to.id, "B");
    }

    #[test]
    fn test_parse_class_annotation_standalone_node() {
        let result = parse_flowchart("graph TD\nA:::highlight\n").unwrap();
        assert_eq!(result.vertices().len(), 1);
        assert_eq!(result.vertices()[0].id, "A");
    }

    // Expanded identifier tests (Task 2.2)
    #[test]
    fn test_parse_numeric_id() {
        let result = parse_flowchart("graph TD\n123 --> 456\n").unwrap();
        assert_eq!(result.edges()[0].from.id, "123");
        assert_eq!(result.edges()[0].to.id, "456");
    }

    #[test]
    fn test_parse_hyphenated_id() {
        let result = parse_flowchart("graph TD\nnode-1 --> node-2\n").unwrap();
        assert_eq!(result.edges()[0].from.id, "node-1");
        assert_eq!(result.edges()[0].to.id, "node-2");
    }

    #[test]
    fn test_parse_dotted_id() {
        let result = parse_flowchart("graph TD\nmy.node --> other.node\n").unwrap();
        assert_eq!(result.edges()[0].from.id, "my.node");
        assert_eq!(result.edges()[0].to.id, "other.node");
    }

    #[test]
    fn test_parse_mixed_id() {
        let result = parse_flowchart("graph TD\nstep1-process.v2 --> end_node\n").unwrap();
        assert_eq!(result.edges()[0].from.id, "step1-process.v2");
    }

    #[test]
    fn test_parse_numeric_id_with_shape() {
        let result = parse_flowchart("graph TD\n123[Start] --> 456[End]\n").unwrap();
        assert_eq!(result.vertices()[0].id, "123");
        assert_eq!(result.vertices()[1].id, "456");
    }

    #[test]
    fn test_hyphen_id_does_not_consume_arrow() {
        let result = parse_flowchart("graph TD\nA --> B\n").unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_hyphenated_id_with_arrow() {
        let result = parse_flowchart("graph TD\nnode-1 --> node-2\n").unwrap();
        assert_eq!(result.edges().len(), 1);
        assert_eq!(result.edges()[0].from.id, "node-1");
    }

    // Default direction tests (Task 2.1)
    #[test]
    fn test_parse_graph_no_direction() {
        let result = parse_flowchart("graph\nA --> B\n").unwrap();
        assert_eq!(result.direction, Direction::TopDown);
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_parse_flowchart_no_direction() {
        let result = parse_flowchart("flowchart\nA --> B\n").unwrap();
        assert_eq!(result.direction, Direction::TopDown);
        assert_eq!(result.edges().len(), 1);
    }

    // Phase 1: Header tests
    #[test]
    fn test_parse_graph_td() {
        let result = parse_flowchart("graph TD\n").unwrap();
        assert_eq!(result.direction, Direction::TopDown);
    }

    #[test]
    fn test_parse_graph_lr() {
        let result = parse_flowchart("graph LR\n").unwrap();
        assert_eq!(result.direction, Direction::LeftRight);
    }

    #[test]
    fn test_parse_flowchart_tb() {
        let result = parse_flowchart("flowchart TB\n").unwrap();
        assert_eq!(result.direction, Direction::TopDown);
    }

    #[test]
    fn test_parse_flowchart_rl() {
        let result = parse_flowchart("flowchart RL\n").unwrap();
        assert_eq!(result.direction, Direction::RightLeft);
    }

    #[test]
    fn test_parse_graph_bt() {
        let result = parse_flowchart("graph BT\n").unwrap();
        assert_eq!(result.direction, Direction::BottomTop);
    }

    #[test]
    fn test_case_insensitive() {
        let result = parse_flowchart("GRAPH td\n").unwrap();
        assert_eq!(result.direction, Direction::TopDown);
    }

    // Phase 2: Node tests
    #[test]
    fn test_parse_node_bare() {
        let result = parse_flowchart("graph TD\nA\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "A");
        assert!(vertices[0].shape.is_none());
    }

    #[test]
    fn test_parse_node_rectangle() {
        let result = parse_flowchart("graph TD\nA[Hello World]\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "A");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Rectangle("Hello World".to_string()))
        );
    }

    #[test]
    fn test_parse_node_round() {
        let result = parse_flowchart("graph TD\nB(Rounded Node)\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "B");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Round("Rounded Node".to_string()))
        );
    }

    #[test]
    fn test_parse_node_diamond() {
        let result = parse_flowchart("graph TD\nC{Decision?}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "C");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Diamond("Decision?".to_string()))
        );
    }

    #[test]
    fn test_parse_node_shape_config_document() {
        let result = parse_flowchart("graph TD\nA@{shape: doc, label: \"Doc\"}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "A");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Document("Doc".to_string()))
        );
    }

    #[test]
    fn test_parse_node_shape_config_small_circle_unlabeled() {
        let result = parse_flowchart("graph TD\nJ@{shape: sm-circ}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "J");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::SmallCircle("".to_string()))
        );
    }

    #[test]
    fn test_parse_node_shape_config_label_only_defaults_to_rect() {
        let result = parse_flowchart("graph TD\nA@{label: \"Only\"}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].id, "A");
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Rectangle("Only".to_string()))
        );
    }

    #[test]
    fn test_parse_multiple_nodes() {
        let result = parse_flowchart("graph TD\nA[Start]\nB(Process)\nC{End?}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(vertices.len(), 3);
        assert_eq!(vertices[0].id, "A");
        assert_eq!(vertices[1].id, "B");
        assert_eq!(vertices[2].id, "C");
    }

    #[test]
    fn test_parse_node_with_underscore() {
        let result = parse_flowchart("graph TD\nmy_node[Label]\n").unwrap();
        assert_eq!(result.vertices()[0].id, "my_node");
    }

    #[test]
    fn test_parse_node_with_numbers() {
        let result = parse_flowchart("graph TD\nnode123[Label]\n").unwrap();
        assert_eq!(result.vertices()[0].id, "node123");
    }

    // Phase 3: Edge tests
    #[test]
    fn test_parse_solid_arrow() {
        let result = parse_flowchart("graph TD\nA --> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Solid);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_parse_solid_arrow_with_label() {
        let result = parse_flowchart("graph TD\nA -->|yes| B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[0].connector.label(), Some("yes"));
    }

    #[test]
    fn test_parse_solid_arrow_with_inline_label() {
        let result = parse_flowchart("graph TD\nA -- yes --> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[0].connector.label(), Some("yes"));
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Solid);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_parse_dotted_arrow() {
        let result = parse_flowchart("graph TD\nA -.-> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Dotted);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_parse_dotted_arrow_with_inline_label() {
        let result = parse_flowchart("graph TD\nA -. no .-> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Dotted);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
        assert_eq!(edges[0].connector.label(), Some("no"));
    }

    #[test]
    fn test_parse_thick_arrow() {
        let result = parse_flowchart("graph TD\nA ==> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Thick);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_parse_thick_arrow_with_inline_label() {
        let result = parse_flowchart("graph TD\nA == \"maybe\" ==> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Thick);
        assert_eq!(edges[0].connector.right, ArrowHead::Normal);
        assert_eq!(edges[0].connector.label(), Some("maybe"));
    }

    #[test]
    fn test_parse_open_line() {
        let result = parse_flowchart("graph TD\nA --- B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].connector.stroke, StrokeSpec::Solid);
        assert_eq!(edges[0].connector.right, ArrowHead::None);
        assert!(!edges[0].connector.has_arrow());
    }

    #[test]
    fn test_parse_edge_with_node_shapes() {
        let result = parse_flowchart("graph TD\nA[Start] --> B{Decision}\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(
            edges[0].from.shape,
            Some(ShapeSpec::Rectangle("Start".to_string()))
        );
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(
            edges[0].to.shape,
            Some(ShapeSpec::Diamond("Decision".to_string()))
        );
    }

    #[test]
    fn test_parse_multiple_edges() {
        let result = parse_flowchart("graph TD\nA --> B\nB --> C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[1].from.id, "B");
        assert_eq!(edges[1].to.id, "C");
    }

    #[test]
    fn test_parse_comment() {
        let result = parse_flowchart("graph TD\n%% This is a comment\nA --> B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
    }

    // Phase 4: Chain and ampersand tests
    #[test]
    fn test_parse_chain() {
        let result = parse_flowchart("graph TD\nA --> B --> C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[1].from.id, "B");
        assert_eq!(edges[1].to.id, "C");
    }

    #[test]
    fn test_parse_long_chain() {
        let result = parse_flowchart("graph TD\nA --> B --> C --> D\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 3);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[1].from.id, "B");
        assert_eq!(edges[1].to.id, "C");
        assert_eq!(edges[2].from.id, "C");
        assert_eq!(edges[2].to.id, "D");
    }

    #[test]
    fn test_parse_ampersand_source() {
        let result = parse_flowchart("graph TD\nA & B --> C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "C");
        assert_eq!(edges[1].from.id, "B");
        assert_eq!(edges[1].to.id, "C");
    }

    #[test]
    fn test_parse_ampersand_target() {
        let result = parse_flowchart("graph TD\nA --> B & C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from.id, "A");
        assert_eq!(edges[0].to.id, "B");
        assert_eq!(edges[1].from.id, "A");
        assert_eq!(edges[1].to.id, "C");
    }

    #[test]
    fn test_parse_ampersand_both() {
        let result = parse_flowchart("graph TD\nA & B --> C & D\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 4);
        // A -> C, A -> D, B -> C, B -> D
        let edge_pairs: Vec<(&str, &str)> = edges
            .iter()
            .map(|e| (e.from.id.as_str(), e.to.id.as_str()))
            .collect();
        assert!(edge_pairs.contains(&("A", "C")));
        assert!(edge_pairs.contains(&("A", "D")));
        assert!(edge_pairs.contains(&("B", "C")));
        assert!(edge_pairs.contains(&("B", "D")));
    }

    #[test]
    fn test_parse_chain_with_labels() {
        let result = parse_flowchart("graph TD\nA -->|step1| B -->|step2| C\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].connector.label(), Some("step1"));
        assert_eq!(edges[1].connector.label(), Some("step2"));
    }

    // Subgraph tests
    #[test]
    fn test_parse_subgraph_with_title() {
        let input = "graph TD\nsubgraph sg1[My Group]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        let subgraphs: Vec<_> = result
            .statements
            .iter()
            .filter(|s| matches!(s, Statement::Subgraph(_)))
            .collect();
        assert_eq!(subgraphs.len(), 1, "Expected 1 subgraph statement");
        match &subgraphs[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "sg1");
                assert_eq!(sg.title, "My Group");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_subgraph_without_title() {
        let input = "graph TD\nsubgraph sg1\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        let subgraphs: Vec<_> = result
            .statements
            .iter()
            .filter(|s| matches!(s, Statement::Subgraph(_)))
            .collect();
        assert_eq!(subgraphs.len(), 1, "Expected 1 subgraph statement");
        match &subgraphs[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "sg1");
                assert_eq!(sg.title, "sg1"); // title defaults to id
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_subgraph_quoted_title_strips_quotes() {
        let input = "graph TD\nsubgraph sg1[\"My Group\"]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.title, "My Group");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_subgraph_space_title_for_untitled() {
        let input = "graph TD\nsubgraph sg1[\" \"]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.title, " ");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_node_quoted_text_strips_quotes() {
        let result = parse_flowchart("graph TD\nA[\"Hello World\"]\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Rectangle("Hello World".to_string()))
        );
    }

    #[test]
    fn test_parse_node_quoted_rect_text_allows_closing_bracket_inside() {
        let result = parse_flowchart("graph TD\nA[\"addSubGraph(nodes[], id, title)\"]\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Rectangle(
                "addSubGraph(nodes[], id, title)".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_edge_target_quoted_rect_text_allows_closing_bracket_inside() {
        let input = "graph TD\nDBMethods --> addSubGraph[\"addSubGraph(nodes[], id, title)\"]\n";
        let result = parse_flowchart(input).unwrap();
        let edges = result.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].to.shape,
            Some(ShapeSpec::Rectangle(
                "addSubGraph(nodes[], id, title)".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_node_round_quoted_text_strips_quotes() {
        let result = parse_flowchart("graph TD\nA(\"Rounded\")\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Round("Rounded".to_string()))
        );
    }

    #[test]
    fn test_parse_node_diamond_quoted_text_strips_quotes() {
        let result = parse_flowchart("graph TD\nA{\"Decision?\"}\n").unwrap();
        let vertices = result.vertices();
        assert_eq!(
            vertices[0].shape,
            Some(ShapeSpec::Diamond("Decision?".to_string()))
        );
    }

    #[test]
    fn test_parse_edge_label_quoted_text_strips_quotes() {
        let result = parse_flowchart("graph TD\nA -->|\"yes\"| B\n").unwrap();
        let edges = result.edges();
        assert_eq!(edges[0].connector.label(), Some("yes"));
    }

    // Additional node shape tests (Task 2.1)
    #[test]
    fn test_parse_stadium_shape() {
        let fc = parse_flowchart("graph TD\nA([Stadium])\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Stadium(_))
        ));
    }

    #[test]
    fn test_parse_subroutine_shape() {
        let fc = parse_flowchart("graph TD\nA[[Subroutine]]\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Subroutine(_))
        ));
    }

    #[test]
    fn test_parse_cylinder_shape() {
        let fc = parse_flowchart("graph TD\nA[(Database)]\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Cylinder(_))
        ));
    }

    #[test]
    fn test_parse_circle_shape() {
        let fc = parse_flowchart("graph TD\nA((Circle))\n").unwrap();
        assert!(matches!(fc.vertices()[0].shape, Some(ShapeSpec::Circle(_))));
    }

    #[test]
    fn test_parse_double_circle_shape() {
        let fc = parse_flowchart("graph TD\nA(((Double)))\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::DoubleCircle(_))
        ));
    }

    #[test]
    fn test_parse_hexagon_shape() {
        let fc = parse_flowchart("graph TD\nA{{Hexagon}}\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Hexagon(_))
        ));
    }

    #[test]
    fn test_parse_asymmetric_shape() {
        let fc = parse_flowchart("graph TD\nA>Flag]\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Asymmetric(_))
        ));
    }

    #[test]
    fn test_parse_trapezoid_shape() {
        let fc = parse_flowchart("graph TD\nA[/Trapezoid\\]\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::Trapezoid(_))
        ));
    }

    #[test]
    fn test_parse_inv_trapezoid_shape() {
        let fc = parse_flowchart("graph TD\nA[\\InvTrapezoid/]\n").unwrap();
        assert!(matches!(
            fc.vertices()[0].shape,
            Some(ShapeSpec::InvTrapezoid(_))
        ));
    }

    #[test]
    fn test_parse_flowchart_keeps_node_style_statements() {
        let input = "graph TD\nA[Alpha]\nstyle A fill:#ffeeaa,stroke:#333,color:#111\n";
        let chart = parse_flowchart(input).unwrap();

        assert!(
            chart
                .statements
                .iter()
                .any(|stmt| matches!(stmt, Statement::NodeStyle(_)))
        );
    }

    #[test]
    fn test_parse_style_statement_extracts_supported_node_properties() {
        let input = "graph TD\nstyle A fill:#ffeeaa,stroke:#333,color:#111\n";
        let chart = parse_flowchart(input).unwrap();

        let style = chart
            .statements
            .iter()
            .find_map(|stmt| match stmt {
                Statement::NodeStyle(style) => Some(style),
                _ => None,
            })
            .unwrap();

        assert_eq!(style.node_id, "A");
        assert_eq!(style.style.fill.as_ref().unwrap().raw(), "#ffeeaa");
        assert_eq!(style.style.stroke.as_ref().unwrap().raw(), "#333");
        assert_eq!(style.style.color.as_ref().unwrap().raw(), "#111");
    }

    #[test]
    fn test_classdef_statement_ignored() {
        let input = "graph TD\nA --> B\nclassDef warning fill:#ff0\nclass A warning\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_click_statement_ignored() {
        let input = "graph TD\nA --> B\nclick A callback\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_linkstyle_statement_ignored() {
        let input = "graph TD\nA --> B\nlinkStyle 0 stroke:#ff3,stroke-width:4px\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
    }

    #[test]
    fn test_style_keyword_node_ids_still_work() {
        // Node IDs that start with style keywords should still parse as nodes
        let input = "graph TD\nstyleA --> classB\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 1);
        assert_eq!(result.edges()[0].from.id, "styleA");
        assert_eq!(result.edges()[0].to.id, "classB");
    }

    // Semicolon separator tests (Task 1.1)
    #[test]
    fn test_semicolon_separator_two_statements() {
        let input = "graph TD\nA --> B; B --> C\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 2);
    }

    #[test]
    fn test_semicolon_separator_mixed_with_newlines() {
        let input = "graph TD\nA --> B;\nB --> C\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 2);
    }

    #[test]
    fn test_semicolon_separator_multiple() {
        let input = "graph TD\nA --> B; B --> C; C --> D\n";
        let result = parse_flowchart(input).unwrap();
        assert_eq!(result.edges().len(), 3);
    }

    #[test]
    fn test_parse_subgraph_with_external_nodes() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n";
        let result = parse_flowchart(input).unwrap();
        let subgraphs: Vec<_> = result
            .statements
            .iter()
            .filter(|s| matches!(s, Statement::Subgraph(_)))
            .collect();
        assert_eq!(subgraphs.len(), 1);
        // External edge should also be present
        let edges = result.edges();
        assert!(!edges.is_empty(), "Expected external edge C --> A");
    }

    // Extended edge syntax tests (Task 5.1)
    #[test]
    fn test_long_solid_edge() {
        let fc = parse_flowchart("graph TD\nA ----> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.stroke, StrokeSpec::Solid);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
        assert!(edge.connector.length > 1);
    }

    #[test]
    fn test_bidirectional_arrow() {
        let fc = parse_flowchart("graph TD\nA <--> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.left, ArrowHead::Normal);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_cross_arrow_right() {
        let fc = parse_flowchart("graph TD\nA --x B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.right, ArrowHead::Cross);
    }

    #[test]
    fn test_cross_arrow_both() {
        let fc = parse_flowchart("graph TD\nA x--x B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.left, ArrowHead::Cross);
        assert_eq!(edge.connector.right, ArrowHead::Cross);
    }

    #[test]
    fn test_circle_arrow() {
        let fc = parse_flowchart("graph TD\nA --o B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.right, ArrowHead::Circle);
    }

    #[test]
    fn test_circle_arrow_both() {
        let fc = parse_flowchart("graph TD\nA o--o B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.left, ArrowHead::Circle);
        assert_eq!(edge.connector.right, ArrowHead::Circle);
    }

    #[test]
    fn test_long_dotted_edge() {
        let fc = parse_flowchart("graph TD\nA -..-> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.stroke, StrokeSpec::Dotted);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_long_thick_edge() {
        let fc = parse_flowchart("graph TD\nA ===> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.stroke, StrokeSpec::Thick);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_dotted_bidirectional() {
        let fc = parse_flowchart("graph TD\nA <-.-> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.stroke, StrokeSpec::Dotted);
        assert_eq!(edge.connector.left, ArrowHead::Normal);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_thick_bidirectional() {
        let fc = parse_flowchart("graph TD\nA <==> B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.stroke, StrokeSpec::Thick);
        assert_eq!(edge.connector.left, ArrowHead::Normal);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
    }

    #[test]
    fn test_parse_subgraph_quoted_multi_word_title() {
        let input = "graph TD\nsubgraph \"Multi Word Title\"\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.title, "Multi Word Title");
                assert!(!sg.id.is_empty());
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_id_space_quoted_title() {
        let input = "graph TD\nsubgraph myId \"My Title\"\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "myId");
                assert_eq!(sg.title, "My Title");
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_direction_lr() {
        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "sg1");
                assert_eq!(sg.dir, Some(Direction::LeftRight));
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_direction_bt() {
        let input = "graph TD\nsubgraph sg1[Group]\ndirection BT\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.dir, Some(Direction::BottomTop));
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_no_direction() {
        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.dir, None);
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_auto_ids_are_unique() {
        let input = "graph TD\nsubgraph \"First\"\nA\nend\nsubgraph \"Second\"\nB\nend\n";
        let result = parse_flowchart(input).unwrap();
        let subgraphs: Vec<_> = result
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .collect();
        assert_eq!(subgraphs.len(), 2);
        assert_ne!(
            subgraphs[0].id, subgraphs[1].id,
            "Auto-generated IDs should be unique"
        );
    }

    #[test]
    fn test_auto_ids_unique_same_title() {
        let input = "graph TD\nsubgraph \"Same\"\nA\nend\nsubgraph \"Same\"\nB\nend\n";
        let result = parse_flowchart(input).unwrap();
        let subgraphs: Vec<_> = result
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .collect();
        assert_eq!(subgraphs.len(), 2);
        assert_ne!(
            subgraphs[0].id, subgraphs[1].id,
            "Auto-generated IDs should be unique even with same title"
        );
    }

    #[test]
    fn test_parse_subgraph_numeric_starting_id() {
        let input = "graph TD\nsubgraph 1test[Group]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "1test");
                assert_eq!(sg.title, "Group");
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_all_numeric_id() {
        let input = "graph TD\nsubgraph 123[Numbers]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "123");
                assert_eq!(sg.title, "Numbers");
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_parse_subgraph_existing_bracket_syntax_unchanged() {
        let input = "graph TD\nsubgraph sg1[My Group]\nA --> B\nend\n";
        let result = parse_flowchart(input).unwrap();
        match &result.statements[0] {
            Statement::Subgraph(sg) => {
                assert_eq!(sg.id, "sg1");
                assert_eq!(sg.title, "My Group");
            }
            _ => panic!("Expected subgraph"),
        }
    }

    #[test]
    fn test_extended_edge_with_label() {
        let fc = parse_flowchart("graph TD\nA <-->|both ways| B\n").unwrap();
        let edge = &fc.edges()[0];
        assert_eq!(edge.connector.left, ArrowHead::Normal);
        assert_eq!(edge.connector.right, ArrowHead::Normal);
        assert_eq!(edge.connector.label(), Some("both ways"));
    }

    mod owner_local_fixture_regressions {
        use super::*;

        #[test]
        fn comments_are_ignored() {
            let flowchart = parse_fixture_flowchart("git_workflow.mmd");
            assert_eq!(flowchart.edges().len(), 4);
            assert!(flowchart.vertices().iter().all(|vertex| vertex.id != "%%"));
        }

        #[test]
        fn shape_keywords_parse_document_and_card() {
            let flowchart = parse_fixture_flowchart("shapes_document.mmd");

            assert_eq!(
                fixture_vertex(&flowchart, "doc").shape,
                Some(ShapeSpec::Document("Doc".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "docs").shape,
                Some(ShapeSpec::Documents("Docs".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "tagdoc").shape,
                Some(ShapeSpec::TaggedDocument("TagDoc".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "card").shape,
                Some(ShapeSpec::Card("Card".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "tag").shape,
                Some(ShapeSpec::TaggedRect("Tag".to_string()))
            );
        }

        #[test]
        fn directive_stripped() {
            let flowchart = parse_fixture_flowchart("compat_directive.mmd");
            assert_eq!(flowchart.edges().len(), 3);
            assert_eq!(
                fixture_vertex(&flowchart, "A").shape,
                Some(ShapeSpec::Rectangle("Start".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "B").shape,
                Some(ShapeSpec::Diamond("Decision".to_string()))
            );
        }

        #[test]
        fn frontmatter_stripped() {
            let flowchart = parse_fixture_flowchart("compat_frontmatter.mmd");
            assert_eq!(flowchart.edges().len(), 2);
            assert_eq!(fixture_vertex(&flowchart, "A").id, "A");
            assert_eq!(fixture_vertex(&flowchart, "B").id, "B");
            assert_eq!(fixture_vertex(&flowchart, "C").id, "C");
        }

        #[test]
        fn no_direction_defaults_to_td() {
            let flowchart = parse_fixture_flowchart("compat_no_direction.mmd");
            assert_eq!(flowchart.direction, Direction::TopDown);
            assert_eq!(flowchart.edges().len(), 1);
        }

        #[test]
        fn numeric_ids() {
            let flowchart = parse_fixture_flowchart("compat_numeric_ids.mmd");
            assert_eq!(
                fixture_vertex(&flowchart, "1").shape,
                Some(ShapeSpec::Rectangle("First".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "2").shape,
                Some(ShapeSpec::Rectangle("Second".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "3").shape,
                Some(ShapeSpec::Rectangle("Third".to_string()))
            );
        }

        #[test]
        fn hyphenated_ids() {
            let flowchart = parse_fixture_flowchart("compat_hyphenated_ids.mmd");
            assert_eq!(
                fixture_vertex(&flowchart, "start-node").shape,
                Some(ShapeSpec::Rectangle("Start".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "process-1").shape,
                Some(ShapeSpec::Rectangle("Process A".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "decision-point").shape,
                Some(ShapeSpec::Diamond("Check".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "end-node").shape,
                Some(ShapeSpec::Rectangle("Done".to_string()))
            );
        }

        #[test]
        fn class_annotation_ignored() {
            let flowchart = parse_fixture_flowchart("compat_class_annotation.mmd");
            assert_eq!(flowchart.edges().len(), 3);
            assert_eq!(
                fixture_vertex(&flowchart, "A").shape,
                Some(ShapeSpec::Rectangle("Start".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "B").shape,
                Some(ShapeSpec::Diamond("Decision".to_string()))
            );
        }

        #[test]
        fn invisible_edge_not_rendered() {
            let flowchart = parse_fixture_flowchart("compat_invisible_edge.mmd");
            assert_eq!(flowchart.edges().len(), 3);
            assert_eq!(
                flowchart
                    .edges()
                    .iter()
                    .filter(|edge| edge.connector.stroke == StrokeSpec::Invisible)
                    .count(),
                1
            );
        }

        #[test]
        fn kitchen_sink() {
            let flowchart = parse_fixture_flowchart("compat_kitchen_sink.mmd");
            assert_eq!(flowchart.edges().len(), 5);
            assert_eq!(
                fixture_vertex(&flowchart, "start-node").shape,
                Some(ShapeSpec::Rectangle("Start".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "check-1").shape,
                Some(ShapeSpec::Diamond("Check Input".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "error-1").shape,
                Some(ShapeSpec::Rectangle("Error".to_string()))
            );
            assert_eq!(
                fixture_vertex(&flowchart, "end-node").shape,
                Some(ShapeSpec::Rectangle("Done".to_string()))
            );
        }
    }

    fn parse_fixture_flowchart(name: &str) -> Flowchart {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flowchart")
            .join(name);
        let input = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error));
        parse_flowchart(&input).unwrap_or_else(|error| {
            panic!(
                "Failed to parse flowchart fixture {}: {}",
                path.display(),
                error
            )
        })
    }

    fn fixture_vertex<'a>(flowchart: &'a Flowchart, id: &str) -> &'a Vertex {
        flowchart
            .vertices()
            .into_iter()
            .find(|vertex| vertex.id == id && vertex.shape.is_some())
            .or_else(|| {
                flowchart
                    .vertices()
                    .into_iter()
                    .find(|vertex| vertex.id == id)
            })
            .unwrap_or_else(|| panic!("Missing fixture vertex {id}"))
    }
}
