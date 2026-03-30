//! State diagram parser.
//!
//! Hand-written recursive descent parser for `stateDiagram-v2` syntax.
//! Supports states, transitions, `[*]` pseudo-states, composite `state { }`
//! blocks, stereotypes, direction overrides, and state descriptions.

use crate::mermaid::flowchart::strip_frontmatter;

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
}

/// An explicit state declaration.
#[derive(Debug, Clone)]
pub struct StateDecl {
    /// State identifier.
    pub id: String,
    /// Optional description text.
    pub description: Option<String>,
    /// Optional alias (from `state "label" as id`).
    pub alias: Option<String>,
    /// Optional stereotype (fork, join, choice).
    pub stereotype: Option<StateStereotype>,
    /// Child statements (for composite states).
    pub children: Vec<StateStatement>,
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
}

/// Parse a `stateDiagram-v2` input into a [`StateModel`].
///
/// Returns an error if the `stateDiagram-v2` header is missing.
pub fn parse_state_diagram(
    input: &str,
) -> Result<StateModel, Box<dyn std::error::Error + Send + Sync>> {
    let input = strip_frontmatter(input);
    let lines: Vec<&str> = input.lines().collect();
    let mut pos = 0;
    let mut direction: Option<String> = None;

    // Skip leading comments and whitespace, then consume header.
    while pos < lines.len() {
        let trimmed = lines[pos].trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            pos += 1;
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("statediagram-v2") {
            pos += 1;
            break;
        }
        return Err(format!("Expected 'stateDiagram-v2' header, got: {trimmed}").into());
    }

    if pos == 0 {
        return Err("Missing 'stateDiagram-v2' header".into());
    }

    let statements = parse_body(&lines, &mut pos, &mut direction);

    Ok(StateModel {
        direction,
        statements,
    })
}

/// Parse statement lines until EOF or a closing `}`.
fn parse_body(
    lines: &[&str],
    pos: &mut usize,
    direction: &mut Option<String>,
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
                decl.children = parse_body(lines, pos, &mut inner_dir);
            }
            statements.push(StateStatement::State(decl));
            continue;
        }

        // Permissive: skip unrecognized lines.
        *pos += 1;
    }

    statements
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
    lower.starts_with("classdef ")
        || lower.starts_with("style ")
        || lower.starts_with("class ")
        || lower.starts_with("click ")
        || lower.starts_with("acctitle")
        || lower.starts_with("accdescr")
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
        "LR" | "RL" | "BT" | "TB" => Some(upper),
        _ => None,
    }
}

/// Try to parse a transition line: `from --> to` or `from --> to : label`.
fn try_parse_transition(line: &str) -> Option<StateTransition> {
    let arrow_pos = line.find("-->")?;
    let from = line[..arrow_pos].trim();
    let rest = line[arrow_pos + 3..].trim();

    if from.is_empty() || rest.is_empty() {
        return None;
    }

    let (to, label) = if let Some(colon_pos) = rest.find(':') {
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

    if to.is_empty() {
        return None;
    }

    Some(StateTransition {
        from: from.to_string(),
        to: to.to_string(),
        label,
    })
}

/// Try to parse `Id : description text` (inline state description).
fn try_parse_inline_description(line: &str) -> Option<StateDecl> {
    // Must not start with `state` keyword (that's handled separately).
    if line.to_lowercase().starts_with("state ") {
        return None;
    }
    let colon_pos = line.find(':')?;
    let id = line[..colon_pos].trim();
    let description = line[colon_pos + 1..].trim();

    // Id must be a valid identifier (no spaces, not empty, not [*]).
    if id.is_empty() || id.contains(' ') || id == "[*]" || description.is_empty() {
        return None;
    }

    Some(StateDecl {
        id: id.to_string(),
        description: Some(description.to_string()),
        alias: None,
        stereotype: None,
        children: Vec::new(),
    })
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
        let id = alias.clone().unwrap_or_else(|| description.clone());
        return Some(StateDecl {
            id,
            description: Some(description),
            alias,
            stereotype: None,
            children: Vec::new(),
        });
    }

    // `state id ...`
    let id = rest.split(|c: char| c.is_whitespace() || c == '{').next()?;
    if id.is_empty() {
        return None;
    }

    // Check for stereotype: `state id <<fork>>` etc.
    let stereotype = if rest.contains("<<fork>>") {
        Some(StateStereotype::Fork)
    } else if rest.contains("<<join>>") {
        Some(StateStereotype::Join)
    } else if rest.contains("<<choice>>") {
        Some(StateStereotype::Choice)
    } else {
        None
    };

    Some(StateDecl {
        id: id.to_string(),
        description: None,
        alias: None,
        stereotype,
        children: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_state_diagram() {
        let model = parse_state_diagram("stateDiagram-v2\n").unwrap();
        assert!(model.statements.is_empty());
        assert!(model.direction.is_none());
    }

    #[test]
    fn parse_missing_header_errors() {
        let result = parse_state_diagram("A --> B");
        assert!(result.is_err());
    }

    #[test]
    fn parse_basic_transition() {
        let model = parse_state_diagram("stateDiagram-v2\n    A --> B").unwrap();
        assert_eq!(model.statements.len(), 1);
        let StateStatement::Transition(t) = &model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.from, "A");
        assert_eq!(t.to, "B");
        assert!(t.label.is_none());
    }

    #[test]
    fn parse_transition_with_label() {
        let model = parse_state_diagram("stateDiagram-v2\n    A --> B : submit").unwrap();
        let StateStatement::Transition(t) = &model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t.label, Some("submit".to_string()));
    }

    #[test]
    fn parse_star_markers() {
        let model =
            parse_state_diagram("stateDiagram-v2\n    [*] --> Idle\n    Done --> [*]").unwrap();
        assert_eq!(model.statements.len(), 2);
        let StateStatement::Transition(t0) = &model.statements[0] else {
            panic!("expected transition");
        };
        assert_eq!(t0.from, "[*]");
        assert_eq!(t0.to, "Idle");
        let StateStatement::Transition(t1) = &model.statements[1] else {
            panic!("expected transition");
        };
        assert_eq!(t1.from, "Done");
        assert_eq!(t1.to, "[*]");
    }

    #[test]
    fn parse_direction_directive() {
        let model = parse_state_diagram("stateDiagram-v2\n    direction LR\n    A --> B").unwrap();
        assert_eq!(model.direction, Some("LR".to_string()));
    }

    #[test]
    fn parse_state_declaration_with_description() {
        let model =
            parse_state_diagram("stateDiagram-v2\n    state \"Waiting\" as waiting").unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "waiting");
        assert_eq!(decl.description, Some("Waiting".to_string()));
        assert_eq!(decl.alias, Some("waiting".to_string()));
    }

    #[test]
    fn parse_skips_comments() {
        let model = parse_state_diagram("stateDiagram-v2\n    %% comment\n    A --> B\n").unwrap();
        assert_eq!(model.statements.len(), 1);
    }

    #[test]
    fn parse_case_insensitive_header() {
        let model = parse_state_diagram("STATEDIAGRAM-V2\n    A --> B").unwrap();
        assert_eq!(model.statements.len(), 1);
    }

    #[test]
    fn parse_stereotype_fork() {
        let model = parse_state_diagram("stateDiagram-v2\n    state forkNode <<fork>>").unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
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
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 4);
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
        let model = parse_state_diagram(input).unwrap();
        // [*] --> Active, state Active { ... }, Active --> [*]
        assert_eq!(model.statements.len(), 3);
        let StateStatement::State(decl) = &model.statements[1] else {
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
        let model = parse_state_diagram(input).unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
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
        let model = parse_state_diagram(input).unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.id, "Active");
        assert_eq!(decl.description, Some("The system is active".to_string()));
    }

    #[test]
    fn parse_discards_classdef_style() {
        let input = "\
stateDiagram-v2
    classDef badState fill:red
    class Error badState
    style Active fill:green
    A --> B";
        let model = parse_state_diagram(input).unwrap();
        assert_eq!(model.statements.len(), 1); // only the transition
    }

    #[test]
    fn parse_stereotype_join() {
        let model = parse_state_diagram("stateDiagram-v2\n    state jn <<join>>").unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Join));
    }

    #[test]
    fn parse_stereotype_choice() {
        let model = parse_state_diagram("stateDiagram-v2\n    state ch <<choice>>").unwrap();
        let StateStatement::State(decl) = &model.statements[0] else {
            panic!("expected state decl");
        };
        assert_eq!(decl.stereotype, Some(StateStereotype::Choice));
    }
}
