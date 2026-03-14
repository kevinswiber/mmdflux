//! Shared timeline layout engine for sequence diagrams.
//!
//! Computes character-grid positions for participants, messages,
//! and notes. Output is consumed by the text renderer.

use super::model::{MessageStyle, Participant, ParticipantKind, Sequence, SequenceEvent};

/// Minimum gap between participant centers (characters).
const MIN_PARTICIPANT_GAP: usize = 20;

/// Extra padding around message labels.
const LABEL_PADDING: usize = 4;

/// Height of the participant header box (top border + label + bottom border).
const HEADER_HEIGHT: usize = 3;

/// Vertical gap between events.
const EVENT_GAP: usize = 1;

/// Height of a self-message (outgoing + vertical + return).
const SELF_MSG_HEIGHT: usize = 3;

/// Width of the self-message loop arm.
pub const SELF_MSG_WIDTH: usize = 4;

/// Layout data for one participant.
#[derive(Debug, Clone)]
pub struct ParticipantLayout {
    /// Center X column (where the lifeline is drawn).
    pub center_x: usize,
    /// Left X of the header box.
    pub box_x: usize,
    /// Width of the header box.
    pub box_width: usize,
    /// Display label.
    pub label: String,
    /// Participant or Actor.
    #[allow(dead_code)]
    pub kind: ParticipantKind,
}

/// A positioned event row in the layout.
#[derive(Debug, Clone)]
pub enum RowLayout {
    /// A message arrow between (or within) lifelines.
    Message {
        /// Y row where the arrow is drawn.
        y: usize,
        /// Source participant index.
        from_idx: usize,
        /// Target participant index.
        to_idx: usize,
        /// Solid or dashed.
        style: MessageStyle,
        /// Label text.
        text: String,
        /// Autonumber prefix if any.
        number: Option<usize>,
    },
    /// A note box over a participant.
    Note {
        /// Y row of the note top border.
        y: usize,
        /// Participant index.
        over_idx: usize,
        /// Note text.
        text: String,
    },
}

/// Complete sequence diagram layout.
#[derive(Debug, Clone)]
pub struct SequenceLayout {
    /// Participant positions.
    pub participants: Vec<ParticipantLayout>,
    /// Positioned event rows.
    pub rows: Vec<RowLayout>,
    /// Total canvas width.
    pub width: usize,
    /// Total canvas height.
    pub height: usize,
}

/// Compute layout for a sequence model.
pub fn layout(model: &Sequence) -> SequenceLayout {
    if model.participants.is_empty() {
        return SequenceLayout {
            participants: Vec::new(),
            rows: Vec::new(),
            width: 0,
            height: 0,
        };
    }

    let participant_gap = compute_participant_gap(model);
    let participants = layout_participants(&model.participants, participant_gap);

    let mut rows = Vec::new();
    let mut cursor_y = HEADER_HEIGHT + EVENT_GAP;

    for event in &model.events {
        match event {
            SequenceEvent::Message {
                from,
                to,
                style,
                text,
                number,
            } => {
                let is_self = from == to;
                if is_self {
                    // Self-message: label on the row, then the loop
                    rows.push(RowLayout::Message {
                        y: cursor_y,
                        from_idx: *from,
                        to_idx: *to,
                        style: *style,
                        text: text.clone(),
                        number: *number,
                    });
                    cursor_y += SELF_MSG_HEIGHT + EVENT_GAP;
                } else {
                    // Normal message: label row + arrow row = 2 rows
                    rows.push(RowLayout::Message {
                        y: cursor_y,
                        from_idx: *from,
                        to_idx: *to,
                        style: *style,
                        text: text.clone(),
                        number: *number,
                    });
                    cursor_y += 1 + EVENT_GAP;
                }
            }
            SequenceEvent::Note { over, text } => {
                rows.push(RowLayout::Note {
                    y: cursor_y,
                    over_idx: *over,
                    text: text.clone(),
                });
                // Note box: 3 rows (border + text + border)
                cursor_y += 3 + EVENT_GAP;
            }
        }
    }

    // Compute total dimensions
    let max_participant_right = participants
        .iter()
        .map(|p| p.box_x + p.box_width)
        .max()
        .unwrap_or(0);

    // Account for self-message loops + labels extending right
    let self_msg_extra = model
        .events
        .iter()
        .filter_map(|e| match e {
            SequenceEvent::Message {
                from,
                to,
                text,
                number,
                ..
            } if from == to => {
                let prefix_len = number.map(|n| format!("{n}. ").len()).unwrap_or(0);
                let label_len = text.len() + prefix_len;
                // arm (SELF_MSG_WIDTH) + gap (2) + label
                Some(SELF_MSG_WIDTH + 2 + label_len)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);

    // Account for note boxes extending right
    let note_extra = model
        .events
        .iter()
        .filter_map(|e| match e {
            SequenceEvent::Note { over, text } => {
                let box_width = text.len() + 4;
                let center_x = participants[*over].center_x;
                let box_right = center_x.saturating_sub(box_width / 2) + box_width;
                Some(box_right)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let base_width = max_participant_right.max(note_extra);
    let width = base_width + self_msg_extra + 2; // +2 for right margin
    let height = cursor_y;

    SequenceLayout {
        participants,
        rows,
        width,
        height,
    }
}

/// Compute the gap between participant centers based on message labels.
fn compute_participant_gap(model: &Sequence) -> usize {
    let max_label_len = model
        .events
        .iter()
        .filter_map(|e| match e {
            SequenceEvent::Message {
                from,
                to,
                text,
                number,
                ..
            } if from != to => {
                let prefix_len = number.map(|n| format!("{n}. ").len()).unwrap_or(0);
                Some(text.len() + prefix_len)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);

    (max_label_len + LABEL_PADDING).max(MIN_PARTICIPANT_GAP)
}

/// Compute horizontal positions for all participants.
fn layout_participants(participants: &[Participant], gap: usize) -> Vec<ParticipantLayout> {
    let mut result = Vec::with_capacity(participants.len());
    let mut x = 1; // left margin

    for (i, p) in participants.iter().enumerate() {
        let box_width = p.label.len() + 4; // │ + space + label + space + │
        let center_x = x + box_width / 2;

        result.push(ParticipantLayout {
            center_x,
            box_x: x,
            box_width,
            label: p.label.clone(),
            kind: p.kind.clone(),
        });

        if i < participants.len() - 1 {
            // Next participant starts at center_x + gap - half of next box
            let next_label_len = participants[i + 1].label.len();
            let next_box_width = next_label_len + 4;
            x = center_x + gap - next_box_width / 2;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagrams::sequence::compiler;
    use crate::mermaid::sequence::parse_sequence;

    fn layout_input(input: &str) -> SequenceLayout {
        let stmts = parse_sequence(input).unwrap();
        let model = compiler::compile(&stmts).unwrap();
        layout(&model)
    }

    #[test]
    fn layout_empty_model() {
        let layout = layout(&Sequence {
            participants: Vec::new(),
            events: Vec::new(),
            autonumber: false,
        });
        assert!(layout.participants.is_empty());
        assert!(layout.rows.is_empty());
        assert_eq!(layout.width, 0);
        assert_eq!(layout.height, 0);
    }

    #[test]
    fn layout_participants_ordered_left_to_right() {
        let layout = layout_input("sequenceDiagram\nparticipant A\nparticipant B\nparticipant C");
        assert_eq!(layout.participants.len(), 3);
        assert!(layout.participants[0].center_x < layout.participants[1].center_x);
        assert!(layout.participants[1].center_x < layout.participants[2].center_x);
    }

    #[test]
    fn layout_participant_gap_accommodates_labels() {
        let layout = layout_input(
            "sequenceDiagram\nparticipant A\nparticipant B\nA->>B: a very long message label here",
        );
        let gap = layout.participants[1].center_x - layout.participants[0].center_x;
        // Gap must be at least as wide as the label + padding
        assert!(gap >= "a very long message label here".len() + LABEL_PADDING);
    }

    #[test]
    fn layout_y_advances_monotonically() {
        let layout = layout_input(
            "sequenceDiagram\nparticipant A\nparticipant B\nA->>B: first\nB->>A: second\nA->>B: third",
        );
        let ys: Vec<usize> = layout
            .rows
            .iter()
            .map(|r| match r {
                RowLayout::Message { y, .. } => *y,
                RowLayout::Note { y, .. } => *y,
            })
            .collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn layout_self_message_takes_more_rows() {
        let layout = layout_input(
            "sequenceDiagram\nparticipant A\nparticipant B\nA->>A: self\nA->>B: normal",
        );
        assert_eq!(layout.rows.len(), 2);
        let y0 = match &layout.rows[0] {
            RowLayout::Message { y, .. } => *y,
            _ => panic!("expected message"),
        };
        let y1 = match &layout.rows[1] {
            RowLayout::Message { y, .. } => *y,
            _ => panic!("expected message"),
        };
        // Self-message should advance more than a normal message
        assert!(y1 - y0 >= SELF_MSG_HEIGHT);
    }

    #[test]
    fn layout_note_has_height() {
        let layout = layout_input(
            "sequenceDiagram\nparticipant A\nparticipant B\nNote over A: note\nA->>B: msg",
        );
        assert_eq!(layout.rows.len(), 2);
        let note_y = match &layout.rows[0] {
            RowLayout::Note { y, .. } => *y,
            _ => panic!("expected note"),
        };
        let msg_y = match &layout.rows[1] {
            RowLayout::Message { y, .. } => *y,
            _ => panic!("expected message"),
        };
        // Note occupies 3 rows + gap
        assert!(msg_y >= note_y + 3);
    }

    #[test]
    fn layout_header_height() {
        let layout = layout_input("sequenceDiagram\nparticipant A\nA->>A: hi");
        // First event should start after header + gap
        let first_y = match &layout.rows[0] {
            RowLayout::Message { y, .. } => *y,
            _ => panic!("expected message"),
        };
        assert_eq!(first_y, HEADER_HEIGHT + EVENT_GAP);
    }

    #[test]
    fn layout_box_width_matches_label() {
        let layout = layout_input("sequenceDiagram\nparticipant A as Alice");
        assert_eq!(layout.participants[0].box_width, "Alice".len() + 4);
    }
}
