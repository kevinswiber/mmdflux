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
        // Use precomputed label positions only when the edge was NOT re-routed.
        // Re-routed edges (e.g. backward edges with channel detours) have their
        // labels placed on the rendered path, which can differ from the
        // precomputed geometry position.
        let use_precomputed = edge.from_subgraph.is_none()
            && edge.to_subgraph.is_none()
            && !rendered_edge_paths.contains_key(&edge.index);

        // Prefer label_geometry.rect when precomputed positions would be used.
        // label_geometry is the authoritative source populated by the routing
        // label-lane pass; it carries both center and padded rect.
        // TODO(plan 0145 PR 3 / task 3.7): remove precomputed/revalidate fallback
        // once label_lanes populates label_geometry for all edges.
        let layout_edge = geom.edges.iter().find(|e| e.index == edge_idx);
        if let Some(g) = layout_edge.and_then(|e| e.label_geometry.as_ref())
            && use_precomputed
        {
            bounds.update_rect(&g.rect);
            continue;
        }

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

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::compute_svg_bounds;
    use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide, GraphGeometry, LayoutEdge};
    use crate::graph::measure::default_proportional_text_metrics;
    use crate::graph::space::{FPoint, FRect};
    use crate::graph::{Direction, Edge, Graph};

    /// Constructs a minimal Graph + GraphGeometry with one labeled edge and one
    /// LayoutEdge that has `label_position` set. The LayoutEdge starts with
    /// `label_geometry: None`.
    fn minimal_labeled_edge_fixtures() -> (Graph, GraphGeometry) {
        let mut diagram = Graph::new(Direction::TopDown);
        let mut edge = Edge::new("A", "B").with_label("yes");
        edge.index = 0;
        diagram.edges.push(edge);

        let geom = GraphGeometry {
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
                effective_wrapped_lines: None,
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
        };

        (diagram, geom)
    }

    #[test]
    fn svg_bounds_uses_label_geometry_rect_when_present() {
        let (diagram, mut geom) = minimal_labeled_edge_fixtures();
        let metrics = default_proportional_text_metrics();
        let empty_map = HashMap::new();

        // Set label_geometry with a rect far outside the normal layout bounds.
        geom.edges[0].label_geometry = Some(EdgeLabelGeometry {
            center: FPoint::new(500.0, 500.0),
            rect: FRect::new(490.0, 495.0, 20.0, 10.0),
            padding: (4.0, 2.0),
            side: EdgeLabelSide::Above,
            track: 0,
        });

        let bounds = compute_svg_bounds(&diagram, &geom, &metrics, &empty_map, &empty_map);
        let (min_x, min_y, max_x, max_y) = bounds.finalize(200.0, 200.0);

        // The bounds must include the label_geometry rect (490..510, 495..505).
        assert!(
            max_x >= 510.0,
            "max_x ({max_x}) must be >= 510.0 to include label_geometry rect"
        );
        assert!(
            max_y >= 505.0,
            "max_y ({max_y}) must be >= 505.0 to include label_geometry rect"
        );
        assert!(
            min_x <= 490.0,
            "min_x ({min_x}) must be <= 490.0 to include label_geometry rect"
        );
        assert!(
            min_y <= 495.0,
            "min_y ({min_y}) must be <= 495.0 to include label_geometry rect"
        );
    }
}
