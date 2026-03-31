//! ER diagram parser.
//!
//! Hand-written line-oriented parser for `erDiagram` syntax.
//! Supports entities, typed attributes (PK/FK/UK keys, comments),
//! and relationships with cardinality notation.

use std::collections::HashMap;

use crate::mermaid::flowchart::strip_frontmatter;

/// Parsed ER diagram model.
#[derive(Debug, Clone)]
pub struct ErModel {
    /// Entities with optional attributes.
    pub entities: Vec<ErEntity>,
    /// Relationships between entities.
    pub relationships: Vec<ErRelationship>,
}

/// An entity in the ER diagram.
#[derive(Debug, Clone)]
pub struct ErEntity {
    /// Entity name / identifier.
    pub name: String,
    /// Typed attributes.
    pub attributes: Vec<ErAttribute>,
}

/// A typed attribute within an entity.
#[derive(Debug, Clone)]
pub struct ErAttribute {
    /// Data type (e.g., "string", "int").
    pub attr_type: String,
    /// Attribute name.
    pub name: String,
    /// Key constraints (e.g., "PK", "FK", "UK").
    pub keys: Vec<String>,
    /// Optional quoted comment.
    pub comment: Option<String>,
}

/// A relationship between two entities.
#[derive(Debug, Clone)]
pub struct ErRelationship {
    /// Left entity.
    pub entity_a: String,
    /// Right entity.
    pub entity_b: String,
    /// Cardinality on the left (entity_a) side.
    pub cardinality_a: Cardinality,
    /// Cardinality on the right (entity_b) side.
    pub cardinality_b: Cardinality,
    /// Whether this is an identifying relationship (solid line).
    pub identifying: bool,
    /// Optional role label.
    pub role: Option<String>,
}

/// Cardinality (participation constraint) on one side of a relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// Exactly one: `||`
    OnlyOne,
    /// Zero or one: `o|` / `|o`
    ZeroOrOne,
    /// One or more: `}|` / `|{`
    OneOrMore,
    /// Zero or more: `}o` / `o{`
    ZeroOrMore,
}

/// Parse an `erDiagram` input into an [`ErModel`].
///
/// Returns an error if the `erDiagram` header is missing.
pub fn parse_er_diagram(input: &str) -> Result<ErModel, Box<dyn std::error::Error + Send + Sync>> {
    let input = strip_frontmatter(input);
    let lines: Vec<&str> = input.lines().collect();
    let mut pos = 0;

    // Skip leading comments and whitespace, then consume header.
    while pos < lines.len() {
        let trimmed = lines[pos].trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            pos += 1;
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("erdiagram") {
            pos += 1;
            break;
        }
        return Err(format!("Expected 'erDiagram' header, got: {trimmed}").into());
    }

    if pos == 0 {
        return Err("Missing 'erDiagram' header".into());
    }

    let mut entity_attrs: HashMap<String, Vec<ErAttribute>> = HashMap::new();
    let mut relationships = Vec::new();
    let mut entity_order: Vec<String> = Vec::new();

    parse_body(
        &lines,
        &mut pos,
        &mut entity_attrs,
        &mut relationships,
        &mut entity_order,
    );

    // Collect all entity names: from attribute blocks and relationships.
    for rel in &relationships {
        if !entity_order.contains(&rel.entity_a) {
            entity_order.push(rel.entity_a.clone());
        }
        if !entity_order.contains(&rel.entity_b) {
            entity_order.push(rel.entity_b.clone());
        }
    }

    let entities = entity_order
        .into_iter()
        .map(|name| {
            let attributes = entity_attrs.remove(&name).unwrap_or_default();
            ErEntity { name, attributes }
        })
        .collect();

    Ok(ErModel {
        entities,
        relationships,
    })
}

/// Parse body lines until EOF or a closing `}`.
fn parse_body(
    lines: &[&str],
    pos: &mut usize,
    entity_attrs: &mut HashMap<String, Vec<ErAttribute>>,
    relationships: &mut Vec<ErRelationship>,
    entity_order: &mut Vec<String>,
) {
    while *pos < lines.len() {
        let trimmed = strip_inline_comment(lines[*pos].trim());

        if trimmed.is_empty() || trimmed.starts_with("%%") {
            *pos += 1;
            continue;
        }

        if trimmed == "}" {
            *pos += 1;
            break;
        }

        // Discard known unimplemented directives.
        if is_discardable(trimmed) {
            *pos += 1;
            continue;
        }

        // Attribute block: `ENTITY_NAME {`
        if let Some(entity_name) = try_parse_attribute_block_start(trimmed) {
            if !entity_order.contains(&entity_name) {
                entity_order.push(entity_name.clone());
            }
            *pos += 1;
            let attrs = parse_attribute_block(lines, pos);
            entity_attrs.insert(entity_name, attrs);
            continue;
        }

        // Relationship line.
        if let Some(rel) = try_parse_relationship(trimmed) {
            relationships.push(rel);
            *pos += 1;
            continue;
        }

        // Permissive: skip unrecognized lines.
        *pos += 1;
    }
}

/// Strip an inline `%%` comment from a line.
fn strip_inline_comment(line: &str) -> &str {
    match line.find("%%") {
        Some(pos) => line[..pos].trim_end(),
        None => line,
    }
}

/// Check if a line is a known-discardable directive.
fn is_discardable(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("classdef ")
        || lower.starts_with("style ")
        || lower.starts_with("class ")
        || lower.starts_with("click ")
        || lower.starts_with("acctitle")
        || lower.starts_with("accdescr")
        || lower.starts_with("direction ")
}

/// Try to detect an attribute block opening: `ENTITY_NAME {`.
fn try_parse_attribute_block_start(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('{') {
        return None;
    }
    let name = trimmed[..trimmed.len() - 1].trim();
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    // Must look like an identifier, not a relationship operator.
    if name.contains("--") || name.contains("..") {
        return None;
    }
    Some(name.to_string())
}

/// Parse attribute lines inside `{ ... }`.
fn parse_attribute_block(lines: &[&str], pos: &mut usize) -> Vec<ErAttribute> {
    let mut attrs = Vec::new();

    while *pos < lines.len() {
        let trimmed = strip_inline_comment(lines[*pos].trim());

        if trimmed.is_empty() || trimmed.starts_with("%%") {
            *pos += 1;
            continue;
        }

        if trimmed == "}" {
            *pos += 1;
            break;
        }

        if let Some(attr) = try_parse_attribute(trimmed) {
            attrs.push(attr);
        }
        *pos += 1;
    }

    attrs
}

/// Parse a single attribute line: `type name [PK|FK|UK[,...]] ["comment"]`.
fn try_parse_attribute(line: &str) -> Option<ErAttribute> {
    // Extract comment if present (last quoted string).
    let (line, comment) = extract_trailing_comment(line);
    let line = line.trim();

    let mut tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    let attr_type = tokens.remove(0).to_string();
    let name = tokens.remove(0).to_string();

    // Remaining tokens are key constraints (PK, FK, UK), possibly comma-separated.
    let keys: Vec<String> = tokens
        .iter()
        .flat_map(|t| t.split(','))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    Some(ErAttribute {
        attr_type,
        name,
        keys,
        comment,
    })
}

/// Extract a trailing quoted comment from an attribute line.
/// Returns (remaining_line, optional_comment).
fn extract_trailing_comment(line: &str) -> (&str, Option<String>) {
    // Find the last quoted string.
    if let Some(last_quote_end) = line.rfind('"') {
        let before_last = &line[..last_quote_end];
        if let Some(open_quote) = before_last.rfind('"') {
            let comment = line[open_quote + 1..last_quote_end].to_string();
            let remaining = line[..open_quote].trim_end();
            return (remaining, Some(comment));
        }
    }
    (line, None)
}

/// Try to parse a relationship line.
///
/// Format: `ENTITY_A <left-card><reltype><right-card> ENTITY_B : "role"`
///
/// The operator is a 6-char sequence like `||--o{` where:
/// - chars 0-1: left cardinality (`||`, `o|`, `}|`, `}o`)
/// - chars 2-3: relationship type (`--` or `..`)
/// - chars 4-5: right cardinality (`||`, `|o`, `|{`, `o{`)
fn try_parse_relationship(line: &str) -> Option<ErRelationship> {
    // Split on `:` first to get role label.
    let (rel_part, role) = if let Some(colon_pos) = line.rfind(':') {
        let role_text = line[colon_pos + 1..].trim();
        let role = if role_text.is_empty() {
            None
        } else {
            // Strip surrounding quotes if present.
            Some(strip_quotes(role_text).to_string())
        };
        (line[..colon_pos].trim(), role)
    } else {
        (line.trim(), None)
    };

    let tokens: Vec<&str> = rel_part.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }

    let entity_a = tokens[0];
    let operator = tokens[1];
    let entity_b = tokens[2];

    // Parse the operator. Must contain `--` or `..`.
    let (cardinality_a, identifying, cardinality_b) = parse_operator(operator)?;

    Some(ErRelationship {
        entity_a: entity_a.to_string(),
        entity_b: entity_b.to_string(),
        cardinality_a,
        cardinality_b,
        identifying,
        role,
    })
}

/// Parse the relationship operator (e.g., `||--o{`).
fn parse_operator(op: &str) -> Option<(Cardinality, bool, Cardinality)> {
    // Find the relationship type delimiter.
    let (rel_pos, identifying) = if let Some(pos) = op.find("--") {
        (pos, true)
    } else if let Some(pos) = op.find("..") {
        (pos, false)
    } else {
        return None;
    };

    let left_str = &op[..rel_pos];
    let right_str = &op[rel_pos + 2..];

    let card_a = parse_left_cardinality(left_str)?;
    let card_b = parse_right_cardinality(right_str)?;

    Some((card_a, identifying, card_b))
}

/// Parse a left-side cardinality token.
/// Tokens read left-to-right: `||`, `o|`, `}|`, `}o`
fn parse_left_cardinality(s: &str) -> Option<Cardinality> {
    match s {
        "||" => Some(Cardinality::OnlyOne),
        "o|" => Some(Cardinality::ZeroOrOne),
        "|o" => Some(Cardinality::ZeroOrOne),
        "}|" => Some(Cardinality::OneOrMore),
        "|{" => Some(Cardinality::OneOrMore),
        "}o" => Some(Cardinality::ZeroOrMore),
        "o{" => Some(Cardinality::ZeroOrMore),
        _ => None,
    }
}

/// Parse a right-side cardinality token.
/// Tokens read left-to-right: `||`, `|o`, `|{`, `o{`
fn parse_right_cardinality(s: &str) -> Option<Cardinality> {
    match s {
        "||" => Some(Cardinality::OnlyOne),
        "|o" => Some(Cardinality::ZeroOrOne),
        "o|" => Some(Cardinality::ZeroOrOne),
        "|{" => Some(Cardinality::OneOrMore),
        "{|" => Some(Cardinality::OneOrMore),
        "o{" => Some(Cardinality::ZeroOrMore),
        "{o" => Some(Cardinality::ZeroOrMore),
        _ => None,
    }
}

/// Strip surrounding double quotes from a string.
fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_er_diagram() {
        let model = parse_er_diagram("erDiagram\n").unwrap();
        assert!(model.entities.is_empty());
        assert!(model.relationships.is_empty());
    }

    #[test]
    fn parse_missing_header_errors() {
        let result = parse_er_diagram("A ||--|| B : rel");
        assert!(result.is_err());
    }

    #[test]
    fn parse_case_insensitive_header() {
        let model = parse_er_diagram("ERDIAGRAM\n    A ||--|| B : rel").unwrap();
        assert_eq!(model.relationships.len(), 1);
    }

    #[test]
    fn parse_basic_relationship() {
        let model = parse_er_diagram("erDiagram\n    CUSTOMER ||--o{ ORDER : places").unwrap();
        assert_eq!(model.relationships.len(), 1);
        let rel = &model.relationships[0];
        assert_eq!(rel.entity_a, "CUSTOMER");
        assert_eq!(rel.entity_b, "ORDER");
        assert_eq!(rel.cardinality_a, Cardinality::OnlyOne);
        assert_eq!(rel.cardinality_b, Cardinality::ZeroOrMore);
        assert!(rel.identifying);
        assert_eq!(rel.role.as_deref(), Some("places"));
    }

    #[test]
    fn parse_relationship_without_label() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B").unwrap();
        assert_eq!(model.relationships.len(), 1);
        let rel = &model.relationships[0];
        assert!(rel.role.is_none());
    }

    #[test]
    fn parse_relationship_with_quoted_label() {
        let model = parse_er_diagram("erDiagram\n    A ||--|| B : \"has many\"").unwrap();
        let rel = &model.relationships[0];
        assert_eq!(rel.role.as_deref(), Some("has many"));
    }

    #[test]
    fn parse_all_cardinality_types() {
        let input = "\
erDiagram
    A ||--|| B : one-to-one
    C ||--o| D : one-to-zero-or-one
    E ||--|{ F : one-to-one-or-more
    G ||--o{ H : one-to-zero-or-more";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.relationships.len(), 4);

        assert_eq!(model.relationships[0].cardinality_b, Cardinality::OnlyOne);
        assert_eq!(model.relationships[1].cardinality_b, Cardinality::ZeroOrOne);
        assert_eq!(model.relationships[2].cardinality_b, Cardinality::OneOrMore);
        assert_eq!(
            model.relationships[3].cardinality_b,
            Cardinality::ZeroOrMore
        );
    }

    #[test]
    fn parse_left_cardinality_types() {
        let input = "\
erDiagram
    A ||--|| B : r1
    C o|--|| D : r2
    E }|--|| F : r3
    G }o--|| H : r4";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.relationships[0].cardinality_a, Cardinality::OnlyOne);
        assert_eq!(model.relationships[1].cardinality_a, Cardinality::ZeroOrOne);
        assert_eq!(model.relationships[2].cardinality_a, Cardinality::OneOrMore);
        assert_eq!(
            model.relationships[3].cardinality_a,
            Cardinality::ZeroOrMore
        );
    }

    #[test]
    fn parse_identifying_vs_non_identifying() {
        let input = "\
erDiagram
    A ||--|| B : identifying
    C ||..|| D : non-identifying";
        let model = parse_er_diagram(input).unwrap();
        assert!(model.relationships[0].identifying);
        assert!(!model.relationships[1].identifying);
    }

    #[test]
    fn parse_entity_attributes() {
        let input = "\
erDiagram
    CUSTOMER {
        string name PK
        string email UK
        int age
    }";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.entities.len(), 1);
        let entity = &model.entities[0];
        assert_eq!(entity.name, "CUSTOMER");
        assert_eq!(entity.attributes.len(), 3);

        assert_eq!(entity.attributes[0].attr_type, "string");
        assert_eq!(entity.attributes[0].name, "name");
        assert_eq!(entity.attributes[0].keys, vec!["PK"]);

        assert_eq!(entity.attributes[1].keys, vec!["UK"]);
        assert_eq!(entity.attributes[2].keys.len(), 0);
    }

    #[test]
    fn parse_attribute_with_comment() {
        let input = "\
erDiagram
    ORDER {
        int id PK \"primary key\"
        string status \"order status\"
    }";
        let model = parse_er_diagram(input).unwrap();
        let attr = &model.entities[0].attributes[0];
        assert_eq!(attr.comment.as_deref(), Some("primary key"));
        assert_eq!(attr.keys, vec!["PK"]);
    }

    #[test]
    fn parse_attribute_multiple_keys() {
        let input = "\
erDiagram
    TABLE {
        int id PK,FK
    }";
        let model = parse_er_diagram(input).unwrap();
        let attr = &model.entities[0].attributes[0];
        assert_eq!(attr.keys, vec!["PK", "FK"]);
    }

    #[test]
    fn parse_implicit_entities() {
        let input = "\
erDiagram
    A ||--|| B : rel";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.entities.len(), 2);
        assert_eq!(model.entities[0].name, "A");
        assert_eq!(model.entities[1].name, "B");
        assert!(model.entities[0].attributes.is_empty());
    }

    #[test]
    fn parse_merges_attribute_and_relationship_entities() {
        let input = "\
erDiagram
    CUSTOMER {
        string name PK
    }
    CUSTOMER ||--o{ ORDER : places";
        let model = parse_er_diagram(input).unwrap();
        // CUSTOMER from attribute block + ORDER from relationship.
        assert_eq!(model.entities.len(), 2);
        assert_eq!(model.entities[0].name, "CUSTOMER");
        assert!(!model.entities[0].attributes.is_empty());
        assert_eq!(model.entities[1].name, "ORDER");
    }

    #[test]
    fn parse_skips_comments() {
        let input = "\
erDiagram
    %% This is a comment
    A ||--|| B : rel";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.relationships.len(), 1);
    }

    #[test]
    fn parse_strips_inline_comments() {
        let input = "\
erDiagram
    A ||--|| B : rel %% inline comment";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].role.as_deref(), Some("rel"));
    }

    #[test]
    fn parse_discards_directives() {
        let input = "\
erDiagram
    classDef default fill:white
    style Entity fill:blue
    direction LR
    A ||--|| B : rel";
        let model = parse_er_diagram(input).unwrap();
        assert_eq!(model.relationships.len(), 1);
    }
}
