//! SVG label placement and emission helpers for graph rendering.

use std::collections::HashMap;

use super::text::render_text_centered;
use super::writer::SvgWriter;
use super::{Point, TEXT_COLOR};
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::compute_end_label_positions;
use crate::graph::{Graph, Stroke};

const LABEL_POINT_EPS: f64 = 0.000_001;

fn point_distance_svg(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn svg_path_midpoint(path: &[Point]) -> Option<Point> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return path.first().copied();
    }
    let total_len: f64 = path
        .windows(2)
        .map(|segment| point_distance_svg(segment[0], segment[1]))
        .sum();
    if total_len <= LABEL_POINT_EPS {
        return path.get(path.len() / 2).copied();
    }

    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let seg_len = point_distance_svg(a, b);
        if seg_len <= LABEL_POINT_EPS {
            continue;
        }
        if traversed + seg_len >= target {
            let t = (target - traversed) / seg_len;
            return Some(Point {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
            });
        }
        traversed += seg_len;
    }
    path.last().copied()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_edge_labels(
    writer: &mut SvgWriter,
    diagram: &Graph,
    geom: &GraphGeometry,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
    override_nodes: &HashMap<String, String>,
    metrics: &ProportionalTextMetrics,
    scale: f64,
) {
    let label_positions = precomputed_label_positions(geom);

    writer.start_group("edgeLabels");

    for edge in diagram.edges.iter() {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(label) = edge.label.as_ref() else {
            continue;
        };
        let edge_idx = edge.index;
        let cross_boundary = if edge.from_subgraph.is_none() && edge.to_subgraph.is_none() {
            let from_override = override_nodes.get(&edge.from);
            let to_override = override_nodes.get(&edge.to);
            matches!(
                (from_override, to_override),
                (Some(a), Some(b)) if a != b
            ) || matches!(
                (from_override, to_override),
                (Some(_), None) | (None, Some(_))
            )
        } else {
            false
        };
        // Prefer the arc-length midpoint of the rendered path when available,
        // since the layout engine's precomputed label position may not correspond
        // to the visual midpoint of the final edge path.
        let position = rendered_edge_paths
            .get(&edge_idx)
            .and_then(|path| svg_path_midpoint(path))
            .or_else(|| {
                let use_precomputed =
                    edge.from_subgraph.is_none() && edge.to_subgraph.is_none() && !cross_boundary;
                if use_precomputed {
                    label_positions.get(&edge_idx).copied()
                } else {
                    None
                }
            })
            .or_else(|| {
                fallback_label_position(geom, edge_idx, self_edge_paths, rendered_edge_paths)
            });
        let Some(point) = position else {
            continue;
        };
        render_text_centered(
            writer,
            point.x * scale,
            point.y * scale,
            label,
            TEXT_COLOR,
            metrics,
            scale,
        );
    }

    // Render head/tail end labels from routed edge paths.
    for edge in diagram.edges.iter() {
        if edge.head_label.is_none() && edge.tail_label.is_none() {
            continue;
        }
        // Get the routed path for this edge from geometry.
        let path: Vec<Point> = geom
            .edges
            .iter()
            .find(|e| e.index == edge.index)
            .and_then(|e| e.layout_path_hint.clone())
            .unwrap_or_default();
        if path.len() < 2 {
            continue;
        }
        let (head_pos, tail_pos) = compute_end_label_positions(&path);
        if let (Some(label), Some(pos)) = (&edge.head_label, head_pos) {
            render_text_centered(
                writer,
                pos.x * scale,
                pos.y * scale,
                label,
                TEXT_COLOR,
                metrics,
                scale,
            );
        }
        if let (Some(label), Some(pos)) = (&edge.tail_label, tail_pos) {
            render_text_centered(
                writer,
                pos.x * scale,
                pos.y * scale,
                label,
                TEXT_COLOR,
                metrics,
                scale,
            );
        }
    }

    writer.end_group();
}

pub(super) fn fallback_label_position(
    geom: &GraphGeometry,
    edge_index: usize,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
) -> Option<Point> {
    if let Some(points) = self_edge_paths.get(&edge_index) {
        return svg_path_midpoint(points).or_else(|| points.get(points.len() / 2).copied());
    }

    // Try regular edges via layout_path_hint
    if let Some(layout_edge) = geom.edges.iter().find(|e| e.index == edge_index)
        && let Some(path) = &layout_edge.layout_path_hint
    {
        return path.get(path.len() / 2).copied();
    }

    // Try self-edges
    if let Some(se) = geom.self_edges.iter().find(|e| e.edge_index == edge_index) {
        return se.points.get(se.points.len() / 2).copied();
    }

    if let Some(points) = rendered_edge_paths.get(&edge_index) {
        return svg_path_midpoint(points).or_else(|| points.get(points.len() / 2).copied());
    }

    None
}

pub(super) fn precomputed_label_positions(geom: &GraphGeometry) -> HashMap<usize, Point> {
    geom.edges
        .iter()
        .filter_map(|edge| edge.label_position.map(|point| (edge.index, point)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Point, svg_path_midpoint};

    #[test]
    fn svg_path_midpoint_handles_multi_segment_paths_by_distance() {
        let path = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 6.0, y: 0.0 },
            Point { x: 6.0, y: 6.0 },
        ];

        assert_eq!(svg_path_midpoint(&path), Some(Point { x: 6.0, y: 0.0 }));
    }
}
