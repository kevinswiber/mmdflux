//! Sequence layout tests that require cross-boundary imports (diagrams + mermaid).
//! Moved from timeline::sequence::layout to respect module boundary rules.

use crate::diagrams::sequence::compiler;
use crate::mermaid::sequence::parse_sequence;
use crate::timeline::sequence::layout::{
    EVENT_GAP, HEADER_HEIGHT, LABEL_PADDING, RowLayout, SELF_MSG_HEIGHT, SequenceLayout, layout,
};

fn layout_input(input: &str) -> SequenceLayout {
    let result = parse_sequence(input).unwrap();
    let model = compiler::compile(&result.statements).unwrap();
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

#[test]
fn layout_create_participant_moves_header_to_creation_point() {
    let layout = layout_input(
        "\
sequenceDiagram
    participant A
    create participant B
    A->>B: Create
    B-->>A: Ready",
    );

    assert!(layout.participants[1].box_y > layout.participants[0].box_y);
    assert_eq!(
        layout.participants[1].lifeline_start_y,
        layout.participants[1].box_y + HEADER_HEIGHT
    );
    match &layout.rows[0] {
        RowLayout::Message {
            y,
            from_x,
            to_x,
            from_idx,
            to_idx,
            ..
        } => {
            assert_eq!(*from_idx, 0);
            assert_eq!(*to_idx, 1);
            assert_eq!(*to_x, layout.participants[1].box_x);
            assert_eq!(*y, layout.participants[1].box_y + 1);
            assert!(from_x < to_x);
        }
        _ => panic!("expected create message row"),
    }
}

#[test]
fn layout_destroy_participant_truncates_lifeline() {
    let layout = layout_input(
        "\
sequenceDiagram
    participant A
    participant B
    destroy B
    A->>B: Goodbye",
    );

    let destroy_y = layout.participants[1]
        .destroy_y
        .expect("destroy marker should be recorded");
    assert_eq!(layout.participants[1].lifeline_end_y, destroy_y);
    assert!(layout.participants[1].lifeline_end_y < layout.height);
}

#[test]
fn layout_title_offsets_headers() {
    let layout =
        layout_input("sequenceDiagram\ntitle Authentication Flow\nparticipant A\nA->>A: hi");
    assert_eq!(
        layout.title.as_ref().map(|title| title.text.as_str()),
        Some("Authentication Flow")
    );
    assert!(layout.participants[0].box_y > 0);
}

#[test]
fn layout_tracks_interaction_blocks() {
    let layout = layout_input(
        "\
sequenceDiagram
    participant A
    participant B
    alt available
        A->>B: Request
    else busy
        B->>A: Retry later
    end",
    );
    assert_eq!(layout.blocks.len(), 1);
    let block = &layout.blocks[0];
    assert!(block.left_x < block.right_x);
    assert_eq!(block.dividers.len(), 1);
    assert!(block.top_y < block.dividers[0].y);
    assert!(block.dividers[0].y < block.bottom_y);
}

#[test]
fn layout_nested_blocks_have_increasing_depth() {
    let layout = layout_input(
        "\
sequenceDiagram
    participant A
    participant B
    loop outer
        alt ready
            A->>B: Request
        else later
            B->>A: Retry
        end
    end",
    );
    assert_eq!(layout.blocks.len(), 2);
    assert_eq!(layout.blocks[0].depth, 0);
    assert_eq!(layout.blocks[1].depth, 1);
}

#[test]
fn layout_measures_participant_boxes_in_terminal_cells() {
    let layout = layout_input(
        "sequenceDiagram\nparticipant A as 日本語の参加者\nparticipant B as Bob\nA->>B: hi",
    );
    // Seven wide glyphs are 14 cells; the box adds a border and a space each side.
    assert_eq!(layout.participants[0].box_width, 18);
    assert_eq!(layout.participants[1].box_width, 7);
}

#[test]
fn layout_keeps_adjacent_participant_boxes_apart() {
    let layout = layout_input(
        "sequenceDiagram\nparticipant A as A participant with a very long display name\nparticipant B as Another participant with a long name too\nA->>B: hi\nB-->>A: ok",
    );
    for pair in layout.participants.windows(2) {
        assert!(
            pair[0].box_x + pair[0].box_width < pair[1].box_x,
            "boxes overlap: {:?} and {:?}",
            (pair[0].box_x, pair[0].box_width),
            (pair[1].box_x, pair[1].box_width)
        );
    }
}
