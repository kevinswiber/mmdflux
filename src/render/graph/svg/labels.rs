//! SVG label placement and emission helpers for graph rendering.

use std::collections::HashMap;

use super::text::{BackgroundStyle, TextRenderStyle, render_text_centered};
use super::{GraphSvgPalette, Point, dynamic_css_attrs};
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::compute_end_label_positions;
use crate::graph::{Graph, Stroke};
use crate::render::svg::SvgWriter;

const LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE: f64 = 2.0;
const LABEL_POINT_EPS: f64 = 0.000_001;

fn revalidate_svg_label_anchor(candidate: Point, rendered_path: Option<&[Point]>) -> Point {
    let Some(path) = rendered_path else {
        return candidate;
    };
    if path.is_empty() {
        return candidate;
    }

    let drift = distance_point_to_svg_path(candidate, path);
    if drift <= LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE {
        return candidate;
    }
    svg_path_midpoint(path).unwrap_or(candidate)
}

fn point_distance_svg(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn distance_point_to_svg_segment(point: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= LABEL_POINT_EPS {
        return point_distance_svg(point, a);
    }
    let projection = ((point.x - a.x) * dx + (point.y - a.y) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = Point {
        x: a.x + t * dx,
        y: a.y + t * dy,
    };
    point_distance_svg(point, closest)
}

fn distance_point_to_svg_path(point: Point, path: &[Point]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return point_distance_svg(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_svg_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
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
    palette: &GraphSvgPalette,
) {
    let label_positions = precomputed_label_positions(geom);
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "graph-edge-text",
        &["fill:var(--_text);"],
    );
    let bg_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "graph-edge-label-bg",
        &["fill:var(--bg);"],
    );
    let bg_style = BackgroundStyle {
        fill: &palette.edge_label_background,
        extra_attrs: bg_dynamic_attrs.as_str(),
    };

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
        let use_precomputed =
            edge.from_subgraph.is_none() && edge.to_subgraph.is_none() && !cross_boundary;

        // Prefer label_geometry.center (populated by the routing label-lane pass)
        // over the precomputed label_position when available. Falls through to
        // fallback + revalidation when label_geometry is None.
        // TODO(plan 0145 PR 3 / task 3.7): remove precomputed/revalidate fallback
        // once label_lanes populates label_geometry for all edges.
        let layout_edge = geom.edges.iter().find(|e| e.index == edge_idx);
        let label_geom_center =
            layout_edge.and_then(|e| e.label_geometry.as_ref().map(|g| g.center));
        let position = if use_precomputed {
            label_geom_center.or_else(|| label_positions.get(&edge_idx).copied())
        } else {
            None
        }
        .or_else(|| fallback_label_position(geom, edge_idx, self_edge_paths, rendered_edge_paths))
        .map(|candidate| {
            revalidate_svg_label_anchor(
                candidate,
                rendered_edge_paths
                    .get(&edge_idx)
                    .map(|path| path.as_slice()),
            )
        });
        let Some(point) = position else {
            continue;
        };
        render_text_centered(
            writer,
            Point {
                x: point.x * scale,
                y: point.y * scale,
            },
            label,
            metrics,
            scale,
            TextRenderStyle {
                color: &palette.edge_label_text,
                extra_attrs: dynamic_attrs.as_str(),
                background: Some(BackgroundStyle {
                    fill: bg_style.fill,
                    extra_attrs: bg_style.extra_attrs,
                }),
            },
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
                Point {
                    x: pos.x * scale,
                    y: pos.y * scale,
                },
                label,
                metrics,
                scale,
                TextRenderStyle {
                    color: &palette.edge_label_text,
                    extra_attrs: dynamic_attrs.as_str(),
                    background: Some(BackgroundStyle {
                        fill: bg_style.fill,
                        extra_attrs: bg_style.extra_attrs,
                    }),
                },
            );
        }
        if let (Some(label), Some(pos)) = (&edge.tail_label, tail_pos) {
            render_text_centered(
                writer,
                Point {
                    x: pos.x * scale,
                    y: pos.y * scale,
                },
                label,
                metrics,
                scale,
                TextRenderStyle {
                    color: &palette.edge_label_text,
                    extra_attrs: dynamic_attrs.as_str(),
                    background: Some(BackgroundStyle {
                        fill: bg_style.fill,
                        extra_attrs: bg_style.extra_attrs,
                    }),
                },
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

/// Resolve the label center for a given edge index, preferring
/// `label_geometry.center` when present, falling back to `label_position`
/// (precomputed by the layout engine), then to path-based midpoint fallbacks.
// TODO(plan 0145 PR 3 / task 3.7): once label_lanes populates label_geometry
// for all edges, the precomputed/fallback paths may be simplified.
#[cfg(test)]
fn resolve_edge_label_center(
    geom: &GraphGeometry,
    edge_index: usize,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
) -> Option<Point> {
    // Prefer label_geometry.center (populated by the routing label-lane pass).
    if let Some(layout_edge) = geom.edges.iter().find(|e| e.index == edge_index)
        && let Some(g) = &layout_edge.label_geometry
    {
        return Some(g.center);
    }

    // Fall back to precomputed label_position from the layout engine.
    let label_positions = precomputed_label_positions(geom);
    if let Some(&pos) = label_positions.get(&edge_index) {
        return Some(pos);
    }

    // Final fallback: path-based midpoint.
    fallback_label_position(geom, edge_index, self_edge_paths, rendered_edge_paths)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{Point, revalidate_svg_label_anchor, svg_path_midpoint};
    use crate::graph::Direction;
    use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide, GraphGeometry, LayoutEdge};
    use crate::graph::space::{FPoint, FRect};

    #[test]
    fn revalidate_svg_label_anchor_keeps_nearby_anchor() {
        let candidate = Point { x: 5.0, y: 1.0 };
        let path = [Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }];

        assert_eq!(
            revalidate_svg_label_anchor(candidate, Some(&path)),
            candidate
        );
    }

    #[test]
    fn revalidate_svg_label_anchor_falls_back_to_path_midpoint_when_drifted() {
        let candidate = Point { x: 50.0, y: 25.0 };
        let path = [Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }];

        assert_eq!(
            revalidate_svg_label_anchor(candidate, Some(&path)),
            Point { x: 5.0, y: 0.0 }
        );
    }

    #[test]
    fn svg_path_midpoint_handles_multi_segment_paths_by_distance() {
        let path = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 6.0, y: 0.0 },
            Point { x: 6.0, y: 6.0 },
        ];

        assert_eq!(svg_path_midpoint(&path), Some(Point { x: 6.0, y: 0.0 }));
    }

    /// Constructs a minimal GraphGeometry with one LayoutEdge that has
    /// `label_position` set. The LayoutEdge starts with `label_geometry: None`.
    fn minimal_geom_with_labeled_edge() -> GraphGeometry {
        GraphGeometry {
            nodes: HashMap::new(),
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: Some(FPoint::new(50.0, 50.0)),
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: Some(vec![FPoint::new(50.0, 0.0), FPoint::new(50.0, 100.0)]),
                preserve_orthogonal_topology: false,
                label_geometry: None,
            }],
            subgraphs: HashMap::new(),
            self_edges: vec![],
            direction: Direction::TopDown,
            node_directions: HashMap::new(),
            bounds: FRect::new(0.0, 0.0, 100.0, 100.0),
            reversed_edges: vec![],
            engine_hints: None,
            grid_projection: None,
            rerouted_edges: HashSet::new(),
            enhanced_backward_routing: false,
        }
    }

    #[test]
    fn svg_labels_uses_label_geometry_center_when_present() {
        let mut geom = minimal_geom_with_labeled_edge();
        let empty_map: HashMap<usize, Vec<Point>> = HashMap::new();

        // Set label_geometry with a center far from the normal label_position.
        geom.edges[0].label_geometry = Some(EdgeLabelGeometry {
            center: FPoint::new(123.0, 456.0),
            rect: FRect::new(113.0, 451.0, 20.0, 10.0),
            padding: (4.0, 2.0),
            side: EdgeLabelSide::Above,
            track: 0,
        });

        // resolve_edge_label_center should prefer label_geometry.center.
        let center = super::resolve_edge_label_center(&geom, 0, &empty_map, &empty_map);
        assert_eq!(
            center,
            Some(FPoint::new(123.0, 456.0)),
            "must prefer label_geometry.center when present"
        );
    }

    #[test]
    fn svg_labels_falls_back_to_label_position_when_no_label_geometry() {
        let geom = minimal_geom_with_labeled_edge();
        let empty_map: HashMap<usize, Vec<Point>> = HashMap::new();

        // label_geometry is None, so it should fall back to precomputed label_position.
        let center = super::resolve_edge_label_center(&geom, 0, &empty_map, &empty_map);
        assert_eq!(
            center,
            Some(FPoint::new(50.0, 50.0)),
            "must fall back to label_position when label_geometry is None"
        );
    }
}
