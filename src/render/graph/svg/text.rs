//! Shared SVG text emission helpers for graph rendering.

use crate::graph::geometry::FPoint;
use crate::graph::measure::{
    DEFAULT_LABEL_PADDING_X, DEFAULT_LABEL_PADDING_Y, ProportionalTextMetrics,
};
use crate::render::svg::{SvgWriter, escape_text, fmt_f64};

pub(super) struct TextRenderStyle<'a> {
    pub(super) color: &'a str,
    pub(super) extra_attrs: &'a str,
    pub(super) background: Option<BackgroundStyle<'a>>,
}

pub(super) struct BackgroundStyle<'a> {
    pub(super) fill: &'a str,
    pub(super) extra_attrs: &'a str,
}

// Plan 0145 Task 1.7: label padding defaults now live in `graph::measure`
// so layout dummy reservations and render backgrounds stay in lockstep.
pub(super) const LABEL_BG_PAD_X: f64 = DEFAULT_LABEL_PADDING_X;
pub(super) const LABEL_BG_PAD_Y: f64 = DEFAULT_LABEL_PADDING_Y;

pub(super) fn render_text_centered(
    writer: &mut SvgWriter,
    center: FPoint,
    text: &str,
    metrics: &ProportionalTextMetrics,
    scale: f64,
    style: TextRenderStyle<'_>,
) {
    render_text_centered_with_wrap(writer, center, text, None, metrics, scale, style);
}

/// Like [`render_text_centered`] but uses a pre-wrapped line vector when
/// `wrapped_lines` is `Some`, so the background rect and text lines come
/// from the same wrap decision the layout engine reserved space for.
/// Plan 0147 Task 1.6.
pub(super) fn render_text_centered_with_wrap(
    writer: &mut SvgWriter,
    center: FPoint,
    text: &str,
    wrapped_lines: Option<&[String]>,
    metrics: &ProportionalTextMetrics,
    scale: f64,
    style: TextRenderStyle<'_>,
) {
    if let Some(bg) = &style.background {
        let (w, h) = match wrapped_lines {
            Some(lines) => {
                measure_wrapped_with_padding(metrics, lines, LABEL_BG_PAD_X, LABEL_BG_PAD_Y)
            }
            None => metrics.measure_text_with_padding(text, LABEL_BG_PAD_X, LABEL_BG_PAD_Y),
        };
        let rect_w = w * scale;
        let rect_h = h * scale;
        let rect = format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"{extra} />",
            x = fmt_f64(center.x - rect_w / 2.0),
            y = fmt_f64(center.y - rect_h / 2.0),
            w = fmt_f64(rect_w),
            h = fmt_f64(rect_h),
            fill = bg.fill,
            extra = bg.extra_attrs,
        );
        writer.push_line(&rect);
    }

    // Prefer the pre-wrapped artifact if present; otherwise fall back to
    // splitting the raw text on '\n' (hard breaks from `<br>` normalization).
    let owned_fallback: Vec<&str>;
    let lines: &[&str] = match wrapped_lines {
        Some(wrapped) => {
            // Collect into a local Vec<&str> to share the rendering path.
            owned_fallback = wrapped.iter().map(String::as_str).collect();
            &owned_fallback
        }
        None => {
            owned_fallback = text.split('\n').collect();
            &owned_fallback
        }
    };
    if lines.len() == 1 {
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\"{extra_attrs}>{text}</text>",
            x = fmt_f64(center.x),
            y = fmt_f64(center.y),
            color = style.color,
            extra_attrs = style.extra_attrs,
            text = escape_text(lines[0])
        );
        writer.push_line(&line);
        return;
    }

    let line_height = metrics.line_height * scale;
    let total_height = line_height * (lines.len().saturating_sub(1) as f64);
    let start_y = center.y - total_height / 2.0;

    for (idx, line_text) in lines.iter().enumerate() {
        let line_y = start_y + line_height * idx as f64;
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\"{extra_attrs}>{text}</text>",
            x = fmt_f64(center.x),
            y = fmt_f64(line_y),
            color = style.color,
            extra_attrs = style.extra_attrs,
            text = escape_text(line_text)
        );
        writer.push_line(&line);
    }
}

fn measure_wrapped_with_padding(
    metrics: &ProportionalTextMetrics,
    lines: &[String],
    padding_x: f64,
    padding_y: f64,
) -> (f64, f64) {
    // Mirrors ProportionalTextMetrics::measure_text_with_padding but sources
    // the line vector from the caller's pre-wrapped artifact.
    let (w, h) = metrics.edge_label_dimensions_wrapped(lines);
    // `edge_label_dimensions_wrapped` bakes in `metrics.label_padding_*`; peel
    // those off and add the caller-requested padding to match the untreated
    // `measure_text_with_padding` behavior.
    let raw_w = w - 2.0 * metrics.label_padding_x;
    let raw_h = h - 2.0 * metrics.label_padding_y;
    (raw_w + 2.0 * padding_x, raw_h + 2.0 * padding_y)
}
