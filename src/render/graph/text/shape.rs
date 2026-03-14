//! Node and glyph drawing for graph text output.

use crate::graph::grid::NodeBounds;
use crate::graph::measure::grid_node_dimensions;
use crate::graph::{Direction, Node, Shape};
use crate::render::text::canvas::{Canvas, CellStyle};
use crate::render::text::chars::CharSet;

/// Corner style for text node boxes.
///
/// This is intentionally node-shape-local and distinct from SVG edge corner styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCornerStyle {
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
        corners: NodeCornerStyle,
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
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier::default(),
        },
        Shape::Round | Shape::Stadium | Shape::Circle | Shape::DoubleCircle => ShapeCategory::Box {
            corners: NodeCornerStyle::Rounded,
            modifier: BoxModifier::default(),
        },
        Shape::Subroutine => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier {
                double_vertical: true,
                ..Default::default()
            },
        },
        Shape::Cylinder => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier {
                cylinder_sides: true,
                ..Default::default()
            },
        },
        Shape::Document => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                ..Default::default()
            },
        },
        Shape::Documents => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                shadow: true,
                ..Default::default()
            },
        },
        Shape::TaggedDocument => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
            modifier: BoxModifier {
                wavy_bottom: true,
                folded_corner: true,
                ..Default::default()
            },
        },
        Shape::Card | Shape::TaggedRect => ShapeCategory::Box {
            corners: NodeCornerStyle::Square,
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
            corners: NodeCornerStyle::Square,
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
    grid_node_dimensions(node, direction)
}

#[derive(Debug, Clone, Copy, Default)]
struct ResolvedTextNodeStyle {
    fill: Option<(u8, u8, u8)>,
    stroke: Option<(u8, u8, u8)>,
    color: Option<(u8, u8, u8)>,
}

impl ResolvedTextNodeStyle {
    fn from_node(node: &Node) -> Self {
        Self {
            fill: node.style.fill.as_ref().and_then(|color| color.to_rgb()),
            stroke: node.style.stroke.as_ref().and_then(|color| color.to_rgb()),
            color: node.style.color.as_ref().and_then(|color| color.to_rgb()),
        }
    }
}

fn merge_fg(canvas: &mut Canvas, x: usize, y: usize, rgb: Option<(u8, u8, u8)>) {
    if let Some((r, g, b)) = rgb {
        canvas.merge_style(x, y, CellStyle::fg_rgb(r, g, b));
    }
}

fn merge_bg_span(
    canvas: &mut Canvas,
    start_x: usize,
    end_x: usize,
    y: usize,
    rgb: Option<(u8, u8, u8)>,
) {
    let Some((r, g, b)) = rgb else {
        return;
    };

    for x in start_x..end_x {
        canvas.merge_style(x, y, CellStyle::bg_rgb(r, g, b));
    }
}

fn merge_text_fg(
    canvas: &mut Canvas,
    start_x: usize,
    y: usize,
    text: &str,
    rgb: Option<(u8, u8, u8)>,
) {
    let Some((r, g, b)) = rgb else {
        return;
    };

    for (offset, _) in text.chars().enumerate() {
        canvas.merge_style(start_x + offset, y, CellStyle::fg_rgb(r, g, b));
    }
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
    let style = ResolvedTextNodeStyle::from_node(node);

    match categorize_shape(node.shape) {
        ShapeCategory::Diamond => {
            render_diamond(canvas, x, y, width, label_len, label, charset, style);
        }
        ShapeCategory::Box { corners, modifier } => {
            let corners = match corners {
                NodeCornerStyle::Square => (
                    charset.corner_tl,
                    charset.corner_tr,
                    charset.corner_bl,
                    charset.corner_br,
                ),
                NodeCornerStyle::Rounded => (
                    charset.round_tl,
                    charset.round_tr,
                    charset.round_bl,
                    charset.round_br,
                ),
            };
            render_box(
                canvas, x, y, width, height, label, charset, corners, modifier, style,
            );
        }
        ShapeCategory::Borderless => {
            render_borderless(canvas, x, y, width, height, label, style);
        }
        ShapeCategory::Glyph(kind) => {
            if label.trim().is_empty() {
                render_glyph(canvas, x, y, width, height, kind, charset, style);
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
                    style,
                );
            }
        }
        ShapeCategory::Bar => {
            if label.trim().is_empty() {
                render_bar(canvas, x, y, width, height, charset, direction, style);
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
                    style,
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
    style: ResolvedTextNodeStyle,
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
        render_shadow_box(canvas, x + 1, y + 1, width, height, charset, corners, style);
    }

    // Top border
    canvas.set(x, y, tl);
    merge_fg(canvas, x, y, style.stroke);
    for dx in 1..width - 1 {
        let ch = if fold_col == Some(x + dx) {
            charset.fold_corner
        } else {
            top_horizontal
        };
        canvas.set(x + dx, y, ch);
        merge_fg(canvas, x + dx, y, style.stroke);
    }
    canvas.set(x + width - 1, y, tr);
    merge_fg(canvas, x + width - 1, y, style.stroke);

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
        merge_fg(canvas, x, mid_y, style.stroke);
        merge_bg_span(canvas, x + 1, x + width - 1, mid_y, style.fill);
        let label_start = x + (width - label.chars().count()) / 2;
        canvas.write_str(label_start, mid_y, label);
        merge_text_fg(canvas, label_start, mid_y, label, style.color);
        canvas.set(x + width - 1, mid_y, right_vertical);
        merge_fg(canvas, x + width - 1, mid_y, style.stroke);
    } else {
        // Multi-line: by default left-aligned with padding.
        // For compartment-style labels (`---` separators), center title/attributes
        // rows and keep operation rows left-aligned.
        for (i, line) in lines.iter().enumerate() {
            let row_y = y + 1 + i;
            if *line == Node::SEPARATOR {
                canvas.set(x, row_y, charset.tee_right);
                merge_fg(canvas, x, row_y, style.stroke);
                for dx in 1..width - 1 {
                    canvas.set(x + dx, row_y, top_horizontal);
                    merge_fg(canvas, x + dx, row_y, style.stroke);
                }
                canvas.set(x + width - 1, row_y, charset.tee_left);
                merge_fg(canvas, x + width - 1, row_y, style.stroke);
            } else {
                canvas.set(x, row_y, left_vertical);
                merge_fg(canvas, x, row_y, style.stroke);
                merge_bg_span(canvas, x + 1, x + width - 1, row_y, style.fill);
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
                merge_text_fg(canvas, label_start, row_y, line, style.color);
                canvas.set(x + width - 1, row_y, right_vertical);
                merge_fg(canvas, x + width - 1, row_y, style.stroke);
            }
        }
    };

    // Bottom border
    let bot_y = y + height - 1;
    canvas.set(x, bot_y, bl);
    merge_fg(canvas, x, bot_y, style.stroke);
    for dx in 1..width - 1 {
        canvas.set(x + dx, bot_y, bottom_horizontal);
        merge_fg(canvas, x + dx, bot_y, style.stroke);
    }
    canvas.set(x + width - 1, bot_y, br);
    merge_fg(canvas, x + width - 1, bot_y, style.stroke);
}

#[allow(clippy::too_many_arguments)]
fn render_shadow_box(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    charset: &CharSet,
    corners: (char, char, char, char),
    style: ResolvedTextNodeStyle,
) {
    let (_tl, _tr, _bl, br) = corners;
    let bottom_horizontal = charset.horizontal;
    let right_x = x + width - 1;
    let bot_y = y + height - 1;

    // Right edge only (shadow)
    for dy in 0..height {
        canvas.set(right_x, y + dy, charset.vertical);
        merge_fg(canvas, right_x, y + dy, style.stroke);
    }

    // Bottom edge only (shadow)
    for dx in 0..width {
        canvas.set(x + dx, bot_y, bottom_horizontal);
        merge_fg(canvas, x + dx, bot_y, style.stroke);
    }

    canvas.set(right_x, bot_y, br);
    merge_fg(canvas, right_x, bot_y, style.stroke);
}

/// Render a borderless text block (label only).
fn render_borderless(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    label: &str,
    style: ResolvedTextNodeStyle,
) {
    let mid_y = y + height / 2;
    let label_len = label.chars().count();
    if label_len == 0 {
        return;
    }
    let label_start = x + (width - label_len) / 2;
    canvas.write_str(label_start, mid_y, label);
    merge_text_fg(canvas, label_start, mid_y, label, style.color);
}

/// Render a bar (fork/join), perpendicular to flow direction.
#[allow(clippy::too_many_arguments)]
fn render_bar(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    charset: &CharSet,
    direction: Direction,
    style: ResolvedTextNodeStyle,
) {
    if matches!(direction, Direction::LeftRight | Direction::RightLeft) {
        // Vertical bar for horizontal flow
        let mid_x = x + width / 2;
        for dy in 0..height {
            canvas.set(mid_x, y + dy, charset.heavy_vertical);
            merge_fg(canvas, mid_x, y + dy, style.stroke);
        }
    } else {
        // Horizontal bar for vertical flow
        let mid_y = y + height / 2;
        for dx in 0..width {
            canvas.set(x + dx, mid_y, charset.heavy_horizontal);
            merge_fg(canvas, x + dx, mid_y, style.stroke);
        }
    }
}

/// Render a glyph node (single character or short string).
#[allow(clippy::too_many_arguments)]
fn render_glyph(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    kind: GlyphKind,
    charset: &CharSet,
    style: ResolvedTextNodeStyle,
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
    merge_text_fg(canvas, start_x, mid_y, glyph, style.stroke);
}

/// Render a diamond shape.
///
/// Rendered as a rectangle with angle brackets on the sides:
/// ```text
/// ┌───────────┐
/// < Christmas >
/// └───────────┘
/// ```
#[allow(clippy::too_many_arguments)]
fn render_diamond(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    width: usize,
    label_len: usize,
    label: &str,
    charset: &CharSet,
    style: ResolvedTextNodeStyle,
) {
    // Top border
    canvas.set(x, y, charset.corner_tl);
    merge_fg(canvas, x, y, style.stroke);
    for dx in 1..width - 1 {
        canvas.set(x + dx, y, charset.horizontal);
        merge_fg(canvas, x + dx, y, style.stroke);
    }
    canvas.set(x + width - 1, y, charset.corner_tr);
    merge_fg(canvas, x + width - 1, y, style.stroke);

    // Middle row with label and angle brackets
    let mid_y = y + 1;
    canvas.set(x, mid_y, '<');
    merge_fg(canvas, x, mid_y, style.stroke);
    merge_bg_span(canvas, x + 1, x + width - 1, mid_y, style.fill);
    let label_start = x + (width - label_len) / 2;
    canvas.write_str(label_start, mid_y, label);
    merge_text_fg(canvas, label_start, mid_y, label, style.color);
    canvas.set(x + width - 1, mid_y, '>');
    merge_fg(canvas, x + width - 1, mid_y, style.stroke);

    // Bottom border
    let bot_y = y + 2;
    canvas.set(x, bot_y, charset.corner_bl);
    merge_fg(canvas, x, bot_y, style.stroke);
    for dx in 1..width - 1 {
        canvas.set(x + dx, bot_y, charset.horizontal);
        merge_fg(canvas, x + dx, bot_y, style.stroke);
    }
    canvas.set(x + width - 1, bot_y, charset.corner_br);
    merge_fg(canvas, x + width - 1, bot_y, style.stroke);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::grid::{NodeFace, face_extent, face_fixed_coord};

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
            assert_eq!(corners, NodeCornerStyle::Square);
            assert_eq!(modifier, BoxModifier::default());
        } else {
            panic!("Rectangle should be Box");
        }

        if let ShapeCategory::Box { corners, modifier } = categorize_shape(Shape::Round) {
            assert_eq!(corners, NodeCornerStyle::Rounded);
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
                assert_eq!(corners, NodeCornerStyle::Square);
                assert_eq!(modifier, BoxModifier::default());
            } else {
                panic!("{shape:?} should be Box fallback");
            }
        }
    }

    #[test]
    fn categorize_shape_keeps_rounded_box_for_round_family() {
        let category = categorize_shape(Shape::Round);
        assert!(matches!(
            category,
            ShapeCategory::Box {
                corners: NodeCornerStyle::Rounded,
                ..
            }
        ));
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
        assert_eq!(face_extent(&bounds, &NodeFace::Top), (6, 13));
        assert_eq!(face_extent(&bounds, &NodeFace::Bottom), (6, 13));
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
        assert_eq!(face_extent(&bounds, &NodeFace::Left), (10, 12));
        assert_eq!(face_extent(&bounds, &NodeFace::Right), (10, 12));
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
        assert_eq!(face_extent(&bounds, &NodeFace::Top), (1, 1));
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
        assert_eq!(face_fixed_coord(&bounds, &NodeFace::Top), 10);
        assert_eq!(face_fixed_coord(&bounds, &NodeFace::Bottom), 12);
        assert_eq!(face_fixed_coord(&bounds, &NodeFace::Left), 5);
        assert_eq!(face_fixed_coord(&bounds, &NodeFace::Right), 14);
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
