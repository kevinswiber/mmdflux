//! SVG text measurement utilities.

/// Mermaid default font family for SVG output.
pub const DEFAULT_FONT_FAMILY: &str = "\"trebuchet ms\", verdana, arial, sans-serif";
/// Default font size (px) for SVG output.
pub const DEFAULT_FONT_SIZE: f64 = 16.0;
/// Scale factor applied to approximate Mermaid's measured text widths.
const TEXT_WIDTH_SCALE: f64 = 1.16;

#[derive(Debug, Clone)]
pub struct SvgTextMetrics {
    pub font_size: f64,
    pub line_height: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub label_padding_x: f64,
    pub label_padding_y: f64,
}

impl SvgTextMetrics {
    pub fn new(font_size: f64, node_padding_x: f64, node_padding_y: f64) -> Self {
        Self {
            font_size,
            line_height: font_size * 1.5,
            node_padding_x,
            node_padding_y,
            label_padding_x: 0.0,
            label_padding_y: 0.0,
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

    fn measure_line_width(&self, text: &str) -> f64 {
        text.chars()
            .map(|c| self.char_width_ratio(c) * self.font_size)
            .sum::<f64>()
    }

    fn char_width_ratio(&self, c: char) -> f64 {
        match c {
            'i' | 'l' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' => 0.25,
            'f' | 'j' | 't' | 'r' => 0.32,
            'm' | 'w' | 'M' | 'W' => 0.7,
            'A'..='Z' => 0.48,
            _ => 0.46,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_text_uses_proportional_heuristic() {
        let metrics = SvgTextMetrics::new(16.0, 8.0, 6.4);
        let (w, h) = metrics.measure_text_with_padding("ABC", 0.0, 0.0);

        assert!(w > 16.0);
        assert!(h > 16.0);
    }
}
