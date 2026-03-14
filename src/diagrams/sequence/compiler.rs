//! Sequence diagram compiler.
//!
//! Compiles raw parsed AST statements into a validated `Sequence`.
//! Resolves participant references, assigns stable ordering, and
//! applies autonumbering.

use std::collections::HashMap;

use crate::mermaid::sequence::ast::{ArrowType, SequenceStatement};
use crate::timeline::sequence::model::{
    MessageStyle, Participant, ParticipantKind, Sequence, SequenceEvent,
};

/// Compile parsed sequence statements into a validated model.
///
/// Participants are ordered by first appearance (explicit declarations first,
/// then implicit from message endpoints). Unknown participant references in
/// notes produce an error.
pub fn compile(
    statements: &[SequenceStatement],
) -> Result<Sequence, Box<dyn std::error::Error + Send + Sync>> {
    let mut participants: Vec<Participant> = Vec::new();
    let mut participant_index: HashMap<String, usize> = HashMap::new();
    let mut events: Vec<SequenceEvent> = Vec::new();
    let mut autonumber = false;
    let mut message_counter: usize = 0;

    // First pass: collect explicit participant declarations (preserving order)
    for stmt in statements {
        if let SequenceStatement::Participant { kind, id, alias } = stmt
            && !participant_index.contains_key(id)
        {
            let idx = participants.len();
            participants.push(Participant {
                id: id.clone(),
                label: alias.as_deref().unwrap_or(id).to_string(),
                kind: kind.clone(),
            });
            participant_index.insert(id.clone(), idx);
        }
        if matches!(stmt, SequenceStatement::Autonumber) {
            autonumber = true;
        }
    }

    // Second pass: process messages and notes, creating implicit participants as needed
    for stmt in statements {
        match stmt {
            SequenceStatement::Message {
                from,
                to,
                arrow,
                text,
            } => {
                let from_idx = ensure_participant(&mut participants, &mut participant_index, from);
                let to_idx = ensure_participant(&mut participants, &mut participant_index, to);

                message_counter += 1;
                events.push(SequenceEvent::Message {
                    from: from_idx,
                    to: to_idx,
                    style: match arrow {
                        ArrowType::Solid => MessageStyle::Solid,
                        ArrowType::Dashed => MessageStyle::Dashed,
                    },
                    text: text.clone(),
                    number: if autonumber {
                        Some(message_counter)
                    } else {
                        None
                    },
                });
            }
            SequenceStatement::Note { over, text } => {
                let over_idx = participant_index.get(over.as_str()).copied().ok_or_else(
                    || -> Box<dyn std::error::Error + Send + Sync> {
                        format!("Note references unknown participant: {over}").into()
                    },
                )?;
                events.push(SequenceEvent::Note {
                    over: over_idx,
                    text: text.clone(),
                });
            }
            SequenceStatement::Participant { .. } | SequenceStatement::Autonumber => {
                // Already handled in first pass
            }
        }
    }

    Ok(Sequence {
        participants,
        events,
        autonumber,
    })
}

/// Ensure a participant exists, creating an implicit one if needed.
/// Returns the participant index.
fn ensure_participant(
    participants: &mut Vec<Participant>,
    index: &mut HashMap<String, usize>,
    id: &str,
) -> usize {
    if let Some(&idx) = index.get(id) {
        return idx;
    }
    let idx = participants.len();
    participants.push(Participant {
        id: id.to_string(),
        label: id.to_string(),
        kind: ParticipantKind::Participant,
    });
    index.insert(id.to_string(), idx);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::sequence::parse_sequence;

    fn compile_input(input: &str) -> Sequence {
        let stmts = parse_sequence(input).unwrap();
        compile(&stmts).unwrap()
    }

    #[test]
    fn compile_empty_diagram() {
        let model = compile_input("sequenceDiagram\n");
        assert!(model.participants.is_empty());
        assert!(model.events.is_empty());
        assert!(!model.autonumber);
    }

    #[test]
    fn compile_participants_in_order() {
        let model = compile_input("sequenceDiagram\nparticipant B\nparticipant A");
        assert_eq!(model.participants.len(), 2);
        assert_eq!(model.participants[0].id, "B");
        assert_eq!(model.participants[1].id, "A");
    }

    #[test]
    fn compile_participant_alias() {
        let model = compile_input("sequenceDiagram\nparticipant A as Alice");
        assert_eq!(model.participants[0].id, "A");
        assert_eq!(model.participants[0].label, "Alice");
    }

    #[test]
    fn compile_actor_kind() {
        let model = compile_input("sequenceDiagram\nactor B as Bob");
        assert_eq!(model.participants[0].kind, ParticipantKind::Actor);
    }

    #[test]
    fn compile_message_resolves_indices() {
        let model = compile_input("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hi");
        assert_eq!(model.events.len(), 1);
        match &model.events[0] {
            SequenceEvent::Message { from, to, .. } => {
                assert_eq!(*from, 0);
                assert_eq!(*to, 1);
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn compile_implicit_participants_from_messages() {
        let model = compile_input("sequenceDiagram\nA->>B: hi");
        assert_eq!(model.participants.len(), 2);
        assert_eq!(model.participants[0].id, "A");
        assert_eq!(model.participants[1].id, "B");
    }

    #[test]
    fn compile_explicit_before_implicit() {
        let model = compile_input("sequenceDiagram\nparticipant B\nA->>B: hi\nA->>C: hello");
        // B is explicit (index 0), A and C are implicit (1, 2)
        assert_eq!(model.participants[0].id, "B");
        assert_eq!(model.participants[1].id, "A");
        assert_eq!(model.participants[2].id, "C");
    }

    #[test]
    fn compile_self_message() {
        let model = compile_input("sequenceDiagram\nparticipant A\nA->>A: think");
        match &model.events[0] {
            SequenceEvent::Message { from, to, .. } => {
                assert_eq!(from, to);
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn compile_note_resolves_participant() {
        let model = compile_input("sequenceDiagram\nparticipant A\nNote over A: done");
        assert_eq!(model.events.len(), 1);
        match &model.events[0] {
            SequenceEvent::Note { over, text } => {
                assert_eq!(*over, 0);
                assert_eq!(text, "done");
            }
            _ => panic!("expected note"),
        }
    }

    #[test]
    fn compile_note_unknown_participant_errors() {
        let stmts = parse_sequence("sequenceDiagram\nNote over X: oops").unwrap();
        let result = compile(&stmts);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown participant"));
    }

    #[test]
    fn compile_autonumber() {
        let model = compile_input(
            "sequenceDiagram\nautonumber\nparticipant A\nparticipant B\nA->>B: first\nB->>A: second",
        );
        assert!(model.autonumber);
        match &model.events[0] {
            SequenceEvent::Message { number, .. } => assert_eq!(*number, Some(1)),
            _ => panic!("expected message"),
        }
        match &model.events[1] {
            SequenceEvent::Message { number, .. } => assert_eq!(*number, Some(2)),
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn compile_no_autonumber_no_numbers() {
        let model = compile_input("sequenceDiagram\nA->>B: hi");
        assert!(!model.autonumber);
        match &model.events[0] {
            SequenceEvent::Message { number, .. } => assert_eq!(*number, None),
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn compile_message_style_mapping() {
        let model = compile_input("sequenceDiagram\nA->>B: solid\nA-->>B: dashed");
        match &model.events[0] {
            SequenceEvent::Message { style, .. } => assert_eq!(*style, MessageStyle::Solid),
            _ => panic!("expected message"),
        }
        match &model.events[1] {
            SequenceEvent::Message { style, .. } => assert_eq!(*style, MessageStyle::Dashed),
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn compile_full_mvp() {
        let model = compile_input(
            "\
sequenceDiagram
    autonumber
    participant A as Alice
    participant B as Bob
    A->>B: hello
    B-->>A: hi back
    A->>A: think
    Note over A: done",
        );
        assert_eq!(model.participants.len(), 2);
        assert_eq!(model.participants[0].label, "Alice");
        assert_eq!(model.participants[1].label, "Bob");
        assert_eq!(model.events.len(), 4);
        assert!(model.autonumber);
    }
}
