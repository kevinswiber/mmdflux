//! Shared SVG text emission helpers for graph rendering.

use super::writer::{SvgWriter, escape_text, fmt_f64};
use crate::graph::measure::ProportionalTextMetrics;

pub(super) fn render_text_centered(
    writer: &mut SvgWriter,
    x: f64,
    y: f64,
    text: &str,
    color: &str,
    metrics: &ProportionalTextMetrics,
    scale: f64,
) {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() == 1 {
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
            x = fmt_f64(x),
            y = fmt_f64(y),
            color = color,
            text = escape_text(text)
        );
        writer.push_line(&line);
        return;
    }

    let line_height = metrics.line_height * scale;
    let total_height = line_height * (lines.len().saturating_sub(1) as f64);
    let start_y = y - total_height / 2.0;

    for (idx, line_text) in lines.iter().enumerate() {
        let line_y = start_y + line_height * idx as f64;
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
            x = fmt_f64(x),
            y = fmt_f64(line_y),
            color = color,
            text = escape_text(line_text)
        );
        writer.push_line(&line);
    }
}
