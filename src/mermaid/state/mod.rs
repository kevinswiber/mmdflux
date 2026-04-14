//! State diagram parser.
//!
//! Hand-written recursive descent parser for `stateDiagram-v2` syntax.
//! Supports states, transitions, `[*]` pseudo-states, composite `state { }`
//! blocks, stereotypes, direction overrides, and state descriptions.

use crate::errors::ParseDiagnostic;
use crate::graph::style::{
    parse_class_apply_statement, parse_classdef_statement_multi, parse_node_style_statement,
};
use crate::mermaid::ast::{ClassApplyStatement, ClassDefStatement, NodeStyleStatement};
use crate::mermaid::flowchart::strip_frontmatter;

/// Result of parsing a state diagram.
///
/// Contains the parsed model and any warnings collected during parsing.
#[derive(Debug)]
pub struct StateParseResult {
    /// Parsed state model.
    pub model: StateModel,
    /// Warnings collected during parsing (e.g., skipped lines).
    pub warnings: Vec<ParseDiagnostic>,
}

/// Parsed state diagram model.
#[derive(Debug, Clone)]
pub struct StateModel {
    /// Optional layout direction (`LR`, `RL`, `TB`, `BT`).
    pub direction: Option<String>,
    /// Top-level statements.
    pub statements: Vec<StateStatement>,
}

/// A statement inside a state diagram.
#[derive(Debug, Clone)]
pub enum StateStatement {
    /// Explicit state declaration (may include children for composites).
    State(StateDecl),
    /// Transition between states.
    Transition(StateTransition),
    /// Direction directive.
    Direction(String),
    /// Note attached to a state.
    Note(StateNote),
    /// A `style NODE ...` declaration.
    Style(NodeStyleStatement),
    /// A `classDef className ...` declaration.
    ClassDef(ClassDefStatement),
    /// A `class nodeA,nodeB className` declaration.
    ClassApply(ClassApplyStatement),
}

/// A note attached to a state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateNote {
    /// The state this note is attached to.
    pub state_id: String,
    /// Note placement relative to the state.
    pub position: NotePosition,
    /// Note text content.
    pub text: String,
}

/// Position of a note relative to its attached state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePosition {
    Left,
    Right,
}

/// An explicit state declaration.
#[derive(Debug, Clone)]
pub struct StateDecl {
    /// State identifier.
    pub id: String,
    /// Description lines (accumulated from repeated `Id : desc` statements).
    pub descriptions: Vec<String>,
    /// Optional alias (from `state "label" as id`).
    pub alias: Option<String>,
    /// Optional stereotype (fork, join, choice).
    pub stereotype: Option<StateStereotype>,
    /// Child statements (for composite states).
    pub children: Vec<StateStatement>,
    /// Optional class name from `:::className` annotation.
    pub class_name: Option<String>,
}

/// State stereotypes (UML notation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateStereotype {
    Fork,
    Join,
    Choice,
}

/// A transition between two states.
#[derive(Debug, Clone)]
pub struct StateTransition {
    /// Source state ID (may be `"[*]"`).
    pub from: String,
    /// Target state ID (may be `"[*]"`).
    pub to: String,
    /// Optional transition label.
    pub label: Option<String>,
    /// Optional `:::className` annotation on the source state.
    pub from_class: Option<String>,
    /// Optional `:::className` annotation on the target state.
    pub to_class: Option<String>,
}

/// Parse a `stateDiagram-v2` input into a [`StateModel`].
///
/// Returns an error if the `stateDiagram-v2` header is missing.
pub fn parse_state_diagram(
    input: &str,
) -> Result<StateParseResult, Box<dyn std::error::Error + Send + Sync>> {
    let stripped = strip_frontmatter(input);
    // Count lines consumed by frontmatter so warnings report original line numbers.
    let frontmatter_lines = if std::ptr::eq(stripped.as_ptr(), input.as_ptr()) {
        0
    } else {
        let consumed = stripped.as_ptr() as usize - input.as_ptr() as usize;
        input[..consumed].lines().count()
    };
    let lines: Vec<&str> = stripped.lines().collect();
    let mut pos = 0;
    let mut direction: Option<String> = None;
    let mut warnings: Vec<ParseDiagnostic> = Vec::new();

    // Skip leading comments and whitespace, then consume header.
    while pos < lines.len() {
        let trimmed = lines[pos].trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            pos += 1;
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("statediagram-v2") || lower.starts_with("statediagram") {
            pos += 1;
            break;
        }
        return Err(format!("Expected 'stateDiagram' header, got: {trimmed}").into());
    }

    if pos == 0 {
        return Err("Missing 'stateDiagram' header".into());
    }

    let statements = parse_body(&lines, &mut pos, &mut direction, &mut warnings);

    // Adjust warning line numbers to account for stripped frontmatter.
    if frontmatter_lines > 0 {
        for warning in &mut warnings {
            if let Some(line) = &mut warning.line {
                *line += frontmatter_lines;
            }
        }
    }

    Ok(StateParseResult {
        model: StateModel {
            direction,
            statements,
        },
        warnings,
    })
}

/// Parse statement lines until EOF or a closing `}`.
fn parse_body(
    lines: &[&str],
    pos: &mut usize,
    direction: &mut Option<String>,
    warnings: &mut Vec<ParseDiagnostic>,
) -> Vec<StateStatement> {
    let mut statements = Vec::new();

    while *pos < lines.len() {
        // Strip inline comments before processing.
        let trimmed = strip_inline_comment(lines[*pos].trim());

        // Skip empty lines and comments.
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            *pos += 1;
            continue;
        }

        // Closing brace ends this composite block.
        if trimmed == "}" {
            *pos += 1;
            break;
        }

        // Discard known unimplemented directives.
        if is_discardable(trimmed) {
            *pos += 1;
            continue;
        }

        // classDef statement (supports multi-class: `classDef a,b fill:#f00`).
        let classdefs = parse_classdef_statement_multi(trimmed);
        if !classdefs.is_empty() {
            for cd in classdefs {
                statements.push(StateStatement::ClassDef(ClassDefStatement {
                    class_name: cd.class_name,
                    style: cd.style,
                }));
            }
            *pos += 1;
            continue;
        }

        // style statement.
        if let Some(parsed) = parse_node_style_statement(trimmed) {
            statements.push(StateStatement::Style(NodeStyleStatement {
                node_id: parsed.node_id,
                style: parsed.style,
            }));
            *pos += 1;
            continue;
        }

        // class apply statement (must come after classDef check).
        if let Some(parsed) = parse_class_apply_statement(trimmed) {
            statements.push(StateStatement::ClassApply(ClassApplyStatement {
                node_ids: parsed.node_ids,
                class_name: parsed.class_name,
            }));
            *pos += 1;
            continue;
        }

        // Note: `note right of State : text` or multi-line `note ... end note`
        if let Some(note) = try_parse_note(trimmed, lines, pos) {
            statements.push(StateStatement::Note(note));
            continue;
        }

        // Direction directive.
        if let Some(rest) = strip_keyword(trimmed, "direction") {
            if let Some(dir) = normalize_direction(rest.trim()) {
                if direction.is_none() {
                    *direction = Some(dir.clone());
                }
                statements.push(StateStatement::Direction(dir));
            }
            *pos += 1;
            continue;
        }

        // Transition: `A --> B` or `A --> B : label`
        if let Some(transition) = try_parse_transition(trimmed) {
            statements.push(StateStatement::Transition(transition));
            *pos += 1;
            continue;
        }

        // Inline state description: `Id : description text`
        // (Must check BEFORE state keyword, since `state` itself is a valid id)
        if let Some(decl) = try_parse_inline_description(trimmed) {
            statements.push(StateStatement::State(decl));
            *pos += 1;
            continue;
        }

        // Explicit state declaration: `state ...`
        if let Some(mut decl) = try_parse_state_decl(trimmed) {
            *pos += 1;
            // Composite state: `state Id { ... }`
            if trimmed.trim_end().ends_with('{') {
                let mut inner_dir = None;
                decl.children = parse_body(lines, pos, &mut inner_dir, warnings);
            }
            statements.push(StateStatement::State(decl));
            continue;
        }

        // Permissive: skip unrecognized lines but collect a warning.
        warnings.push(ParseDiagnostic::warning(
            Some(*pos + 1), // 1-indexed
            None,
            format!("skipped unrecognized line: {trimmed}"),
        ));
        *pos += 1;
    }

    statements
}

/// Try to parse a note statement.
///
/// Single-line: `note right of State : text`
/// Multi-line:  `note right of State\n  text...\nend note`
fn try_parse_note(first_line: &str, lines: &[&str], pos: &mut usize) -> Option<StateNote> {
    let lower = first_line.to_lowercase();
    if !lower.starts_with("note ") {
        return None;
    }

    let rest = first_line["note ".len()..].trim();

    // Parse position and state ID: `right of StateId` or `left of StateId`
    let (position, after_pos) = if let Some(r) = strip_keyword_ci(rest, "right of") {
        (NotePosition::Right, r)
    } else if let Some(r) = strip_keyword_ci(rest, "left of") {
        (NotePosition::Left, r)
    } else {
        return None;
    };

    let after_pos = after_pos.trim();

    // Single-line form: `note right of State : text`
    if let Some(colon_pos) = after_pos.find(':') {
        let state_id = after_pos[..colon_pos].trim();
        let text = after_pos[colon_pos + 1..].trim();
        if state_id.is_empty() {
            return None;
        }
        *pos += 1;
        return Some(StateNote {
            state_id: state_id.to_string(),
            position,
            text: text.to_string(),
        });
    }

    // Multi-line form: state ID is the rest of the line, text until `end note`
    let state_id = after_pos.trim();
    if state_id.is_empty() {
        return None;
    }

    *pos += 1;
    let mut text_lines = Vec::new();
    while *pos < lines.len() {
        let line = strip_inline_comment(lines[*pos].trim());
        if line.to_lowercase() == "end note" {
            *pos += 1;
            break;
        }
        text_lines.push(line.to_string());
        *pos += 1;
    }

    Some(StateNote {
        state_id: state_id.to_string(),
        position,
        text: text_lines.join("\n"),
    })
}

/// Case-insensitive keyword prefix strip (returns rest after the keyword + whitespace).
fn strip_keyword_ci<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = line.to_lowercase();
    if lower.starts_with(keyword) {
        let rest = &line[keyword.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// Strip an inline `%%` comment from a line, returning the content before it.
fn strip_inline_comment(line: &str) -> &str {
    match line.find("%%") {
        Some(pos) => line[..pos].trim_end(),
        None => line,
    }
}

/// Check if a line is a known-discardable directive.
fn is_discardable(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("click ") || lower.starts_with("acctitle") || lower.starts_with("accdescr")
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

/// Normalize a direction token to canonical uppercase form.
fn normalize_direction(token: &str) -> Option<String> {
    let upper = token.to_ascii_uppercase();
    match upper.as_str() {
        "LR" | "RL" | "BT" | "TB" | "TD" => Some(upper),
        _ => None,
    }
}

/// Strip a `:::className` suffix from a state identifier.
fn strip_class_annotation(id: &str) -> (&str, Option<&str>) {
    if let Some(pos) = id.find(":::") {
        let base = id[..pos].trim();
        let class = id[pos + 3..].trim();
        if class.is_empty() {
            (base, None)
        } else {
            (base, Some(class))
        }
    } else {
        (id, None)
    }
}

/// Try to parse a transition line: `from --> to` or `from --> to : label`.
fn try_parse_transition(line: &str) -> Option<StateTransition> {
    let arrow_pos = line.find("-->")?;
    let from_raw = line[..arrow_pos].trim();
    let rest = line[arrow_pos + 3..].trim();

    if from_raw.is_empty() || rest.is_empty() {
        return None;
    }

    let (from, from_class) = strip_class_annotation(from_raw);

    // Use find_description_colon to skip `:::` class annotations.
    let (to_raw, label) = if let Some(colon_pos) = find_description_colon(rest) {
        let to = rest[..colon_pos].trim();
        let label = rest[colon_pos + 1..].trim();
        (
            to,
            if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            },
        )
    } else {
        (rest, None)
    };

    let (to, to_class) = strip_class_annotation(to_raw);

    if to.is_empty() {
        return None;
    }

    Some(StateTransition {
        from: from.to_string(),
        to: to.to_string(),
        label,
        from_class: from_class.map(|s| s.to_string()),
        to_class: to_class.map(|s| s.to_string()),
    })
}

/// Try to parse `Id : description text` (inline state description).
fn try_parse_inline_description(line: &str) -> Option<StateDecl> {
    // Must not start with `state` keyword (that's handled separately).
    if line.to_lowercase().starts_with("state ") {
        return None;
    }
    // Find the first `:` that is NOT part of `:::`.
    let colon_pos = find_description_colon(line)?;
    let id_raw = line[..colon_pos].trim();
    let description = line[colon_pos + 1..].trim();

    // Strip `:::className` from the identifier.
    let (id, class) = strip_class_annotation(id_raw);

    // Id must be a valid identifier (no spaces, not empty, not [*]).
    if id.is_empty() || id.contains(' ') || id == "[*]" || description.is_empty() {
        return None;
    }

    Some(StateDecl {
        id: id.to_string(),
        descriptions: vec![description.to_string()],
        alias: None,
        stereotype: None,
        children: Vec::new(),
        class_name: class.map(|s| s.to_string()),
    })
}

/// Find the position of a description colon (`:`) that is not part of `:::`.
fn find_description_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // Skip `:::` (class annotation).
            if i + 2 < bytes.len() && bytes[i + 1] == b':' && bytes[i + 2] == b':' {
                i += 3;
                // Skip the class name after `:::`.
                while i < bytes.len() && bytes[i] != b':' && bytes[i] != b' ' {
                    i += 1;
                }
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Try to parse an explicit state declaration: `state "Desc" as alias`,
/// `state id <<fork>>`, or `state id { ... }`.
fn try_parse_state_decl(line: &str) -> Option<StateDecl> {
    let rest = strip_keyword(line, "state")?;

    // `state "Description" as alias`
    if let Some(quoted) = rest.strip_prefix('"') {
        let end_quote = quoted.find('"')?;
        let description = quoted[..end_quote].to_string();
        let after_quote = quoted[end_quote + 1..].trim();
        let alias = strip_keyword(after_quote, "as").map(|a| {
            // Strip trailing `{` if present (composite with alias).
            a.trim().trim_end_matches('{').trim().to_string()
        });
        // Strip :::className from alias (e.g. `state "Running" as R:::active`).
        let (alias, class_name) = match &alias {
            Some(a) => {
                let (base, cls) = strip_class_annotation(a);
                (Some(base.to_string()), cls.map(|s| s.to_string()))
            }
            None => (None, None),
        };
        let id = alias.clone().unwrap_or_else(|| description.clone());
        return Some(StateDecl {
            id,
            descriptions: vec![description],
            alias,
            stereotype: None,
            children: Vec::new(),
            class_name,
        });
    }

    // `state id ...`
    let id_raw = rest.split(|c: char| c.is_whitespace() || c == '{').next()?;
    if id_raw.is_empty() {
        return None;
    }

    // Strip :::className from id (e.g. `state Running:::active`).
    let (id, class_name) = strip_class_annotation(id_raw);

    // Check for stereotype: `state id <<fork>>` or `state id [[fork]]` etc.
    let stereotype = if rest.contains("<<fork>>") || rest.contains("[[fork]]") {
        Some(StateStereotype::Fork)
    } else if rest.contains("<<join>>") || rest.contains("[[join]]") {
        Some(StateStereotype::Join)
    } else if rest.contains("<<choice>>") || rest.contains("[[choice]]") {
        Some(StateStereotype::Choice)
    } else {
        None
    };

    Some(StateDecl {
        id: id.to_string(),
        descriptions: Vec::new(),
        alias: None,
        stereotype,
        children: Vec::new(),
        class_name: class_name.map(|s| s.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_state_diagram() {
        let result = parse_state_diagram("stateDiagram-v2\n").unwrap();
        assert!(result.model.statements.is_empty());
        assert!(result.model.direction.is_none());
    }

    #[test]
    fn parse_missing_header_errors() {
        let result = parse_state_diagram("A --> B");
        assert!(result.is_err());
    }

    #[test]
    fn parse_basic_transition() {
        let result = parse_state_diagram("stateDiagram-v2\n    A --> B").unwrap();
        assert_eq!(result.model.statements.len(), 1);
        let StateStatement::Transition(t) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.from, "A");
        assert_eq!(t.to, "B");
        assert!(t.label.is_none());
    }

    #[test]
    fn parse_transition_with_label() {
        let result = parse_state_diagram("stateDiagram-v2\n    A --> B : submit").unwrap();
        let StateStatement::Transition(t) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.label, Some("submit".to_string()));
    }

    #[test]
    fn parse_star_markers() {
        let result =
            parse_state_diagram("stateDiagram-v2\n    [*] --> Idle\n    Done --> [*]").unwrap();
        assert_eq!(result.model.statements.len(), 2);
        let StateStatement::Transition(t0) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t0.from, "[*]");
        assert_eq!(t0.to, "Idle");
        let StateStatement::Transition(t1) = &result.model.statements[1] else {
            panic!("expected transition");
        };
        assert_eq!(t1.from, "Done");
        assert_eq!(t1.to, "[*]");
    }

    #[test]
    fn parse_direction_directive() {
        let result = parse_state_diagram("stateDiagram-v2\n    direction LR\n    A --> B").unwrap();
        assert_eq!(result.model.direction, Some("LR".to_string()));
    }

    #[test]
    fn parse_state_declaration_with_description() {
        let result =
            parse_state_diagram("stateDiagram-v2\n    state \"Waiting\" as waiting").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "waiting");
        assert_eq!(decl.descriptions, vec!["Waiting".to_string()]);
        assert_eq!(decl.alias, Some("waiting".to_string()));
    }

    #[test]
    fn parse_skips_comments() {
        let result = parse_state_diagram("stateDiagram-v2\n    %% comment\n    A --> B\n").unwrap();
        assert_eq!(result.model.statements.len(), 1);
    }

    #[test]
    fn parse_case_insensitive_header() {
        let result = parse_state_diagram("STATEDIAGRAM-V2\n    A --> B").unwrap();
        assert_eq!(result.model.statements.len(), 1);
    }

    #[test]
    fn parse_stereotype_fork() {
        let result = parse_state_diagram("stateDiagram-v2\n    state forkNode <<fork>>").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "forkNode");
        assert_eq!(decl.stereotype, Some(StateStereotype::Fork));
    }

    #[test]
    fn parse_full_example() {
        let input = "\
stateDiagram-v2
    [*] --> Idle
    Idle --> Processing : submit
    Processing --> Done : complete
    Done --> [*]";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 4);
    }

    #[test]
    fn parse_composite_state() {
        let input = "\
stateDiagram-v2
    [*] --> Active
    state Active {
        [*] --> Running
        Running --> [*]
    }
    Active --> [*]";
        let result = parse_state_diagram(input).unwrap();
        // [*] --> Active, state Active { ... }, Active --> [*]
        assert_eq!(result.model.statements.len(), 3);
        let StateStatement::State(decl) = &result.model.statements[1] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "Active");
        assert_eq!(decl.children.len(), 2);
    }

    #[test]
    fn parse_composite_with_direction() {
        let input = "\
stateDiagram-v2
    state Processing {
        direction LR
        [*] --> Validating
        Validating --> [*]
    }";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "Processing");
        // direction LR + two transitions
        assert_eq!(decl.children.len(), 3);
        let StateStatement::Direction(dir) = &decl.children[0] else {
            panic!("expected direction");
        };
        assert_eq!(dir, "LR");
    }

    #[test]
    fn parse_inline_description() {
        let input = "stateDiagram-v2\n    Active : The system is active";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "Active");
        assert_eq!(decl.descriptions, vec!["The system is active".to_string()]);
    }

    #[test]
    fn parse_captures_classdef_style_class() {
        let input = "\
stateDiagram-v2
    classDef badState fill:red
    class Error badState
    style Active fill:green
    A --> B";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 4); // classDef + class + style + transition
        assert!(matches!(
            &result.model.statements[0],
            StateStatement::ClassDef(_)
        ));
        assert!(matches!(
            &result.model.statements[1],
            StateStatement::ClassApply(_)
        ));
        assert!(matches!(
            &result.model.statements[2],
            StateStatement::Style(_)
        ));
        assert!(matches!(
            &result.model.statements[3],
            StateStatement::Transition(_)
        ));
    }

    #[test]
    fn parse_stereotype_join() {
        let result = parse_state_diagram("stateDiagram-v2\n    state jn <<join>>").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Join));
    }

    #[test]
    fn parse_stereotype_choice() {
        let result = parse_state_diagram("stateDiagram-v2\n    state ch <<choice>>").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Choice));
    }

    #[test]
    fn parse_bracket_stereotype_fork() {
        let result = parse_state_diagram("stateDiagram-v2\n    state fk [[fork]]").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Fork));
    }

    #[test]
    fn parse_bracket_stereotype_join() {
        let result = parse_state_diagram("stateDiagram-v2\n    state jn [[join]]").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Join));
    }

    #[test]
    fn parse_bracket_stereotype_choice() {
        let result = parse_state_diagram("stateDiagram-v2\n    state ch [[choice]]").unwrap();
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Choice));
    }

    #[test]
    fn parse_v1_header() {
        let result = parse_state_diagram("stateDiagram\n    A --> B").unwrap();
        assert_eq!(result.model.statements.len(), 1);
    }

    #[test]
    fn detect_v1_header() {
        assert_eq!(
            crate::mermaid::detect_diagram_type("stateDiagram\n[*] --> Idle"),
            Some(crate::mermaid::DiagramType::State)
        );
    }

    #[test]
    fn parse_note_single_line() {
        let input = "stateDiagram-v2\n    note right of State1 : Important info";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 1);
        let StateStatement::Note(note) = &result.model.statements[0] else {
            panic!("expected note");
        };
        assert_eq!(note.state_id, "State1");
        assert_eq!(note.position, NotePosition::Right);
        assert_eq!(note.text, "Important info");
    }

    #[test]
    fn parse_note_left_of() {
        let input = "stateDiagram-v2\n    note left of State2 : Left note";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::Note(note) = &result.model.statements[0] else {
            panic!("expected note");
        };
        assert_eq!(note.position, NotePosition::Left);
        assert_eq!(note.state_id, "State2");
    }

    #[test]
    fn parse_note_multiline() {
        let input = "\
stateDiagram-v2
    note right of State1
        Line one
        Line two
    end note";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::Note(note) = &result.model.statements[0] else {
            panic!("expected note");
        };
        assert_eq!(note.state_id, "State1");
        assert_eq!(note.position, NotePosition::Right);
        assert_eq!(note.text, "Line one\nLine two");
    }

    #[test]
    fn parse_transition_with_class_annotation_to() {
        let input = "stateDiagram-v2\n    [*] --> Active:::running\n";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::Transition(t) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.to, "Active");
        assert_eq!(t.to_class, Some("running".to_string()));
        assert_eq!(t.from_class, None);
    }

    #[test]
    fn parse_transition_with_class_annotation_from() {
        let input = "stateDiagram-v2\n    Active:::running --> Done\n";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::Transition(t) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.from, "Active");
        assert_eq!(t.from_class, Some("running".to_string()));
        assert_eq!(t.to_class, None);
    }

    #[test]
    fn parse_transition_no_class_annotation() {
        let input = "stateDiagram-v2\n    [*] --> Active\n";
        let result = parse_state_diagram(input).unwrap();
        let StateStatement::Transition(t) = &result.model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.from_class, None);
        assert_eq!(t.to_class, None);
    }

    #[test]
    fn parse_still_discards_click() {
        let input = "stateDiagram-v2\n    click Idle callback\n    [*] --> Idle\n";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 1);
        assert!(matches!(
            &result.model.statements[0],
            StateStatement::Transition(_)
        ));
    }

    #[test]
    fn parse_state_decl_class_annotation() {
        let input = "stateDiagram-v2\n    state Running:::active\n";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 1);
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "Running");
        assert_eq!(decl.class_name.as_deref(), Some("active"));
    }

    #[test]
    fn parse_state_decl_alias_class_annotation() {
        let input = "stateDiagram-v2\n    state \"Running\" as R:::active\n";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 1);
        let StateStatement::State(decl) = &result.model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "R");
        assert_eq!(decl.class_name.as_deref(), Some("active"));
    }

    #[test]
    fn parse_skipped_lines_produce_warnings() {
        let input = "stateDiagram-v2\n    [*] --> Idle\n    unknown directive here\n";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.model.statements.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0]
                .message
                .contains("unknown directive here")
        );
    }

    #[test]
    fn parse_skipped_line_with_frontmatter_reports_original_line_number() {
        let input = "---\nconfig:\n  theme: dark\n---\nstateDiagram-v2\n    [*] --> Idle\n    unknown line\n";
        let result = parse_state_diagram(input).unwrap();
        assert_eq!(result.warnings.len(), 1);
        // "unknown line" is on physical line 7 (1-indexed), not line 3.
        assert_eq!(result.warnings[0].line, Some(7));
    }

    #[test]
    fn parse_no_warnings_for_clean_input() {
        let input = "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Done\n    Done --> [*]\n";
        let result = parse_state_diagram(input).unwrap();
        assert!(result.warnings.is_empty());
    }
}
