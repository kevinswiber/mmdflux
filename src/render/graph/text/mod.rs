//! Low-level text drawing for graph-family rendering.
//!
//! This module owns the render-side step that consumes graph-owned grid
//! geometry and paints it onto a character canvas.

mod edge;
pub(crate) mod label_placement;
mod label_util;
mod shape;
mod subgraph;

#[cfg(test)]
pub(crate) use edge::{compute_edge_containment, render_all_edges, render_edge};

#[cfg(test)]
pub(crate) fn required_canvas_size_for_test(
    layout: &GridLayout,
    routed_edges: &[RoutedEdge],
) -> (usize, usize) {
    required_canvas_size(layout, routed_edges)
}

/// Test-only: render into a canvas and return it (no trimming). Lets harnesses
/// compare placer output coords directly to canvas cells.
#[cfg(test)]
pub(crate) fn render_text_canvas_for_test(
    diagram: &Graph,
    layout: &GridLayout,
    routed: Option<&RoutedGraphGeometry>,
    options: &TextRenderOptions,
) -> Canvas {
    let charset = match options.output_format {
        OutputFormat::Ascii => CharSet::ascii(),
        _ => CharSet::unicode(),
    };

    let routed_edges = route_all_edges(&diagram.edges, layout, diagram.direction);
    let (canvas_width, canvas_height) = required_canvas_size(layout, &routed_edges);
    let mut canvas = Canvas::new(canvas_width, canvas_height);

    if !layout.subgraph_bounds.is_empty() {
        subgraph::render_subgraph_borders(&mut canvas, &layout.subgraph_bounds, &charset);
    }

    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for node_id in node_keys {
        let node = &diagram.nodes[node_id];
        if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
            shape::render_node(&mut canvas, node, x, y, &charset, diagram.direction);
        }
    }

    let edge_containment =
        edge::compute_edge_containment(&diagram.edges, &diagram.subgraphs, &layout.subgraph_bounds);

    edge::render_all_edges_with_labels(
        &mut canvas,
        &routed_edges,
        &charset,
        diagram.direction,
        &edge_containment,
        layout,
        routed,
    );

    apply_subgraph_border_junctions(
        &mut canvas,
        &layout.subgraph_bounds,
        &routed_edges,
        &charset,
    );

    canvas
}
#[cfg(test)]
mod regression_tests;

use super::TextRenderOptions;
use crate::format::OutputFormat;
use crate::graph::Graph;
use crate::graph::geometry::RoutedGraphGeometry;
use crate::graph::grid::{GridLayout, RoutedEdge, Segment, SubgraphBounds, route_all_edges};
use crate::render::text::canvas::{Cell, Connections};
use crate::render::text::{Canvas, CharSet};

/// Render text output from a derived grid layout.
pub fn render_text_from_grid_layout(
    diagram: &Graph,
    layout: &GridLayout,
    routed: Option<&RoutedGraphGeometry>,
    options: &TextRenderOptions,
) -> String {
    let charset = match options.output_format {
        OutputFormat::Ascii => CharSet::ascii(),
        _ => CharSet::unicode(),
    };

    let routed_edges = route_all_edges(&diagram.edges, layout, diagram.direction);
    let (canvas_width, canvas_height) = required_canvas_size(layout, &routed_edges);
    let mut canvas = Canvas::new(canvas_width, canvas_height);

    if !layout.subgraph_bounds.is_empty() {
        subgraph::render_subgraph_borders(&mut canvas, &layout.subgraph_bounds, &charset);
    }

    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for node_id in node_keys {
        let node = &diagram.nodes[node_id];
        if let Some(&(x, y)) = layout.draw_positions.get(node_id) {
            shape::render_node(&mut canvas, node, x, y, &charset, diagram.direction);
        }
    }

    let edge_containment =
        edge::compute_edge_containment(&diagram.edges, &diagram.subgraphs, &layout.subgraph_bounds);

    edge::render_all_edges_with_labels(
        &mut canvas,
        &routed_edges,
        &charset,
        diagram.direction,
        &edge_containment,
        layout,
        routed,
    );

    apply_subgraph_border_junctions(
        &mut canvas,
        &layout.subgraph_bounds,
        &routed_edges,
        &charset,
    );

    if options.text_color_mode.uses_ansi() {
        canvas.to_ansi_string()
    } else {
        canvas.to_string()
    }
}

fn required_canvas_size(layout: &GridLayout, routed_edges: &[RoutedEdge]) -> (usize, usize) {
    let mut width = layout.width.max(1);
    let mut height = layout.height.max(1);

    for bounds in layout.node_bounds.values() {
        width = width.max(bounds.x + bounds.width);
        height = height.max(bounds.y + bounds.height);
    }

    for bounds in layout.subgraph_bounds.values() {
        width = width.max(bounds.x + bounds.width);
        height = height.max(bounds.y + bounds.height);
    }

    for routed in routed_edges {
        width = width.max(routed.start.x.saturating_add(1));
        height = height.max(routed.start.y.saturating_add(1));
        width = width.max(routed.end.x.saturating_add(1));
        height = height.max(routed.end.y.saturating_add(1));

        for segment in &routed.segments {
            match *segment {
                Segment::Vertical { x, y_start, y_end } => {
                    width = width.max(x.saturating_add(1));
                    height = height.max(y_start.max(y_end).saturating_add(1));
                }
                Segment::Horizontal { y, x_start, x_end } => {
                    width = width.max(x_start.max(x_end).saturating_add(1));
                    height = height.max(y.saturating_add(1));
                }
            }
        }
    }

    (width, height)
}

fn apply_subgraph_border_junctions(
    canvas: &mut Canvas,
    subgraph_bounds: &std::collections::HashMap<String, SubgraphBounds>,
    routed_edges: &[RoutedEdge],
    charset: &CharSet,
) {
    if subgraph_bounds.is_empty() || routed_edges.is_empty() {
        return;
    }

    let should_skip_title_cell =
        |cell: &Cell| cell.is_subgraph_title && cell.ch != charset.horizontal && cell.ch != ' ';
    let conns_all = Connections {
        up: true,
        down: true,
        left: true,
        right: true,
    };

    for bounds in subgraph_bounds.values() {
        if bounds.width < 2 || bounds.height < 2 {
            continue;
        }

        let left = bounds.x;
        let right = bounds.x.saturating_add(bounds.width.saturating_sub(1));
        let top = bounds.y;
        let bottom = bounds.y.saturating_add(bounds.height.saturating_sub(1));

        for routed in routed_edges {
            for segment in &routed.segments {
                match *segment {
                    Segment::Vertical { x, y_start, y_end } => {
                        let (y_min, y_max) = if y_start <= y_end {
                            (y_start, y_end)
                        } else {
                            (y_end, y_start)
                        };
                        if x > left && x < right {
                            if y_min < top
                                && top <= y_max
                                && let Some(cell) = canvas.get(x, top)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, x, top, conns_all, charset);
                            }
                            if y_min <= bottom
                                && bottom < y_max
                                && let Some(cell) = canvas.get(x, bottom)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, x, bottom, conns_all, charset);
                            }
                        }
                    }
                    Segment::Horizontal { y, x_start, x_end } => {
                        let (x_min, x_max) = if x_start <= x_end {
                            (x_start, x_end)
                        } else {
                            (x_end, x_start)
                        };
                        if y > top && y < bottom {
                            if x_min < left
                                && left <= x_max
                                && let Some(cell) = canvas.get(left, y)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, left, y, conns_all, charset);
                            }
                            if x_min <= right
                                && right < x_max
                                && let Some(cell) = canvas.get(right, y)
                                && !should_skip_title_cell(cell)
                            {
                                set_junction_cell(canvas, right, y, conns_all, charset);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn set_junction_cell(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    conns: Connections,
    charset: &CharSet,
) {
    if let Some(cell) = canvas.get_mut(x, y) {
        cell.ch = charset.junction(conns);
        cell.connections = conns;
        cell.is_edge = true;
    }
}
