//! Node shape rendering.

use crate::graph::{Direction, Node, Shape};
use crate::render::canvas::Canvas;
use crate::render::chars::CharSet;
use crate::render::intersect::NodeFace;

/// Split a label into lines, returning the lines and maximum content-line width.
///
/// Lines matching `Node::SEPARATOR` are excluded from the width calculation
/// since they span the full box width regardless of content.
fn label_lines(label: &str) -> (Vec<&str>, usize) {
    let lines: Vec<&str> = label.split('\n').collect();
    let max_width = lines
        .iter()
        .filter(|l| **l != Node::SEPARATOR)
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    (lines, max_width)
}

/// Bounding box for a rendered node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeBounds {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    /// Layout-derived center x, avoids integer division rounding.
    pub layout_center_x: Option<usize>,
    /// Layout-derived center y, avoids integer division rounding.
    pub layout_center_y: Option<usize>,
}

impl NodeBounds {
    /// Get the center x coordinate.
    /// Uses the stored layout center if available, otherwise falls back to integer division.
    pub fn center_x(&self) -> usize {
        self.layout_center_x.unwrap_or(self.x + self.width / 2)
    }

    /// Get the center y coordinate.
    /// Uses the stored layout center if available, otherwise falls back to integer division.
    pub fn center_y(&self) -> usize {
        self.layout_center_y.unwrap_or(self.y + self.height / 2)
    }

    /// Check if a point (x, y) falls inside this bounding box.
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Get the top attachment point (center of top edge).
    pub fn top(&self) -> (usize, usize) {
        (self.center_x(), self.y)
    }

    /// Get the bottom attachment point (center of bottom edge).
    pub fn bottom(&self) -> (usize, usize) {
        (self.center_x(), self.y + self.height - 1)
    }

    /// Get the left attachment point (center of left edge).
    pub fn left(&self) -> (usize, usize) {
        (self.x, self.center_y())
    }

    /// Get the right attachment point (center of right edge).
    pub fn right(&self) -> (usize, usize) {
        (self.x + self.width - 1, self.center_y())
    }

    /// Returns the usable range (start, end) along a face for edge attachment.
    /// For Top/Bottom: x-range excluding corner cells (border characters).
    /// For Left/Right: y-range (full height).
    pub fn face_extent(&self, face: &NodeFace) -> (usize, usize) {
        match face {
            NodeFace::Top | NodeFace::Bottom => {
                // Exclude corner columns (first and last chars are corner/bracket chars)
                let start = self.x + 1;
                let end = (self.x + self.width).saturating_sub(2);
                (start, end.max(start))
            }
            NodeFace::Left | NodeFace::Right => {
                // Include full height — corner rows are valid attachment points
                // for horizontal edges entering/exiting side faces. This ensures
                // multiple edges on a side face can be spread apart even on
                // minimum-height (3-cell) nodes.
                let start = self.y;
                let end = self.y + self.height.saturating_sub(1);
                (start, end.max(start))
            }
        }
    }

    /// Returns the fixed coordinate for a face.
    /// Top/Bottom: the y-coordinate of that edge. Left/Right: the x-coordinate.
    pub fn face_fixed_coord(&self, face: &NodeFace) -> usize {
        match face {
            NodeFace::Top => self.y,
            NodeFace::Bottom => self.y + self.height.saturating_sub(1),
            NodeFace::Left => self.x,
            NodeFace::Right => self.x + self.width.saturating_sub(1),
        }
    }
}

/// Corner style for box-shaped nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerStyle {
    /// Sharp 90-degree corners: `┌┐└┘`
    Square,
    /// Rounded corners: `╭╮╰╯`
    Rounded,
}

/// Box modifier flags for special box variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoxModifier {
    /// Double vertical borders (subroutine)
    pub double_vertical: bool,
    /// Curved sides (cylinder)
    pub cylinder_sides: bool,
    /// Wavy bottom edge (document)
    pub wavy_bottom: bool,
    /// Folded corner (card/tagged)
    pub folded_corner: bool,
    /// Shadow offset (stacked docs)
    pub shadow: bool,
}

/// Glyph kinds for single-character nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum GlyphKind {
    SmallCircle,
    FramedCircle,
    CrossedCircle,
}

/// Shape rendering category.
///
/// Shapes are grouped into categories that share rendering logic.
/// This simplifies the render dispatch and makes fallback behavior explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeCategory {
    /// Box with borders and optional corner style/modifier
    Box {
        corners: CornerStyle,
        modifier: BoxModifier,
    },
    /// Diamond/angular shape: `< >`
    Diamond,
    /// No border (text only)
    Borderless,
    /// Single glyph character (for unlabeled nodes)
    Glyph(GlyphKind),
    /// Horizontal bar
    Bar,
}

/// Categorize a shape for rendering.
///
/// Fallback table (explicit):
/// - Stadium, Circle, DoubleCircle -> Rounded box
/// - Hexagon -> Diamond
/// - Documents -> Document (wavy bottom + shadow)
/// - TaggedRect -> Card (folded corner)
/// - Trapezoid, InvTrapezoid, Parallelogram, InvParallelogram, ManualInput, Asymmetric -> Rectangle
pub fn categorize_shape(shape: Shape) -> ShapeCategory {
    match shape {
        Shape::Rectangle => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier::default(),
        },
        Shape::Round | Shape::Stadium | Shape::Circle | Shape::DoubleCircle => ShapeCategory::Box {
            corners: CornerStyle::Rounded,
            modifier: BoxModifier::default(),
        },
        Shape::Subroutine => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                double_vertical: true,
                ..Default::default()
            },
        },
        Shape::Cylinder => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                cylinder_sides: true,
                ..Default::default()
            },
        },
        Shape::Document => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                ..Default::default()
            },
        },
        Shape::Documents => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                shadow: true,
                ..Default::default()
            },
        },
        Shape::TaggedDocument => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                folded_corner: true,
                ..Default::default()
            },
        },
        Shape::Card | Shape::TaggedRect => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier {
                folded_corner: true,
                ..Default::default()
            },
        },
        Shape::Diamond | Shape::Hexagon => ShapeCategory::Diamond,
        Shape::TextBlock => ShapeCategory::Borderless,
        Shape::ForkJoin => ShapeCategory::Bar,
        Shape::SmallCircle => ShapeCategory::Glyph(GlyphKind::SmallCircle),
        Shape::FramedCircle => ShapeCategory::Glyph(GlyphKind::FramedCircle),
        Shape::CrossedCircle => ShapeCategory::Glyph(GlyphKind::CrossedCircle),
        Shape::Trapezoid
        | Shape::InvTrapezoid
        | Shape::Parallelogram
        | Shape::InvParallelogram
        | Shape::ManualInput
        | Shape::Asymmetric => ShapeCategory::Box {
            corners: CornerStyle::Square,
            modifier: BoxModifier::default(),
        },
    }
}

/// Calculate the dimensions needed to render a node.
///
/// Most shapes use: width = label_len + 4 (2 for borders/delimiters, 2 for padding),
/// height = 3 (top border, label row, bottom border).
///
/// ForkJoin bars are perpendicular to the flow direction: horizontal for TD/BT,
/// vertical for LR/RL. When rendered vertically, width and height are swapped.
pub fn node_dimensions(node: &Node, direction: Direction) -> (usize, usize) {
    let (lines, max_line_len) = label_lines(&node.label);
    let (w, h) = (max_line_len + 4, lines.len() + 2);

    // ForkJoin bars without labels are rendered as bars perpendicular to flow.
    // In LR/RL, the bar is vertical so we swap dimensions.
    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        return (h, w);
    }

    (w, h)
}

/// Render a node at the specified position.
///
/// Returns the bounding box of the rendered node.
pub fn render_node(
    canvas: &mut Canvas,
    node: &Node,
    x: usize,
    y: usize,
    charset: &CharSet,
    direction: Direction,
) -> NodeBounds {
    let (width, height) = node_dimensions(node, direction);
    let label = &node.label;
    let label_len = label.chars().count();

    match categorize_shape(node.shape) {
        ShapeCategory::Diamond => {
            render_diamond(canvas, x, y, width, label_len, label, charset);
        }
        ShapeCategory::Box { corners, modifier } => {
            let corners = match corners {
                CornerStyle::Square => (
                    charset.corner_tl,
                    charset.corner_tr,
                    charset.corner_bl,
                    charset.corner_br,
                ),
                CornerStyle::Rounded => (
                    charset.round_tl,
                    charset.round_tr,
                    charset.round_bl,
                    charset.round_br,
                ),
            };
            render_box(
                canvas, x, y, width, height, label, charset, corners, modifier,
            );
        }
        ShapeCategory::Borderless => {
            render_borderless(canvas, x, y, width, height, label);
        }
        ShapeCategory::Glyph(kind) => {
            if label.trim().is_empty() {
                render_glyph(canvas, x, y, width, height, kind, charset);
            } else {
                let corners = (
                    charset.round_tl,
                    charset.round_tr,
                    charset.round_bl,
                    charset.round_br,
                );
                render_box(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    label,
                    charset,
                    corners,
                    BoxModifier::default(),
                );
            }
        }
        ShapeCategory::Bar => {
            if label.trim().is_empty() {
                render_bar(canvas, x, y, width, height, charset, direction);
            } else {
                let corners = (
                    charset.corner_tl,
                    charset.corner_tr,
                    charset.corner_bl,
                    charset.corner_br,
                );
                render_box(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    label,
                    charset,
                    corners,
                    BoxModifier::default(),
                );
            }
        }
    }

    // Mark all cells as part of a node
    for dy in 0..height {
        for dx in 0..width {
            canvas.mark_as_node(x + dx, y + dy);
        }
    }

    NodeBounds {
        x,
        y,
        width,
        height,
        layout_center_x: None,
        layout_center_y: None,
    }
}

/// Render a box shape (rectangle or rounded rectangle).
///
/// The only difference between rectangle and rounded shapes is the corner
/// characters, so this shared function handles both.
#[allow(clippy::too_many_arguments)]
fn render_box(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    label: &str,
    charset: &CharSet,
    corners: (char, char, char, char),
    modifier: BoxModifier,
) {
    let (tl, tr, bl, br) = corners;
    let top_horizontal = charset.horizontal;
    let mut bottom_horizontal = charset.horizontal;
    let mut left_vertical = charset.vertical;
    let mut right_vertical = charset.vertical;
    let mut fold_col = None;

    if modifier.cylinder_sides {
        left_vertical = charset.cylinder_left;
        right_vertical = charset.cylinder_right;
    } else if modifier.double_vertical {
        left_vertical = charset.double_vertical;
        right_vertical = charset.double_vertical;
    }
    if modifier.wavy_bottom {
        bottom_horizontal = charset.wavy_horizontal;
    }
    if modifier.folded_corner && width > 2 {
        fold_col = Some(x + width - 2);
    }
    if modifier.shadow {
        render_shadow_box(canvas, x + 1, y + 1, width, height, charset, corners);
    }

    // Top border
    canvas.set(x, y, tl);
    for dx in 1..width - 1 {
        let ch = if fold_col == Some(x + dx) {
            charset.fold_corner
        } else {
            top_horizontal
        };
        canvas.set(x + dx, y, ch);
    }
    canvas.set(x + width - 1, y, tr);

    // Content rows
    let lines: Vec<&str> = label.split('\n').collect();
    let first_separator_idx = lines.iter().position(|line| *line == Node::SEPARATOR);
    let second_separator_idx = first_separator_idx.and_then(|first| {
        lines[first + 1..]
            .iter()
            .position(|line| *line == Node::SEPARATOR)
            .map(|offset| first + 1 + offset)
    });
    if lines.len() <= 1 {
        // Single-line: centered
        let mid_y = y + height / 2;
        canvas.set(x, mid_y, left_vertical);
        let label_start = x + (width - label.chars().count()) / 2;
        canvas.write_str(label_start, mid_y, label);
        canvas.set(x + width - 1, mid_y, right_vertical);
    } else {
        // Multi-line: by default left-aligned with padding.
        // For compartment-style labels (`---` separators), center title/attributes
        // rows and keep operation rows left-aligned.
        for (i, line) in lines.iter().enumerate() {
            let row_y = y + 1 + i;
            if *line == Node::SEPARATOR {
                canvas.set(x, row_y, charset.tee_right);
                for dx in 1..width - 1 {
                    canvas.set(x + dx, row_y, top_horizontal);
                }
                canvas.set(x + width - 1, row_y, charset.tee_left);
            } else {
                canvas.set(x, row_y, left_vertical);
                let center_line = if let Some(first_sep) = first_separator_idx {
                    if let Some(second_sep) = second_separator_idx {
                        i < second_sep
                    } else {
                        i < first_sep
                    }
                } else {
                    false
                };
                let label_start = if center_line {
                    x + (width - line.chars().count()) / 2
                } else {
                    x + 2
                };
                canvas.write_str(label_start, row_y, line);
                canvas.set(x + width - 1, row_y, right_vertical);
            }
        }
    };

    // Bottom border
    let bot_y = y + height - 1;
    canvas.set(x, bot_y, bl);
    for dx in 1..width - 1 {
        canvas.set(x + dx, bot_y, bottom_horizontal);
    }
    canvas.set(x + width - 1, bot_y, br);
}

fn render_shadow_box(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    charset: &CharSet,
    corners: (char, char, char, char),
) {
    let (_tl, _tr, _bl, br) = corners;
    let bottom_horizontal = charset.horizontal;
    let right_x = x + width - 1;
    let bot_y = y + height - 1;

    // Right edge only (shadow)
    for dy in 0..height {
        canvas.set(right_x, y + dy, charset.vertical);
    }

    // Bottom edge only (shadow)
    for dx in 0..width {
        canvas.set(x + dx, bot_y, bottom_horizontal);
    }

    canvas.set(right_x, bot_y, br);
}

/// Render a borderless text block (label only).
fn render_borderless(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    label: &str,
) {
    let mid_y = y + height / 2;
    let label_len = label.chars().count();
    if label_len == 0 {
        return;
    }
    let label_start = x + (width - label_len) / 2;
    canvas.write_str(label_start, mid_y, label);
}

/// Render a bar (fork/join), perpendicular to flow direction.
fn render_bar(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    charset: &CharSet,
    direction: Direction,
) {
    if matches!(direction, Direction::LeftRight | Direction::RightLeft) {
        // Vertical bar for horizontal flow
        let mid_x = x + width / 2;
        for dy in 0..height {
            canvas.set(mid_x, y + dy, charset.heavy_vertical);
        }
    } else {
        // Horizontal bar for vertical flow
        let mid_y = y + height / 2;
        for dx in 0..width {
            canvas.set(x + dx, mid_y, charset.heavy_horizontal);
        }
    }
}

/// Render a glyph node (single character or short string).
fn render_glyph(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    kind: GlyphKind,
    charset: &CharSet,
) {
    let glyph = match kind {
        GlyphKind::SmallCircle => charset.glyph_small_circle,
        GlyphKind::FramedCircle => charset.glyph_framed_circle,
        GlyphKind::CrossedCircle => charset.glyph_crossed_circle,
    };
    let glyph_len = glyph.chars().count();
    let mid_y = y + height / 2;
    let start_x = x + (width.saturating_sub(glyph_len)) / 2;
    canvas.write_str(start_x, mid_y, glyph);
}

/// Render a diamond shape.
///
/// Rendered as a rectangle with angle brackets on the sides:
/// ```text
/// ┌───────────┐
/// < Christmas >
/// └───────────┘
/// ```
fn render_diamond(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    label_len: usize,
    label: &str,
    charset: &CharSet,
) {
    // Top border
    canvas.set(x, y, charset.corner_tl);
    for dx in 1..width - 1 {
        canvas.set(x + dx, y, charset.horizontal);
    }
    canvas.set(x + width - 1, y, charset.corner_tr);

    // Middle row with label and angle brackets
    let mid_y = y + 1;
    canvas.set(x, mid_y, '<');
    let label_start = x + (width - label_len) / 2;
    canvas.write_str(label_start, mid_y, label);
    canvas.set(x + width - 1, mid_y, '>');

    // Bottom border
    let bot_y = y + 2;
    canvas.set(x, bot_y, charset.corner_bl);
    for dx in 1..width - 1 {
        canvas.set(x + dx, bot_y, charset.horizontal);
    }
    canvas.set(x + width - 1, bot_y, charset.corner_br);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_dimensions_rectangle() {
        let node = Node::new("A").with_label("Start");
        let (w, h) = node_dimensions(&node, Direction::TopDown);
        // "Start" is 5 chars, +4 = 9 width
        assert_eq!(w, 9);
        assert_eq!(h, 3);
    }

    #[test]
    fn test_node_dimensions_round() {
        let node = Node::new("B")
            .with_label("Process")
            .with_shape(Shape::Round);
        let (w, h) = node_dimensions(&node, Direction::TopDown);
        // "Process" is 7 chars, +4 = 11 width
        assert_eq!(w, 11);
        assert_eq!(h, 3);
    }

    #[test]
    fn test_node_dimensions_diamond() {
        let node = Node::new("C").with_label("Yes").with_shape(Shape::Diamond);
        let (w, h) = node_dimensions(&node, Direction::TopDown);
        // "Yes" is 3 chars, +4 = 7 width
        assert_eq!(w, 7);
        assert_eq!(h, 3);
    }

    #[test]
    fn test_render_rectangle() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("A").with_label("Start");
        let charset = CharSet::unicode();

        let bounds = render_node(&mut canvas, &node, 2, 1, &charset, Direction::TopDown);

        assert_eq!(bounds.x, 2);
        assert_eq!(bounds.y, 1);
        assert_eq!(bounds.width, 9);
        assert_eq!(bounds.height, 3);

        let output = canvas.to_string();
        assert!(output.contains("┌───────┐"));
        assert!(output.contains("│ Start │"));
        assert!(output.contains("└───────┘"));
    }

    #[test]
    fn test_render_rectangle_ascii() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("A").with_label("Start");
        let charset = CharSet::ascii();

        render_node(&mut canvas, &node, 2, 1, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(output.contains("+-------+"));
        assert!(output.contains("| Start |"));
    }

    #[test]
    fn test_render_round() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("B").with_label("Hi").with_shape(Shape::Round);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 2, 1, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(output.contains("╭────╮"));
        assert!(output.contains("│ Hi │"));
        assert!(output.contains("╰────╯"));
    }

    #[test]
    fn test_render_diamond() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("C").with_label("?").with_shape(Shape::Diamond);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 2, 1, &charset, Direction::TopDown);

        let output = canvas.to_string();
        assert!(output.contains("┌───┐"));
        assert!(output.contains("< ? >"));
        assert!(output.contains("└───┘"));
    }

    #[test]
    fn test_render_diamond_wide() {
        let mut canvas = Canvas::new(20, 5);
        let node = Node::new("B")
            .with_label("Decision")
            .with_shape(Shape::Diamond);
        let charset = CharSet::unicode();

        let bounds = render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);

        assert_eq!(bounds.width, 12);
        assert_eq!(bounds.height, 3);

        let output = canvas.to_string();
        assert!(output.contains("┌──────────┐"));
        assert!(output.contains("< Decision >"));
        assert!(output.contains("└──────────┘"));
    }

    #[test]
    fn test_categorize_shape_fallbacks() {
        if let ShapeCategory::Box { corners, modifier } = categorize_shape(Shape::Rectangle) {
            assert_eq!(corners, CornerStyle::Square);
            assert_eq!(modifier, BoxModifier::default());
        } else {
            panic!("Rectangle should be Box");
        }

        if let ShapeCategory::Box { corners, modifier } = categorize_shape(Shape::Round) {
            assert_eq!(corners, CornerStyle::Rounded);
            assert_eq!(modifier, BoxModifier::default());
        } else {
            panic!("Round should be Box");
        }

        assert!(matches!(
            categorize_shape(Shape::Diamond),
            ShapeCategory::Diamond
        ));
        assert!(matches!(
            categorize_shape(Shape::SmallCircle),
            ShapeCategory::Glyph(GlyphKind::SmallCircle)
        ));
        assert!(matches!(
            categorize_shape(Shape::TextBlock),
            ShapeCategory::Borderless
        ));
        assert!(matches!(
            categorize_shape(Shape::ForkJoin),
            ShapeCategory::Bar
        ));
        for shape in [
            Shape::Trapezoid,
            Shape::InvTrapezoid,
            Shape::Parallelogram,
            Shape::InvParallelogram,
            Shape::ManualInput,
            Shape::Asymmetric,
        ] {
            if let ShapeCategory::Box { corners, modifier } = categorize_shape(shape) {
                assert_eq!(corners, CornerStyle::Square);
                assert_eq!(modifier, BoxModifier::default());
            } else {
                panic!("{shape:?} should be Box fallback");
            }
        }
    }

    #[test]
    fn test_render_subroutine_double_vertical() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("S")
            .with_label("Sub")
            .with_shape(Shape::Subroutine);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains("║ Sub ║"));
    }

    #[test]
    fn test_render_document_wavy_bottom() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("D").with_label("Doc").with_shape(Shape::Document);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains("~"));
    }

    #[test]
    fn test_render_tagged_document_fold_and_wavy() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("T")
            .with_label("Tag")
            .with_shape(Shape::TaggedDocument);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains("~"));
        assert!(output.contains(charset.fold_corner));
    }

    #[test]
    fn test_render_documents_shadow_offset() {
        let mut canvas = Canvas::new(16, 7);
        let node = Node::new("D")
            .with_label("Docs")
            .with_shape(Shape::Documents);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let shadow_cell = canvas.get(9, 4).unwrap().ch;
        assert_eq!(shadow_cell, charset.corner_br);
    }

    #[test]
    fn test_render_small_circle_glyph_unlabeled() {
        let mut canvas = Canvas::new(7, 5);
        let node = Node::new("J").with_label("").with_shape(Shape::SmallCircle);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains(charset.glyph_small_circle));
    }

    #[test]
    fn test_render_small_circle_with_label_falls_back_to_round() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("J")
            .with_label("Hub")
            .with_shape(Shape::SmallCircle);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains("╭"));
        assert!(output.contains("╯"));
    }

    #[test]
    fn test_render_fork_join_bar() {
        let mut canvas = Canvas::new(10, 5);
        let node = Node::new("F").with_label("").with_shape(Shape::ForkJoin);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();
        assert!(output.contains(charset.heavy_horizontal));
    }

    #[test]
    fn test_render_compartment_multiline_centers_header_and_attributes() {
        let mut canvas = Canvas::new(20, 8);
        let label = format!("Header\n{}\nA\n{}\nm()", Node::SEPARATOR, Node::SEPARATOR);
        let node = Node::new("C").with_label(label);
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();

        // Title and attributes are centered in text mode.
        assert!(output.contains("│ Header │"));
        assert!(output.contains("│   A    │"));
        // Operations remain left-aligned.
        assert!(output.contains("│ m()    │"));
    }

    #[test]
    fn test_render_multiline_without_separator_remains_left_aligned() {
        let mut canvas = Canvas::new(20, 6);
        let node = Node::new("C").with_label("Long\nx");
        let charset = CharSet::unicode();

        render_node(&mut canvas, &node, 1, 1, &charset, Direction::TopDown);
        let output = canvas.to_string();

        // Non-compartment multiline labels keep left padding alignment.
        assert!(output.contains("│ Long │"));
        assert!(output.contains("│ x    │"));
    }

    #[test]
    fn test_node_bounds_attachment_points() {
        let bounds = NodeBounds {
            x: 10,
            y: 5,
            width: 8,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };

        assert_eq!(bounds.center_x(), 14); // 10 + 8/2 = 14
        assert_eq!(bounds.center_y(), 6); // 5 + 3/2 = 6

        assert_eq!(bounds.top(), (14, 5));
        assert_eq!(bounds.bottom(), (14, 7)); // y + height - 1 = 5 + 3 - 1 = 7
        assert_eq!(bounds.left(), (10, 6));
        assert_eq!(bounds.right(), (17, 6)); // x + width - 1 = 10 + 8 - 1 = 17
    }

    #[test]
    fn test_face_extent_top_bottom() {
        let bounds = NodeBounds {
            x: 5,
            y: 10,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        // Top/Bottom: exclude corners => x+1 to x+width-2 = 6 to 13
        assert_eq!(bounds.face_extent(&NodeFace::Top), (6, 13));
        assert_eq!(bounds.face_extent(&NodeFace::Bottom), (6, 13));
    }

    #[test]
    fn test_face_extent_left_right() {
        let bounds = NodeBounds {
            x: 5,
            y: 10,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        // Left/Right: full height => 10 to 12
        assert_eq!(bounds.face_extent(&NodeFace::Left), (10, 12));
        assert_eq!(bounds.face_extent(&NodeFace::Right), (10, 12));
    }

    #[test]
    fn test_face_extent_narrow_node() {
        let bounds = NodeBounds {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            layout_center_x: None,
            layout_center_y: None,
        };
        // width=2: start=1, end=max(0,1)=1 => (1, 1)
        assert_eq!(bounds.face_extent(&NodeFace::Top), (1, 1));
    }

    #[test]
    fn test_face_fixed_coord() {
        let bounds = NodeBounds {
            x: 5,
            y: 10,
            width: 10,
            height: 3,
            layout_center_x: None,
            layout_center_y: None,
        };
        assert_eq!(bounds.face_fixed_coord(&NodeFace::Top), 10);
        assert_eq!(bounds.face_fixed_coord(&NodeFace::Bottom), 12);
        assert_eq!(bounds.face_fixed_coord(&NodeFace::Left), 5);
        assert_eq!(bounds.face_fixed_coord(&NodeFace::Right), 14);
    }

    #[test]
    fn test_node_cells_marked_as_protected() {
        let mut canvas = Canvas::new(15, 5);
        let node = Node::new("A").with_label("X");
        let charset = CharSet::unicode();

        let bounds = render_node(&mut canvas, &node, 2, 1, &charset, Direction::TopDown);

        // Check that cells within the node bounds are marked as protected
        for dy in 0..bounds.height {
            for dx in 0..bounds.width {
                let cell = canvas.get(bounds.x + dx, bounds.y + dy).unwrap();
                assert!(
                    cell.is_node,
                    "Cell at ({}, {}) should be marked as node",
                    dx, dy
                );
            }
        }
    }
}
