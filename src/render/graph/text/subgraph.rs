//! Subgraph border drawing for graph text output.

use std::collections::HashMap;

use crate::graph::grid::SubgraphBounds;
use crate::render::text::canvas::Canvas;
use crate::render::text::chars::CharSet;

/// Render subgraph border rectangles on the canvas.
///
/// Draws borders BEFORE nodes and edges so they appear in the background.
/// Cells are marked as `is_subgraph_border` (not protected from overwrite).
/// Title is placed above the top-left corner of the border.
pub fn render_subgraph_borders(
    canvas: &mut Canvas,
    subgraph_bounds: &HashMap<String, SubgraphBounds>,
    charset: &CharSet,
) {
    // Sort by depth: outer borders first (background), inner last (foreground)
    let mut sorted_bounds: Vec<_> = subgraph_bounds.values().collect();
    sorted_bounds.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.title.cmp(&b.title)));

    for bounds in sorted_bounds {
        let x = bounds.x;
        let y = bounds.y;
        let w = bounds.width;
        let h = bounds.height;

        // Need at least 2×2 to draw a border box (corners only)
        if w < 2 || h < 2 {
            continue;
        }

        // Top edge with embedded title: ┌─ Title ─┐
        canvas.set_subgraph_border(x, y, charset.corner_tl);
        canvas.set_subgraph_border(x + w - 1, y, charset.corner_tr);

        let inner_width = w.saturating_sub(2); // space between corners
        let has_visible_title = !bounds.title.is_empty() && !bounds.title.trim().is_empty();
        if has_visible_title && inner_width >= 5 {
            // Title section: "─ Title ─" = title.len() + 4 chars overhead
            let max_title_len = inner_width.saturating_sub(4);
            let title: String = bounds.title.chars().take(max_title_len).collect();
            let title_section_len = title.len() + 4; // "─ " + title + " ─"
            let left_fill = (inner_width.saturating_sub(title_section_len)) / 2;

            // Left horizontal fill
            for i in 0..left_fill {
                canvas.set_subgraph_border(x + 1 + i, y, charset.horizontal);
            }
            // "─ " prefix
            canvas.set_subgraph_border(x + 1 + left_fill, y, charset.horizontal);
            canvas.set_subgraph_border(x + 1 + left_fill + 1, y, ' ');
            // Centered title
            let title_start = x + 1 + left_fill + 2;
            for (i, ch) in title.chars().enumerate() {
                canvas.set_subgraph_title_char(title_start + i, y, ch);
            }
            // " " suffix
            let title_end = title_start + title.len();
            canvas.set_subgraph_border(title_end, y, ' ');

            // Right horizontal fill
            for dx in (title_end + 1)..(x + w - 1) {
                canvas.set_subgraph_border(dx, y, charset.horizontal);
            }

            // Protect the entire top border row so edges cannot corrupt
            // the embedded title segment.
            for dx in 1..(w - 1) {
                if let Some(cell) = canvas.get(x + dx, y) {
                    let _ = canvas.set_subgraph_title_char(x + dx, y, cell.ch);
                }
            }
        } else {
            // No title or too narrow: plain horizontal
            for dx in 1..(w - 1) {
                canvas.set_subgraph_border(x + dx, y, charset.horizontal);
            }
        }

        // Sides
        for dy in 1..h - 1 {
            canvas.set_subgraph_border(x, y + dy, charset.vertical);
            canvas.set_subgraph_border(x + w - 1, y + dy, charset.vertical);
        }

        // Bottom edge
        canvas.set_subgraph_border(x, y + h - 1, charset.corner_bl);
        for dx in 1..w - 1 {
            canvas.set_subgraph_border(x + dx, y + h - 1, charset.horizontal);
        }
        canvas.set_subgraph_border(x + w - 1, y + h - 1, charset.corner_br);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_subgraph_border_characters() {
        let mut canvas = Canvas::new(20, 10);
        let bounds = SubgraphBounds {
            x: 2,
            y: 3,
            width: 13,
            height: 5,
            title: "Group".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);
        let charset = CharSet::unicode();

        render_subgraph_borders(&mut canvas, &map, &charset);

        // Verify corners
        assert_eq!(canvas.get(2, 3).unwrap().ch, charset.corner_tl);
        assert_eq!(canvas.get(14, 3).unwrap().ch, charset.corner_tr);
        assert_eq!(canvas.get(2, 7).unwrap().ch, charset.corner_bl);
        assert_eq!(canvas.get(14, 7).unwrap().ch, charset.corner_br);

        // Verify centered title in top border: ┌── Group ──┐
        // left_fill=1: x+1=3 → '─', x+2=4 → '─' (prefix), x+3=5 → ' ',
        // x+4..x+8=6..10 → "Group", x+9=11 → ' ', x+10..x+11=12..13 → '─'
        assert_eq!(canvas.get(3, 3).unwrap().ch, charset.horizontal);
        assert_eq!(canvas.get(4, 3).unwrap().ch, charset.horizontal);
        assert_eq!(canvas.get(5, 3).unwrap().ch, ' ');
        assert_eq!(canvas.get(6, 3).unwrap().ch, 'G');
        assert_eq!(canvas.get(10, 3).unwrap().ch, 'p');
        assert_eq!(canvas.get(11, 3).unwrap().ch, ' ');

        // Verify vertical edges
        assert_eq!(canvas.get(2, 5).unwrap().ch, charset.vertical);

        // Verify is_subgraph_border flag
        assert!(canvas.get(2, 3).unwrap().is_subgraph_border);
    }

    #[test]
    fn test_render_subgraph_title_embedded_in_border() {
        let mut canvas = Canvas::new(20, 10);
        let bounds = SubgraphBounds {
            x: 2,
            y: 3,
            width: 13,
            height: 5,
            title: "Group".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &CharSet::unicode());

        // Title centered in top border row (y=3), not above it
        assert_eq!(canvas.get(6, 3).unwrap().ch, 'G');
        assert_eq!(canvas.get(7, 3).unwrap().ch, 'r');
        assert_eq!(canvas.get(8, 3).unwrap().ch, 'o');
        assert_eq!(canvas.get(9, 3).unwrap().ch, 'u');
        assert_eq!(canvas.get(10, 3).unwrap().ch, 'p');

        // Row above border should NOT have the title
        assert_ne!(canvas.get(5, 2).unwrap().ch, 'G');
    }

    #[test]
    fn test_render_subgraph_ascii_mode() {
        let mut canvas = Canvas::new(20, 10);
        let bounds = SubgraphBounds {
            x: 2,
            y: 3,
            width: 10,
            height: 5,
            title: "Test".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &CharSet::ascii());

        assert_eq!(canvas.get(2, 3).unwrap().ch, '+');
        // ASCII mode: embedded title in top border
        assert_eq!(canvas.get(3, 3).unwrap().ch, '-'); // prefix dash
        assert_eq!(canvas.get(5, 3).unwrap().ch, 'T'); // title start
        assert_eq!(canvas.get(2, 5).unwrap().ch, '|');
    }

    // =========================================================================
    // Embedded Title Tests (Plan 0026, Task 2.1)
    // =========================================================================

    #[test]
    fn test_render_subgraph_embedded_title() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(20, 7);
        let bounds = SubgraphBounds {
            x: 2,
            y: 2,
            width: 14,
            height: 5,
            title: "Group".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &charset);

        let output = canvas.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // First line (after empty row trimming) is the top border with embedded title
        assert!(
            lines[0].contains("─ Group ─"),
            "Expected embedded title in top border, got: {}",
            lines[0]
        );

        // Side rows should NOT contain title text
        assert!(
            !lines[1].contains("Group"),
            "Title should not appear in side row, got: {}",
            lines[1]
        );
    }

    #[test]
    fn test_render_subgraph_title_at_y0() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(20, 7);
        let bounds = SubgraphBounds {
            x: 0,
            y: 0,
            width: 16,
            height: 5,
            title: "TopGroup".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &charset);

        let output = canvas.to_string();
        let lines: Vec<&str> = output.lines().collect();

        // Title should be visible even at y=0 (embedded in border)
        assert!(
            lines[0].contains("TopGroup"),
            "Title should render at y=0, got: {}",
            lines[0]
        );
    }

    #[test]
    fn test_render_subgraph_narrow_border_truncates_title() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(15, 5);
        let bounds = SubgraphBounds {
            x: 0,
            y: 0,
            width: 8,
            height: 5,
            title: "Very Long Title".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &charset);

        let output = canvas.to_string();
        // Title should be truncated to fit within border
        assert!(
            !output.contains("Very Long Title"),
            "Full title should not appear in narrow border"
        );
        // Border corners should still be intact
        assert!(output.contains("┌"), "Top-left corner should exist");
        assert!(output.contains("┐"), "Top-right corner should exist");
    }

    #[test]
    fn test_render_subgraph_whitespace_title_renders_no_title() {
        let charset = CharSet::unicode();
        let mut canvas = Canvas::new(20, 7);
        let bounds = SubgraphBounds {
            x: 2,
            y: 2,
            width: 14,
            height: 5,
            title: " ".to_string(),
            depth: 0,
        };
        let mut map = HashMap::new();
        map.insert("sg1".to_string(), bounds);

        render_subgraph_borders(&mut canvas, &map, &charset);

        // Top border should be plain horizontal line, no title text
        let output = canvas.to_string();
        let lines: Vec<&str> = output.lines().collect();
        assert!(
            !lines[0].contains("─  ─"),
            "Should not have title gaps in border, got: {}",
            lines[0]
        );
        // Should just be corner + horizontal fill + corner
        assert!(
            lines[0].contains("┌────────────┐"),
            "Expected plain top border, got: {}",
            lines[0]
        );
    }
}
