//! Bounds helpers for graph SVG rendering.

use std::collections::HashMap;

use super::labels::{
    fallback_label_position, precomputed_label_positions, precomputed_label_sides,
    reciprocal_labeled_edge_indices,
};
use super::{Point, Rect};
use crate::graph::geometry::{EdgeLabelSide, GraphGeometry};
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::{Direction, Graph, Stroke};

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
    let label_sides = precomputed_label_sides(geom);
    let reciprocal_edges = reciprocal_labeled_edge_indices(diagram);

    for edge in diagram.edges.iter() {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(label) = edge.label.as_ref() else {
            continue;
        };
        let edge_idx = edge.index;
        let use_precomputed = edge.from_subgraph.is_none()
            && edge.to_subgraph.is_none()
            && !rendered_edge_paths.contains_key(&edge.index);
        let precomputed = if use_precomputed {
            label_positions.get(&edge_idx).copied()
        } else {
            None
        };
        let is_reciprocal = reciprocal_edges.contains(&edge_idx);
        let has_side = label_sides
            .get(&edge_idx)
            .is_some_and(|s| *s != EdgeLabelSide::Center);
        let position = if is_reciprocal && has_side && precomputed.is_some() {
            precomputed
        } else {
            let fallback =
                fallback_label_position(geom, edge_idx, self_edge_paths, rendered_edge_paths);
            precomputed.or(fallback)
        };
        let Some(mut point) = position else {
            continue;
        };
        if is_reciprocal && has_side {
            let side = label_sides[&edge_idx];
            let sign = match side {
                EdgeLabelSide::Above => -1.0,
                EdgeLabelSide::Below => 1.0,
                EdgeLabelSide::Center => 0.0,
            };
            let nudge = metrics.line_height * 0.5;
            match geom.direction {
                Direction::TopDown | Direction::BottomTop => point.y += sign * nudge,
                Direction::LeftRight | Direction::RightLeft => point.x += sign * nudge,
            }
        }
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
