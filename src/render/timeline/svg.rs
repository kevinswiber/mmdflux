//! SVG rendering for sequence diagrams.
//!
//! Consumes an `SvgSequenceLayout` and emits SVG markup using the shared
//! `SvgWriter` utilities.

use std::collections::BTreeSet;

use super::svg_layout::{
    SvgActivation, SvgBlock, SvgBlockDivider, SvgDestroyMarker, SvgLifeline, SvgMessage, SvgNote,
    SvgParticipant, SvgParticipantBox, SvgRow, SvgSelfMessage, SvgSequenceLayout, SvgTitle,
};
use crate::render::svg::theme::{ResolvedSvgTheme, SvgRootStyle};
use crate::render::svg::{SvgWriter, escape_text, fmt_f64};
use crate::timeline::sequence::model::{ArrowHead, LineStyle, ParticipantKind};

const UNTHEMED_STROKE_COLOR: &str = "#333";
const UNTHEMED_FILL_COLOR: &str = "white";
const UNTHEMED_TEXT_COLOR: &str = "#333";
const UNTHEMED_NOTE_FILL: &str = "#ffffcc";
const UNTHEMED_ACTIVATION_FILL: &str = "#ddd";
const UNTHEMED_ACTOR_STROKE: &str = "#333";
const UNTHEMED_BLOCK_STROKE: &str = "#666";
const UNTHEMED_BLOCK_LABEL_BG: &str = "white";
const UNTHEMED_PARTICIPANT_BOX_STROKE: &str = "#8a8a8a";
const UNTHEMED_PARTICIPANT_BOX_LABEL_BG: &str = "white";
const BLOCK_STROKE_DASH: &str = "3,3";
const BLOCK_TAB_HEIGHT: f64 = 20.0;
const BLOCK_TAB_MIN_WIDTH: f64 = 42.0;
const BLOCK_TAB_PADDING_X: f64 = 8.0;
const BLOCK_TAB_CUT: f64 = 7.0;
const BLOCK_TAB_CHAR_WIDTH: f64 = 8.0;
const BLOCK_HEADER_TEXT_Y: f64 = 15.0;
const BLOCK_SECTION_TEXT_Y: f64 = 15.0;

#[derive(Debug, Clone)]
struct SequenceSvgPalette {
    root_style: SvgRootStyle,
    participant_fill: String,
    participant_stroke: String,
    participant_text: String,
    lifeline_stroke: String,
    marker_color: String,
    note_fill: String,
    activation_fill: String,
    actor_stroke: String,
    block_stroke: String,
    block_label_bg: String,
    participant_box_stroke: String,
    participant_box_label_bg: String,
    dynamic_css: bool,
}

impl SequenceSvgPalette {
    fn from_theme(theme: Option<&ResolvedSvgTheme>) -> Self {
        match theme {
            Some(theme) => Self {
                root_style: theme
                    .dynamic
                    .as_ref()
                    .map(|dynamic| dynamic.root_style.clone())
                    .unwrap_or_else(|| SvgRootStyle {
                        background_color: Some(theme.roles.background.clone()),
                        ..SvgRootStyle::default()
                    }),
                participant_fill: theme.roles.node_fill.clone(),
                participant_stroke: theme.roles.node_stroke.clone(),
                participant_text: theme.roles.text.clone(),
                lifeline_stroke: theme.roles.line.clone(),
                marker_color: theme.roles.arrow.clone(),
                note_fill: theme.roles.node_fill.clone(),
                activation_fill: theme.roles.node_fill.clone(),
                actor_stroke: theme.roles.node_stroke.clone(),
                block_stroke: theme.roles.node_stroke.clone(),
                block_label_bg: theme.roles.group_fill.clone(),
                participant_box_stroke: theme.roles.node_stroke.clone(),
                participant_box_label_bg: theme.roles.group_fill.clone(),
                dynamic_css: theme.dynamic.is_some(),
            },
            None => Self {
                root_style: SvgRootStyle::default(),
                participant_fill: UNTHEMED_FILL_COLOR.to_string(),
                participant_stroke: UNTHEMED_STROKE_COLOR.to_string(),
                participant_text: UNTHEMED_TEXT_COLOR.to_string(),
                lifeline_stroke: UNTHEMED_STROKE_COLOR.to_string(),
                marker_color: UNTHEMED_STROKE_COLOR.to_string(),
                note_fill: UNTHEMED_NOTE_FILL.to_string(),
                activation_fill: UNTHEMED_ACTIVATION_FILL.to_string(),
                actor_stroke: UNTHEMED_ACTOR_STROKE.to_string(),
                block_stroke: UNTHEMED_BLOCK_STROKE.to_string(),
                block_label_bg: UNTHEMED_BLOCK_LABEL_BG.to_string(),
                participant_box_stroke: UNTHEMED_PARTICIPANT_BOX_STROKE.to_string(),
                participant_box_label_bg: UNTHEMED_PARTICIPANT_BOX_LABEL_BG.to_string(),
                dynamic_css: false,
            },
        }
    }
}

fn dynamic_css_attrs(enabled: bool, role: &'static str, declarations: &[&'static str]) -> String {
    if !enabled || declarations.is_empty() {
        return String::new();
    }

    let mut style = String::new();
    for declaration in declarations {
        style.push_str(declaration);
    }

    format!(" data-svg-role=\"{role}\" style=\"{style}\"")
}

/// Render an SVG sequence layout to an SVG string.
pub fn render(layout: &SvgSequenceLayout, theme: Option<&ResolvedSvgTheme>) -> String {
    let mut writer = SvgWriter::new();
    let palette = SequenceSvgPalette::from_theme(theme);
    let used_marker_ids = collect_used_marker_ids(layout);

    writer.start_svg_with_root_style(
        layout.width,
        layout.height,
        &layout.font_family,
        layout.font_size,
        &palette.root_style,
    );

    render_defs(&mut writer, &palette, &used_marker_ids);

    if let Some(title) = &layout.title {
        writer.start_group("title");
        render_title(&mut writer, layout, title, &palette);
        writer.end_group();
    }

    if !layout.participant_boxes.is_empty() {
        writer.start_group("participant-boxes");
        for participant_box in &layout.participant_boxes {
            render_participant_group_box(&mut writer, participant_box, &palette);
        }
        writer.end_group();
    }

    // Lifelines (behind everything else)
    writer.start_group("lifelines");
    for lifeline in &layout.lifelines {
        render_lifeline(&mut writer, lifeline, &palette);
    }
    for marker in &layout.destroy_markers {
        render_destroy_marker(&mut writer, marker, &palette);
    }
    writer.end_group();

    if !layout.blocks.is_empty() {
        writer.start_group("blocks");
        for block in &layout.blocks {
            render_block(&mut writer, block, &palette);
        }
        writer.end_group();
    }

    // Activation bars (behind messages, on top of lifelines)
    if !layout.activations.is_empty() {
        writer.start_group("activations");
        for activation in &layout.activations {
            render_activation(&mut writer, activation, &palette);
        }
        writer.end_group();
    }

    // Participant boxes (on top of lifelines)
    writer.start_group("participants");
    for participant in &layout.participants {
        render_participant(&mut writer, participant, &palette);
    }
    writer.end_group();

    // Messages and notes
    writer.start_group("events");
    for row in &layout.rows {
        match row {
            SvgRow::Message(msg) => render_message(&mut writer, msg, &palette),
            SvgRow::SelfMessage(sm) => render_self_message(&mut writer, sm, &palette),
            SvgRow::Note(note) => render_note(&mut writer, note, &palette),
        }
    }
    writer.end_group();

    writer.end_svg();
    writer.finish()
}

fn render_title(
    writer: &mut SvgWriter,
    layout: &SvgSequenceLayout,
    title: &SvgTitle,
    palette: &SequenceSvgPalette,
) {
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{fill}\"{dynamic_attrs}>{title}</text>",
        x = fmt_f64(layout.width / 2.0),
        y = fmt_f64(title.y),
        fill = palette.participant_text,
        dynamic_attrs = dynamic_attrs,
        title = escape_text(&title.text),
    ));
}

fn render_participant_group_box(
    writer: &mut SvgWriter,
    participant_box: &SvgParticipantBox,
    palette: &SequenceSvgPalette,
) {
    let rect = &participant_box.rect;
    let fill = participant_box
        .color
        .as_deref()
        .filter(|color| !color.eq_ignore_ascii_case("transparent"));
    let stroke_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-participant-group-stroke",
        &["stroke:var(--_node-stroke);"],
    );

    match fill {
        Some(color) => writer.push_line(&format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\" fill-opacity=\"0.14\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
            x = fmt_f64(rect.x),
            y = fmt_f64(rect.y),
            w = fmt_f64(rect.width),
            h = fmt_f64(rect.height),
            fill = escape_text(color),
            stroke = palette.participant_box_stroke,
            dynamic_attrs = stroke_dynamic_attrs,
        )),
        None => writer.push_line(&format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
            x = fmt_f64(rect.x),
            y = fmt_f64(rect.y),
            w = fmt_f64(rect.width),
            h = fmt_f64(rect.height),
            stroke = palette.participant_box_stroke,
            dynamic_attrs = stroke_dynamic_attrs,
        )),
    }

    if let Some(label) = participant_box.label.as_deref() {
        let label_x = rect.x + rect.width / 2.0;
        let label_y = rect.y + 14.0;
        let label_bg_width = ((label.chars().count() as f64) * 8.0 + 24.0)
            .max(48.0)
            .min(rect.width - 8.0);
        let label_bg_dynamic_attrs = dynamic_css_attrs(
            palette.dynamic_css,
            "sequence-group-label-bg",
            &["fill:var(--_group-fill);"],
        );
        writer.push_line(&format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"20\" fill=\"{bg}\" stroke=\"none\"{dynamic_attrs} />",
            x = fmt_f64(label_x - label_bg_width / 2.0),
            y = fmt_f64(rect.y + 2.0),
            w = fmt_f64(label_bg_width),
            bg = palette.participant_box_label_bg,
            dynamic_attrs = label_bg_dynamic_attrs,
        ));
        let text_dynamic_attrs = dynamic_css_attrs(
            palette.dynamic_css,
            "sequence-text",
            &["fill:var(--_text);"],
        );
        writer.push_line(&format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
            x = fmt_f64(label_x),
            y = fmt_f64(label_y),
            fill = palette.participant_text,
            dynamic_attrs = text_dynamic_attrs,
            label = escape_text(label),
        ));
    }
}

// ---------------------------------------------------------------------------
// Marker definitions
// ---------------------------------------------------------------------------

fn render_defs(
    writer: &mut SvgWriter,
    palette: &SequenceSvgPalette,
    used_marker_ids: &BTreeSet<&'static str>,
) {
    writer.start_tag("<defs>");
    let fill_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-marker-fill",
        &["fill:var(--_arrow);"],
    );
    let stroke_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-marker-stroke",
        &["stroke:var(--_arrow);"],
    );

    if used_marker_ids.contains("seq-arrowhead") {
        writer.start_tag(
            "<marker id=\"seq-arrowhead\" viewBox=\"0 0 10 10\" refX=\"10\" refY=\"5\" \
             markerWidth=\"8\" markerHeight=\"8\" orient=\"auto-start-reverse\" \
             markerUnits=\"userSpaceOnUse\">",
        );
        writer.push_line(&format!(
            "<path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"{color}\"{dynamic_attrs} />",
            color = palette.marker_color,
            dynamic_attrs = fill_dynamic_attrs,
        ));
        writer.end_tag("</marker>");
    }

    if used_marker_ids.contains("seq-crosshead") {
        writer.start_tag(
            "<marker id=\"seq-crosshead\" viewBox=\"0 0 11 11\" refX=\"11\" refY=\"5.5\" \
             markerWidth=\"11\" markerHeight=\"11\" orient=\"auto-start-reverse\" \
             markerUnits=\"userSpaceOnUse\">",
        );
        writer.push_line(&format!(
            "<path d=\"M 1,1 l 9,9 M 10,1 l -9,9\" stroke=\"{color}\" stroke-width=\"2\"{dynamic_attrs} />",
            color = palette.marker_color,
            dynamic_attrs = stroke_dynamic_attrs,
        ));
        writer.end_tag("</marker>");
    }

    if used_marker_ids.contains("seq-async-arrowhead") {
        writer.start_tag(
            "<marker id=\"seq-async-arrowhead\" viewBox=\"0 0 10 10\" refX=\"0\" refY=\"5\" \
             markerWidth=\"8\" markerHeight=\"8\" orient=\"auto-start-reverse\" \
             markerUnits=\"userSpaceOnUse\">",
        );
        writer.push_line(&format!(
            "<path d=\"M 0 0 L 10 5 L 0 10\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\"{dynamic_attrs} />",
            color = palette.marker_color,
            dynamic_attrs = stroke_dynamic_attrs,
        ));
        writer.end_tag("</marker>");
    }

    writer.end_tag("</defs>");
}

// ---------------------------------------------------------------------------
// Element renderers
// ---------------------------------------------------------------------------

fn render_participant(writer: &mut SvgWriter, p: &SvgParticipant, palette: &SequenceSvgPalette) {
    match p.kind {
        ParticipantKind::Participant => render_participant_box(writer, p, palette),
        ParticipantKind::Actor => render_actor(writer, p, palette),
    }
}

fn render_participant_box(
    writer: &mut SvgWriter,
    p: &SvgParticipant,
    palette: &SequenceSvgPalette,
) {
    let r = &p.rect;
    let rect_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-participant-box",
        &["fill:var(--_node-fill);", "stroke:var(--_node-stroke);"],
    );
    writer.push_line(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
        x = fmt_f64(r.x),
        y = fmt_f64(r.y),
        w = fmt_f64(r.width),
        h = fmt_f64(r.height),
        fill = palette.participant_fill,
        stroke = palette.participant_stroke,
        dynamic_attrs = rect_dynamic_attrs,
    ));

    let text_x = p.center_x;
    let text_y = r.y + r.height / 2.0;
    let text_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" \
         fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
        x = fmt_f64(text_x),
        y = fmt_f64(text_y),
        fill = palette.participant_text,
        dynamic_attrs = text_dynamic_attrs,
        label = escape_text(&p.label),
    ));
}

fn render_actor(writer: &mut SvgWriter, p: &SvgParticipant, palette: &SequenceSvgPalette) {
    // Stick figure centered at (center_x, rect center_y)
    let cx = p.center_x;
    let r = &p.rect;
    let top = r.y + 4.0;
    let stroke_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-actor-stroke",
        &["stroke:var(--_node-stroke);"],
    );

    // Head (circle)
    let head_r = 8.0;
    let head_cy = top + head_r;
    writer.push_line(&format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" \
         fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        cx = fmt_f64(cx),
        cy = fmt_f64(head_cy),
        r = fmt_f64(head_r),
        stroke = palette.actor_stroke,
        dynamic_attrs = stroke_dynamic_attrs,
    ));

    // Body
    let body_top = head_cy + head_r;
    let body_bottom = body_top + 14.0;
    writer.push_line(&format!(
        "<line x1=\"{x}\" y1=\"{y1}\" x2=\"{x}\" y2=\"{y2}\" \
         stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x = fmt_f64(cx),
        y1 = fmt_f64(body_top),
        y2 = fmt_f64(body_bottom),
        stroke = palette.actor_stroke,
        dynamic_attrs = stroke_dynamic_attrs,
    ));

    // Arms
    let arm_y = body_top + 6.0;
    let arm_span = 14.0;
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" \
         stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x1 = fmt_f64(cx - arm_span),
        x2 = fmt_f64(cx + arm_span),
        y = fmt_f64(arm_y),
        stroke = palette.actor_stroke,
        dynamic_attrs = stroke_dynamic_attrs,
    ));

    // Legs
    let leg_span = 10.0;
    let leg_bottom = body_bottom + 12.0;
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" \
         stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x1 = fmt_f64(cx),
        y1 = fmt_f64(body_bottom),
        x2 = fmt_f64(cx - leg_span),
        y2 = fmt_f64(leg_bottom),
        stroke = palette.actor_stroke,
        dynamic_attrs = stroke_dynamic_attrs,
    ));
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" \
         stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x1 = fmt_f64(cx),
        y1 = fmt_f64(body_bottom),
        x2 = fmt_f64(cx + leg_span),
        y2 = fmt_f64(leg_bottom),
        stroke = palette.actor_stroke,
        dynamic_attrs = stroke_dynamic_attrs,
    ));

    // Label below the figure
    let label_y = r.y + r.height - 2.0;
    let text_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"auto\" \
         fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
        x = fmt_f64(cx),
        y = fmt_f64(label_y),
        fill = palette.participant_text,
        dynamic_attrs = text_dynamic_attrs,
        label = escape_text(&p.label),
    ));
}

fn render_lifeline(writer: &mut SvgWriter, ll: &SvgLifeline, palette: &SequenceSvgPalette) {
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-line",
        &["stroke:var(--_line);"],
    );
    writer.push_line(&format!(
        "<line x1=\"{x}\" y1=\"{y1}\" x2=\"{x}\" y2=\"{y2}\" \
         stroke=\"{stroke}\" stroke-width=\"1\" stroke-dasharray=\"5,5\"{dynamic_attrs} />",
        x = fmt_f64(ll.x),
        y1 = fmt_f64(ll.y_start),
        y2 = fmt_f64(ll.y_end),
        stroke = palette.lifeline_stroke,
        dynamic_attrs = dynamic_attrs,
    ));
}

fn render_destroy_marker(
    writer: &mut SvgWriter,
    marker: &SvgDestroyMarker,
    palette: &SequenceSvgPalette,
) {
    let size = 6.0;
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-line",
        &["stroke:var(--_line);"],
    );
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x1 = fmt_f64(marker.x - size),
        y1 = fmt_f64(marker.y - size),
        x2 = fmt_f64(marker.x + size),
        y2 = fmt_f64(marker.y + size),
        stroke = palette.lifeline_stroke,
        dynamic_attrs = dynamic_attrs,
    ));
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{stroke}\" stroke-width=\"1.5\"{dynamic_attrs} />",
        x1 = fmt_f64(marker.x - size),
        y1 = fmt_f64(marker.y + size),
        x2 = fmt_f64(marker.x + size),
        y2 = fmt_f64(marker.y - size),
        stroke = palette.lifeline_stroke,
        dynamic_attrs = dynamic_attrs,
    ));
}

fn render_block(writer: &mut SvgWriter, block: &SvgBlock, palette: &SequenceSvgPalette) {
    let rect = &block.rect;
    let rect_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-block-stroke",
        &["stroke:var(--_node-stroke);"],
    );
    writer.push_line(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" stroke-dasharray=\"{BLOCK_STROKE_DASH}\"{dynamic_attrs} />",
        x = fmt_f64(rect.x),
        y = fmt_f64(rect.y),
        w = fmt_f64(rect.width),
        h = fmt_f64(rect.height),
        stroke = palette.block_stroke,
        dynamic_attrs = rect_dynamic_attrs,
    ));

    let operator = block.kind.keyword();
    let tab_width = block_tab_width(operator);
    render_block_operator_tab(
        writer,
        operator,
        rect.x,
        rect.y,
        tab_width,
        BLOCK_TAB_HEIGHT,
        palette,
    );

    if let Some(title) = format_fragment_guard(&block.label) {
        let title_x = rect.x + tab_width + (rect.width - tab_width) / 2.0;
        render_centered_block_text(
            writer,
            &title,
            title_x,
            rect.y + BLOCK_HEADER_TEXT_Y,
            palette,
        );
    }

    for divider in &block.dividers {
        render_block_divider(writer, rect, divider, palette);
    }
}

fn render_block_divider(
    writer: &mut SvgWriter,
    rect: &super::svg_layout::SvgRect,
    divider: &SvgBlockDivider,
    palette: &SequenceSvgPalette,
) {
    let _divider_keyword = divider.kind.keyword();
    let line_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-block-stroke",
        &["stroke:var(--_node-stroke);"],
    );
    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"{stroke}\" stroke-width=\"1\" stroke-dasharray=\"{BLOCK_STROKE_DASH}\"{dynamic_attrs} />",
        x1 = fmt_f64(rect.x),
        x2 = fmt_f64(rect.x + rect.width),
        y = fmt_f64(divider.y),
        stroke = palette.block_stroke,
        dynamic_attrs = line_dynamic_attrs,
    ));

    if let Some(label) = format_fragment_guard(&divider.label) {
        render_centered_block_text(
            writer,
            &label,
            rect.x + rect.width / 2.0,
            divider.y + BLOCK_SECTION_TEXT_Y,
            palette,
        );
    }
}

fn render_block_operator_tab(
    writer: &mut SvgWriter,
    keyword: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    palette: &SequenceSvgPalette,
) {
    let cut = BLOCK_TAB_CUT;
    let tab_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-block-tab",
        &["fill:var(--_group-fill);", "stroke:var(--_node-stroke);"],
    );
    writer.push_line(&format!(
        "<polygon points=\"{x0},{y0} {x1},{y0} {x1},{y1} {x2},{y2} {x0},{y2}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
        x0 = fmt_f64(x),
        y0 = fmt_f64(y),
        x1 = fmt_f64(x + width),
        y1 = fmt_f64(y + height - cut),
        x2 = fmt_f64(x + width - cut * 1.2),
        y2 = fmt_f64(y + height),
        fill = palette.block_label_bg,
        stroke = palette.block_stroke,
        dynamic_attrs = tab_dynamic_attrs,
    ));
    let text_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
        x = fmt_f64(x + width / 2.0),
        y = fmt_f64(y + height / 2.0),
        fill = palette.participant_text,
        dynamic_attrs = text_dynamic_attrs,
        label = escape_text(keyword),
    ));
}

fn render_centered_block_text(
    writer: &mut SvgWriter,
    label: &str,
    x: f64,
    y: f64,
    palette: &SequenceSvgPalette,
) {
    let text_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
        x = fmt_f64(x),
        y = fmt_f64(y),
        fill = palette.participant_text,
        dynamic_attrs = text_dynamic_attrs,
        label = escape_text(label),
    ));
}

fn render_message(writer: &mut SvgWriter, msg: &SvgMessage, palette: &SequenceSvgPalette) {
    let marker = marker_attr(&msg.arrow_head);
    let dash = dash_attr(&msg.line_style);
    let target_x = pull_back_sequence_endpoint(msg.from_x, msg.to_x, &msg.arrow_head);
    let line_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-line",
        &["stroke:var(--_line);"],
    );

    writer.push_line(&format!(
        "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" \
         stroke=\"{stroke}\" stroke-width=\"1\"{dash}{marker}{dynamic_attrs}/>",
        x1 = fmt_f64(msg.from_x),
        x2 = fmt_f64(target_x),
        y = fmt_f64(msg.y),
        stroke = palette.lifeline_stroke,
        marker = marker,
        dynamic_attrs = line_dynamic_attrs,
    ));

    if !msg.label.is_empty() {
        let text_dynamic_attrs = dynamic_css_attrs(
            palette.dynamic_css,
            "sequence-text",
            &["fill:var(--_text);"],
        );
        writer.push_line(&format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"auto\" \
             fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
            x = fmt_f64(msg.label_x),
            y = fmt_f64(msg.label_y),
            fill = palette.participant_text,
            dynamic_attrs = text_dynamic_attrs,
            label = escape_text(&msg.label),
        ));
    }
}

fn render_self_message(writer: &mut SvgWriter, sm: &SvgSelfMessage, palette: &SequenceSvgPalette) {
    let x = sm.x;
    let y = sm.y;
    let arm = sm.arm_width;
    let h = sm.height;
    let end_x = x + sequence_arrow_pullback(&sm.arrow_head);
    let dash = dash_attr(&sm.line_style);
    let marker = marker_attr(&sm.arrow_head);
    let path_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-line",
        &["stroke:var(--_line);"],
    );

    // Right-angle loop: right → down → left with arrow
    writer.push_line(&format!(
        "<path d=\"M {x0} {y0} L {x1} {y0} L {x1} {y1} L {x2} {y1}\" \
         fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\"{dash}{marker}{dynamic_attrs}/>",
        x0 = fmt_f64(x),
        y0 = fmt_f64(y),
        x1 = fmt_f64(x + arm),
        y1 = fmt_f64(y + h),
        x2 = fmt_f64(end_x),
        stroke = palette.lifeline_stroke,
        marker = marker,
        dynamic_attrs = path_dynamic_attrs,
    ));

    if !sm.label.is_empty() {
        let text_dynamic_attrs = dynamic_css_attrs(
            palette.dynamic_css,
            "sequence-text",
            &["fill:var(--_text);"],
        );
        writer.push_line(&format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"start\" dominant-baseline=\"auto\" \
             fill=\"{fill}\"{dynamic_attrs}>{label}</text>",
            x = fmt_f64(sm.label_x),
            y = fmt_f64(sm.label_y),
            fill = palette.participant_text,
            dynamic_attrs = text_dynamic_attrs,
            label = escape_text(&sm.label),
        ));
    }
}

fn render_note(writer: &mut SvgWriter, note: &SvgNote, palette: &SequenceSvgPalette) {
    let r = &note.rect;

    // Folded-corner note box
    let fold = 7.0;
    let x = r.x;
    let y = r.y;
    let w = r.width;
    let h = r.height;
    let note_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-note",
        &["fill:var(--_node-fill);", "stroke:var(--_line);"],
    );
    let note_stroke_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-line",
        &["stroke:var(--_line);"],
    );

    // Main body with folded corner
    writer.push_line(&format!(
        "<path d=\"M {x0} {y0} L {x1} {y0} L {x2} {y1} L {x2} {y2} L {x0} {y2} Z\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
        x0 = fmt_f64(x),
        y0 = fmt_f64(y),
        x1 = fmt_f64(x + w - fold),
        x2 = fmt_f64(x + w),
        y1 = fmt_f64(y + fold),
        y2 = fmt_f64(y + h),
        fill = palette.note_fill,
        stroke = palette.lifeline_stroke,
        dynamic_attrs = note_dynamic_attrs,
    ));

    // Fold line
    writer.push_line(&format!(
        "<path d=\"M {x1} {y0} L {x1} {y1} L {x2} {y1}\" \
         fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
        x1 = fmt_f64(x + w - fold),
        y0 = fmt_f64(y),
        x2 = fmt_f64(x + w),
        y1 = fmt_f64(y + fold),
        stroke = palette.lifeline_stroke,
        dynamic_attrs = note_stroke_dynamic_attrs,
    ));

    // Text centered in the note
    let text_x = x + w / 2.0;
    let text_y = y + h / 2.0;
    let text_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-text",
        &["fill:var(--_text);"],
    );
    writer.push_line(&format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" \
         fill=\"{fill}\"{dynamic_attrs}>{text}</text>",
        x = fmt_f64(text_x),
        y = fmt_f64(text_y),
        fill = palette.participant_text,
        dynamic_attrs = text_dynamic_attrs,
        text = escape_text(&note.text),
    ));
}

fn render_activation(writer: &mut SvgWriter, act: &SvgActivation, palette: &SequenceSvgPalette) {
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "sequence-activation",
        &["fill:var(--_node-fill);", "stroke:var(--_line);"],
    );
    writer.push_line(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1\"{dynamic_attrs} />",
        x = fmt_f64(act.x),
        y = fmt_f64(act.y_start),
        w = fmt_f64(act.width),
        h = fmt_f64(act.y_end - act.y_start),
        fill = palette.activation_fill,
        stroke = palette.lifeline_stroke,
        dynamic_attrs = dynamic_attrs,
    ));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_used_marker_ids(layout: &SvgSequenceLayout) -> BTreeSet<&'static str> {
    let mut used_marker_ids = BTreeSet::new();

    for row in &layout.rows {
        match row {
            SvgRow::Message(msg) => {
                if let Some(id) = marker_id_for_arrow_head(&msg.arrow_head) {
                    used_marker_ids.insert(id);
                }
            }
            SvgRow::SelfMessage(sm) => {
                if let Some(id) = marker_id_for_arrow_head(&sm.arrow_head) {
                    used_marker_ids.insert(id);
                }
            }
            SvgRow::Note(_) => {}
        }
    }

    used_marker_ids
}

fn marker_id_for_arrow_head(arrow_head: &ArrowHead) -> Option<&'static str> {
    match arrow_head {
        ArrowHead::None => None,
        ArrowHead::Filled => Some("seq-arrowhead"),
        ArrowHead::Cross => Some("seq-crosshead"),
        ArrowHead::Async => Some("seq-async-arrowhead"),
    }
}

fn marker_attr(arrow_head: &ArrowHead) -> String {
    match marker_id_for_arrow_head(arrow_head) {
        Some(id) => format!(" marker-end=\"url(#{id})\""),
        None => String::new(),
    }
}

fn sequence_arrow_pullback(arrow_head: &ArrowHead) -> f64 {
    match arrow_head {
        ArrowHead::None => 0.0,
        ArrowHead::Async => 10.0,
        ArrowHead::Filled | ArrowHead::Cross => 0.0,
    }
}

fn pull_back_sequence_endpoint(from_x: f64, to_x: f64, arrow_head: &ArrowHead) -> f64 {
    let pullback = sequence_arrow_pullback(arrow_head);
    if pullback == 0.0 {
        to_x
    } else if to_x >= from_x {
        to_x - pullback
    } else {
        to_x + pullback
    }
}

fn dash_attr(line_style: &LineStyle) -> String {
    match line_style {
        LineStyle::Solid => String::new(),
        LineStyle::Dashed => " stroke-dasharray=\"6,4\"".to_string(),
    }
}

fn block_tab_width(keyword: &str) -> f64 {
    ((keyword.len() as f64) * BLOCK_TAB_CHAR_WIDTH + BLOCK_TAB_PADDING_X * 2.0)
        .max(BLOCK_TAB_MIN_WIDTH)
}

fn format_fragment_guard(label: &str) -> Option<String> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("[{trimmed}]"))
    }
}
