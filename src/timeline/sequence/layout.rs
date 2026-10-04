//! Shared timeline layout engine for sequence diagrams.
//!
//! Computes character-grid positions for participants, messages,
//! and notes. Output is consumed by the text renderer.

use super::model::{
    ArrowHead, BlockDividerKind, BlockKind, LineStyle, NotePlacement, Participant, ParticipantBox,
    ParticipantKind, Sequence, SequenceEvent,
};
use crate::format::cell_advance_width;

/// Minimum gap between participant centers (characters).
const MIN_PARTICIPANT_GAP: usize = 20;

/// Extra padding around message labels.
pub(crate) const LABEL_PADDING: usize = 4;

/// Height of the participant header box (top border + label + bottom border).
pub(crate) const HEADER_HEIGHT: usize = 3;

/// Height reserved for an optional diagram title row.
const TITLE_HEIGHT: usize = 1;

/// Gap between the optional title row and participant content.
const TITLE_GAP: usize = 1;

/// Y offset applied to participant headers when participant boxes are present.
pub(crate) const PARTICIPANT_BOX_HEADER_OFFSET: usize = 2;

/// Horizontal padding around grouped participant columns.
const PARTICIPANT_BOX_PADDING_X: usize = 2;

/// Extra bottom padding inside participant boxes.
const PARTICIPANT_BOX_PADDING_BOTTOM: usize = 1;

/// Vertical gap between events.
pub(crate) const EVENT_GAP: usize = 1;

/// Height of a self-message (outgoing + vertical + return).
pub(crate) const SELF_MSG_HEIGHT: usize = 3;

/// Width of the self-message loop arm.
pub const SELF_MSG_WIDTH: usize = 4;

/// Layout data for an optional diagram title.
#[derive(Debug, Clone)]
pub struct TitleLayout {
    /// Title text displayed above participants.
    pub text: String,
    /// Y row for the centered title text.
    pub y: usize,
}

/// Layout data for one participant.
#[derive(Debug, Clone)]
pub struct ParticipantLayout {
    /// Center X column (where the lifeline is drawn).
    pub center_x: usize,
    /// Top Y of the header box.
    pub box_y: usize,
    /// Left X of the header box.
    pub box_x: usize,
    /// Width of the header box.
    pub box_width: usize,
    /// Y row where the participant lifeline starts.
    pub lifeline_start_y: usize,
    /// Exclusive Y row where the participant lifeline ends.
    pub lifeline_end_y: usize,
    /// Optional Y row for a destruction marker.
    pub destroy_y: Option<usize>,
    /// Display label.
    pub label: String,
    /// Participant or Actor.
    #[allow(dead_code)]
    pub kind: ParticipantKind,
}

/// A positioned box around a group of participant columns.
#[derive(Debug, Clone)]
pub struct ParticipantBoxLayout {
    pub top_y: usize,
    pub bottom_y: usize,
    pub left_x: usize,
    pub right_x: usize,
    pub label: Option<String>,
    pub color: Option<String>,
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
        /// Rendered message start column.
        from_x: usize,
        /// Rendered message end column.
        to_x: usize,
        /// Solid or dashed line.
        line_style: LineStyle,
        /// Arrowhead shape.
        arrow_head: ArrowHead,
        /// Label text.
        text: String,
        /// Autonumber prefix if any.
        number: Option<usize>,
    },
    /// A note box positioned relative to participant(s).
    Note {
        /// Y row of the note top border.
        y: usize,
        /// Note placement mode.
        placement: NotePlacement,
        /// Participant indices (1 for left/right/over-single, 2 for spanning).
        participant_indices: Vec<usize>,
        /// Note text.
        text: String,
    },
}

/// An activation rectangle on a participant's lifeline.
#[derive(Debug, Clone)]
pub struct ActivationRect {
    /// Participant index.
    pub participant_idx: usize,
    /// Y row where the activation starts.
    pub y_start: usize,
    /// Y row where the activation ends (inclusive).
    pub y_end: usize,
    /// Nesting depth (0-based). Deeper activations render slightly offset.
    pub depth: usize,
}

/// A divider line inside a block region.
#[derive(Debug, Clone)]
pub struct BlockDividerLayout {
    pub y: usize,
    pub kind: BlockDividerKind,
    pub label: String,
}

/// A labeled interaction block region.
#[derive(Debug, Clone)]
pub struct BlockLayout {
    pub top_y: usize,
    pub bottom_y: usize,
    pub left_x: usize,
    pub right_x: usize,
    pub depth: usize,
    pub kind: BlockKind,
    pub label: String,
    pub dividers: Vec<BlockDividerLayout>,
}

/// Complete sequence diagram layout.
#[derive(Debug, Clone)]
pub struct SequenceLayout {
    /// Optional centered diagram title.
    pub title: Option<TitleLayout>,
    /// Participant positions.
    pub participants: Vec<ParticipantLayout>,
    /// Participant grouping regions.
    pub participant_boxes: Vec<ParticipantBoxLayout>,
    /// Positioned event rows.
    pub rows: Vec<RowLayout>,
    /// Interaction block regions.
    pub blocks: Vec<BlockLayout>,
    /// Activation rectangles to render on lifelines.
    pub activations: Vec<ActivationRect>,
    /// Total canvas width.
    pub width: usize,
    /// Total canvas height.
    pub height: usize,
}

/// Compute layout for a sequence model.
pub fn layout(model: &Sequence) -> SequenceLayout {
    if model.participants.is_empty() {
        return SequenceLayout {
            title: None,
            participants: Vec::new(),
            participant_boxes: Vec::new(),
            rows: Vec::new(),
            blocks: Vec::new(),
            activations: Vec::new(),
            width: 0,
            height: 0,
        };
    }

    let title = model.title.as_ref().map(|text| TitleLayout {
        text: text.clone(),
        y: 0,
    });
    let title_offset = title
        .as_ref()
        .map(|_| TITLE_HEIGHT + TITLE_GAP)
        .unwrap_or(0);
    let participant_gap = compute_participant_gap(model);
    let left_margin = compute_left_note_margin(model);
    let participant_box_y = title_offset;
    let participant_header_y = participant_box_y
        + if model.participant_boxes.is_empty() {
            0
        } else {
            PARTICIPANT_BOX_HEADER_OFFSET
        };
    let mut participants = layout_participants(
        &model.participants,
        participant_gap,
        left_margin,
        participant_header_y,
    );

    let mut rows = Vec::new();
    let mut blocks = Vec::new();
    let mut cursor_y = participant_header_y + HEADER_HEIGHT + EVENT_GAP;

    // Activation tracking: stack of (y_start, depth) per participant
    let num_participants = model.participants.len();
    let mut activation_stacks: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_participants];
    let mut activation_depth: Vec<usize> = vec![0; num_participants];
    let mut activations: Vec<ActivationRect> = Vec::new();
    let mut open_blocks: Vec<OpenBlock> = Vec::new();
    let mut pending_create: Option<usize> = None;
    // Track the Y of the last message row so activation start/end aligns with
    // the triggering message rather than the cursor after it.
    let mut last_message_y = cursor_y;

    for event in &model.events {
        match event {
            SequenceEvent::CreateParticipant { participant } => {
                pending_create = Some(*participant);
            }
            SequenceEvent::Message {
                from,
                to,
                line_style,
                arrow_head,
                text,
                number,
            } => {
                let created_participant = pending_create.take();
                if let Some(participant_idx) = created_participant {
                    let header_y = cursor_y.saturating_sub(1);
                    let participant = &mut participants[participant_idx];
                    participant.box_y = header_y;
                    participant.lifeline_start_y = header_y + HEADER_HEIGHT;
                    update_open_block_extents(
                        &mut open_blocks,
                        participant.box_x,
                        participant.box_x + participant.box_width.saturating_sub(1),
                    );
                }

                let is_self = from == to;
                last_message_y = cursor_y;
                let from_x = participants[*from].center_x;
                let to_x = message_target_x(&participants, *from, *to, created_participant);
                if is_self {
                    let row = RowLayout::Message {
                        y: cursor_y,
                        from_idx: *from,
                        to_idx: *to,
                        from_x,
                        to_x,
                        line_style: *line_style,
                        arrow_head: *arrow_head,
                        text: text.clone(),
                        number: *number,
                    };
                    let (left, right) = row_extent(&row, &participants);
                    update_open_block_extents(&mut open_blocks, left, right);
                    rows.push(row);
                    cursor_y += SELF_MSG_HEIGHT + EVENT_GAP;
                } else {
                    let row = RowLayout::Message {
                        y: cursor_y,
                        from_idx: *from,
                        to_idx: *to,
                        from_x,
                        to_x,
                        line_style: *line_style,
                        arrow_head: *arrow_head,
                        text: text.clone(),
                        number: *number,
                    };
                    let (left, right) = row_extent(&row, &participants);
                    update_open_block_extents(&mut open_blocks, left, right);
                    rows.push(row);
                    cursor_y += 1 + EVENT_GAP;
                    if created_participant.is_some() {
                        cursor_y += 1;
                    }
                }
            }
            SequenceEvent::Note {
                placement,
                participants: indices,
                text,
            } => {
                let row = RowLayout::Note {
                    y: cursor_y,
                    placement: *placement,
                    participant_indices: indices.clone(),
                    text: text.clone(),
                };
                let (left, right) = row_extent(&row, &participants);
                update_open_block_extents(&mut open_blocks, left, right);
                rows.push(row);
                cursor_y += 3 + EVENT_GAP;
            }
            SequenceEvent::ActivateStart { participant } => {
                let depth = activation_depth[*participant];
                activation_stacks[*participant].push((last_message_y, depth));
                activation_depth[*participant] += 1;
            }
            SequenceEvent::ActivateEnd { participant } => {
                if let Some((y_start, depth)) = activation_stacks[*participant].pop() {
                    // End at the last message row (inclusive).
                    let y_end = last_message_y.max(y_start);
                    activations.push(ActivationRect {
                        participant_idx: *participant,
                        y_start,
                        y_end,
                        depth,
                    });
                    activation_depth[*participant] =
                        activation_depth[*participant].saturating_sub(1);
                }
            }
            SequenceEvent::DestroyParticipant { participant } => {
                participants[*participant].lifeline_end_y = last_message_y;
                participants[*participant].destroy_y = Some(last_message_y);
            }
            SequenceEvent::BlockStart { kind, label } => {
                open_blocks.push(OpenBlock {
                    top_y: cursor_y,
                    depth: open_blocks.len(),
                    kind: *kind,
                    label: label.clone(),
                    dividers: Vec::new(),
                    min_x: None,
                    max_x: None,
                });
                cursor_y += 1;
            }
            SequenceEvent::BlockDivider { kind, label } => {
                if let Some(block) = open_blocks.last_mut() {
                    block.dividers.push(BlockDividerLayout {
                        y: cursor_y,
                        kind: *kind,
                        label: label.clone(),
                    });
                }
                cursor_y += 1;
            }
            SequenceEvent::BlockEnd => {
                if let Some(block) = open_blocks.pop() {
                    let finalized = finalize_block_layout(block, cursor_y, &participants);
                    update_open_block_extents(
                        &mut open_blocks,
                        finalized.left_x,
                        finalized.right_x,
                    );
                    blocks.push(finalized);
                }
                cursor_y += 1 + EVENT_GAP;
            }
        }
    }

    // Close any unclosed activations at the bottom of the diagram
    for (participant_index, stack) in activation_stacks.iter_mut().enumerate() {
        while let Some((y_start, depth)) = stack.pop() {
            let y_end = cursor_y.saturating_sub(1).max(y_start);
            activations.push(ActivationRect {
                participant_idx: participant_index,
                y_start,
                y_end,
                depth,
            });
        }
    }

    blocks.sort_by_key(|block| block.depth);

    let diagram_bottom = cursor_y
        + if model.participant_boxes.is_empty() {
            0
        } else {
            PARTICIPANT_BOX_PADDING_BOTTOM
        };
    for participant in &mut participants {
        if participant.destroy_y.is_none() {
            participant.lifeline_end_y = if model.participant_boxes.is_empty() {
                diagram_bottom
            } else {
                diagram_bottom.saturating_sub(1)
            };
        }
    }
    let participant_boxes = layout_participant_boxes(
        &model.participant_boxes,
        &participants,
        participant_box_y,
        diagram_bottom.saturating_sub(1),
    );

    // Compute total dimensions
    let max_participant_right = participants
        .iter()
        .map(|p| p.box_x + p.box_width)
        .max()
        .unwrap_or(0);
    let max_participant_box_right = participant_boxes
        .iter()
        .map(|box_layout| box_layout.right_x)
        .max()
        .unwrap_or(0);

    let max_row_right = rows
        .iter()
        .map(|row| row_extent(row, &participants).1)
        .max()
        .unwrap_or(0);

    let max_block_right = blocks.iter().map(|block| block.right_x).max().unwrap_or(0);

    let title_width = title
        .as_ref()
        .map(|title| cell_advance_width(&title.text))
        .unwrap_or(0);
    let width = max_participant_right
        .max(max_participant_box_right)
        .max(max_row_right)
        .max(max_block_right)
        .max(title_width)
        + 2;
    let height = diagram_bottom.max(cursor_y);

    SequenceLayout {
        title,
        participants,
        participant_boxes,
        rows,
        blocks,
        activations,
        width,
        height,
    }
}

#[derive(Debug, Clone)]
struct OpenBlock {
    top_y: usize,
    depth: usize,
    kind: BlockKind,
    label: String,
    dividers: Vec<BlockDividerLayout>,
    min_x: Option<usize>,
    max_x: Option<usize>,
}

fn update_open_block_extents(open_blocks: &mut [OpenBlock], left: usize, right: usize) {
    for block in open_blocks {
        block.min_x = Some(block.min_x.map_or(left, |current| current.min(left)));
        block.max_x = Some(block.max_x.map_or(right, |current| current.max(right)));
    }
}

fn finalize_block_layout(
    block: OpenBlock,
    bottom_y: usize,
    participants: &[ParticipantLayout],
) -> BlockLayout {
    let fallback_center = participants.first().map(|p| p.center_x).unwrap_or(1);
    let raw_left = block.min_x.unwrap_or(fallback_center.saturating_sub(2));
    let raw_right = block.max_x.unwrap_or(fallback_center + 2);

    let inset = block.depth * 2;
    let left_x = raw_left.saturating_sub(2).saturating_add(inset);
    let mut right_x = raw_right.saturating_add(2).saturating_sub(inset);

    let min_width = block
        .dividers
        .iter()
        .map(|divider| block_label_len(divider.kind.keyword(), &divider.label))
        .fold(
            block_label_len(block.kind.keyword(), &block.label),
            usize::max,
        )
        .max(6);

    if right_x < left_x + min_width.saturating_sub(1) {
        right_x = left_x + min_width.saturating_sub(1);
    }

    BlockLayout {
        top_y: block.top_y,
        bottom_y,
        left_x,
        right_x,
        depth: block.depth,
        kind: block.kind,
        label: block.label,
        dividers: block.dividers,
    }
}

fn message_target_x(
    participants: &[ParticipantLayout],
    from_idx: usize,
    to_idx: usize,
    created_participant: Option<usize>,
) -> usize {
    let target = &participants[to_idx];
    if created_participant == Some(to_idx) && from_idx != to_idx {
        if target.center_x > participants[from_idx].center_x {
            target.box_x
        } else {
            target.box_x + target.box_width.saturating_sub(1)
        }
    } else {
        target.center_x
    }
}

fn block_label_len(keyword: &str, label: &str) -> usize {
    let badge_len = cell_advance_width(keyword) + 2;
    let text_len = if label.is_empty() {
        badge_len
    } else {
        badge_len + 1 + cell_advance_width(label)
    };
    text_len + 3
}

fn row_extent(row: &RowLayout, participants: &[ParticipantLayout]) -> (usize, usize) {
    match row {
        RowLayout::Message {
            from_idx,
            to_idx,
            from_x,
            text,
            number,
            ..
        } if from_idx == to_idx => {
            let right = *from_x + SELF_MSG_WIDTH + 2 + format_label_len(text, number);
            (*from_x, right)
        }
        RowLayout::Message {
            from_x,
            to_x,
            text,
            number,
            ..
        } => {
            let left = (*from_x).min(*to_x);
            let right = (*from_x)
                .max(*to_x)
                .max(left + 1 + format_label_len(text, number));
            (left, right)
        }
        RowLayout::Note {
            placement,
            participant_indices,
            text,
            ..
        } => {
            let box_width = cell_advance_width(text) + 4;
            match placement {
                NotePlacement::LeftOf => {
                    let center_x = participants[participant_indices[0]].center_x;
                    let left = center_x.saturating_sub(box_width + 1);
                    (left, left + box_width.saturating_sub(1))
                }
                NotePlacement::RightOf => {
                    let center_x = participants[participant_indices[0]].center_x;
                    let left = center_x + 2;
                    (left, left + box_width.saturating_sub(1))
                }
                NotePlacement::Over if participant_indices.len() == 2 => {
                    let cx1 = participants[participant_indices[0]].center_x;
                    let cx2 = participants[participant_indices[1]].center_x;
                    let left = cx1.min(cx2);
                    let right = cx1.max(cx2);
                    let span_width = box_width.max(right - left + 4);
                    let box_left = ((left + right) / 2).saturating_sub(span_width / 2);
                    (box_left, box_left + span_width.saturating_sub(1))
                }
                NotePlacement::Over => {
                    let center_x = participants[participant_indices[0]].center_x;
                    let left = center_x.saturating_sub(box_width / 2);
                    (left, left + box_width.saturating_sub(1))
                }
            }
        }
    }
}

fn format_label_len(text: &str, number: &Option<usize>) -> usize {
    match number {
        Some(n) => {
            if text.is_empty() {
                cell_advance_width(&format!("{n}."))
            } else {
                cell_advance_width(&format!("{n}. {text}"))
            }
        }
        None => cell_advance_width(text),
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
                let prefix_len = number
                    .map(|n| cell_advance_width(&format!("{n}. ")))
                    .unwrap_or(0);
                Some(cell_advance_width(text) + prefix_len)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);

    (max_label_len + LABEL_PADDING).max(MIN_PARTICIPANT_GAP)
}

/// Centre-to-centre distance between adjacent participants: the uniform
/// message gap, widened so the two boxes keep at least one free cell between
/// them.
fn participant_center_distance(gap: usize, left_box: usize, right_box: usize) -> usize {
    gap.max((left_box + right_box).div_ceil(2) + 1)
}

/// Compute extra left margin needed for left-of notes.
///
/// A left-of note on the first participant needs space to the left of the
/// lifeline. Without this, the note box would overflow into negative x
/// (clamped to 0 by saturating_sub), overlapping the diagram.
fn compute_left_note_margin(model: &Sequence) -> usize {
    // Tentative center_x for each participant with default margin of 1
    let mut centers = Vec::with_capacity(model.participants.len());
    let mut x = 1usize;
    for (i, p) in model.participants.iter().enumerate() {
        let box_width = cell_advance_width(&p.label) + 4;
        centers.push(x + box_width / 2);
        if i < model.participants.len() - 1 {
            let next_bw = cell_advance_width(&model.participants[i + 1].label) + 4;
            let distance = participant_center_distance(MIN_PARTICIPANT_GAP, box_width, next_bw);
            x = (centers[i] + distance).saturating_sub(next_bw / 2);
        }
    }

    let mut max_overhang = 0usize;
    for event in &model.events {
        if let SequenceEvent::Note {
            placement: NotePlacement::LeftOf,
            participants: indices,
            text,
        } = event
        {
            let box_width = cell_advance_width(text) + 4;
            let center_x = centers[indices[0]];
            // The renderer places the box at center_x - (box_width + 1)
            let needed = box_width + 1;
            if needed > center_x {
                max_overhang = max_overhang.max(needed - center_x);
            }
        }
    }
    max_overhang
}

/// Compute horizontal positions for all participants.
fn layout_participants(
    participants: &[Participant],
    gap: usize,
    left_margin: usize,
    box_y: usize,
) -> Vec<ParticipantLayout> {
    let mut result = Vec::with_capacity(participants.len());
    let mut x = 1 + left_margin; // left margin + space for left-of notes

    for (i, p) in participants.iter().enumerate() {
        let box_width = cell_advance_width(&p.label) + 4; // | + space + label + space + |
        let center_x = x + box_width / 2;

        result.push(ParticipantLayout {
            center_x,
            box_y,
            box_x: x,
            box_width,
            lifeline_start_y: box_y + HEADER_HEIGHT,
            lifeline_end_y: box_y + HEADER_HEIGHT,
            destroy_y: None,
            label: p.label.clone(),
            kind: p.kind.clone(),
        });

        if i < participants.len() - 1 {
            // Next participant starts at its centre minus half its box.
            let next_box_width = cell_advance_width(&participants[i + 1].label) + 4;
            let distance = participant_center_distance(gap, box_width, next_box_width);
            x = (center_x + distance).saturating_sub(next_box_width / 2);
        }
    }

    result
}

fn layout_participant_boxes(
    participant_boxes: &[ParticipantBox],
    participants: &[ParticipantLayout],
    top_y: usize,
    bottom_y: usize,
) -> Vec<ParticipantBoxLayout> {
    participant_boxes
        .iter()
        .filter_map(|participant_box| {
            let first_idx = *participant_box.participants.first()?;
            let last_idx = *participant_box.participants.last()?;
            let first = participants.get(first_idx)?;
            let last = participants.get(last_idx)?;

            let mut left_x = first.box_x.saturating_sub(PARTICIPANT_BOX_PADDING_X);
            let mut right_x = last.box_x + last.box_width - 1 + PARTICIPANT_BOX_PADDING_X;
            let min_width = participant_box
                .label
                .as_ref()
                .map(|label| cell_advance_width(label) + 4)
                .unwrap_or(0)
                .max(6);
            let current_width = right_x.saturating_sub(left_x) + 1;
            if current_width < min_width {
                let extra = min_width - current_width;
                left_x = left_x.saturating_sub(extra / 2);
                right_x += extra - extra / 2;
            }

            Some(ParticipantBoxLayout {
                top_y,
                bottom_y,
                left_x,
                right_x,
                label: participant_box.label.clone(),
                color: participant_box.color.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::sequence::model::AutonumberState;

    #[test]
    fn layout_empty_model() {
        let layout = layout(&Sequence {
            title: None,
            participants: Vec::new(),
            participant_boxes: Vec::new(),
            events: Vec::new(),
            autonumber: AutonumberState::default(),
        });
        assert!(layout.participants.is_empty());
        assert!(layout.rows.is_empty());
        assert_eq!(layout.width, 0);
        assert_eq!(layout.height, 0);
    }

    #[test]
    fn layout_reserves_space_for_title() {
        let layout = layout(&Sequence {
            title: Some("Authentication Flow".into()),
            participants: vec![Participant {
                id: "A".into(),
                label: "A".into(),
                kind: ParticipantKind::Participant,
            }],
            participant_boxes: Vec::new(),
            events: Vec::new(),
            autonumber: AutonumberState::default(),
        });

        let title = layout.title.expect("title should be present");
        assert_eq!(title.y, 0);
        assert!(layout.participants[0].box_y >= TITLE_HEIGHT + TITLE_GAP);
    }
}
