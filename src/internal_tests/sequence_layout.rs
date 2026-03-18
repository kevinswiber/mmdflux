//! Sequence layout tests that require cross-boundary imports (diagrams + mermaid).
//! Moved from timeline::sequence::layout to respect module boundary rules.

use crate::diagrams::sequence::compiler;
use crate::mermaid::sequence::parse_sequence;
use crate::timeline::sequence::layout::{
    EVENT_GAP, HEADER_HEIGHT, LABEL_PADDING, RowLayout, SELF_MSG_HEIGHT, SequenceLayout, layout,
};

fn layout_input(input: &str) -> SequenceLayout {
    let stmts = parse_sequence(input).unwrap();
    let model = compiler::compile(&stmts).unwrap();
    layout(&model)
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
    let layout =
        layout_input("sequenceDiagram\nparticipant A\nparticipant B\nA->>A: self\nA->>B: normal");
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
