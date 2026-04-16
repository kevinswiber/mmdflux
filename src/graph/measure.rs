//! Shared graph-family measurement primitives.
//!
//! Graph-owned measurement stays renderer-agnostic: engines use grid
//! measurement for discrete replay layouts and proportional measurement for
//! float-space geometry.

#![allow(dead_code)]

use crate::graph::{Direction, Node, Shape};

/// Default font size used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_FONT_SIZE: f64 = 16.0;
/// Default horizontal node padding used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_NODE_PADDING_X: f64 = 15.0;
/// Default vertical node padding used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_NODE_PADDING_Y: f64 = 15.0;
/// Default horizontal padding applied around edge labels (per side).
///
/// This matches the `<rect class="graph-edge-label-bg">` horizontal padding
/// emitted by `render::graph::svg::text` so layout-time dummy reservations and
/// render-time backgrounds stay in lockstep.
pub const DEFAULT_LABEL_PADDING_X: f64 = 4.0;
/// Default vertical padding applied around edge labels (per side).
pub const DEFAULT_LABEL_PADDING_Y: f64 = 2.0;
/// Scale factor applied to approximate Mermaid's measured text widths.
const TEXT_WIDTH_SCALE: f64 = 1.16;

#[derive(Debug, Clone)]
pub struct ProportionalTextMetrics {
    pub font_size: f64,
    pub line_height: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub label_padding_x: f64,
    pub label_padding_y: f64,
}

impl ProportionalTextMetrics {
    pub fn new(font_size: f64, node_padding_x: f64, node_padding_y: f64) -> Self {
        Self {
            font_size,
            line_height: font_size * 1.5,
            node_padding_x,
            node_padding_y,
            label_padding_x: DEFAULT_LABEL_PADDING_X,
            label_padding_y: DEFAULT_LABEL_PADDING_Y,
        }
    }

    pub fn measure_text_with_padding(
        &self,
        text: &str,
        padding_x: f64,
        padding_y: f64,
    ) -> (f64, f64) {
        let lines: Vec<&str> = text.split('\n').collect();
        let line_count = lines.len().max(1) as f64;
        let max_width = lines
            .iter()
            .map(|line| self.measure_line_width(line))
            .fold(0.0, f64::max);
        let width = max_width * TEXT_WIDTH_SCALE + padding_x * 2.0;
        let height = self.line_height * line_count + padding_y * 2.0;
        (width, height)
    }

    pub fn edge_label_dimensions(&self, label: &str) -> (f64, f64) {
        self.measure_text_with_padding(label, self.label_padding_x, self.label_padding_y)
    }

    /// Dimensions of an edge label that has already been wrapped into `lines`.
    ///
    /// Width = max width across lines; height = `line_height * lines.len()`
    /// plus symmetric label padding on both axes. Mirrors
    /// [`Self::edge_label_dimensions`] so the wrapped path is a drop-in
    /// replacement wherever `wrapped_label_lines` is populated. Plan 0147
    /// Task 1.6 / design.md §6.3.
    pub fn edge_label_dimensions_wrapped(&self, lines: &[String]) -> (f64, f64) {
        let line_count = lines.len().max(1) as f64;
        let max_width = lines
            .iter()
            .map(|line| self.measure_line_width(line))
            .fold(0.0, f64::max);
        let width = max_width * TEXT_WIDTH_SCALE + self.label_padding_x * 2.0;
        let height = self.line_height * line_count + self.label_padding_y * 2.0;
        (width, height)
    }

    pub(crate) fn measure_line_width(&self, text: &str) -> f64 {
        text.chars()
            .map(|c| self.char_width_ratio(c) * self.font_size)
            .sum::<f64>()
    }

    pub(crate) fn char_width_ratio(&self, c: char) -> f64 {
        match c {
            'i' | 'l' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' => 0.25,
            'f' | 'j' | 't' | 'r' => 0.32,
            'm' | 'w' | 'M' | 'W' => 0.7,
            'A'..='Z' => 0.48,
            _ => 0.46,
        }
    }
}

/// Greedy word-wrap that honours `max_width` in pixels using `metrics` for
/// per-character width estimates. `'\n'` in `text` is treated as a hard
/// break; each segment is wrapped independently. Falls back to per-character
/// splits when a single word exceeds `max_width`.
///
/// **Sequencing contract:** callers MUST normalize `<br>`/`<br/>`/`<br />`
/// variants to `'\n'` before calling this function (see
/// [`crate::diagrams::flowchart::compiler::normalize_br_tags`]). `wrap_lines`
/// does not inspect the raw Mermaid source.
pub fn wrap_lines(metrics: &ProportionalTextMetrics, text: &str, max_width: f64) -> Vec<String> {
    let space_w = metrics.char_width_ratio(' ') * metrics.font_size * TEXT_WIDTH_SCALE;
    let mut out = Vec::new();
    for segment in text.split('\n') {
        let mut current = String::new();
        let mut current_w = 0.0_f64;
        for word in segment.split_whitespace() {
            let ww = metrics.measure_line_width(word) * TEXT_WIDTH_SCALE;
            if ww > max_width {
                // Oversized word: fall back to per-character splits regardless
                // of whether the word is first on the line. GPT-5.4 review of
                // PR #235 found the previous `&& current.is_empty()` guard let
                // oversized trailing words overflow `max_width`. Flush the
                // current line first so the char-split starts at column 0.
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    current_w = 0.0;
                }
                for ch in word.chars() {
                    let cw = metrics.char_width_ratio(ch) * metrics.font_size * TEXT_WIDTH_SCALE;
                    if current_w + cw > max_width && !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                        current_w = 0.0;
                    }
                    current.push(ch);
                    current_w += cw;
                }
                continue;
            }
            let sep_w = if current.is_empty() { 0.0 } else { space_w };
            if current_w + sep_w + ww > max_width && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_w = 0.0;
            }
            if !current.is_empty() {
                current.push(' ');
                current_w += space_w;
            }
            current.push_str(word);
            current_w += ww;
        }
        if !current.is_empty() {
            out.push(current);
        } else {
            out.push(String::new());
        }
    }
    out
}

/// Default proportional metrics used by engine-side float/MMDS layout flows.
pub fn default_proportional_text_metrics() -> ProportionalTextMetrics {
    ProportionalTextMetrics::new(
        DEFAULT_PROPORTIONAL_FONT_SIZE,
        DEFAULT_PROPORTIONAL_NODE_PADDING_X,
        DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
    )
}

/// Calculate the grid dimensions needed to replay a node in the grid surface.
pub fn grid_node_dimensions(node: &Node, direction: Direction) -> (usize, usize) {
    let lines: Vec<&str> = node.label.split('\n').collect();
    let max_line_len = lines
        .iter()
        .filter(|l| **l != Node::SEPARATOR)
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    // TextBlock nodes are borderless labels (e.g. lollipop interface names).
    // Minimal dimensions keep the label tightly adjacent to the edge marker.
    if node.shape == Shape::TextBlock {
        let w = max_line_len.max(1);
        let h = lines.len().max(1);
        return (w, h);
    }

    let (w, h) = (max_line_len + 4, lines.len() + 2);

    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        return (h, w);
    }

    (w, h)
}

/// Grid edge-label dimensions used by layered measurement and grid replay.
pub fn grid_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Grid edge-label dimensions for a pre-wrapped label. Mirrors
/// [`grid_edge_label_dimensions`] but consumes a persisted wrap artifact
/// instead of `'\n'`-splitting raw text, so wrap decisions made in pixel
/// units (plan 0147) are honoured by the Grid measurement path too.
pub fn grid_edge_label_dimensions_wrapped(lines: &[String]) -> (f64, f64) {
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Proportional node dimensions used during layered measurement and float layout.
pub fn proportional_node_dimensions(
    metrics: &ProportionalTextMetrics,
    node: &Node,
    direction: Direction,
) -> (f64, f64) {
    let (label_w, label_h) = metrics.measure_text_with_padding(&node.label, 0.0, 0.0);

    let (mut width, mut height) = match node.shape {
        Shape::Rectangle => (
            label_w + metrics.node_padding_x * 4.0,
            label_h + metrics.node_padding_y * 2.0,
        ),
        Shape::Diamond => {
            let w = label_w + metrics.node_padding_x;
            let h = label_h + metrics.node_padding_y;
            let size = w + h;
            (size, size)
        }
        Shape::Stadium => {
            let h = label_h + metrics.node_padding_y * 2.0;
            let radius = h / 2.0;
            (label_w + metrics.node_padding_x * 2.0 + radius, h)
        }
        Shape::Cylinder => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let rx = w / 2.0;
            let ry = rx / (2.5 + w / 50.0);
            (w, label_h + metrics.node_padding_y * 2.0 + ry)
        }
        Shape::Document => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w, h + h / 8.0)
        }
        Shape::Documents => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            let offset = 5.0;
            (w + 2.0 * offset, h + h / 4.0 + 2.0 * offset)
        }
        Shape::TaggedDocument => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 3.0;
            (w * 1.1, h + h / 4.0)
        }
        Shape::Card => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + 12.0, h)
        }
        Shape::TaggedRect => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + 0.2 * h, h)
        }
        Shape::Subroutine => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + 20.0, h)
        }
        Shape::Hexagon => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + h / 2.0, h)
        }
        Shape::Parallelogram | Shape::InvParallelogram | Shape::Trapezoid | Shape::InvTrapezoid => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + h / 3.0, h)
        }
        Shape::Asymmetric => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + h / 3.0, h)
        }
        Shape::SmallCircle => (14.0, 14.0),
        Shape::FramedCircle => (28.0, 28.0),
        Shape::CrossedCircle => (60.0, 60.0),
        Shape::ForkJoin if node.label.trim().is_empty() => (70.0, 7.0),
        Shape::TextBlock => (label_w, label_h),
        _ => (
            label_w + metrics.node_padding_x * 2.0,
            label_h + metrics.node_padding_y * 2.0,
        ),
    };

    match node.shape {
        Shape::Hexagon | Shape::Trapezoid | Shape::InvTrapezoid | Shape::Asymmetric => {
            width *= 1.15;
        }
        Shape::Circle
        | Shape::DoubleCircle
        | Shape::SmallCircle
        | Shape::FramedCircle
        | Shape::CrossedCircle => {
            let size = width.max(height);
            width = size;
            height = size;
        }
        _ => {}
    }

    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        std::mem::swap(&mut width, &mut height);
    }

    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;

    #[test]
    fn measure_text_uses_proportional_heuristic() {
        let metrics = ProportionalTextMetrics::new(16.0, 8.0, 6.4);
        let (w, h) = metrics.measure_text_with_padding("ABC", 0.0, 0.0);

        assert!(w > 16.0);
        assert!(h > 16.0);
    }

    #[test]
    fn grid_node_dimensions_match_grid_box_model() {
        let node = Node::new("A").with_label("Hello");
        let (w, h) = grid_node_dimensions(&node, Direction::TopDown);
        assert_eq!((w, h), (9, 3));
    }

    // -- Plan 0147, Task 1.6: wrapped-dims measurement primitive --

    #[test]
    fn edge_label_dimensions_wrapped_measures_multi_line_height() {
        let metrics = default_proportional_text_metrics();
        let lines = vec!["short".to_string(), "another line".to_string()];
        let (w, h) = metrics.edge_label_dimensions_wrapped(&lines);
        let expected_min_h = 2.0 * metrics.line_height + 2.0 * metrics.label_padding_y - 0.001;
        assert!(
            h >= expected_min_h,
            "expected >= 2 line heights (~{expected_min_h}), got {h}"
        );
        assert!(
            w < 200.0,
            "width should be bounded by max line, got {w} for lines {lines:?}"
        );
    }

    #[test]
    fn edge_label_dimensions_wrapped_matches_unwrapped_single_line() {
        let metrics = default_proportional_text_metrics();
        let (w_single, h_single) = metrics.edge_label_dimensions("exactly the same");
        let (w_wrap, h_wrap) =
            metrics.edge_label_dimensions_wrapped(&["exactly the same".to_string()]);
        assert!((w_single - w_wrap).abs() < 0.001);
        assert!((h_single - h_wrap).abs() < 0.001);
    }

    // -- Plan 0145, Task 1.7: Label padding contract --

    #[test]
    fn proportional_text_metrics_default_label_padding_matches_svg_constants() {
        let metrics = ProportionalTextMetrics::new(16.0, 15.0, 15.0);
        assert_eq!(metrics.label_padding_x, DEFAULT_LABEL_PADDING_X);
        assert_eq!(metrics.label_padding_y, DEFAULT_LABEL_PADDING_Y);
        assert_eq!(DEFAULT_LABEL_PADDING_X, 4.0);
        assert_eq!(DEFAULT_LABEL_PADDING_Y, 2.0);
    }

    // -- Plan 0147, Task 1.1: wrap_lines greedy word-wrap (Red) --

    #[test]
    fn wrap_lines_greedy_breaks_on_word_boundaries() {
        let metrics = ProportionalTextMetrics::new(
            DEFAULT_PROPORTIONAL_FONT_SIZE,
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
        );
        // max_width 100 is wider than every word in the fixture so the
        // greedy path never hits the char-fallback — this test pins
        // whitespace-boundary wrap specifically (char fallback is covered
        // by `wrap_lines_char_fallback_triggers_for_mid_line_oversized_word`).
        let lines = wrap_lines(&metrics, "this is a deliberately long label", 100.0);
        assert!(lines.len() >= 2, "expected multi-line wrap, got {lines:?}");
        // Word boundaries preserved: no line begins or ends mid-word.
        for line in &lines {
            assert!(
                !line.starts_with(' ') && !line.ends_with(' '),
                "line has stray whitespace: {line:?}"
            );
        }
        // Round-tripping words preserves the original token order and content.
        let all: String = lines.join(" ");
        assert_eq!(all, "this is a deliberately long label");
    }

    #[test]
    fn wrap_lines_empty_returns_single_empty_line() {
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "", 200.0);
        assert_eq!(lines, vec![String::new()]);
    }

    #[test]
    fn wrap_lines_fits_when_max_width_is_large() {
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "single", 10_000.0);
        assert_eq!(lines, vec!["single".to_string()]);
    }

    // -- Plan 0147, Task 1.3: <br>-derived '\n' acts as a hard segment break --

    #[test]
    fn wrap_lines_preserves_br_hard_breaks_as_segment_boundaries() {
        let metrics = ProportionalTextMetrics::new(
            DEFAULT_PROPORTIONAL_FONT_SIZE,
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
        );
        // Simulate output of normalize_br_tags: <br> has already become '\n'.
        // max_width 100 keeps every word under the threshold so the test
        // focuses on segment-boundary behaviour rather than char fallback.
        let input = "yes\nsome very long continuation";
        let lines = wrap_lines(&metrics, input, 100.0);

        // The first segment ("yes") stands alone on its own line — not merged with "some".
        assert_eq!(
            lines[0], "yes",
            "first segment must stand alone, got {lines:?}"
        );

        // The second segment's tokens are wrapped across the remaining lines, in order.
        let rest: String = lines[1..].join(" ");
        assert_eq!(rest, "some very long continuation");
    }

    // GPT-5.4 review of PR #235: oversized-word char-fallback must apply
    // even when the oversized word is NOT the first token on a line.
    #[test]
    fn wrap_lines_char_fallback_triggers_for_mid_line_oversized_word() {
        let metrics = default_proportional_text_metrics();
        let max_width = 200.0;
        let lines = wrap_lines(
            &metrics,
            "short supercalifragilisticexpialidocious",
            max_width,
        );
        for line in &lines {
            let w = metrics.measure_line_width(line) * TEXT_WIDTH_SCALE;
            assert!(
                w <= max_width + 0.5,
                "line {line:?} is {w} px wide, exceeds max_width {max_width}"
            );
        }
    }

    #[test]
    fn wrap_lines_empty_middle_segment_preserved() {
        // Refactor invariant from Task 1.3: "a\n\nb" → ["a", "", "b"].
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "a\n\nb", 1000.0);
        assert_eq!(lines, vec!["a".to_string(), String::new(), "b".to_string()]);
    }

    #[test]
    fn edge_label_dimensions_includes_padding() {
        let metrics = ProportionalTextMetrics::new(16.0, 15.0, 15.0);
        let (w_padded, h_padded) = metrics.edge_label_dimensions("foo");
        let (w_raw, h_raw) = metrics.measure_text_with_padding("foo", 0.0, 0.0);
        assert!(
            (w_padded - (w_raw + 2.0 * metrics.label_padding_x)).abs() < 0.01,
            "expected w_padded={} to equal w_raw={} + 2*pad_x={}",
            w_padded,
            w_raw,
            metrics.label_padding_x,
        );
        assert!(
            (h_padded - (h_raw + 2.0 * metrics.label_padding_y)).abs() < 0.01,
            "expected h_padded={} to equal h_raw={} + 2*pad_y={}",
            h_padded,
            h_raw,
            metrics.label_padding_y,
        );
    }
}
