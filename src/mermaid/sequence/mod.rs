//! Sequence diagram parser.
//!
//! Hand-written line-oriented parser for Mermaid sequence diagram syntax.
//! Supports MVP scope: participant/actor declarations, solid/dashed messages,
//! self-messages, notes over one participant, and autonumber.

pub mod ast;

use ast::{ArrowType, ParticipantKind, SequenceStatement};

/// Parse a sequence diagram from Mermaid input text.
///
/// Expects the input to start with `sequenceDiagram` (case-insensitive).
/// Returns a list of parsed statements in source order.
pub fn parse_sequence(
    input: &str,
) -> Result<Vec<SequenceStatement>, Box<dyn std::error::Error + Send + Sync>> {
    let mut statements = Vec::new();
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
        if trimmed.to_lowercase() == "sequencediagram" {
            found_header = true;
            lines.next();
            break;
        }
        return Err(format!("Expected 'sequenceDiagram' header, got: {trimmed}").into());
    }

    if !found_header {
        return Err("Missing 'sequenceDiagram' header".into());
    }

    // Parse body lines
    for line in lines {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }

        // Try each construct in order
        if trimmed.to_lowercase() == "autonumber" {
            statements.push(SequenceStatement::Autonumber);
            continue;
        }

        if let Some(stmt) = try_parse_participant(trimmed) {
            statements.push(stmt);
            continue;
        }

        if let Some(stmt) = try_parse_note(trimmed) {
            statements.push(stmt);
            continue;
        }

        if let Some(stmt) = try_parse_message(trimmed) {
            statements.push(stmt);
            continue;
        }

        // Permissive: skip unrecognized lines
    }

    Ok(statements)
}

/// Try to parse a `participant` or `actor` declaration.
fn try_parse_participant(line: &str) -> Option<SequenceStatement> {
    let lower = line.to_lowercase();
    let (kind, rest) = if lower.starts_with("participant ") || lower.starts_with("participant\t") {
        (ParticipantKind::Participant, &line["participant".len()..])
    } else if lower.starts_with("actor ") || lower.starts_with("actor\t") {
        (ParticipantKind::Actor, &line["actor".len()..])
    } else {
        return None;
    };

    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // Check for alias: `participant A as Alice`
    let lower_rest = rest.to_lowercase();
    if let Some(as_pos) = lower_rest.find(" as ") {
        let id = rest[..as_pos].trim().to_string();
        let alias = rest[as_pos + 4..].trim().to_string();
        if !id.is_empty() && !alias.is_empty() {
            return Some(SequenceStatement::Participant {
                kind,
                id,
                alias: Some(alias),
            });
        }
    }

    Some(SequenceStatement::Participant {
        kind,
        id: rest.to_string(),
        alias: None,
    })
}

/// Try to parse a `Note over <participant>: text` statement.
fn try_parse_note(line: &str) -> Option<SequenceStatement> {
    let lower = line.to_lowercase();
    if !lower.starts_with("note over ") {
        return None;
    }

    let rest = &line["note over ".len()..];
    let colon_pos = rest.find(':')?;
    let over = rest[..colon_pos].trim().to_string();
    let text = rest[colon_pos + 1..].trim().to_string();

    if over.is_empty() || text.is_empty() {
        return None;
    }

    Some(SequenceStatement::Note { over, text })
}

/// Try to parse a message: `A->>B: text` or `A-->>B: text`.
fn try_parse_message(line: &str) -> Option<SequenceStatement> {
    // Arrow patterns, ordered by length (longest first)
    static ARROWS: &[(&str, ArrowType)] = &[("-->>", ArrowType::Dashed), ("->>", ArrowType::Solid)];

    for &(pattern, arrow) in ARROWS {
        if let Some(arrow_pos) = line.find(pattern) {
            let from = line[..arrow_pos].trim().to_string();
            let rest = line[arrow_pos + pattern.len()..].trim();

            if from.is_empty() {
                continue;
            }

            // Split on first colon for "to: text"
            let (to, text) = if let Some(colon_pos) = rest.find(':') {
                let to = rest[..colon_pos].trim().to_string();
                let text = rest[colon_pos + 1..].trim().to_string();
                (to, text)
            } else {
                (rest.to_string(), String::new())
            };

            if to.is_empty() {
                continue;
            }

            return Some(SequenceStatement::Message {
                from,
                to,
                arrow,
                text,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use ast::*;

    use super::*;

    #[test]
    fn parse_empty_diagram() {
        let stmts = parse_sequence("sequenceDiagram\n").unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn parse_participant() {
        let stmts = parse_sequence("sequenceDiagram\nparticipant A").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Participant {
                kind: ParticipantKind::Participant,
                id: "A".to_string(),
                alias: None,
            }
        );
    }

    #[test]
    fn parse_actor() {
        let stmts = parse_sequence("sequenceDiagram\nactor Bob").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Participant {
                kind: ParticipantKind::Actor,
                id: "Bob".to_string(),
                alias: None,
            }
        );
    }

    #[test]
    fn parse_participant_with_alias() {
        let stmts = parse_sequence("sequenceDiagram\nparticipant A as Alice").unwrap();
        assert_eq!(
            stmts[0],
            SequenceStatement::Participant {
                kind: ParticipantKind::Participant,
                id: "A".to_string(),
                alias: Some("Alice".to_string()),
            }
        );
    }

    #[test]
    fn parse_actor_with_alias() {
        let stmts = parse_sequence("sequenceDiagram\nactor B as Bob").unwrap();
        assert_eq!(
            stmts[0],
            SequenceStatement::Participant {
                kind: ParticipantKind::Actor,
                id: "B".to_string(),
                alias: Some("Bob".to_string()),
            }
        );
    }

    #[test]
    fn parse_solid_message() {
        let stmts = parse_sequence("sequenceDiagram\nA->>B: hello").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Message {
                from: "A".to_string(),
                to: "B".to_string(),
                arrow: ArrowType::Solid,
                text: "hello".to_string(),
            }
        );
    }

    #[test]
    fn parse_dashed_message() {
        let stmts = parse_sequence("sequenceDiagram\nA-->>B: response").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Message {
                from: "A".to_string(),
                to: "B".to_string(),
                arrow: ArrowType::Dashed,
                text: "response".to_string(),
            }
        );
    }

    #[test]
    fn parse_self_message() {
        let stmts = parse_sequence("sequenceDiagram\nA->>A: think").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Message {
                from: "A".to_string(),
                to: "A".to_string(),
                arrow: ArrowType::Solid,
                text: "think".to_string(),
            }
        );
    }

    #[test]
    fn parse_note_over() {
        let stmts = parse_sequence("sequenceDiagram\nNote over A: done").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            SequenceStatement::Note {
                over: "A".to_string(),
                text: "done".to_string(),
            }
        );
    }

    #[test]
    fn parse_autonumber() {
        let stmts = parse_sequence("sequenceDiagram\nautonumber").unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0], SequenceStatement::Autonumber);
    }

    #[test]
    fn parse_full_mvp_example() {
        let input = "\
sequenceDiagram
    autonumber
    participant A
    participant B
    A->>B: hello
    B-->>A: hi back
    A->>A: think
    Note over A: done";
        let stmts = parse_sequence(input).unwrap();
        assert_eq!(stmts.len(), 7);
        assert_eq!(stmts[0], SequenceStatement::Autonumber);
        assert!(matches!(&stmts[1], SequenceStatement::Participant { id, .. } if id == "A"));
        assert!(matches!(&stmts[2], SequenceStatement::Participant { id, .. } if id == "B"));
        assert!(
            matches!(&stmts[3], SequenceStatement::Message { from, to, arrow: ArrowType::Solid, .. } if from == "A" && to == "B")
        );
        assert!(
            matches!(&stmts[4], SequenceStatement::Message { from, to, arrow: ArrowType::Dashed, .. } if from == "B" && to == "A")
        );
        assert!(
            matches!(&stmts[5], SequenceStatement::Message { from, to, .. } if from == "A" && to == "A")
        );
        assert!(matches!(&stmts[6], SequenceStatement::Note { over, .. } if over == "A"));
    }

    #[test]
    fn parse_skips_comments() {
        let input = "sequenceDiagram\n%% comment\nparticipant A";
        let stmts = parse_sequence(input).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parse_skips_empty_lines() {
        let input = "sequenceDiagram\n\nparticipant A\n\nA->>B: hi";
        let stmts = parse_sequence(input).unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn parse_case_insensitive_header() {
        let stmts = parse_sequence("SEQUENCEDIAGRAM\nparticipant A").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parse_missing_header_errors() {
        let result = parse_sequence("participant A\nA->>B: hi");
        assert!(result.is_err());
    }

    #[test]
    fn parse_skips_frontmatter() {
        let input = "---\ntitle: Test\n---\nsequenceDiagram\nparticipant A";
        let stmts = parse_sequence(input).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parse_note_case_insensitive() {
        let stmts = parse_sequence("sequenceDiagram\nnote over A: done").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], SequenceStatement::Note { .. }));
    }

    #[test]
    fn parse_message_without_text() {
        let stmts = parse_sequence("sequenceDiagram\nA->>B:").unwrap();
        // Message with empty text after colon
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], SequenceStatement::Message { text, .. } if text.is_empty()));
    }

    #[test]
    fn parse_message_no_colon() {
        let stmts = parse_sequence("sequenceDiagram\nA->>B").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], SequenceStatement::Message { text, .. } if text.is_empty()));
    }

    #[test]
    fn parse_permissive_skips_unknown() {
        let input = "sequenceDiagram\nactivate A\nparticipant B\ndeactivate A";
        let stmts = parse_sequence(input).unwrap();
        // Only participant B is recognized, activate/deactivate are skipped
        assert_eq!(stmts.len(), 1);
    }
}
