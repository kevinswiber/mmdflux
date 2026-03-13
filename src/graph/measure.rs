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
}
