//! Sequence diagram text renderer.
//!
//! Renders a `SequenceLayout` onto a shared `Canvas` using box-drawing
//! characters from `CharSet`. Supports both Unicode and ASCII output.

use crate::graph::Stroke;
use crate::render::text::canvas::Canvas;
use crate::render::text::chars::CharSet;
use crate::render::text::connections::Connections;
use crate::timeline::sequence::layout::{
    ActivationRect, BlockLayout, ParticipantBoxLayout, ParticipantLayout, RowLayout,
    SELF_MSG_WIDTH, SequenceLayout, TitleLayout,
};
use crate::timeline::sequence::model::{
    ArrowHead, BlockDividerKind, BlockKind, LineStyle, NotePlacement,
};

/// Render a sequence layout to a string.
pub fn render(layout: &SequenceLayout, charset: &CharSet) -> String {
    if layout.participants.is_empty() {
        return String::new();
    }

    let mut canvas = Canvas::new(layout.width, layout.height);

    if let Some(title) = &layout.title {
        draw_title(&mut canvas, title);
    }

    for participant_box in &layout.participant_boxes {
        draw_participant_group_box(&mut canvas, participant_box, charset);
    }

    for p in &layout.participants {
        draw_participant_header(&mut canvas, p, charset);
    }

    for p in &layout.participants {
        draw_lifeline(
            &mut canvas,
            p.center_x,
            p.lifeline_start_y,
            p.lifeline_end_y,
            charset,
        );
    }

    for block in &layout.blocks {
        draw_block(&mut canvas, block, charset);
    }

    // Draw activation boxes on lifelines
    for activation in &layout.activations {
        draw_activation(&mut canvas, activation, &layout.participants, charset);
    }

    for row in &layout.rows {
        match row {
            RowLayout::Message {
                y,
                from_idx,
                to_idx,
                from_x,
                to_x,
                line_style,
                arrow_head,
                text,
                number,
            } => {
                if from_idx == to_idx {
                    draw_self_message(
                        &mut canvas,
                        *from_x,
                        *y,
                        text,
                        number,
                        line_style,
                        arrow_head,
                        charset,
                    );
                } else {
                    draw_message(
                        &mut canvas,
                        *from_x,
                        *to_x,
                        *y,
                        text,
                        number,
                        line_style,
                        arrow_head,
                        charset,
                    );
                }
            }
            RowLayout::Note {
                y,
                placement,
                participant_indices,
                text,
            } => {
                draw_note(
                    &mut canvas,
                    &layout.participants,
                    placement,
                    participant_indices,
                    *y,
                    text,
                    charset,
                );
            }
        }
    }

    for participant in &layout.participants {
        if let Some(y) = participant.destroy_y {
            draw_destroy_marker(&mut canvas, participant.center_x, y);
        }
    }

    canvas.to_string()
}

fn draw_title(canvas: &mut Canvas, title: &TitleLayout) {
    let x = canvas.width().saturating_sub(title.text.len()) / 2;
    canvas.write_str(x, title.y, &title.text);
}

fn draw_participant_header(canvas: &mut Canvas, p: &ParticipantLayout, cs: &CharSet) {
    let x = p.box_x;
    let w = p.box_width;
    let y = p.box_y;

    canvas.set(x, y, cs.corner_tl);
    for i in 1..w - 1 {
        canvas.set(x + i, y, cs.horizontal);
    }
    canvas.set(x + w - 1, y, cs.corner_tr);

    canvas.set(x, y + 1, cs.vertical);
    canvas.set(x + 1, y + 1, ' ');
    canvas.write_str(x + 2, y + 1, &p.label);
    canvas.set(x + 2 + p.label.len(), y + 1, ' ');
    canvas.set(x + w - 1, y + 1, cs.vertical);

    canvas.set(x, y + 2, cs.corner_bl);
    for i in 1..w - 1 {
        canvas.set(x + i, y + 2, cs.horizontal);
    }
    canvas.set(x + w - 1, y + 2, cs.corner_br);

    canvas.set(p.center_x, y + 2, cs.tee_down);
}

fn draw_participant_group_box(
    canvas: &mut Canvas,
    participant_box: &ParticipantBoxLayout,
    cs: &CharSet,
) {
    let left = participant_box.left_x;
    let right = participant_box.right_x;
    let top = participant_box.top_y;
    let bottom = participant_box.bottom_y;

    canvas.set(left, top, cs.corner_tl);
    canvas.set(right, top, cs.corner_tr);
    canvas.set(left, bottom, cs.corner_bl);
    canvas.set(right, bottom, cs.corner_br);

    for x in (left + 1)..right {
        canvas.set(x, top, cs.horizontal);
        canvas.set(x, bottom, cs.horizontal);
    }

    for y in (top + 1)..bottom {
        canvas.set(left, y, cs.vertical);
        canvas.set(right, y, cs.vertical);
    }

    if let Some(label) = &participant_box.label {
        for x in (left + 1)..right {
            canvas.set(x, top + 1, ' ');
        }
        let available_width = right.saturating_sub(left + 1);
        let label_x = left + 1 + available_width.saturating_sub(label.len()) / 2;
        canvas.write_str(label_x, top + 1, label);
    }
}

fn draw_lifeline(canvas: &mut Canvas, x: usize, y_start: usize, y_end: usize, cs: &CharSet) {
    for y in y_start..y_end {
        canvas.set_with_connection(
            x,
            y,
            Connections {
                up: true,
                down: true,
                left: false,
                right: false,
            },
            cs,
            Stroke::Solid,
        );
    }
}

fn draw_destroy_marker(canvas: &mut Canvas, x: usize, y: usize) {
    if x > 0 {
        canvas.set(x - 1, y, 'X');
    }
    canvas.set(x, y, 'X');
    canvas.set(x + 1, y, 'X');
}

fn draw_block(canvas: &mut Canvas, block: &BlockLayout, cs: &CharSet) {
    let left = block.left_x;
    let right = block.right_x;
    let top = block.top_y;
    let bottom = block.bottom_y;

    set_connection(
        canvas,
        left,
        top,
        Connections {
            up: false,
            down: true,
            left: false,
            right: true,
        },
        Stroke::Solid,
        cs,
    );
    set_connection(
        canvas,
        right,
        top,
        Connections {
            up: false,
            down: true,
            left: true,
            right: false,
        },
        Stroke::Solid,
        cs,
    );
    set_connection(
        canvas,
        left,
        bottom,
        Connections {
            up: true,
            down: false,
            left: false,
            right: true,
        },
        Stroke::Solid,
        cs,
    );
    set_connection(
        canvas,
        right,
        bottom,
        Connections {
            up: true,
            down: false,
            left: true,
            right: false,
        },
        Stroke::Solid,
        cs,
    );

    for x in (left + 1)..right {
        set_connection(
            canvas,
            x,
            top,
            Connections {
                up: false,
                down: false,
                left: true,
                right: true,
            },
            Stroke::Solid,
            cs,
        );
        set_connection(
            canvas,
            x,
            bottom,
            Connections {
                up: false,
                down: false,
                left: true,
                right: true,
            },
            Stroke::Solid,
            cs,
        );
    }

    for y in (top + 1)..bottom {
        set_connection(
            canvas,
            left,
            y,
            Connections {
                up: true,
                down: true,
                left: false,
                right: false,
            },
            Stroke::Solid,
            cs,
        );
        set_connection(
            canvas,
            right,
            y,
            Connections {
                up: true,
                down: true,
                left: false,
                right: false,
            },
            Stroke::Solid,
            cs,
        );
    }

    let label = format_block_label(block.kind, &block.label);
    canvas.write_str(left + 2, top, &label);

    for divider in &block.dividers {
        draw_block_divider(
            canvas,
            left,
            right,
            divider.y,
            divider.kind,
            &divider.label,
            cs,
        );
    }
}

fn draw_block_divider(
    canvas: &mut Canvas,
    left: usize,
    right: usize,
    y: usize,
    kind: BlockDividerKind,
    label: &str,
    cs: &CharSet,
) {
    set_connection(
        canvas,
        left,
        y,
        Connections {
            up: true,
            down: true,
            left: false,
            right: true,
        },
        Stroke::Dotted,
        cs,
    );
    set_connection(
        canvas,
        right,
        y,
        Connections {
            up: true,
            down: true,
            left: true,
            right: false,
        },
        Stroke::Dotted,
        cs,
    );

    for x in (left + 1)..right {
        set_connection(
            canvas,
            x,
            y,
            Connections {
                up: false,
                down: false,
                left: true,
                right: true,
            },
            Stroke::Dotted,
            cs,
        );
    }

    let label_text = format_divider_label(kind, label);
    canvas.write_str(left + 2, y, &label_text);
}

fn set_connection(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    connections: Connections,
    stroke: Stroke,
    cs: &CharSet,
) {
    let _ = canvas.set_with_connection(x, y, connections, cs, stroke);
}

/// Resolve the arrowhead character for the given direction and head type.
fn arrow_char(arrow_head: &ArrowHead, left_to_right: bool) -> Option<char> {
    match arrow_head {
        ArrowHead::None => None,
        ArrowHead::Filled => {
            if left_to_right {
                Some('>')
            } else {
                Some('<')
            }
        }
        ArrowHead::Cross => Some('x'),
        ArrowHead::Async => {
            if left_to_right {
                Some(')')
            } else {
                Some('(')
            }
        }
    }
}

/// Resolve the line character for the given line style.
fn line_char(line_style: &LineStyle, cs: &CharSet) -> char {
    match line_style {
        LineStyle::Solid => cs.horizontal,
        LineStyle::Dashed => cs.dotted_horizontal,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_message(
    canvas: &mut Canvas,
    from_x: usize,
    to_x: usize,
    y: usize,
    text: &str,
    number: &Option<usize>,
    line_style: &LineStyle,
    arrow_head: &ArrowHead,
    cs: &CharSet,
) {
    let left_to_right = to_x > from_x;
    let (start_x, end_x) = if left_to_right {
        (from_x + 1, to_x)
    } else {
        (to_x + 1, from_x)
    };

    let lc = line_char(line_style, cs);
    let ac = arrow_char(arrow_head, left_to_right);

    for x in start_x..end_x {
        canvas.set(x, y, lc);
    }

    if let Some(ac) = ac {
        if left_to_right {
            canvas.set(end_x - 1, y, ac);
        } else {
            canvas.set(start_x, y, ac);
        }
    }

    let label = format_label(text, number);
    if !label.is_empty() {
        let offset = if !left_to_right && ac.is_some() { 2 } else { 1 };
        let label_x = start_x + offset;
        canvas.write_str(label_x, y, &label);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_self_message(
    canvas: &mut Canvas,
    center_x: usize,
    y: usize,
    text: &str,
    number: &Option<usize>,
    _line_style: &LineStyle,
    arrow_head: &ArrowHead,
    cs: &CharSet,
) {
    let arm_end = center_x + SELF_MSG_WIDTH;

    canvas.set(center_x, y, cs.tee_right);
    for x in (center_x + 1)..arm_end {
        canvas.set(x, y, cs.horizontal);
    }
    canvas.set(arm_end, y, cs.corner_tr);

    let label = format_label(text, number);
    if !label.is_empty() {
        canvas.write_str(arm_end + 2, y, &label);
    }

    canvas.set(arm_end, y + 1, cs.vertical);

    canvas.set(
        center_x,
        y + 2,
        arrow_char(arrow_head, false).unwrap_or(cs.tee_right),
    );
    for x in (center_x + 1)..arm_end {
        canvas.set(x, y + 2, cs.horizontal);
    }
    canvas.set(arm_end, y + 2, cs.corner_br);
}

fn draw_note(
    canvas: &mut Canvas,
    participants: &[ParticipantLayout],
    placement: &NotePlacement,
    participant_indices: &[usize],
    y: usize,
    text: &str,
    cs: &CharSet,
) {
    let min_box_width = text.len() + 4;

    let (box_x, box_width) = match placement {
        NotePlacement::LeftOf => {
            let center_x = participants[participant_indices[0]].center_x;
            let bx = center_x.saturating_sub(min_box_width + 1);
            (bx, min_box_width)
        }
        NotePlacement::RightOf => {
            let center_x = participants[participant_indices[0]].center_x;
            (center_x + 2, min_box_width)
        }
        NotePlacement::Over if participant_indices.len() == 2 => {
            let cx1 = participants[participant_indices[0]].center_x;
            let cx2 = participants[participant_indices[1]].center_x;
            let left = cx1.min(cx2);
            let right = cx1.max(cx2);
            let span_width = min_box_width.max(right - left + 4);
            let mid = (left + right) / 2;
            let bx = mid.saturating_sub(span_width / 2);
            (bx, span_width)
        }
        NotePlacement::Over => {
            let center_x = participants[participant_indices[0]].center_x;
            let bx = center_x.saturating_sub(min_box_width / 2);
            (bx, min_box_width)
        }
    };

    canvas.set(box_x, y, cs.corner_tl);
    for i in 1..box_width - 1 {
        canvas.set(box_x + i, y, cs.horizontal);
    }
    canvas.set(box_x + box_width - 1, y, cs.corner_tr);

    canvas.set(box_x, y + 1, cs.vertical);
    // Fill entire interior with spaces first (overwrites lifelines)
    for i in 1..box_width - 1 {
        canvas.set(box_x + i, y + 1, ' ');
    }
    let text_offset = (box_width - 2 - text.len()) / 2;
    canvas.write_str(box_x + 1 + text_offset, y + 1, text);
    canvas.set(box_x + box_width - 1, y + 1, cs.vertical);

    canvas.set(box_x, y + 2, cs.corner_bl);
    for i in 1..box_width - 1 {
        canvas.set(box_x + i, y + 2, cs.horizontal);
    }
    canvas.set(box_x + box_width - 1, y + 2, cs.corner_br);
}

fn format_label(text: &str, number: &Option<usize>) -> String {
    match number {
        Some(n) => {
            if text.is_empty() {
                format!("{n}.")
            } else {
                format!("{n}. {text}")
            }
        }
        None => text.to_string(),
    }
}

fn format_block_label(kind: BlockKind, label: &str) -> String {
    format_badge(kind.keyword(), label)
}

fn format_divider_label(kind: BlockDividerKind, label: &str) -> String {
    format_badge(kind.keyword(), label)
}

fn format_badge(keyword: &str, label: &str) -> String {
    if label.is_empty() {
        format!("[{keyword}]")
    } else {
        format!("[{keyword}] {label}")
    }
}

/// Draw an activation bar on a participant's lifeline.
///
/// Replaces the thin `│` lifeline with `║` during the active region.
/// Nested activations are drawn one column to the right of the lifeline.
fn draw_activation(
    canvas: &mut Canvas,
    activation: &ActivationRect,
    participants: &[ParticipantLayout],
    cs: &CharSet,
) {
    let center_x = participants[activation.participant_idx].center_x;
    // depth 0 draws on the lifeline itself; deeper levels offset right
    let x = center_x + activation.depth;

    let activation_char = if cs.is_ascii() { '#' } else { '║' };
    for y in activation.y_start..=activation.y_end {
        canvas.set(x, y, activation_char);
    }
}
