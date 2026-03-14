//! Bounds helpers for graph SVG rendering.

use std::collections::HashMap;

use super::labels::{fallback_label_position, precomputed_label_positions};
use super::{Point, Rect};
use crate::graph::geometry::GraphGeometry;
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::{Graph, Stroke};

pub(super) struct SvgBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl SvgBounds {
    fn new() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    fn update_point(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn update_rect(&mut self, rect: &Rect) {
        self.update_point(rect.x, rect.y);
        self.update_point(rect.x + rect.width, rect.y + rect.height);
    }

    pub(super) fn finalize(
        &self,
        fallback_width: f64,
        fallback_height: f64,
    ) -> (f64, f64, f64, f64) {
        if !self.min_x.is_finite() || !self.min_y.is_finite() {
            return (0.0, 0.0, fallback_width, fallback_height);
        }
        (self.min_x, self.min_y, self.max_x, self.max_y)
    }
}

pub(super) fn compute_svg_bounds(
    diagram: &Graph,
    geom: &GraphGeometry,
    metrics: &ProportionalTextMetrics,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
) -> SvgBounds {
    let mut bounds = SvgBounds::new();

    for pos_node in geom.nodes.values() {
        bounds.update_rect(&pos_node.rect);
    }

    for sg_geom in geom.subgraphs.values() {
        bounds.update_rect(&sg_geom.rect);
    }

    let is_invisible = |index: usize| -> bool {
        diagram
            .edges
            .get(index)
            .is_some_and(|e| e.stroke == Stroke::Invisible)
    };

    for edge in &diagram.edges {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        if let Some(path) = rendered_edge_paths.get(&edge.index) {
            for point in path {
                bounds.update_point(point.x, point.y);
            }
            continue;
        }
        if let Some(layout_edge) = geom.edges.iter().find(|e| e.index == edge.index)
            && let Some(path) = &layout_edge.layout_path_hint
        {
            for point in path {
                bounds.update_point(point.x, point.y);
            }
        }
    }

    for se in &geom.self_edges {
        if is_invisible(se.edge_index) {
            continue;
        }
        if let Some(computed) = self_edge_paths.get(&se.edge_index) {
            for point in computed {
                bounds.update_point(point.x, point.y);
            }
        } else {
            for point in &se.points {
                bounds.update_point(point.x, point.y);
            }
        }
    }

    let label_positions = precomputed_label_positions(geom);

    for edge in diagram.edges.iter() {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(label) = edge.label.as_ref() else {
            continue;
        };
        let edge_idx = edge.index;
        let use_precomputed = edge.from_subgraph.is_none() && edge.to_subgraph.is_none();
        let position = if use_precomputed {
            label_positions.get(&edge_idx).copied()
        } else {
            None
        }
        .or_else(|| fallback_label_position(geom, edge_idx, self_edge_paths, rendered_edge_paths));
        let Some(point) = position else {
            continue;
        };
        let (w, h) = metrics.edge_label_dimensions(label);
        let rect = Rect {
            x: point.x - w / 2.0,
            y: point.y - h / 2.0,
            width: w,
            height: h,
        };
        bounds.update_rect(&rect);
    }

    bounds
}

pub(super) fn scale_rect(rect: &Rect, scale: f64) -> Rect {
    Rect {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}
