//! Sequence diagram text renderer.
//!
//! Renders a `SequenceLayout` onto a shared `Canvas` using box-drawing
//! characters from `CharSet`. Supports both Unicode and ASCII output.

use crate::render::text::canvas::Canvas;
use crate::render::text::chars::CharSet;
use crate::timeline::sequence::layout::{
    ParticipantLayout, RowLayout, SELF_MSG_WIDTH, SequenceLayout,
};
use crate::timeline::sequence::model::MessageStyle;

/// Render a sequence layout to a string.
pub fn render(layout: &SequenceLayout, charset: &CharSet) -> String {
    if layout.participants.is_empty() {
        return String::new();
    }

    let mut canvas = Canvas::new(layout.width, layout.height);

    for p in &layout.participants {
        draw_participant_header(&mut canvas, p, charset);
    }

    let lifeline_start = 3;
    let lifeline_end = layout.height;
    for p in &layout.participants {
        for y in lifeline_start..lifeline_end {
            canvas.set(p.center_x, y, charset.vertical);
        }
    }

    for row in &layout.rows {
        match row {
            RowLayout::Message {
                y,
                from_idx,
                to_idx,
                style,
                text,
                number,
            } => {
                let from_x = layout.participants[*from_idx].center_x;
                let to_x = layout.participants[*to_idx].center_x;

                if from_idx == to_idx {
                    draw_self_message(&mut canvas, from_x, *y, text, number, style, charset);
                } else {
                    draw_message(&mut canvas, from_x, to_x, *y, text, number, style, charset);
                }
            }
            RowLayout::Note { y, over_idx, text } => {
                let center_x = layout.participants[*over_idx].center_x;
                draw_note(&mut canvas, center_x, *y, text, charset);
            }
        }
    }

    canvas.to_string()
}

fn draw_participant_header(canvas: &mut Canvas, p: &ParticipantLayout, cs: &CharSet) {
    let x = p.box_x;
    let w = p.box_width;

    canvas.set(x, 0, cs.corner_tl);
    for i in 1..w - 1 {
        canvas.set(x + i, 0, cs.horizontal);
    }
    canvas.set(x + w - 1, 0, cs.corner_tr);

    canvas.set(x, 1, cs.vertical);
    canvas.set(x + 1, 1, ' ');
    canvas.write_str(x + 2, 1, &p.label);
    canvas.set(x + 2 + p.label.len(), 1, ' ');
    canvas.set(x + w - 1, 1, cs.vertical);

    canvas.set(x, 2, cs.corner_bl);
    for i in 1..w - 1 {
        canvas.set(x + i, 2, cs.horizontal);
    }
    canvas.set(x + w - 1, 2, cs.corner_br);

    canvas.set(p.center_x, 2, cs.tee_down);
}

#[allow(clippy::too_many_arguments)]
fn draw_message(
    canvas: &mut Canvas,
    from_x: usize,
    to_x: usize,
    y: usize,
    text: &str,
    number: &Option<usize>,
    style: &MessageStyle,
    cs: &CharSet,
) {
    let left_to_right = to_x > from_x;
    let (start_x, end_x) = if left_to_right {
        (from_x + 1, to_x)
    } else {
        (to_x + 1, from_x)
    };

    let (line_char, arrow_char) = match style {
        MessageStyle::Solid => (cs.horizontal, if left_to_right { '>' } else { '<' }),
        MessageStyle::Dashed => (cs.dotted_horizontal, if left_to_right { '>' } else { '<' }),
    };

    for x in start_x..end_x {
        canvas.set(x, y, line_char);
    }

    if left_to_right {
        canvas.set(end_x - 1, y, arrow_char);
    } else {
        canvas.set(start_x, y, arrow_char);
    }

    let label = format_label(text, number);
    if !label.is_empty() {
        let label_x = start_x + 1;
        canvas.write_str(label_x, y, &label);
    }
}

fn draw_self_message(
    canvas: &mut Canvas,
    center_x: usize,
    y: usize,
    text: &str,
    number: &Option<usize>,
    _style: &MessageStyle,
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

    canvas.set(center_x, y + 2, '<');
    for x in (center_x + 1)..arm_end {
        canvas.set(x, y + 2, cs.horizontal);
    }
    canvas.set(arm_end, y + 2, cs.corner_br);
}

fn draw_note(canvas: &mut Canvas, center_x: usize, y: usize, text: &str, cs: &CharSet) {
    let box_width = text.len() + 4;
    let box_x = center_x.saturating_sub(box_width / 2);

    canvas.set(box_x, y, cs.corner_tl);
    for i in 1..box_width - 1 {
        canvas.set(box_x + i, y, cs.horizontal);
    }
    canvas.set(box_x + box_width - 1, y, cs.corner_tr);

    canvas.set(box_x, y + 1, cs.vertical);
    canvas.set(box_x + 1, y + 1, ' ');
    canvas.write_str(box_x + 2, y + 1, text);
    canvas.set(box_x + 2 + text.len(), y + 1, ' ');
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
