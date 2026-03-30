// Parser is intentionally unused until compiler + wiring land in a follow-up PR.
#![allow(dead_code)]

//! State diagram parser.
//!
//! Hand-written line-oriented parser for Mermaid `stateDiagram-v2` syntax.
//! Supports: state declarations, transitions (including `[*]` pseudo-states),
//! composite states with recursive nesting, stereotypes (fork/join/choice),
//! direction overrides, state aliases, and descriptions.

pub mod ast;

use ast::{StateDecl, StateModel, StateStatement, StateStereotype, StateTransition};

/// Parse a state diagram from Mermaid input text.
///
/// Expects the input to start with `stateDiagram-v2` (case-insensitive).
pub fn parse_state_diagram(
    input: &str,
) -> Result<StateModel, Box<dyn std::error::Error + Send + Sync>> {
    let mut lines = input.lines().peekable();

    // Skip frontmatter
    if let Some(first) = lines.peek()
        && first.trim() == "---"
    {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }

    // Skip leading comments and whitespace, then consume header
    let mut found_header = false;
    while let Some(line) = lines.peek() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            lines.next();
            continue;
        }
        if trimmed.to_lowercase().starts_with("statediagram-v2") {
            found_header = true;
            lines.next();
            break;
        }
        return Err(format!("Expected 'stateDiagram-v2' header, got: {trimmed}").into());
    }

    if !found_header {
        return Err("Missing 'stateDiagram-v2' header".into());
    }

    let body_lines: Vec<&str> = lines.collect();
    let (statements, direction) = parse_body(&body_lines)?;

    Ok(StateModel {
        direction,
        statements,
    })
}

/// Parse a block of body lines into statements and an optional direction.
///
/// Used both at the top level and recursively inside composite state blocks.
fn parse_body(
    lines: &[&str],
) -> Result<(Vec<StateStatement>, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let mut statements = Vec::new();
    let mut direction: Option<String> = None;
    let mut idx = 0;

    while idx < lines.len() {
        let trimmed = lines[idx].trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            idx += 1;
            continue;
        }

        // Skip known discardable directives
        if is_discardable(trimmed) {
            idx += 1;
            continue;
        }

        // Direction directive
        if let Some(dir) = try_parse_direction(trimmed) {
            if direction.is_none() {
                direction = Some(dir.clone());
            }
            statements.push(StateStatement::Direction(dir));
            idx += 1;
            continue;
        }

        // `state "alias" as Id` or `state Id <<stereotype>>` or `state Id { ... }`
        if let Some(rest) = strip_keyword(trimmed, "state") {
            // Check for composite state with opening brace on this line
            if let Some(composite_start) = rest.strip_suffix('{').map(|s| s.trim()) {
                let (decl, consumed) = parse_composite_state(composite_start, &lines[idx + 1..])?;
                statements.push(StateStatement::State(decl));
                idx += 1 + consumed;
                continue;
            }

            // Check for `state "alias text" as Id`
            if let Some(decl) = try_parse_state_alias(rest) {
                statements.push(StateStatement::State(decl));
                idx += 1;
                continue;
            }

            // Check for `state Id <<stereotype>>`
            if let Some(decl) = try_parse_state_stereotype(rest) {
                statements.push(StateStatement::State(decl));
                idx += 1;
                continue;
            }

            // Bare `state Id` declaration
            let id = rest.trim();
            if is_valid_state_id(id) {
                statements.push(StateStatement::State(StateDecl {
                    id: id.to_string(),
                    description: None,
                    alias: None,
                    stereotype: None,
                    children: Vec::new(),
                }));
            }
            idx += 1;
            continue;
        }

        // Transition: `A --> B` or `A --> B : label`
        if let Some(transition) = try_parse_transition(trimmed) {
            statements.push(StateStatement::Transition(transition));
            idx += 1;
            continue;
        }

        // State with description: `StateId : description text`
        if let Some(decl) = try_parse_state_description(trimmed) {
            statements.push(StateStatement::State(decl));
            idx += 1;
            continue;
        }

        // Permissive: skip unrecognized lines
        idx += 1;
    }

    Ok((statements, direction))
}

/// Strip a case-insensitive keyword prefix followed by whitespace.
fn strip_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = line.to_lowercase();
    if lower.starts_with(keyword) {
        let rest = &line[keyword.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// Check whether a line starts with a known discardable directive.
fn is_discardable(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("classdef ")
        || lower.starts_with("style ")
        || lower.starts_with("class ")
        || lower.starts_with("click ")
        || lower.starts_with("acctitle")
        || lower.starts_with("accdescr")
}

/// Try to parse a direction directive (`direction TB|BT|LR|RL`).
fn try_parse_direction(line: &str) -> Option<String> {
    let rest = strip_keyword(line, "direction")?;
    let token = rest.split_whitespace().next()?;
    normalize_direction(token)
}

/// Normalize a direction token to canonical uppercase form.
fn normalize_direction(token: &str) -> Option<String> {
    let upper = token.to_ascii_uppercase();
    match upper.as_str() {
        "LR" | "RL" | "BT" | "TB" => Some(upper),
        _ => None,
    }
}

/// Parse `state "alias text" as StateId` (with optional trailing stereotype or braces).
fn try_parse_state_alias(rest: &str) -> Option<StateDecl> {
    let rest = rest.trim();
    if !rest.starts_with('"') {
        return None;
    }
    let after_open = &rest[1..];
    let end_quote = after_open.find('"')?;
    let alias_text = after_open[..end_quote].to_string();
    let remainder = after_open[end_quote + 1..].trim();

    // Expect `as` keyword
    let lower = remainder.to_lowercase();
    if !lower.starts_with("as ") {
        return None;
    }
    let after_as = remainder[3..].trim();

    // Extract the state ID (first whitespace-delimited token)
    let id = after_as
        .split_whitespace()
        .next()
        .filter(|s| is_valid_state_id(s))?
        .to_string();

    Some(StateDecl {
        id,
        description: None,
        alias: Some(alias_text),
        stereotype: None,
        children: Vec::new(),
    })
}

/// Parse a `<<stereotype>>` marker from the given text.
fn parse_stereotype(s: &str) -> Option<StateStereotype> {
    let inner = s.strip_prefix("<<")?.strip_suffix(">>")?;
    match inner.trim().to_lowercase().as_str() {
        "fork" => Some(StateStereotype::Fork),
        "join" => Some(StateStereotype::Join),
        "choice" => Some(StateStereotype::Choice),
        _ => None,
    }
}

/// Parse `Id <<fork>>`, `Id <<join>>`, or `Id <<choice>>`.
fn try_parse_state_stereotype(rest: &str) -> Option<StateDecl> {
    let rest = rest.trim();
    let space_idx = rest.find(char::is_whitespace)?;
    let id = &rest[..space_idx];
    if !is_valid_state_id(id) {
        return None;
    }
    let after_id = rest[space_idx..].trim();
    let stereotype = parse_stereotype(after_id)?;

    Some(StateDecl {
        id: id.to_string(),
        description: None,
        alias: None,
        stereotype: Some(stereotype),
        children: Vec::new(),
    })
}

/// Parse a composite state block starting after `state <head> {`.
///
/// `head` is the text between `state ` and `{` (trimmed).
/// `remaining_lines` are the lines after the opening brace line.
///
/// Returns the `StateDecl` and the number of lines consumed from `remaining_lines`.
fn parse_composite_state(
    head: &str,
    remaining_lines: &[&str],
) -> Result<(StateDecl, usize), Box<dyn std::error::Error + Send + Sync>> {
    // Parse the head for: Id, or "alias" as Id, or Id <<stereotype>>
    let (id, alias, stereotype) = parse_composite_head(head)?;

    // Collect inner lines by tracking brace depth
    let mut depth = 1u32;
    let mut inner_lines = Vec::new();
    let mut consumed = 0;

    for (i, line) in remaining_lines.iter().enumerate() {
        let trimmed = line.trim();

        // Count braces (simple: not inside strings, which is fine for Mermaid)
        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }

        consumed = i + 1;

        if depth == 0 {
            break;
        }

        inner_lines.push(*line);
    }

    let (children, _) = parse_body(&inner_lines)?;

    Ok((
        StateDecl {
            id,
            description: None,
            alias,
            stereotype,
            children,
        },
        consumed,
    ))
}

/// Parse the head portion of a composite state declaration.
///
/// Supports: `Id`, `"alias" as Id`, `Id <<stereotype>>`.
type ParseResult = Result<
    (String, Option<String>, Option<StateStereotype>),
    Box<dyn std::error::Error + Send + Sync>,
>;

fn parse_composite_head(head: &str) -> ParseResult {
    let head = head.trim();

    // `"alias" as Id`
    if let Some(after_open) = head.strip_prefix('"')
        && let Some(end_quote) = after_open.find('"')
    {
        let alias = after_open[..end_quote].to_string();
        let remainder = after_open[end_quote + 1..].trim();
        if let Some(rest) = remainder
            .strip_prefix("as ")
            .or_else(|| remainder.strip_prefix("As "))
            .or_else(|| remainder.strip_prefix("AS "))
        {
            let id = rest.trim().to_string();
            if !id.is_empty() {
                return Ok((id, Some(alias), None));
            }
        }
    }

    // `Id <<stereotype>>`
    if let Some(stereo_start) = head.find("<<") {
        let id = head[..stereo_start].trim();
        let stereotype = parse_stereotype(&head[stereo_start..]);
        return Ok((id.to_string(), None, stereotype));
    }

    // Plain `Id`
    if is_valid_state_id(head) {
        Ok((head.to_string(), None, None))
    } else {
        Err(format!("Invalid composite state head: {head}").into())
    }
}

/// Try to parse a transition line: `A --> B` or `A --> B : label`.
fn try_parse_transition(line: &str) -> Option<StateTransition> {
    // Find the `-->` operator
    let arrow_idx = line.find("-->")?;
    let from_raw = line[..arrow_idx].trim();
    let after_arrow = line[arrow_idx + 3..].trim();

    let from = parse_state_endpoint(from_raw)?;

    // Split on `:` for optional label
    let (to_raw, label) = if let Some(colon_idx) = after_arrow.find(':') {
        let to = after_arrow[..colon_idx].trim();
        let lbl = after_arrow[colon_idx + 1..].trim();
        (
            to,
            if lbl.is_empty() {
                None
            } else {
                Some(lbl.to_string())
            },
        )
    } else {
        (after_arrow, None)
    };

    let to = parse_state_endpoint(to_raw)?;

    Some(StateTransition { from, to, label })
}

/// Parse a state endpoint, accepting `[*]` pseudo-state or a regular state id.
fn parse_state_endpoint(s: &str) -> Option<String> {
    let s = s.trim();
    if s == "[*]" {
        return Some("[*]".to_string());
    }
    if is_valid_state_id(s) {
        Some(s.to_string())
    } else {
        None
    }
}

/// Try to parse `StateId : description text`.
fn try_parse_state_description(line: &str) -> Option<StateDecl> {
    let colon_idx = line.find(':')?;
    let id = line[..colon_idx].trim();
    let desc = line[colon_idx + 1..].trim();

    if !is_valid_state_id(id) || desc.is_empty() {
        return None;
    }

    Some(StateDecl {
        id: id.to_string(),
        description: Some(desc.to_string()),
        alias: None,
        stereotype: None,
        children: Vec::new(),
    })
}

/// Check whether a string is a valid state identifier.
///
/// Accepts alphanumeric characters, underscores, and hyphens.
fn is_valid_state_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_transitions() {
        let input = "stateDiagram-v2\n  A --> B\n  B --> C";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 2);
        match &model.statements[0] {
            StateStatement::Transition(t) => {
                assert_eq!(t.from, "A");
                assert_eq!(t.to, "B");
                assert!(t.label.is_none());
            }
            _ => panic!("Expected Transition"),
        }
    }

    #[test]
    fn parse_pseudo_state_transitions() {
        let input = "stateDiagram-v2\n  [*] --> Idle\n  Idle --> [*]";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 2);
        match &model.statements[0] {
            StateStatement::Transition(t) => {
                assert_eq!(t.from, "[*]");
                assert_eq!(t.to, "Idle");
            }
            _ => panic!("Expected Transition"),
        }
        match &model.statements[1] {
            StateStatement::Transition(t) => {
                assert_eq!(t.from, "Idle");
                assert_eq!(t.to, "[*]");
            }
            _ => panic!("Expected Transition"),
        }
    }

    #[test]
    fn parse_transition_with_label() {
        let input = "stateDiagram-v2\n  Idle --> Moving : start engine";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::Transition(t) => {
                assert_eq!(t.from, "Idle");
                assert_eq!(t.to, "Moving");
                assert_eq!(t.label.as_deref(), Some("start engine"));
            }
            _ => panic!("Expected Transition"),
        }
    }

    #[test]
    fn parse_composite_state() {
        let input = "\
stateDiagram-v2
  state Moving {
    Slow --> Fast
  }";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::State(decl) => {
                assert_eq!(decl.id, "Moving");
                assert_eq!(decl.children.len(), 1);
                match &decl.children[0] {
                    StateStatement::Transition(t) => {
                        assert_eq!(t.from, "Slow");
                        assert_eq!(t.to, "Fast");
                    }
                    _ => panic!("Expected inner Transition"),
                }
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn parse_nested_composite_states() {
        let input = "\
stateDiagram-v2
  state Outer {
    state Inner {
      A --> B
    }
  }";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::State(outer) => {
                assert_eq!(outer.id, "Outer");
                assert_eq!(outer.children.len(), 1);
                match &outer.children[0] {
                    StateStatement::State(inner) => {
                        assert_eq!(inner.id, "Inner");
                        assert_eq!(inner.children.len(), 1);
                    }
                    _ => panic!("Expected inner State"),
                }
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn parse_stereotypes() {
        let input = "\
stateDiagram-v2
  state forkState <<fork>>
  state joinState <<join>>
  state choiceState <<choice>>";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 3);

        let stereotypes: Vec<_> = model
            .statements
            .iter()
            .filter_map(|s| match s {
                StateStatement::State(d) => d.stereotype,
                _ => None,
            })
            .collect();
        assert_eq!(
            stereotypes,
            vec![
                StateStereotype::Fork,
                StateStereotype::Join,
                StateStereotype::Choice
            ]
        );
    }

    #[test]
    fn parse_direction_override() {
        let input = "stateDiagram-v2\n  direction LR\n  A --> B";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.direction, Some("LR".to_string()));
        // Direction also appears as a statement
        assert_eq!(model.statements.len(), 2);
        match &model.statements[0] {
            StateStatement::Direction(d) => assert_eq!(d, "LR"),
            _ => panic!("Expected Direction"),
        }
    }

    #[test]
    fn parse_state_description() {
        let input = "stateDiagram-v2\n  Idle : Waiting for input";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::State(decl) => {
                assert_eq!(decl.id, "Idle");
                assert_eq!(decl.description.as_deref(), Some("Waiting for input"));
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn parse_state_alias() {
        let input = "stateDiagram-v2\n  state \"Long description\" as s1";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::State(decl) => {
                assert_eq!(decl.id, "s1");
                assert_eq!(decl.alias.as_deref(), Some("Long description"));
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn skip_comments_and_discardable_lines() {
        let input = "\
stateDiagram-v2
  %% This is a comment
  classDef active fill:#bbb
  style s1 fill:#fff
  class s1 active
  click s1 callback
  A --> B";
        let model = parse_state_diagram(input).unwrap();
        // Only the transition should survive
        assert_eq!(model.statements.len(), 1);
        match &model.statements[0] {
            StateStatement::Transition(t) => {
                assert_eq!(t.from, "A");
                assert_eq!(t.to, "B");
            }
            _ => panic!("Expected Transition"),
        }
    }

    #[test]
    fn reject_non_state_diagram_input() {
        let result = parse_state_diagram("classDiagram\nclass User");
        assert!(result.is_err());
    }

    #[test]
    fn skip_frontmatter() {
        let input = "---\ntitle: test\n---\nstateDiagram-v2\n  A --> B";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
    }

    #[test]
    fn parse_empty_diagram() {
        let model = parse_state_diagram("stateDiagram-v2\n").unwrap();
        assert!(model.statements.is_empty());
        assert!(model.direction.is_none());
    }

    #[test]
    fn parse_direction_inside_composite() {
        let input = "\
stateDiagram-v2
  state Moving {
    direction LR
    Slow --> Fast
  }";
        let model = parse_state_diagram(input).unwrap();
        match &model.statements[0] {
            StateStatement::State(decl) => {
                assert_eq!(decl.children.len(), 2);
                match &decl.children[0] {
                    StateStatement::Direction(d) => assert_eq!(d, "LR"),
                    _ => panic!("Expected Direction"),
                }
            }
            _ => panic!("Expected State"),
        }
    }

    #[test]
    fn parse_case_insensitive_header() {
        let model = parse_state_diagram("STATEDIAGRAM-V2\n  A --> B").unwrap();
        assert_eq!(model.statements.len(), 1);
    }

    #[test]
    fn skip_acctitle_and_accdescr() {
        let input = "\
stateDiagram-v2
  accTitle: My diagram
  accDescr: A description
  A --> B";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1);
    }
}
