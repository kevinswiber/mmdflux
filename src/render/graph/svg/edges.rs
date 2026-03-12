//! Edge path preparation, shaping, and SVG path emission for graph rendering.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::writer::{SvgWriter, fmt_f64};
use super::{MIN_BASIS_VISIBLE_STEM_PX, Point, Rect, STROKE_COLOR};
use crate::graph::direction_policy::effective_edge_direction;
use crate::graph::geometry::{EngineHints, GraphGeometry};
use crate::graph::routing::{
    EdgeRouting, build_orthogonal_path_float, hexagon_vertices, intersect_convex_polygon,
};
use crate::graph::{Arrow, Diagram, Direction, Edge, Shape, Stroke};
use crate::{CornerStyle, Curve, PathSimplification};

#[allow(clippy::too_many_arguments)]
pub(super) struct PreparedRenderedEdges {
    pub(super) paths: HashMap<usize, Vec<Point>>,
    basis_stem_edge_indexes: HashSet<usize>,
    compact_basis_stem_edge_indexes: HashSet<usize>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_rendered_edge_paths(
    diagram: &Diagram,
    geom: &GraphGeometry,
    override_nodes: &HashMap<String, String>,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rerouted_edges: &std::collections::HashSet<usize>,
    edge_routing: EdgeRouting,
    curve: Curve,
    edge_radius: f64,
    path_simplification: PathSimplification,
) -> PreparedRenderedEdges {
    let mut reciprocal_edge_indexes: HashSet<usize> = HashSet::new();
    for edge in &geom.edges {
        if geom.edges.iter().any(|other| {
            other.index != edge.index && other.from == edge.to && other.to == edge.from
        }) {
            reciprocal_edge_indexes.insert(edge.index);
        }
    }

    let mut edge_paths: Vec<(usize, Vec<Point>, bool)> = geom
        .edges
        .iter()
        .map(|edge| {
            let points: Vec<Point> = edge
                .layout_path_hint
                .as_ref()
                .map(|ps| ps.to_vec())
                .unwrap_or_default();
            (edge.index, points, edge.preserve_orthogonal_topology)
        })
        .collect();
    edge_paths.extend(geom.self_edges.iter().map(|se| {
        let points = self_edge_paths
            .get(&se.edge_index)
            .cloned()
            .unwrap_or_else(|| se.points.to_vec());
        (se.edge_index, points, false)
    }));
    edge_paths.sort_by_key(|(index, _, _)| *index);

    let mut rendered_paths: HashMap<usize, Vec<Point>> = HashMap::new();
    let mut basis_stem_edge_indexes: HashSet<usize> = HashSet::new();
    let mut compact_basis_stem_edge_indexes: HashSet<usize> = HashSet::new();
    let mut incoming_edge_counts: HashMap<String, usize> = HashMap::new();
    for edge in &diagram.edges {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        *incoming_edge_counts.entry(edge.to.clone()).or_default() += 1;
    }
    for (index, points, preserve_orthogonal_topology) in edge_paths {
        let Some(edge) = diagram.edges.get(index) else {
            continue;
        };
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let mut points = points;
        let edge_direction = orthogonal_route_edge_direction(
            diagram,
            &geom.node_directions,
            override_nodes,
            &edge.from,
            &edge.to,
            diagram.direction,
        );
        let is_backward = geom.reversed_edges.contains(&index);
        // Engine-owned routing topology determines endpoint contract; style does not.
        // Backward OrthogonalRoute edges use orthogonal approach to preserve path integrity.
        let preserve_orthogonal_endpoint_contract = matches!(
            (edge_routing, is_backward),
            (EdgeRouting::OrthogonalRoute, true)
        );
        // Clip subgraph-as-node edges to subgraph borders (skip for rerouted
        // edges whose endpoints already land on the subgraph border).
        if !rerouted_edges.contains(&index) {
            if let Some(sg_id) = edge.from_subgraph.as_ref()
                && let Some(sg_geom) = geom.subgraphs.get(sg_id)
            {
                points = clip_points_to_rect_start(&points, &sg_geom.rect);
            }
            if let Some(sg_id) = edge.to_subgraph.as_ref()
                && let Some(sg_geom) = geom.subgraphs.get(sg_id)
            {
                points = clip_points_to_rect_end(&points, &sg_geom.rect);
            }
        }

        // Preserve prior rerouted-edge behavior for healthy paths, but still
        // reclip when endpoints detach from expected faces or use non-rect
        // shapes (diamond/hexagon).
        let rerouted = rerouted_edges.contains(&index);
        let should_adjust = !matches!(edge_routing, EdgeRouting::EngineProvided)
            && (!rerouted
                || (matches!(edge_routing, EdgeRouting::OrthogonalRoute)
                    && should_adjust_rerouted_edge_endpoints(
                        diagram,
                        geom,
                        edge,
                        &points,
                        edge_direction,
                    )));
        let mut points = if should_adjust {
            adjust_edge_points_for_shapes(
                diagram,
                geom,
                edge,
                &points,
                edge_direction,
                is_backward,
                edge_routing,
            )
        } else {
            points
        };
        // Derived boolean flags from style model.
        let is_basis = matches!(curve, Curve::Basis);
        let is_rounded_corner = matches!(curve, Curve::Linear(CornerStyle::Rounded));
        let is_sharp = matches!(curve, Curve::Linear(CornerStyle::Sharp));
        let target_incoming_count = incoming_edge_counts.get(&edge.to).copied().unwrap_or(0);
        let has_reciprocal = reciprocal_edge_indexes.contains(&index);
        let simple_reciprocal_pair =
            has_reciprocal && is_simple_two_node_reciprocal_pair(diagram, edge);
        let supports_reciprocal_lane_alignment = simple_reciprocal_pair
            && matches!(
                edge_routing,
                EdgeRouting::PolylineRoute
                    | EdgeRouting::DirectRoute
                    | EdgeRouting::OrthogonalRoute
            );
        if is_basis
            && matches!(
                path_simplification,
                PathSimplification::None | PathSimplification::Lossless
            )
        {
            points = adapt_basis_anchor_points(&points, edge, geom, edge_direction, is_backward);
        }

        // Basis interpolation needs 3+ points to produce curves. For short
        // linear paths, synthesize two control points:
        // - 2-point paths (generic)
        // - reciprocal 3-point collinear paths (Mermaid-layered backward edges)
        //   so lossless simplification cannot collapse them back to a line.
        if is_basis {
            let use_reciprocal_synthesis =
                matches!(edge_routing, EdgeRouting::PolylineRoute) && simple_reciprocal_pair;
            let should_synthesize = points.len() == 2
                || (use_reciprocal_synthesis && points.len() == 3 && points_are_collinear(&points));
            if should_synthesize {
                let mut start = points[0];
                let mut end = points[points.len() - 1];
                let (cp1, cp2) = if use_reciprocal_synthesis {
                    let curve_sign = if is_backward { 1.0 } else { -1.0 };
                    if let Some(((from_rect, from_shape), (to_rect, to_shape))) =
                        edge_endpoint_shape_rects(diagram, geom, edge)
                    {
                        (start, end) = apply_reciprocal_lane_offsets(
                            start,
                            end,
                            edge_direction,
                            curve_sign,
                            from_rect,
                            to_rect,
                        );
                        let projected_start = intersect_svg_node(&from_rect, start, from_shape);
                        let projected_end = intersect_svg_node(&to_rect, end, to_shape);
                        start = projected_start;
                        end = projected_end;
                    }
                    synthesize_reciprocal_bezier_control_points(
                        start,
                        end,
                        edge_direction,
                        curve_sign,
                    )
                } else {
                    synthesize_bezier_control_points(start, end, edge_direction)
                };
                points = vec![start, cp1, cp2, end];
            }
        }
        if !is_basis
            && supports_reciprocal_lane_alignment
            && (points.len() == 2 || (points.len() == 3 && points_are_collinear(&points)))
            && let Some(((from_rect, from_shape), (to_rect, to_shape))) =
                edge_endpoint_shape_rects(diagram, geom, edge)
        {
            let curve_sign = if is_backward { 1.0 } else { -1.0 };
            let (lane_start, lane_end) = apply_reciprocal_lane_offsets(
                points[0],
                points[points.len() - 1],
                edge_direction,
                curve_sign,
                from_rect,
                to_rect,
            );
            let projected_start = intersect_svg_node(&from_rect, lane_start, from_shape);
            let projected_end = intersect_svg_node(&to_rect, lane_end, to_shape);
            points = vec![projected_start, projected_end];
        }

        let preserve_forward_orthogonal_topology =
            matches!(edge_routing, EdgeRouting::OrthogonalRoute)
                && !is_backward
                && edge.from != edge.to
                && preserve_orthogonal_topology
                && points.len() > 4;
        let preserve_orthogonal_marker_contract =
            preserve_orthogonal_endpoint_contract || preserve_forward_orthogonal_topology;

        // Only densify corners for direct/orthogonal sharp paths. For engine-provided
        // polyline geometry, this synthetic densification introduces tiny visible jogs
        // on axis-to-diagonal turns (for example ampersand fan-in).
        if is_sharp
            && !preserve_orthogonal_marker_contract
            && !matches!(
                edge_routing,
                EdgeRouting::PolylineRoute | EdgeRouting::EngineProvided
            )
        {
            points = fix_corner_points(&points);
        }
        if matches!(edge_routing, EdgeRouting::OrthogonalRoute)
            && is_basis
            && !is_backward
            && edge.from != edge.to
            && !preserve_forward_orthogonal_topology
        {
            points = collapse_primary_face_fan_channel_for_curved(
                geom,
                edge,
                edge_direction,
                &points,
                0.5,
            );
        }
        let allow_interior_nudges = !is_sharp;
        let enforce_primary_axis_no_backtrack =
            matches!(edge_routing, EdgeRouting::OrthogonalRoute)
                && !is_rounded_corner
                && !is_backward
                && edge.from != edge.to;
        let target_is_angular_shape = edge_endpoint_shape_rects(diagram, geom, edge)
            .is_some_and(|(_, (_, to_shape))| matches!(to_shape, Shape::Diamond | Shape::Hexagon));
        points = apply_marker_offsets(
            &points,
            edge,
            edge_direction,
            MarkerOffsetOptions {
                is_backward,
                allow_interior_nudges,
                enforce_primary_axis_no_backtrack,
                preserve_orthogonal: preserve_orthogonal_marker_contract,
                collapse_terminal_elbows: !is_basis,
                is_curved_style: is_basis,
                is_rounded_style: is_rounded_corner && target_incoming_count >= 3,
                skip_end_pullback: preserve_orthogonal_endpoint_contract && target_is_angular_shape,
                preserve_terminal_axis: matches!(edge_routing, EdgeRouting::OrthogonalRoute)
                    && !is_rounded_corner,
            },
        );
        if matches!(edge_routing, EdgeRouting::OrthogonalRoute)
            && !is_backward
            && edge.from != edge.to
            && let Some(min_terminal_support) =
                curve_adaptive_orthogonal_terminal_support(curve, edge_radius)
        {
            enforce_primary_axis_tail_contracts_if_primary_terminal(
                &mut points,
                edge_direction,
                min_terminal_support,
            );
        }
        // Collapse tiny near-collinear jogs introduced by SVG marker offset
        // smoothing on orthogonal routing paths.
        if !is_rounded_corner
            && !is_basis
            && !preserve_orthogonal_marker_contract
            && !matches!(edge_routing, EdgeRouting::EngineProvided)
            && edge.from != edge.to
        {
            let jog_tol = if matches!(edge_routing, EdgeRouting::OrthogonalRoute) {
                30.0
            } else {
                12.0
            };
            points = collapse_tiny_straight_smoothing_jogs(&points, jog_tol);
        }
        // For backward edges with orthogonal contract, use rounded-corner routing
        // for path topology (preserves endpoint contract), but keep the user's
        // chosen curve style for actual path drawing.
        let path_curve = if preserve_orthogonal_endpoint_contract {
            Curve::Linear(CornerStyle::Rounded)
        } else {
            curve
        };
        let rendered_points = points_for_svg_path(
            &points,
            diagram.direction,
            edge_routing,
            path_curve,
            path_simplification,
            preserve_forward_orthogonal_topology,
        );
        let rank_span =
            edge_rank_span_for_svg(geom, edge).unwrap_or_else(|| edge.minlen.max(1) as usize);
        let should_enforce_basis_stems =
            is_basis && (is_backward || rank_span >= 2 || edge.minlen > 1);
        if should_enforce_basis_stems {
            basis_stem_edge_indexes.insert(index);
            if matches!(edge_routing, EdgeRouting::PolylineRoute) {
                compact_basis_stem_edge_indexes.insert(index);
            }
        }
        let rendered_points = if is_basis {
            clamp_basis_edge_endpoints_to_boundaries(diagram, geom, edge, &rendered_points)
        } else {
            rendered_points
        };
        if rendered_points.is_empty() {
            continue;
        }
        rendered_paths.insert(index, rendered_points);
    }
    PreparedRenderedEdges {
        paths: rendered_paths,
        basis_stem_edge_indexes,
        compact_basis_stem_edge_indexes,
    }
}

pub(super) fn render_edges(
    writer: &mut SvgWriter,
    diagram: &Diagram,
    prepared_edges: &PreparedRenderedEdges,
    curve: Curve,
    edge_radius: f64,
    scale: f64,
) {
    writer.start_group("edgePaths");

    let mut visible_edge_indexes: Vec<usize> = diagram
        .edges
        .iter()
        .filter(|edge| edge.stroke != Stroke::Invisible)
        .map(|edge| edge.index)
        .collect();
    visible_edge_indexes.sort_unstable();

    for index in visible_edge_indexes {
        let Some(edge) = diagram.edges.get(index) else {
            continue;
        };
        let Some(points) = prepared_edges.paths.get(&index) else {
            continue;
        };
        let enforce_basis_visible_stems = matches!(curve, Curve::Basis)
            && prepared_edges.basis_stem_edge_indexes.contains(&index);
        let compact_basis_visible_stems = enforce_basis_visible_stems
            && prepared_edges
                .compact_basis_stem_edge_indexes
                .contains(&index);
        let d = path_from_prepared_points(
            points,
            edge,
            scale,
            curve,
            edge_radius,
            enforce_basis_visible_stems,
            compact_basis_visible_stems,
        );
        if d.is_empty() {
            continue;
        }
        let mut attrs = edge_style_attrs(edge, scale);
        attrs.push_str(&edge_marker_attrs(edge));
        let line = format!("<path d=\"{d}\"{attrs} />", d = d, attrs = attrs);
        writer.push_line(&line);
    }

    writer.end_group();
}

fn point_inside_rect(rect: &Rect, point: Point) -> bool {
    let eps = 0.01;
    point.x > rect.x + eps
        && point.x < rect.x + rect.width - eps
        && point.y > rect.y + eps
        && point.y < rect.y + rect.height - eps
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentAxis {
    Horizontal,
    Vertical,
}

fn segment_axis(start: Point, end: Point) -> Option<SegmentAxis> {
    const EPS: f64 = 1e-6;
    let dx = (start.x - end.x).abs();
    let dy = (start.y - end.y).abs();
    if dx <= EPS && dy > EPS {
        Some(SegmentAxis::Vertical)
    } else if dy <= EPS && dx > EPS {
        Some(SegmentAxis::Horizontal)
    } else {
        None
    }
}

fn points_are_collinear(points: &[Point]) -> bool {
    const EPS: f64 = 1e-6;
    if points.len() <= 2 {
        return true;
    }
    let start = points[0];
    let end = points[points.len() - 1];
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() <= EPS && dy.abs() <= EPS {
        return points
            .iter()
            .all(|p| (p.x - start.x).abs() <= EPS && (p.y - start.y).abs() <= EPS);
    }
    let norm = (dx.abs() + dy.abs()).max(1.0);
    points[1..points.len() - 1].iter().all(|p| {
        let cross = (p.x - start.x) * dy - (p.y - start.y) * dx;
        cross.abs() <= EPS * norm
    })
}

fn points_approx_equal(a: Point, b: Point) -> bool {
    const EPS: f64 = 0.000_001;
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS
}

fn dedup_consecutive_svg_points(points: &[Point]) -> Vec<Point> {
    let mut deduped: Vec<Point> = Vec::with_capacity(points.len());
    for point in points.iter().copied() {
        if deduped
            .last()
            .is_some_and(|last| points_approx_equal(*last, point))
        {
            continue;
        }
        deduped.push(point);
    }
    deduped
}

fn vectors_share_ray(base: Point, candidate: Point) -> bool {
    const EPS: f64 = 1e-6;
    let cross = base.x * candidate.y - base.y * candidate.x;
    let dot = base.x * candidate.x + base.y * candidate.y;
    let base_len = (base.x * base.x + base.y * base.y).sqrt();
    let candidate_len = (candidate.x * candidate.x + candidate.y * candidate.y).sqrt();
    if base_len <= EPS || candidate_len <= EPS {
        return false;
    }
    cross.abs() <= EPS * base_len * candidate_len && dot > EPS
}

fn insert_basis_start_cap_if_needed(points: &mut Vec<Point>, min_stem: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return;
    }

    let base_vec = Point {
        x: points[1].x - points[0].x,
        y: points[1].y - points[0].y,
    };
    let first_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if first_segment_len <= EPS || first_segment_len + EPS >= min_stem {
        return;
    }

    let mut traversed = 0.0;
    for seg_idx in 0..(points.len() - 1) {
        let seg_vec = Point {
            x: points[seg_idx + 1].x - points[seg_idx].x,
            y: points[seg_idx + 1].y - points[seg_idx].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            if t >= 1.0 - EPS {
                return;
            }
            let cap = Point {
                x: points[seg_idx].x + seg_vec.x * t,
                y: points[seg_idx].y + seg_vec.y * t,
            };
            if !points_approx_equal(cap, points[seg_idx])
                && !points_approx_equal(cap, points[seg_idx + 1])
            {
                points.insert(seg_idx + 1, cap);
            }
            return;
        }
        traversed += seg_len;
    }
}

fn insert_basis_end_cap_if_needed(points: &mut Vec<Point>, min_stem: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return;
    }

    let n = points.len();
    let base_vec = Point {
        x: points[n - 2].x - points[n - 1].x,
        y: points[n - 2].y - points[n - 1].y,
    };
    let last_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if last_segment_len <= EPS || last_segment_len + EPS >= min_stem {
        return;
    }

    let mut traversed = 0.0;
    for seg_idx in (0..(points.len() - 1)).rev() {
        let seg_vec = Point {
            x: points[seg_idx].x - points[seg_idx + 1].x,
            y: points[seg_idx].y - points[seg_idx + 1].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            if t >= 1.0 - EPS {
                return;
            }
            let cap = Point {
                x: points[seg_idx + 1].x + seg_vec.x * t,
                y: points[seg_idx + 1].y + seg_vec.y * t,
            };
            if !points_approx_equal(cap, points[seg_idx + 1])
                && !points_approx_equal(cap, points[seg_idx])
            {
                points.insert(seg_idx + 1, cap);
            }
            return;
        }
        traversed += seg_len;
    }
}

fn start_cap_point_on_existing_run(points: &[Point], min_stem: f64) -> Option<(usize, Point)> {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return None;
    }

    let base_vec = Point {
        x: points[1].x - points[0].x,
        y: points[1].y - points[0].y,
    };
    let first_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if first_segment_len <= EPS {
        return None;
    }

    let mut traversed = 0.0;
    let mut last_seg_idx_on_ray = 0usize;
    for seg_idx in 0..(points.len() - 1) {
        let seg_vec = Point {
            x: points[seg_idx + 1].x - points[seg_idx].x,
            y: points[seg_idx + 1].y - points[seg_idx].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        last_seg_idx_on_ray = seg_idx;
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            let cap = Point {
                x: points[seg_idx].x + seg_vec.x * t,
                y: points[seg_idx].y + seg_vec.y * t,
            };
            return Some((seg_idx, cap));
        }
        traversed += seg_len;
    }

    Some((last_seg_idx_on_ray, points[last_seg_idx_on_ray + 1]))
}

fn rebuild_with_start_cap(points: &[Point], seg_idx: usize, cap: Point) -> Vec<Point> {
    if points.len() < 2 || seg_idx >= points.len() - 1 {
        return points.to_vec();
    }

    let mut rebuilt = Vec::with_capacity(points.len() + 1);
    rebuilt.push(points[0]);
    rebuilt.push(cap);
    rebuilt.extend_from_slice(&points[(seg_idx + 1)..]);
    dedup_consecutive_svg_points(&rebuilt)
}

fn end_cap_point_on_existing_run(points: &[Point], min_stem: f64) -> Option<(usize, Point)> {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_stem <= 0.0 {
        return None;
    }

    let n = points.len();
    let base_vec = Point {
        x: points[n - 2].x - points[n - 1].x,
        y: points[n - 2].y - points[n - 1].y,
    };
    let last_segment_len = (base_vec.x * base_vec.x + base_vec.y * base_vec.y).sqrt();
    if last_segment_len <= EPS {
        return None;
    }

    let mut traversed = 0.0;
    let mut first_seg_idx_on_ray = n - 2;
    for seg_idx in (0..(points.len() - 1)).rev() {
        let seg_vec = Point {
            x: points[seg_idx].x - points[seg_idx + 1].x,
            y: points[seg_idx].y - points[seg_idx + 1].y,
        };
        let seg_len = (seg_vec.x * seg_vec.x + seg_vec.y * seg_vec.y).sqrt();
        if seg_len <= EPS {
            continue;
        }
        if !vectors_share_ray(base_vec, seg_vec) {
            break;
        }
        first_seg_idx_on_ray = seg_idx;
        if traversed + seg_len + EPS >= min_stem {
            let t = ((min_stem - traversed) / seg_len).clamp(0.0, 1.0);
            let cap = Point {
                x: points[seg_idx + 1].x + seg_vec.x * t,
                y: points[seg_idx + 1].y + seg_vec.y * t,
            };
            return Some((seg_idx, cap));
        }
        traversed += seg_len;
    }

    Some((first_seg_idx_on_ray, points[first_seg_idx_on_ray]))
}

fn rebuild_with_end_cap(points: &[Point], seg_idx: usize, cap: Point) -> Vec<Point> {
    if points.len() < 2 || seg_idx >= points.len() - 1 {
        return points.to_vec();
    }

    let mut rebuilt = Vec::with_capacity(points.len() + 1);
    rebuilt.extend_from_slice(&points[..=seg_idx]);
    rebuilt.push(cap);
    rebuilt.push(points[points.len() - 1]);
    dedup_consecutive_svg_points(&rebuilt)
}

fn enforce_basis_visible_terminal_stems(
    points: &[Point],
    min_stem: f64,
    compact_caps: bool,
) -> Vec<Point> {
    let mut adjusted = dedup_consecutive_svg_points(points);
    if adjusted.len() < 2 || min_stem <= 0.0 {
        return adjusted;
    }

    if compact_caps {
        if let Some((seg_idx, cap)) = start_cap_point_on_existing_run(&adjusted, min_stem) {
            adjusted = rebuild_with_start_cap(&adjusted, seg_idx, cap);
        }
        if let Some((seg_idx, cap)) = end_cap_point_on_existing_run(&adjusted, min_stem) {
            adjusted = rebuild_with_end_cap(&adjusted, seg_idx, cap);
        }
    } else {
        insert_basis_start_cap_if_needed(&mut adjusted, min_stem);
        insert_basis_end_cap_if_needed(&mut adjusted, min_stem);
    }
    dedup_consecutive_svg_points(&adjusted)
}

fn clamp_basis_edge_endpoints_to_boundaries(
    diagram: &Diagram,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let mut clamped = points.to_vec();
    if let Some(sg_id) = edge.from_subgraph.as_ref()
        && let Some(sg_geom) = geom.subgraphs.get(sg_id)
    {
        clamped = clip_points_to_rect_start(&clamped, &sg_geom.rect);
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.from), geom.nodes.get(&edge.from))
    {
        let from_rect: Rect = node_geom.rect;
        if !matches!(node.shape, Shape::Diamond | Shape::Hexagon)
            && point_inside_rect(&from_rect, clamped[0])
        {
            clamped[0] = intersect_svg_node(&from_rect, clamped[1], node.shape);
        }
    }

    if clamped.len() < 2 {
        return clamped;
    }

    if let Some(sg_id) = edge.to_subgraph.as_ref()
        && let Some(sg_geom) = geom.subgraphs.get(sg_id)
    {
        clamped = clip_points_to_rect_end(&clamped, &sg_geom.rect);
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.to), geom.nodes.get(&edge.to))
    {
        let to_rect: Rect = node_geom.rect;
        let last = clamped.len() - 1;
        if !matches!(node.shape, Shape::Diamond | Shape::Hexagon)
            && point_inside_rect(&to_rect, clamped[last])
        {
            clamped[last] = intersect_svg_node(&to_rect, clamped[last - 1], node.shape);
        }
    }

    dedup_consecutive_svg_points(&clamped)
}

fn primary_axis_for_direction(direction: Direction) -> SegmentAxis {
    match direction {
        Direction::TopDown | Direction::BottomTop => SegmentAxis::Vertical,
        Direction::LeftRight | Direction::RightLeft => SegmentAxis::Horizontal,
    }
}

fn is_primary_secondary_primary_return(points: &[Point], direction: Direction) -> bool {
    if points.len() != 4 {
        return false;
    }
    let primary = primary_axis_for_direction(direction);
    let secondary = match primary {
        SegmentAxis::Horizontal => SegmentAxis::Vertical,
        SegmentAxis::Vertical => SegmentAxis::Horizontal,
    };
    matches!(
        (
            segment_axis(points[0], points[1]),
            segment_axis(points[1], points[2]),
            segment_axis(points[2], points[3]),
        ),
        (Some(a), Some(b), Some(c)) if a == primary && b == secondary && c == primary
    )
}

fn edge_rank_span_for_svg(geom: &GraphGeometry, edge: &Edge) -> Option<usize> {
    let EngineHints::Layered(hints) = geom.engine_hints.as_ref()?;
    let src_rank = *hints.node_ranks.get(&edge.from)?;
    let dst_rank = *hints.node_ranks.get(&edge.to)?;
    Some(src_rank.abs_diff(dst_rank) as usize)
}

fn adapt_basis_anchor_points(
    points: &[Point],
    edge: &Edge,
    geom: &GraphGeometry,
    direction: Direction,
    is_backward: bool,
) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    if !points_are_axis_aligned(points) {
        return dedup_consecutive_svg_points(points);
    }

    let rank_span =
        edge_rank_span_for_svg(geom, edge).unwrap_or_else(|| edge.minlen.max(1) as usize);
    let starts_on_secondary_axis = first_segment_is_secondary_axis(points, direction);
    let compact_backward_return = is_backward
        && rank_span <= 1
        && edge.minlen <= 1
        && is_primary_secondary_primary_return(points, direction);
    if compact_backward_return {
        return dedup_consecutive_svg_points(&[points[0], points[2], points[3]]);
    }
    let preserve_extended_route = is_backward || rank_span >= 2 || edge.minlen > 1;

    let adapted = if preserve_extended_route {
        if points.len() <= 4 {
            points.to_vec()
        } else {
            vec![
                points[0],
                points[1],
                points[points.len() - 2],
                points[points.len() - 1],
            ]
        }
    } else if starts_on_secondary_axis && ends_on_secondary_axis(points, direction) {
        // For short orthogonal fan-in/out chains that begin and end on the
        // secondary axis (V-H-V in LR/RL, H-V-H in TD/BT), keep the
        // final elbow anchor so marker tangent is stable, but collapse the
        // initial stem to avoid inward-first basis bends.
        vec![
            points[0],
            points[points.len() - 2],
            points[points.len() - 1],
        ]
    } else if starts_on_secondary_axis {
        vec![points[0], points[1], points[points.len() - 1]]
    } else {
        vec![points[0], points[points.len() - 1]]
    };

    dedup_consecutive_svg_points(&adapted)
}

fn first_segment_is_secondary_axis(points: &[Point], direction: Direction) -> bool {
    if points.len() < 2 {
        return false;
    }
    match segment_axis(points[0], points[1]) {
        Some(SegmentAxis::Horizontal) => {
            matches!(direction, Direction::TopDown | Direction::BottomTop)
        }
        Some(SegmentAxis::Vertical) => {
            matches!(direction, Direction::LeftRight | Direction::RightLeft)
        }
        None => false,
    }
}

fn ends_on_secondary_axis(points: &[Point], direction: Direction) -> bool {
    if points.len() < 2 {
        return false;
    }
    let last = points.len() - 1;
    match segment_axis(points[last - 1], points[last]) {
        Some(SegmentAxis::Horizontal) => {
            matches!(direction, Direction::TopDown | Direction::BottomTop)
        }
        Some(SegmentAxis::Vertical) => {
            matches!(direction, Direction::LeftRight | Direction::RightLeft)
        }
        None => false,
    }
}

fn is_simple_two_node_reciprocal_pair(diagram: &Diagram, edge: &Edge) -> bool {
    if edge.from == edge.to || edge.from_subgraph.is_some() || edge.to_subgraph.is_some() {
        return false;
    }

    let mut pair_edges = 0usize;
    let mut has_forward = false;
    let mut has_backward = false;

    for other in &diagram.edges {
        if other.stroke == Stroke::Invisible {
            continue;
        }

        let touches_endpoints = other.from == edge.from
            || other.to == edge.from
            || other.from == edge.to
            || other.to == edge.to;
        if !touches_endpoints {
            continue;
        }

        if other.from_subgraph.is_some() || other.to_subgraph.is_some() {
            return false;
        }

        let is_forward = other.from == edge.from && other.to == edge.to;
        let is_backward = other.from == edge.to && other.to == edge.from;
        if !is_forward && !is_backward {
            return false;
        }

        pair_edges += 1;
        has_forward |= is_forward;
        has_backward |= is_backward;
    }

    has_forward && has_backward && pair_edges == 2
}

fn apply_reciprocal_lane_offsets(
    start: Point,
    end: Point,
    direction: Direction,
    curve_sign: f64,
    source_rect: Rect,
    target_rect: Rect,
) -> (Point, Point) {
    let mut adjusted_start = start;
    let mut adjusted_end = end;
    // Upper lane is typically closer to center in Mermaid; lower lane sits
    // slightly deeper toward the bottom face.
    let source_upper = (source_rect.height * 0.18).clamp(8.0, 14.0);
    let target_upper = (target_rect.height * 0.18).clamp(8.0, 14.0);
    let source_lower = (source_rect.height * 0.26).clamp(10.0, 18.0);
    let target_lower = (target_rect.height * 0.26).clamp(10.0, 18.0);
    let source_lane = if curve_sign < 0.0 {
        source_upper
    } else {
        source_lower
    };
    let target_lane = if curve_sign < 0.0 {
        target_upper
    } else {
        target_lower
    };

    match direction {
        Direction::LeftRight | Direction::RightLeft => {
            let source_center = source_rect.y + source_rect.height / 2.0;
            let target_center = target_rect.y + target_rect.height / 2.0;
            adjusted_start.y = if curve_sign < 0.0 {
                source_center - source_lane
            } else {
                source_center + source_lane
            }
            .clamp(
                source_rect.y + 1.0,
                source_rect.y + source_rect.height - 1.0,
            );
            adjusted_end.y = if curve_sign < 0.0 {
                target_center - target_lane
            } else {
                target_center + target_lane
            }
            .clamp(
                target_rect.y + 1.0,
                target_rect.y + target_rect.height - 1.0,
            );
        }
        Direction::TopDown | Direction::BottomTop => {
            let source_upper = (source_rect.width * 0.18).clamp(8.0, 14.0);
            let target_upper = (target_rect.width * 0.18).clamp(8.0, 14.0);
            let source_lower = (source_rect.width * 0.26).clamp(10.0, 18.0);
            let target_lower = (target_rect.width * 0.26).clamp(10.0, 18.0);
            let source_lane = if curve_sign < 0.0 {
                source_upper
            } else {
                source_lower
            };
            let target_lane = if curve_sign < 0.0 {
                target_upper
            } else {
                target_lower
            };
            let source_center = source_rect.x + source_rect.width / 2.0;
            let target_center = target_rect.x + target_rect.width / 2.0;
            adjusted_start.x = if curve_sign < 0.0 {
                source_center - source_lane
            } else {
                source_center + source_lane
            }
            .clamp(source_rect.x + 1.0, source_rect.x + source_rect.width - 1.0);
            adjusted_end.x = if curve_sign < 0.0 {
                target_center - target_lane
            } else {
                target_center + target_lane
            }
            .clamp(target_rect.x + 1.0, target_rect.x + target_rect.width - 1.0);
        }
    }

    (adjusted_start, adjusted_end)
}

fn segment_manhattan_len(start: Point, end: Point) -> f64 {
    (start.x - end.x).abs() + (start.y - end.y).abs()
}

fn collapse_primary_face_fan_channel_for_curved(
    geom: &GraphGeometry,
    edge: &Edge,
    direction: Direction,
    points: &[Point],
    center_eps: f64,
) -> Vec<Point> {
    const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;
    const MIN_STEM_FOR_COLLAPSE: f64 = 8.0;
    const MAX_STEM_FOR_COLLAPSE: f64 = 18.0;
    const MIN_TERMINAL_STEM_FOR_COLLAPSE: f64 = 10.0;
    const MAX_TERMINAL_STEM_FOR_COLLAPSE: f64 = 22.0;
    const TERMINAL_STEM_BIAS: f64 = 3.0;
    const MIN_CHANNEL_SPAN: f64 = 4.0;

    if points.len() != 4 {
        return points.to_vec();
    }

    let Some(target_geom) = geom.nodes.get(&edge.to) else {
        return points.to_vec();
    };
    let target_rect: Rect = target_geom.rect;
    let mut out = points.to_vec();
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let first_vertical = (out[0].x - out[1].x).abs() <= center_eps
                && (out[0].y - out[1].y).abs() > center_eps;
            let middle_horizontal = (out[1].y - out[2].y).abs() <= center_eps
                && (out[1].x - out[2].x).abs() > center_eps;
            let terminal_vertical = (out[2].x - out[3].x).abs() <= center_eps
                && (out[2].y - out[3].y).abs() > center_eps;
            if !(first_vertical && middle_horizontal && terminal_vertical) {
                return points.to_vec();
            }

            let has_lateral_offset = (out[0].x - out[3].x).abs() > center_eps;
            let target_is_primary_face = match direction {
                Direction::TopDown => out[3].y <= target_rect.y + MARKER_PULLBACK_TOLERANCE,
                Direction::BottomTop => {
                    out[3].y >= target_rect.y + target_rect.height - MARKER_PULLBACK_TOLERANCE
                }
                _ => false,
            };
            if has_lateral_offset && target_is_primary_face {
                let delta = out[3].y - out[0].y;
                if delta.abs()
                    > MIN_STEM_FOR_COLLAPSE + MIN_TERMINAL_STEM_FOR_COLLAPSE + MIN_CHANNEL_SPAN
                {
                    let source_stem =
                        (delta.abs() * 0.28).clamp(MIN_STEM_FOR_COLLAPSE, MAX_STEM_FOR_COLLAPSE);
                    let max_terminal_stem = delta.abs() - source_stem - MIN_CHANNEL_SPAN;
                    if max_terminal_stem < MIN_TERMINAL_STEM_FOR_COLLAPSE {
                        return points.to_vec();
                    }
                    let terminal_stem = (delta.abs() * 0.28 + TERMINAL_STEM_BIAS)
                        .clamp(
                            MIN_TERMINAL_STEM_FOR_COLLAPSE,
                            MAX_TERMINAL_STEM_FOR_COLLAPSE,
                        )
                        .min(max_terminal_stem);
                    let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
                    out[1].y = out[0].y + (dir * source_stem);
                    out[2].y = out[3].y - (dir * terminal_stem);
                }
            }
        }
        Direction::LeftRight | Direction::RightLeft => {
            let first_horizontal = (out[0].y - out[1].y).abs() <= center_eps
                && (out[0].x - out[1].x).abs() > center_eps;
            let middle_vertical = (out[1].x - out[2].x).abs() <= center_eps
                && (out[1].y - out[2].y).abs() > center_eps;
            let terminal_horizontal = (out[2].y - out[3].y).abs() <= center_eps
                && (out[2].x - out[3].x).abs() > center_eps;
            if !(first_horizontal && middle_vertical && terminal_horizontal) {
                return points.to_vec();
            }

            let has_lateral_offset = (out[0].y - out[3].y).abs() > center_eps;
            let target_is_primary_face = match direction {
                Direction::LeftRight => out[3].x <= target_rect.x + MARKER_PULLBACK_TOLERANCE,
                Direction::RightLeft => {
                    out[3].x >= target_rect.x + target_rect.width - MARKER_PULLBACK_TOLERANCE
                }
                _ => false,
            };
            if has_lateral_offset && target_is_primary_face {
                let delta = out[3].x - out[0].x;
                if delta.abs()
                    > MIN_STEM_FOR_COLLAPSE + MIN_TERMINAL_STEM_FOR_COLLAPSE + MIN_CHANNEL_SPAN
                {
                    let source_stem =
                        (delta.abs() * 0.28).clamp(MIN_STEM_FOR_COLLAPSE, MAX_STEM_FOR_COLLAPSE);
                    let max_terminal_stem = delta.abs() - source_stem - MIN_CHANNEL_SPAN;
                    if max_terminal_stem < MIN_TERMINAL_STEM_FOR_COLLAPSE {
                        return points.to_vec();
                    }
                    let terminal_stem = (delta.abs() * 0.28 + TERMINAL_STEM_BIAS)
                        .clamp(
                            MIN_TERMINAL_STEM_FOR_COLLAPSE,
                            MAX_TERMINAL_STEM_FOR_COLLAPSE,
                        )
                        .min(max_terminal_stem);
                    let dir = if delta >= 0.0 { 1.0 } else { -1.0 };
                    out[1].x = out[0].x + (dir * source_stem);
                    out[2].x = out[3].x - (dir * terminal_stem);
                }
            }
        }
    }

    out
}

fn compact_visual_staircases(points: &[Point], short_tol: f64) -> Vec<Point> {
    if points.len() < 4 {
        return points.to_vec();
    }

    let mut compacted = points.to_vec();
    let mut i = 0usize;
    while i + 3 < compacted.len() {
        // Preserve start/end approach geometry so marker orientation keeps a
        // clear supporting segment into/out of the endpoint.
        if i == 0 || i + 3 >= compacted.len() - 1 {
            i += 1;
            continue;
        }

        let p0 = compacted[i];
        let p1 = compacted[i + 1];
        let p2 = compacted[i + 2];
        let p3 = compacted[i + 3];

        let a1 = segment_axis(p0, p1);
        let a2 = segment_axis(p1, p2);
        let a3 = segment_axis(p2, p3);

        let Some(first_axis) = a1 else {
            i += 1;
            continue;
        };
        let Some(middle_axis) = a2 else {
            i += 1;
            continue;
        };
        let Some(last_axis) = a3 else {
            i += 1;
            continue;
        };

        if first_axis != last_axis || first_axis == middle_axis {
            i += 1;
            continue;
        }

        let l1 = segment_manhattan_len(p0, p1);
        let l2 = segment_manhattan_len(p1, p2);
        let l3 = segment_manhattan_len(p2, p3);
        if l1 > short_tol || l2 > short_tol || l3 > short_tol {
            i += 1;
            continue;
        }

        let replacement = match first_axis {
            SegmentAxis::Vertical => Point { x: p0.x, y: p3.y },
            SegmentAxis::Horizontal => Point { x: p3.x, y: p0.y },
        };
        compacted.splice(i + 1..=i + 2, [replacement]);
        i = i.saturating_sub(1);
    }

    compacted
}

fn segment_rect_intersection(start: Point, end: Point, rect: &Rect) -> Option<Point> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return None;
    }

    let mut candidates: Vec<(f64, Point)> = Vec::new();

    let x_min = rect.x;
    let x_max = rect.x + rect.width;
    let y_min = rect.y;
    let y_max = rect.y + rect.height;

    if dx.abs() > f64::EPSILON {
        let t_left = (x_min - start.x) / dx;
        if (0.0..=1.0).contains(&t_left) {
            let y = start.y + t_left * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_left, Point { x: x_min, y }));
            }
        }
        let t_right = (x_max - start.x) / dx;
        if (0.0..=1.0).contains(&t_right) {
            let y = start.y + t_right * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_right, Point { x: x_max, y }));
            }
        }
    }

    if dy.abs() > f64::EPSILON {
        let t_top = (y_min - start.y) / dy;
        if (0.0..=1.0).contains(&t_top) {
            let x = start.x + t_top * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_top, Point { x, y: y_min }));
            }
        }
        let t_bottom = (y_max - start.y) / dy;
        if (0.0..=1.0).contains(&t_bottom) {
            let x = start.x + t_bottom * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_bottom, Point { x, y: y_max }));
            }
        }
    }

    candidates
        .into_iter()
        .filter(|(t, _)| *t >= 0.0 && *t <= 1.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

fn clip_points_to_rect_start(points: &[Point], rect: &Rect) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    if !point_inside_rect(rect, points[0]) {
        return points.to_vec();
    }

    let mut idx = 0usize;
    while idx + 1 < points.len() && point_inside_rect(rect, points[idx]) {
        idx += 1;
    }
    if idx == 0 || idx >= points.len() {
        return points.to_vec();
    }

    let inside = points[idx - 1];
    let outside = points[idx];
    let intersection = segment_rect_intersection(inside, outside, rect).unwrap_or(inside);

    let mut out = Vec::new();
    out.push(intersection);
    out.extend_from_slice(&points[idx..]);
    out
}

fn clip_points_to_rect_end(points: &[Point], rect: &Rect) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let last_idx = points.len() - 1;
    if !point_inside_rect(rect, points[last_idx]) {
        return points.to_vec();
    }

    let mut idx = last_idx;
    while idx > 0 && point_inside_rect(rect, points[idx]) {
        idx -= 1;
    }
    if idx == last_idx || idx >= last_idx {
        return points.to_vec();
    }

    let outside = points[idx];
    let inside = points[idx + 1];
    let intersection = segment_rect_intersection(outside, inside, rect).unwrap_or(inside);

    let mut out = points[..=idx].to_vec();
    out.push(intersection);
    out
}

fn edge_style_attrs(edge: &Edge, scale: f64) -> String {
    let stroke_width = match edge.stroke {
        Stroke::Thick => 2.0 * scale,
        _ => 1.0 * scale,
    };
    let mut attrs = format!(
        " stroke=\"{stroke}\" stroke-width=\"{width}\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
        stroke = STROKE_COLOR,
        width = fmt_f64(stroke_width)
    );
    if edge.stroke == Stroke::Dotted {
        let dash = fmt_f64(2.0 * scale);
        let gap = fmt_f64(4.0 * scale);
        let _ = write!(attrs, " stroke-dasharray=\"{dash},{gap}\"");
    }
    attrs
}

fn edge_marker_attrs(edge: &Edge) -> String {
    let mut attrs = String::new();
    match edge.arrow_start {
        Arrow::Normal => attrs.push_str(" marker-start=\"url(#arrowhead)\""),
        Arrow::Cross => attrs.push_str(" marker-start=\"url(#crosshead)\""),
        Arrow::Circle => attrs.push_str(" marker-start=\"url(#circlehead)\""),
        Arrow::OpenTriangle => attrs.push_str(" marker-start=\"url(#open-arrowhead)\""),
        Arrow::Diamond => attrs.push_str(" marker-start=\"url(#diamondhead)\""),
        Arrow::OpenDiamond => attrs.push_str(" marker-start=\"url(#open-diamondhead)\""),
        Arrow::None => {}
    }
    match edge.arrow_end {
        Arrow::Normal => attrs.push_str(" marker-end=\"url(#arrowhead)\""),
        Arrow::Cross => attrs.push_str(" marker-end=\"url(#crosshead)\""),
        Arrow::Circle => attrs.push_str(" marker-end=\"url(#circlehead)\""),
        Arrow::OpenTriangle => attrs.push_str(" marker-end=\"url(#open-arrowhead)\""),
        Arrow::Diamond => attrs.push_str(" marker-end=\"url(#diamondhead)\""),
        Arrow::OpenDiamond => attrs.push_str(" marker-end=\"url(#open-diamondhead)\""),
        Arrow::None => {}
    }
    attrs
}

fn points_for_svg_path(
    points: &[Point],
    direction: Direction,
    edge_routing: EdgeRouting,
    curve: Curve,
    path_simplification: PathSimplification,
    preserve_orthogonal_topology: bool,
) -> Vec<Point> {
    if points.is_empty() {
        return Vec::new();
    }
    // Orthogonalize when both conditions hold:
    // 1. Routing is orthogonal (OrthogonalRoute) — right-angle paths are required.
    // 2. Curve is linear — basis curves handle smoothness from sparse waypoints
    //    and do not need axis-aligned segments.
    // Corner style (sharp vs rounded) does not affect whether orthogonalization is needed;
    // both require axis-aligned points to produce correct 90° paths.
    // Direct/polyline routing intentionally allows diagonal segments — skip.
    let needs_orthogonalization =
        matches!(edge_routing, EdgeRouting::OrthogonalRoute) && matches!(curve, Curve::Linear(_));
    let points: Vec<Point> = if needs_orthogonalization && !points_are_axis_aligned(points) {
        let start: Point = points[0];
        let end: Point = points.last().copied().unwrap_or(points[0]);
        let waypoints: Vec<Point> = points
            .iter()
            .copied()
            .skip(1)
            .take(points.len().saturating_sub(2))
            .collect();
        build_orthogonal_path_float(start, end, direction, &waypoints)
    } else {
        points.to_vec()
    };
    let points = if needs_orthogonalization {
        collapse_immediate_axis_turnbacks(&points)
    } else {
        points
    };
    match path_simplification {
        PathSimplification::None => points,
        PathSimplification::Lossless => {
            let compacted = compact_visual_staircases(&points, 12.0);
            PathSimplification::Lossless
                .simplify_with_coords(&compacted, |point| (point.x, point.y))
        }
        PathSimplification::Lossy if needs_orthogonalization => {
            simplify_orthogonal_points(&points, direction, preserve_orthogonal_topology)
        }
        _ => path_simplification.simplify_with_coords(&points, |point| (point.x, point.y)),
    }
}

fn path_from_prepared_points(
    points: &[Point],
    _edge: &Edge,
    scale: f64,
    curve: Curve,
    curve_radius: f64,
    enforce_basis_visible_stems: bool,
    compact_basis_visible_stems: bool,
) -> String {
    if points.is_empty() {
        return String::new();
    }
    match curve {
        Curve::Basis => {
            let basis_points = if enforce_basis_visible_stems {
                enforce_basis_visible_terminal_stems(
                    points,
                    MIN_BASIS_VISIBLE_STEM_PX,
                    compact_basis_visible_stems,
                )
            } else {
                dedup_consecutive_svg_points(points)
            };
            let scaled: Vec<(f64, f64)> = basis_points
                .iter()
                .map(|point| (point.x * scale, point.y * scale))
                .collect();
            if enforce_basis_visible_stems {
                path_from_points_curved_with_explicit_caps(&scaled)
            } else {
                path_from_points_curved(&scaled)
            }
        }
        Curve::Linear(CornerStyle::Rounded) => {
            let scaled: Vec<(f64, f64)> = points
                .iter()
                .map(|point| (point.x * scale, point.y * scale))
                .collect();
            path_from_points_rounded(&scaled, curve_radius * scale)
        }
        Curve::Linear(CornerStyle::Sharp) => {
            let scaled: Vec<(f64, f64)> = points
                .iter()
                .map(|point| (point.x * scale, point.y * scale))
                .collect();
            path_from_points_straight(&scaled)
        }
    }
}

fn simplify_orthogonal_points(
    points: &[Point],
    direction: Direction,
    preserve_orthogonal_topology: bool,
) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let compacted =
        PathSimplification::Lossless.simplify_with_coords(points, |point| (point.x, point.y));
    if preserve_orthogonal_topology && compacted.len() > 4 {
        return compacted;
    }

    let start = compacted[0];
    let end = compacted[compacted.len() - 1];
    if segment_axis(start, end).is_some() {
        return vec![start, end];
    }

    let elbow = match direction {
        Direction::TopDown | Direction::BottomTop => Point {
            x: start.x,
            y: end.y,
        },
        Direction::LeftRight | Direction::RightLeft => Point {
            x: end.x,
            y: start.y,
        },
    };
    vec![start, elbow, end]
}

type EndpointShapeRect = (Rect, Shape);

fn edge_endpoint_shape_rects(
    diagram: &Diagram,
    geom: &GraphGeometry,
    edge: &Edge,
) -> Option<(EndpointShapeRect, EndpointShapeRect)> {
    let from = if let Some(sg_id) = edge.from_subgraph.as_ref() {
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect;
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.from)?.rect;
        let node = diagram.nodes.get(&edge.from)?;
        (node_rect, node.shape)
    };

    let to = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect;
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.to)?.rect;
        let node = diagram.nodes.get(&edge.to)?;
        (node_rect, node.shape)
    };

    Some((from, to))
}

fn orthogonal_route_edge_direction(
    diagram: &Diagram,
    node_directions: &HashMap<String, Direction>,
    override_nodes: &HashMap<String, String>,
    from: &str,
    to: &str,
    fallback: Direction,
) -> Direction {
    let from_sg = override_nodes.get(from);
    let to_sg = override_nodes.get(to);

    match (from_sg, to_sg) {
        (None, None) => effective_edge_direction(node_directions, from, to, fallback),
        (Some(sg_a), Some(sg_b)) if sg_a == sg_b => diagram
            .subgraphs
            .get(sg_a.as_str())
            .and_then(|sg| sg.dir)
            .unwrap_or_else(|| effective_edge_direction(node_directions, from, to, fallback)),
        _ => orthogonal_route_cross_boundary_direction(
            diagram,
            node_directions,
            from_sg,
            to_sg,
            from,
            to,
            fallback,
        ),
    }
}

fn orthogonal_route_cross_boundary_direction(
    diagram: &Diagram,
    node_directions: &HashMap<String, Direction>,
    from_sg: Option<&String>,
    to_sg: Option<&String>,
    from_node: &str,
    to_node: &str,
    fallback: Direction,
) -> Direction {
    if let (Some(sg_a), Some(sg_b)) = (from_sg, to_sg) {
        if is_ancestor_subgraph(diagram, sg_a, sg_b) {
            return diagram
                .subgraphs
                .get(sg_a.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        if is_ancestor_subgraph(diagram, sg_b, sg_a) {
            return diagram
                .subgraphs
                .get(sg_b.as_str())
                .and_then(|sg| sg.dir)
                .unwrap_or(fallback);
        }
        return fallback;
    }

    let outside_node = if from_sg.is_some() && to_sg.is_none() {
        to_node
    } else {
        from_node
    };
    node_directions
        .get(outside_node)
        .copied()
        .unwrap_or(fallback)
}

fn is_ancestor_subgraph(diagram: &Diagram, ancestor: &str, descendant: &str) -> bool {
    let mut current = descendant;
    while let Some(parent) = diagram
        .subgraphs
        .get(current)
        .and_then(|sg| sg.parent.as_deref())
    {
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

fn should_adjust_rerouted_edge_endpoints(
    diagram: &Diagram,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
    direction: Direction,
) -> bool {
    const FACE_PROXIMITY: f64 = 6.0;
    const EPS: f64 = 0.5;
    if points.len() < 2 {
        return false;
    }

    let Some(((from_rect, _), (to_rect, _))) = edge_endpoint_shape_rects(diagram, geom, edge)
    else {
        return false;
    };

    // For orthogonal routing, the router produces authoritative endpoint geometry.
    // Keep intentional non-flow-face attachments (e.g. fan-in overflow) but
    // still re-adjust when endpoints drift inside/outside or violate expected
    // flow faces on the primary axis.
    if endpoint_drifted_inside_or_outside(points[0], from_rect, EPS)
        || endpoint_drifted_inside_or_outside(points[points.len() - 1], to_rect, EPS)
    {
        return true;
    }

    let is_backward = geom.reversed_edges.contains(&edge.index);
    if is_backward {
        return endpoint_attachment_is_invalid(points[0], from_rect, direction, true, true, EPS)
            || endpoint_attachment_is_invalid(
                points[points.len() - 1],
                to_rect,
                direction,
                false,
                true,
                EPS,
            );
    }

    if !endpoint_on_non_flow_face(points[0], from_rect, direction, FACE_PROXIMITY)
        && endpoint_attachment_is_invalid(points[0], from_rect, direction, true, false, EPS)
    {
        return true;
    }
    if !endpoint_on_non_flow_face(points[points.len() - 1], to_rect, direction, FACE_PROXIMITY)
        && endpoint_attachment_is_invalid(
            points[points.len() - 1],
            to_rect,
            direction,
            false,
            false,
            EPS,
        )
    {
        return true;
    }

    false
}

fn endpoint_drifted_inside_or_outside(point: Point, rect: Rect, eps: f64) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if point.x < left - eps
        || point.x > right + eps
        || point.y < top - eps
        || point.y > bottom + eps
    {
        return true;
    }

    point_inside_rect(&rect, point)
}

fn endpoint_on_non_flow_face(
    point: Point,
    rect: Rect,
    proximity: Direction,
    face_tol: f64,
) -> bool {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    let near_left = (point.x - left) <= face_tol;
    let near_right = (right - point.x) <= face_tol;
    let near_top = (point.y - top) <= face_tol;
    let near_bottom = (bottom - point.y) <= face_tol;

    match proximity {
        Direction::TopDown | Direction::BottomTop => near_left || near_right,
        Direction::LeftRight | Direction::RightLeft => near_top || near_bottom,
    }
}

fn endpoint_attachment_is_invalid(
    point: Point,
    rect: Rect,
    direction: Direction,
    is_source: bool,
    is_backward: bool,
    eps: f64,
) -> bool {
    const FACE_PROXIMITY: f64 = 6.0;
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    if point.x < left - eps
        || point.x > right + eps
        || point.y < top - eps
        || point.y > bottom + eps
    {
        return true;
    }

    if is_backward {
        // Backward endpoints can legitimately land on different faces
        // depending on local routing contracts (e.g. TD parity bottom entry vs
        // LR/RL right/bottom channels, LR backward source departing from bottom
        // face). Treat any near-boundary attachment as valid and only reclip
        // when the endpoint drifts into the interior.
        let near_left = (point.x - left) <= FACE_PROXIMITY;
        let near_right = (right - point.x) <= FACE_PROXIMITY;
        let near_top = (point.y - top) <= FACE_PROXIMITY;
        let near_bottom = (bottom - point.y) <= FACE_PROXIMITY;
        return !(near_left || near_right || near_top || near_bottom);
    }

    let is_forward_source = is_source != is_backward;

    match direction {
        Direction::TopDown => {
            if is_forward_source {
                (bottom - point.y) > FACE_PROXIMITY
            } else {
                (point.y - top) > FACE_PROXIMITY
            }
        }
        Direction::BottomTop => {
            if is_forward_source {
                (point.y - top) > FACE_PROXIMITY
            } else {
                (bottom - point.y) > FACE_PROXIMITY
            }
        }
        Direction::LeftRight => {
            if is_forward_source {
                (right - point.x) > FACE_PROXIMITY
            } else {
                (point.x - left) > FACE_PROXIMITY
            }
        }
        Direction::RightLeft => {
            if is_forward_source {
                (point.x - left) > FACE_PROXIMITY
            } else {
                (right - point.x) > FACE_PROXIMITY
            }
        }
    }
}

fn adjust_edge_points_for_shapes(
    diagram: &Diagram,
    geom: &GraphGeometry,
    edge: &Edge,
    points: &[Point],
    direction: Direction,
    is_backward: bool,
    edge_routing: EdgeRouting,
) -> Vec<Point> {
    const EPS: f64 = 0.5;
    if points.len() < 2 {
        return points.to_vec();
    }

    let Some(((from_rect, from_shape), (to_rect, to_shape))) =
        edge_endpoint_shape_rects(diagram, geom, edge)
    else {
        return points.to_vec();
    };

    let mut adjusted = points.to_vec();
    let is_self_loop = edge.from == edge.to;
    // In orthogonal routing mode the router already places non-rect shape
    // endpoints on the actual shape boundary (with marker clearance for
    // backward edges) — these are authoritative and must not be re-projected
    // (different approach angles would shift them).
    // In polyline routing mode the layout only clips to the bounding rect, so non-rect
    // shapes always need re-projection to the actual shape boundary.
    let router_placed_source = matches!(edge_routing, EdgeRouting::OrthogonalRoute)
        && !is_self_loop
        && matches!(from_shape, Shape::Diamond | Shape::Hexagon);
    let router_placed_target = matches!(edge_routing, EdgeRouting::OrthogonalRoute)
        && !is_self_loop
        && matches!(to_shape, Shape::Diamond | Shape::Hexagon);
    let source_needs_adjustment = !router_placed_source
        && (matches!(from_shape, Shape::Diamond | Shape::Hexagon)
            || endpoint_attachment_is_invalid(
                points[0],
                from_rect,
                direction,
                true,
                is_backward,
                EPS,
            ));
    let target_needs_adjustment = !router_placed_target
        && (matches!(to_shape, Shape::Diamond | Shape::Hexagon)
            || endpoint_attachment_is_invalid(
                points[points.len() - 1],
                to_rect,
                direction,
                false,
                is_backward,
                EPS,
            ));

    if source_needs_adjustment {
        let from_target = if points.len() > 1 {
            points[1]
        } else {
            from_rect.center()
        };
        adjusted[0] = intersect_svg_node(&from_rect, from_target, from_shape);
    }

    if target_needs_adjustment {
        let to_target = if points.len() > 1 {
            points[points.len() - 2]
        } else {
            to_rect.center()
        };
        let last = adjusted.len() - 1;
        adjusted[last] = intersect_svg_node(&to_rect, to_target, to_shape);
    }

    adjusted
}

fn fix_corner_points(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut corner_positions = Vec::new();
    for i in 1..points.len() - 1 {
        let prev = points[i - 1];
        let curr = points[i];
        let next = points[i + 1];
        let dx_prev = (curr.x - prev.x).abs();
        let dy_prev = (curr.y - prev.y).abs();
        let dx_next = (next.x - curr.x).abs();
        let dy_next = (next.y - curr.y).abs();

        let is_corner =
            (prev.x == curr.x && (curr.y - next.y).abs() > 5.0 && dx_next > 5.0 && dy_prev > 5.0)
                || (prev.y == curr.y
                    && (curr.x - next.x).abs() > 5.0
                    && dx_prev > 5.0
                    && dy_next > 5.0);

        if is_corner {
            corner_positions.push(i);
        }
    }

    if corner_positions.is_empty() {
        return points.to_vec();
    }

    let mut out = Vec::new();
    for i in 0..points.len() {
        if !corner_positions.contains(&i) {
            out.push(points[i]);
            continue;
        }

        let prev = points[i - 1];
        let curr = points[i];
        let next = points[i + 1];

        let new_prev = find_adjacent_point(prev, curr, 5.0);
        let new_next = find_adjacent_point(next, curr, 5.0);

        let x_diff = new_next.x - new_prev.x;
        let y_diff = new_next.y - new_prev.y;
        out.push(new_prev);

        let mut new_corner = curr;
        let a = (2.0_f64).sqrt() * 2.0;
        if (next.x - prev.x).abs() > 10.0 && (next.y - prev.y).abs() >= 10.0 {
            let r = 5.0;
            if (curr.x - new_prev.x).abs() < f64::EPSILON {
                new_corner = Point {
                    x: if x_diff < 0.0 {
                        new_prev.x - r + a
                    } else {
                        new_prev.x + r - a
                    },
                    y: if y_diff < 0.0 {
                        new_prev.y - a
                    } else {
                        new_prev.y + a
                    },
                };
            } else {
                new_corner = Point {
                    x: if x_diff < 0.0 {
                        new_prev.x - a
                    } else {
                        new_prev.x + a
                    },
                    y: if y_diff < 0.0 {
                        new_prev.y - r + a
                    } else {
                        new_prev.y + r - a
                    },
                };
            }
        }

        out.push(new_corner);
        out.push(new_next);
    }

    out
}

fn collapse_tiny_straight_smoothing_jogs(points: &[Point], short_tol: f64) -> Vec<Point> {
    if points.len() < 4 || short_tol <= 0.0 {
        return points.to_vec();
    }

    let mut collapsed = points.to_vec();
    let mut idx = 1usize;
    while idx + 1 < collapsed.len() {
        let prev = collapsed[idx - 1];
        let curr = collapsed[idx];
        let next = collapsed[idx + 1];

        let prev_axis = segment_axis(prev, curr);
        let next_axis = segment_axis(curr, next);
        let prev_len = ((curr.x - prev.x).powi(2) + (curr.y - prev.y).powi(2)).sqrt();
        let next_len = ((next.x - curr.x).powi(2) + (next.y - curr.y).powi(2)).sqrt();
        let both_diagonal = prev_axis.is_none() && next_axis.is_none();
        let axis_then_diagonal = prev_axis.is_some() && next_axis.is_none();
        let diagonal_then_axis = prev_axis.is_none() && next_axis.is_some();

        let v1x = curr.x - prev.x;
        let v1y = curr.y - prev.y;
        let v2x = next.x - curr.x;
        let v2y = next.y - curr.y;
        let dot = v1x * v2x + v1y * v2y;

        let should_collapse = (both_diagonal && (prev_len < short_tol || next_len < short_tol))
            || (axis_then_diagonal && prev_len < short_tol)
            || (diagonal_then_axis && next_len < short_tol);
        if should_collapse && dot > 0.0 {
            collapsed.remove(idx);
            idx = idx.saturating_sub(1).max(1);
            continue;
        }

        idx += 1;
    }

    collapsed
}

fn collapse_immediate_axis_turnbacks(points: &[Point]) -> Vec<Point> {
    const EPS: f64 = 1e-6;
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut current = points.to_vec();
    loop {
        let mut changed = false;
        let mut reduced = Vec::with_capacity(current.len());
        reduced.push(current[0]);

        for idx in 1..(current.len() - 1) {
            let prev = *reduced.last().expect("reduced is non-empty");
            let curr = current[idx];
            let next = current[idx + 1];

            let should_drop = match (segment_axis(prev, curr), segment_axis(curr, next)) {
                (Some(SegmentAxis::Vertical), Some(SegmentAxis::Vertical)) => {
                    let d1 = curr.y - prev.y;
                    let d2 = next.y - curr.y;
                    d1.abs() > EPS && d2.abs() > EPS && d1.signum() != d2.signum()
                }
                (Some(SegmentAxis::Horizontal), Some(SegmentAxis::Horizontal)) => {
                    let d1 = curr.x - prev.x;
                    let d2 = next.x - curr.x;
                    d1.abs() > EPS && d2.abs() > EPS && d1.signum() != d2.signum()
                }
                _ => false,
            };

            if should_drop {
                changed = true;
                continue;
            }
            reduced.push(curr);
        }

        reduced.push(*current.last().expect("points has at least two elements"));
        reduced.dedup_by(|a, b| (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS);

        if !changed {
            return reduced;
        }
        current = reduced;
        if current.len() <= 2 {
            return current;
        }
    }
}

fn points_are_axis_aligned(points: &[Point]) -> bool {
    if points.len() < 2 {
        return true;
    }
    points
        .windows(2)
        .all(|seg| segment_axis(seg[0], seg[1]).is_some())
}

fn find_adjacent_point(point_a: Point, point_b: Point, distance: f64) -> Point {
    let x_diff = point_b.x - point_a.x;
    let y_diff = point_b.y - point_a.y;
    let length = (x_diff * x_diff + y_diff * y_diff).sqrt();
    if length <= f64::EPSILON {
        return point_b;
    }
    let ratio = distance / length;
    Point {
        x: point_b.x - ratio * x_diff,
        y: point_b.y - ratio * y_diff,
    }
}

#[derive(Debug, Clone, Copy)]
struct MarkerOffsetOptions {
    is_backward: bool,
    allow_interior_nudges: bool,
    enforce_primary_axis_no_backtrack: bool,
    preserve_orthogonal: bool,
    collapse_terminal_elbows: bool,
    is_curved_style: bool,
    is_rounded_style: bool,
    skip_end_pullback: bool,
    preserve_terminal_axis: bool,
}

fn apply_marker_offsets(
    points: &[Point],
    edge: &Edge,
    direction: Direction,
    options: MarkerOffsetOptions,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let MarkerOffsetOptions {
        is_backward,
        allow_interior_nudges,
        enforce_primary_axis_no_backtrack,
        preserve_orthogonal,
        collapse_terminal_elbows,
        is_curved_style,
        is_rounded_style,
        skip_end_pullback,
        preserve_terminal_axis,
    } = options;
    let expected_end_axis = preserve_terminal_axis
        .then(|| {
            if points.len() >= 2 {
                segment_axis(points[points.len() - 2], points[points.len() - 1])
            } else {
                None
            }
        })
        .flatten();

    let mut start_offset: f64 = match edge.arrow_start {
        Arrow::Normal | Arrow::OpenTriangle => 4.0,
        Arrow::Diamond | Arrow::OpenDiamond => 5.0,
        // Cross and circle markers have refX past the visible shape,
        // so the marker already sits before the endpoint — no pullback needed.
        Arrow::Cross | Arrow::Circle | Arrow::None => 0.0,
    };
    let mut end_offset: f64 = match edge.arrow_end {
        Arrow::Normal | Arrow::OpenTriangle => 4.0,
        Arrow::Diamond | Arrow::OpenDiamond => 5.0,
        Arrow::Cross | Arrow::Circle | Arrow::None => 0.0,
    };
    if skip_end_pullback {
        end_offset = 0.0;
    }

    let mut points = points.to_vec();
    if preserve_orthogonal {
        // When endpoint support is still diagonal at this stage, orthogonal
        // post-processing may shorten the visible terminal stem significantly
        // after marker pullback. In that case, skip endpoint pullback so the
        // final arrow keeps a clear supporting segment.
        if segment_axis(points[0], points[1]).is_none() {
            start_offset = 0.0;
        }
        if segment_axis(points[points.len() - 2], points[points.len() - 1]).is_none() {
            end_offset = 0.0;
        }
    }
    if !preserve_orthogonal && collapse_terminal_elbows && !is_backward {
        // Non-orth styles (straight/rounded/curved) can look visually cramped when
        // an orthogonal route ends with a short final elbow immediately before
        // the marker. Collapse that elbow into a direct terminal approach.
        // Skip for backward edges: their initial face-to-lane segment is an
        // essential part of the channel routing topology, not a cosmetic elbow.
        points = collapse_narrow_terminal_elbows_for_non_orth(&points, 14.0, is_rounded_style);
    }
    if !preserve_orthogonal && is_backward {
        // Backward edges in non-orth styles can still end up visually cramped
        // after marker pullback. Keep a minimum visible support at both ends.
        const MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT: f64 = 10.0;
        points = enforce_min_orthogonal_endpoint_support(
            &points,
            start_offset + MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT,
            end_offset + MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT,
        );
        let start_support = segment_manhattan_len(points[0], points[1]);
        let end_support = segment_manhattan_len(points[points.len() - 2], points[points.len() - 1]);
        start_offset =
            start_offset.min((start_support - MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT).max(0.0));
        end_offset =
            end_offset.min((end_support - MIN_NON_ORTH_BACKWARD_ENDPOINT_SUPPORT).max(0.0));
    }
    if preserve_orthogonal {
        // Keep endpoint support visibly longer than marker pullback so the
        // terminal stem remains readable in orthogonal mode.
        const MIN_ENDPOINT_SUPPORT: f64 = 12.0;
        const MIN_BACKWARD_CURVED_ENDPOINT_SUPPORT: f64 = 20.0;
        let min_endpoint_support = if is_backward && is_curved_style {
            MIN_BACKWARD_CURVED_ENDPOINT_SUPPORT
        } else {
            MIN_ENDPOINT_SUPPORT
        };
        let start_min_endpoint_support = if edge.arrow_start == Arrow::None {
            0.0
        } else {
            min_endpoint_support
        };
        let end_min_endpoint_support = if edge.arrow_end == Arrow::None {
            0.0
        } else {
            min_endpoint_support
        };
        // Save original endpoints before support extension so we can detect
        // when extension shifts the source/target off the node boundary.
        let original_start = points[0];
        let original_end = points[points.len() - 1];
        points = enforce_min_orthogonal_endpoint_support(
            &points,
            start_offset + start_min_endpoint_support,
            end_offset + end_min_endpoint_support,
        );

        // For backward edges, enforce_min_orthogonal_endpoint_support may shift
        // the source (or target) off the node face when extending a short terminal
        // segment — the extension propagates through collinear points to maintain
        // orthogonality. Re-insert the original endpoint as a connecting stem so
        // the edge remains visually attached to the node.
        if is_backward {
            const DRIFT_EPS: f64 = 0.5;
            let start_drifted = (points[0].x - original_start.x).abs() > DRIFT_EPS
                || (points[0].y - original_start.y).abs() > DRIFT_EPS;
            let end_drifted = {
                let last = points.len() - 1;
                (points[last].x - original_end.x).abs() > DRIFT_EPS
                    || (points[last].y - original_end.y).abs() > DRIFT_EPS
            };
            if start_drifted {
                points.insert(0, original_start);
            }
            if end_drifted {
                points.push(original_end);
            }
        }

        // Keep a visible endpoint stem in orthogonal mode so marker pullback
        // cannot invert the terminal segment direction.
        let start_support = segment_manhattan_len(points[0], points[1]);
        let end_support = segment_manhattan_len(points[points.len() - 2], points[points.len() - 1]);
        start_offset = start_offset.min((start_support - start_min_endpoint_support).max(0.0));
        end_offset = end_offset.min((end_support - end_min_endpoint_support).max(0.0));
    }

    let mut out = Vec::with_capacity(points.len());
    let start = points[0];
    let end = points[points.len() - 1];
    let direction_x = if start.x < end.x { "left" } else { "right" };
    let direction_y = if start.y < end.y { "down" } else { "up" };
    for (i, point) in points.iter().enumerate() {
        let mut offset_x = 0.0;
        if i == 0 && start_offset > 0.0 {
            offset_x = marker_offset_component(points[0], points[1], start_offset, true);
        } else if i == points.len() - 1 && end_offset > 0.0 {
            offset_x = marker_offset_component(
                points[points.len() - 1],
                points[points.len() - 2],
                end_offset,
                true,
            );
        }

        let diff_end = (point.x - end.x).abs();
        let diff_in_y_end = (point.y - end.y).abs();
        let diff_start = (point.x - start.x).abs();
        let diff_in_y_start = (point.y - start.y).abs();
        let extra_room = 1.0;

        if allow_interior_nudges && !preserve_orthogonal {
            if end_offset > 0.0
                && diff_end < end_offset
                && diff_end > 0.0
                && diff_in_y_end < end_offset
            {
                let mut adjustment = end_offset + extra_room - diff_end;
                if direction_x == "right" {
                    adjustment *= -1.0;
                }
                offset_x -= adjustment;
            }

            if start_offset > 0.0
                && diff_start < start_offset
                && diff_start > 0.0
                && diff_in_y_start < start_offset
            {
                let mut adjustment = start_offset + extra_room - diff_start;
                if direction_x == "right" {
                    adjustment *= -1.0;
                }
                offset_x += adjustment;
            }
        }

        let mut offset_y = 0.0;
        if i == 0 && start_offset > 0.0 {
            offset_y = marker_offset_component(points[0], points[1], start_offset, false);
        } else if i == points.len() - 1 && end_offset > 0.0 {
            offset_y = marker_offset_component(
                points[points.len() - 1],
                points[points.len() - 2],
                end_offset,
                false,
            );
        }

        let diff_end_y = (point.y - end.y).abs();
        let diff_in_x_end = (point.x - end.x).abs();
        let diff_start_y = (point.y - start.y).abs();
        let diff_in_x_start = (point.x - start.x).abs();

        if allow_interior_nudges && !preserve_orthogonal {
            if end_offset > 0.0
                && diff_end_y < end_offset
                && diff_end_y > 0.0
                && diff_in_x_end < end_offset
            {
                let mut adjustment = end_offset + extra_room - diff_end_y;
                if direction_y == "up" {
                    adjustment *= -1.0;
                }
                offset_y -= adjustment;
            }

            if start_offset > 0.0
                && diff_start_y < start_offset
                && diff_start_y > 0.0
                && diff_in_x_start < start_offset
            {
                let mut adjustment = start_offset + extra_room - diff_start_y;
                if direction_y == "up" {
                    adjustment *= -1.0;
                }
                offset_y += adjustment;
            }
        }

        out.push(Point {
            x: point.x + offset_x,
            y: point.y + offset_y,
        });
    }

    if enforce_primary_axis_no_backtrack && !preserve_orthogonal {
        enforce_primary_axis_tail_contracts(&mut out, direction, 8.0);
    }
    if let Some(axis) = expected_end_axis {
        preserve_path_terminal_axis(&mut out, axis);
    }

    out
}

fn preserve_path_terminal_axis(points: &mut [Point], axis: SegmentAxis) {
    if points.len() < 3 {
        return;
    }
    let last = points.len() - 1;
    if segment_axis(points[last - 1], points[last]) == Some(axis) {
        return;
    }

    let prev = points[last - 2];
    let end = points[last];
    let candidate = match axis {
        SegmentAxis::Vertical => Point {
            x: end.x,
            y: prev.y,
        },
        SegmentAxis::Horizontal => Point {
            x: prev.x,
            y: end.y,
        },
    };

    if segment_axis(prev, candidate).is_some()
        && segment_axis(candidate, end) == Some(axis)
        && !points_approx_equal(candidate, end)
    {
        points[last - 1] = candidate;
    }
}

fn curve_adaptive_orthogonal_terminal_support(curve: Curve, edge_radius: f64) -> Option<f64> {
    match curve {
        // Rounded corners trim the visible straight stem by approximately the
        // corner radius, so scale required support with radius.
        Curve::Linear(CornerStyle::Rounded) => Some((10.0 + edge_radius).max(12.0)),
        // Basis smoothing softens terminal approach segments; keep a longer
        // pre-target support to preserve readable entry direction.
        Curve::Basis => Some(16.0),
        Curve::Linear(CornerStyle::Sharp) => None,
    }
}

fn enforce_primary_axis_tail_contracts_if_primary_terminal(
    points: &mut [Point],
    direction: Direction,
    min_terminal_support: f64,
) {
    if points.len() < 3 || min_terminal_support <= 0.0 {
        return;
    }
    let n = points.len();
    let expected_axis = match direction {
        Direction::TopDown | Direction::BottomTop => SegmentAxis::Vertical,
        Direction::LeftRight | Direction::RightLeft => SegmentAxis::Horizontal,
    };
    if segment_axis(points[n - 2], points[n - 1]) != Some(expected_axis) {
        return;
    }
    enforce_primary_axis_tail_contracts(points, direction, min_terminal_support);
}

/// Synthesize two control points for a 2-point bezier path so the B-spline
/// produces an outward-bowing S-curve.
///
/// The first control point keeps the source's cross-axis position (vertical
/// departure), the second keeps the target's (vertical arrival). Together
/// they create an S-curve that bows outward for both fan-in and fan-out.
fn synthesize_bezier_control_points(
    start: Point,
    end: Point,
    direction: Direction,
) -> (Point, Point) {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let dy = end.y - start.y;
            (
                Point {
                    x: start.x,
                    y: start.y + dy / 3.0,
                },
                Point {
                    x: end.x,
                    y: start.y + 2.0 * dy / 3.0,
                },
            )
        }
        Direction::LeftRight | Direction::RightLeft => {
            let dx = end.x - start.x;
            (
                Point {
                    x: start.x + dx / 3.0,
                    y: start.y,
                },
                Point {
                    x: start.x + 2.0 * dx / 3.0,
                    y: end.y,
                },
            )
        }
    }
}

/// Synthesize control points for reciprocal two-point edges (A->B and B->A)
/// so Mermaid-layered bezier renders as separated upper/lower arcs.
fn synthesize_reciprocal_bezier_control_points(
    start: Point,
    end: Point,
    direction: Direction,
    curve_sign: f64,
) -> (Point, Point) {
    match direction {
        Direction::TopDown | Direction::BottomTop => {
            let dy = end.y - start.y;
            let bow = (dy.abs() * 0.25).clamp(12.0, 28.0) * curve_sign;
            (
                Point {
                    x: start.x + bow,
                    y: start.y + dy / 3.0,
                },
                Point {
                    x: end.x + bow,
                    y: start.y + 2.0 * dy / 3.0,
                },
            )
        }
        Direction::LeftRight | Direction::RightLeft => {
            let dx = end.x - start.x;
            let bow = (dx.abs() * 0.25).clamp(12.0, 28.0) * curve_sign;
            (
                Point {
                    x: start.x + dx / 3.0,
                    y: start.y + bow,
                },
                Point {
                    x: start.x + 2.0 * dx / 3.0,
                    y: end.y + bow,
                },
            )
        }
    }
}

fn collapse_narrow_terminal_elbows_for_non_orth(
    points: &[Point],
    min_terminal_leg: f64,
    preserve_axis: bool,
) -> Vec<Point> {
    if points.len() < 4 || min_terminal_leg <= 0.0 {
        return points.to_vec();
    }

    let mut collapsed = points.to_vec();

    if collapsed.len() >= 4 {
        let n = collapsed.len();
        let before_pre = (n >= 5).then(|| collapsed[n - 4]);
        let pre = collapsed[n - 3];
        let elbow = collapsed[n - 2];
        let end = collapsed[n - 1];
        let pre_axis = segment_axis(pre, elbow);
        let end_axis = segment_axis(elbow, end);
        if let (Some(a), Some(b)) = (pre_axis, end_axis)
            && a != b
            && segment_manhattan_len(elbow, end) < min_terminal_leg
            && segment_manhattan_len(pre, end) > 0.001
        {
            let mut replacement_pre = pre;
            match b {
                SegmentAxis::Horizontal => replacement_pre.y = end.y,
                SegmentAxis::Vertical => replacement_pre.x = end.x,
            }
            if preserve_axis {
                if segment_axis(replacement_pre, end).is_some()
                    && before_pre.is_none_or(|pp| segment_axis(pp, replacement_pre).is_some())
                    && segment_manhattan_len(replacement_pre, end) > 0.001
                {
                    collapsed[n - 3] = replacement_pre;
                    collapsed.remove(n - 2);
                }
            } else {
                if segment_axis(replacement_pre, end).is_some()
                    && segment_manhattan_len(replacement_pre, end) > 0.001
                {
                    collapsed[n - 3] = replacement_pre;
                }
                collapsed.remove(n - 2);
            }
        }
    }

    if collapsed.len() >= 4 {
        let start = collapsed[0];
        let elbow = collapsed[1];
        let post = collapsed[2];
        let start_axis = segment_axis(start, elbow);
        let post_axis = segment_axis(elbow, post);
        if let (Some(a), Some(b)) = (start_axis, post_axis)
            && a != b
            && segment_manhattan_len(start, elbow) < min_terminal_leg
            && segment_manhattan_len(start, post) > 0.001
        {
            collapsed.remove(1);
        }
    }

    collapsed
}

fn enforce_primary_axis_tail_contracts(
    points: &mut [Point],
    direction: Direction,
    min_terminal_support: f64,
) {
    if points.len() < 2 || min_terminal_support <= 0.0 {
        return;
    }

    let n = points.len();
    let end_idx = n - 1;
    let penult_idx = n - 2;

    match direction {
        Direction::TopDown => {
            let target_penult_y = points[end_idx].y - min_terminal_support;
            if points[penult_idx].y > target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y > points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Direction::BottomTop => {
            let target_penult_y = points[end_idx].y + min_terminal_support;
            if points[penult_idx].y < target_penult_y {
                points[penult_idx].y = target_penult_y;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].y < points[penult_idx].y {
                    points[pre_idx].y = points[penult_idx].y;
                }
            }
        }
        Direction::LeftRight => {
            let target_penult_x = points[end_idx].x - min_terminal_support;
            if points[penult_idx].x > target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x > points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
        Direction::RightLeft => {
            let target_penult_x = points[end_idx].x + min_terminal_support;
            if points[penult_idx].x < target_penult_x {
                points[penult_idx].x = target_penult_x;
            }
            if n >= 3 {
                let pre_idx = n - 3;
                if points[pre_idx].x < points[penult_idx].x {
                    points[pre_idx].x = points[penult_idx].x;
                }
            }
        }
    }
}

fn enforce_min_orthogonal_endpoint_support(
    points: &[Point],
    min_start_support: f64,
    min_end_support: f64,
) -> Vec<Point> {
    let mut adjusted = points.to_vec();
    extend_endpoint_support(&mut adjusted, true, min_start_support);
    extend_endpoint_support(&mut adjusted, false, min_end_support);
    adjusted
}

fn extend_endpoint_support(points: &mut Vec<Point>, at_start: bool, min_support: f64) {
    const EPS: f64 = 1e-6;
    if points.len() < 2 || min_support <= 0.0 {
        return;
    }

    let (anchor_idx, adjacent_idx, before_adjacent_idx, before_before_adjacent_idx) = if at_start {
        (
            0usize,
            1usize,
            (points.len() > 2).then_some(2usize),
            (points.len() > 3).then_some(3usize),
        )
    } else {
        let n = points.len();
        (n - 1, n - 2, n.checked_sub(3), n.checked_sub(4))
    };

    let anchor = points[anchor_idx];
    let adjacent = points[adjacent_idx];
    let Some(axis) = segment_axis(adjacent, anchor) else {
        return;
    };
    let current_support = segment_manhattan_len(adjacent, anchor);
    if current_support >= min_support {
        return;
    }

    let new_adjacent = match axis {
        SegmentAxis::Vertical => {
            let sign = if anchor.y >= adjacent.y { 1.0 } else { -1.0 };
            Point {
                x: anchor.x,
                y: anchor.y - sign * min_support,
            }
        }
        SegmentAxis::Horizontal => {
            let sign = if anchor.x >= adjacent.x { 1.0 } else { -1.0 };
            Point {
                x: anchor.x - sign * min_support,
                y: anchor.y,
            }
        }
    };

    points[adjacent_idx] = new_adjacent;
    let Some(before_adjacent_idx) = before_adjacent_idx else {
        return;
    };

    let before_adjacent = points[before_adjacent_idx];
    if segment_axis(before_adjacent, new_adjacent).is_some() {
        collapse_endpoint_axis_backtrack(points, at_start);
        return;
    }

    // Prefer shifting the elbow coordinate (when possible) over inserting a
    // new jog segment; this avoids tiny backtracks near turns.
    let mut shifted_before = before_adjacent;
    match axis {
        SegmentAxis::Vertical => shifted_before.y = new_adjacent.y,
        SegmentAxis::Horizontal => shifted_before.x = new_adjacent.x,
    }
    let keeps_adjacent_axis = segment_axis(shifted_before, new_adjacent).is_some();
    let keeps_prev_axis = before_before_adjacent_idx
        .and_then(|idx| points.get(idx).copied())
        .is_none_or(|prev| segment_axis(prev, shifted_before).is_some());
    if keeps_adjacent_axis {
        // Prefer coordinate shifting over elbow insertion. If the direct shift
        // breaks the previous segment, try shifting that previous point onto the
        // same axis first.
        if !keeps_prev_axis {
            if let Some(prev_idx) = before_before_adjacent_idx
                && let Some(prev) = points.get(prev_idx).copied()
            {
                let shifted_prev = match axis {
                    SegmentAxis::Vertical => Point {
                        x: prev.x,
                        y: shifted_before.y,
                    },
                    SegmentAxis::Horizontal => Point {
                        x: shifted_before.x,
                        y: prev.y,
                    },
                };
                let keeps_next_axis = segment_axis(shifted_prev, shifted_before).is_some();
                let keeps_prev_axis = prev_idx
                    .checked_sub(1)
                    .and_then(|idx| points.get(idx).copied())
                    .is_none_or(|pprev| segment_axis(pprev, shifted_prev).is_some());
                if keeps_next_axis && keeps_prev_axis {
                    points[before_adjacent_idx] = shifted_before;
                    points[prev_idx] = shifted_prev;
                    collapse_endpoint_axis_backtrack(points, at_start);
                    return;
                }
            }
        } else {
            points[before_adjacent_idx] = shifted_before;
            // If this shift introduced a tiny stair-step just before the endpoint,
            // propagate the same coordinate shift backward through the contiguous
            // collinear run so orthogonal tails stay visually clean.
            let mut propagate_idx = before_before_adjacent_idx;
            while let Some(idx) = propagate_idx {
                let Some(current) = points.get(idx).copied() else {
                    break;
                };
                let should_shift = match axis {
                    SegmentAxis::Vertical => (current.y - before_adjacent.y).abs() <= EPS,
                    SegmentAxis::Horizontal => (current.x - before_adjacent.x).abs() <= EPS,
                };
                if !should_shift {
                    break;
                }

                let candidate = match axis {
                    SegmentAxis::Vertical => Point {
                        x: current.x,
                        y: shifted_before.y,
                    },
                    SegmentAxis::Horizontal => Point {
                        x: shifted_before.x,
                        y: current.y,
                    },
                };
                let keeps_next_axis = points
                    .get(idx + 1)
                    .copied()
                    .is_some_and(|next| segment_axis(candidate, next).is_some());
                let keeps_prev_axis = idx
                    .checked_sub(1)
                    .and_then(|prev_idx| points.get(prev_idx).copied())
                    .is_none_or(|prev| segment_axis(prev, candidate).is_some());
                if !keeps_next_axis || !keeps_prev_axis {
                    break;
                }

                points[idx] = candidate;
                propagate_idx = idx.checked_sub(1);
            }
            collapse_endpoint_axis_backtrack(points, at_start);
            return;
        }
    }

    let elbow = match axis {
        SegmentAxis::Vertical => Point {
            x: before_adjacent.x,
            y: new_adjacent.y,
        },
        SegmentAxis::Horizontal => Point {
            x: new_adjacent.x,
            y: before_adjacent.y,
        },
    };

    if at_start {
        points.insert(2, elbow);
    } else {
        let insert_at = points.len() - 2;
        points.insert(insert_at, elbow);
    }
    collapse_endpoint_axis_backtrack(points, at_start);
}

fn collapse_endpoint_axis_backtrack(points: &mut Vec<Point>, at_start: bool) {
    const EPS: f64 = 1e-6;
    if points.len() < 4 {
        return;
    }

    let (outer_idx, middle_idx, inner_idx) = if at_start {
        (3usize, 2usize, 1usize)
    } else {
        let n = points.len();
        (n - 4, n - 3, n - 2)
    };

    let outer = points[outer_idx];
    let middle = points[middle_idx];
    let inner = points[inner_idx];
    let Some(first_axis) = segment_axis(outer, middle) else {
        return;
    };
    let Some(second_axis) = segment_axis(middle, inner) else {
        return;
    };
    if first_axis != second_axis {
        return;
    }

    let (delta1, delta2) = match first_axis {
        SegmentAxis::Vertical => (middle.y - outer.y, inner.y - middle.y),
        SegmentAxis::Horizontal => (middle.x - outer.x, inner.x - middle.x),
    };
    if delta1.abs() <= EPS || delta2.abs() <= EPS {
        return;
    }
    if delta1.signum() != delta2.signum() {
        points.remove(middle_idx);
    }
}

fn marker_offset_component(point_a: Point, point_b: Point, offset: f64, use_x: bool) -> f64 {
    let delta_x = point_b.x - point_a.x;
    let delta_y = point_b.y - point_a.y;
    let angle = if delta_x.abs() < f64::EPSILON {
        if delta_y >= 0.0 {
            std::f64::consts::FRAC_PI_2
        } else {
            -std::f64::consts::FRAC_PI_2
        }
    } else {
        (delta_y / delta_x).atan()
    };

    if use_x {
        offset * angle.cos() * if delta_x >= 0.0 { 1.0 } else { -1.0 }
    } else {
        offset * angle.sin().abs() * if delta_y >= 0.0 { 1.0 } else { -1.0 }
    }
}

fn intersect_svg_node(rect: &Rect, point: Point, shape: Shape) -> Point {
    match shape {
        Shape::Diamond => intersect_svg_diamond(rect, point),
        Shape::Hexagon => intersect_svg_hexagon(rect, point),
        _ => intersect_svg_rect(rect, point),
    }
}

/// Hexagon boundary intersection using polygon-ray from the shared kernel.
fn intersect_svg_hexagon(rect: &Rect, point: Point) -> Point {
    use crate::graph::geometry::{FPoint, FRect};
    let frect = FRect::new(rect.x, rect.y, rect.width, rect.height);
    let verts = hexagon_vertices(frect);
    let center = FPoint::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let approach = FPoint::new(point.x, point.y);
    let result = intersect_convex_polygon(&verts, approach, center);
    Point {
        x: result.x,
        y: result.y,
    }
}

fn intersect_svg_rect(rect: &Rect, point: Point) -> Point {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = point.x - cx;
    let dy = point.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return Point { x: cx, y: cy + h };
    }

    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        let h = if dy < 0.0 { -h } else { h };
        (h * dx / dy, h)
    } else {
        let w = if dx < 0.0 { -w } else { w };
        (w, w * dy / dx)
    };

    Point {
        x: cx + sx,
        y: cy + sy,
    }
}

fn intersect_svg_diamond(rect: &Rect, point: Point) -> Point {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let dx = point.x - cx;
    let dy = point.y - cy;
    let w = rect.width / 2.0;
    let h = rect.height / 2.0;

    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return Point { x: cx, y: cy + h };
    }

    let t = 1.0 / (dx.abs() / w + dy.abs() / h);
    Point {
        x: cx + t * dx,
        y: cy + t * dy,
    }
}

fn path_from_points_straight(points: &[(f64, f64)]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            let _ = write!(d, "M{},{}", fmt_f64(*x), fmt_f64(*y));
        } else {
            let _ = write!(d, " L{},{}", fmt_f64(*x), fmt_f64(*y));
        }
    }
    d
}

fn append_curved_path_commands(d: &mut String, points: &[(f64, f64)], emit_move: bool) {
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        if emit_move {
            let (x, y) = points[0];
            let _ = write!(d, "M{},{}", fmt_f64(x), fmt_f64(y));
        }
        return;
    }

    let mut x0 = f64::NAN;
    let mut x1 = f64::NAN;
    let mut y0 = f64::NAN;
    let mut y1 = f64::NAN;
    let mut point = 0;

    for &(x, y) in points {
        match point {
            0 => {
                point = 1;
                if emit_move {
                    let _ = write!(d, "M{},{}", fmt_f64(x), fmt_f64(y));
                }
            }
            1 => {
                point = 2;
            }
            2 => {
                point = 3;
                let px = (5.0 * x0 + x1) / 6.0;
                let py = (5.0 * y0 + y1) / 6.0;
                let _ = write!(d, " L{},{}", fmt_f64(px), fmt_f64(py));
                curved_bezier(d, x0, y0, x1, y1, x, y);
            }
            _ => {
                curved_bezier(d, x0, y0, x1, y1, x, y);
            }
        }
        x0 = x1;
        x1 = x;
        y0 = y1;
        y1 = y;
    }

    match point {
        3 => {
            curved_bezier(d, x0, y0, x1, y1, x1, y1);
            let _ = write!(d, " L{},{}", fmt_f64(x1), fmt_f64(y1));
        }
        2 => {
            let _ = write!(d, " L{},{}", fmt_f64(x1), fmt_f64(y1));
        }
        _ => {}
    }
}

fn path_from_points_curved(points: &[(f64, f64)]) -> String {
    let mut d = String::new();
    append_curved_path_commands(&mut d, points, true);
    d
}

fn points_approx_equal_xy(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= 0.001 && (a.1 - b.1).abs() <= 0.001
}

fn path_from_points_curved_with_explicit_caps(points: &[(f64, f64)]) -> String {
    if points.len() < 2 {
        return path_from_points_curved(points);
    }

    let start_cap_enabled = points.len() >= 3;
    let end_cap_enabled = points.len() >= 3;
    if !start_cap_enabled && !end_cap_enabled {
        return path_from_points_curved(points);
    }

    let last = points.len() - 1;
    let core_start = if start_cap_enabled { 1 } else { 0 };
    let core_end_exclusive = if end_cap_enabled { last } else { last + 1 };
    if core_end_exclusive <= core_start {
        return path_from_points_curved(points);
    }
    let mut core: Vec<(f64, f64)> = points[core_start..core_end_exclusive].to_vec();
    if core.len() < 2 {
        return path_from_points_curved(points);
    }
    if core.len() == 2 {
        let a = core[0];
        let b = core[1];
        let mut elbow = if (a.1 - b.1).abs() >= (a.0 - b.0).abs() {
            (a.0, b.1)
        } else {
            (b.0, a.1)
        };
        if points_approx_equal_xy(elbow, a) || points_approx_equal_xy(elbow, b) {
            elbow = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        }
        core.insert(1, elbow);
    }

    let mut d = String::new();
    let start = points[0];
    let _ = write!(d, "M{},{}", fmt_f64(start.0), fmt_f64(start.1));
    let mut current = start;

    if start_cap_enabled {
        let start_cap = points[1];
        if !points_approx_equal_xy(current, start_cap) {
            let _ = write!(d, " L{},{}", fmt_f64(start_cap.0), fmt_f64(start_cap.1));
        }
        current = start_cap;
    }

    if !core.is_empty() {
        let core_start_point = core[0];
        if !points_approx_equal_xy(current, core_start_point) {
            let _ = write!(
                d,
                " L{},{}",
                fmt_f64(core_start_point.0),
                fmt_f64(core_start_point.1)
            );
        }
        append_curved_path_commands(&mut d, &core, false);
        if let Some(last_core) = core.last().copied() {
            current = last_core;
        }
    }

    if end_cap_enabled {
        let end = points[last];
        if !points_approx_equal_xy(current, end) {
            let _ = write!(d, " L{},{}", fmt_f64(end.0), fmt_f64(end.1));
        }
    }

    d
}

fn curved_bezier(d: &mut String, x0: f64, y0: f64, x1: f64, y1: f64, x: f64, y: f64) {
    let c1x = (2.0 * x0 + x1) / 3.0;
    let c1y = (2.0 * y0 + y1) / 3.0;
    let c2x = (x0 + 2.0 * x1) / 3.0;
    let c2y = (y0 + 2.0 * y1) / 3.0;
    let ex = (x0 + 4.0 * x1 + x) / 6.0;
    let ey = (y0 + 4.0 * y1 + y) / 6.0;
    let _ = write!(
        d,
        " C{},{} {},{} {},{}",
        fmt_f64(c1x),
        fmt_f64(c1y),
        fmt_f64(c2x),
        fmt_f64(c2y),
        fmt_f64(ex),
        fmt_f64(ey)
    );
}

fn path_from_points_rounded(points: &[(f64, f64)], radius: f64) -> String {
    if points.is_empty() {
        return String::new();
    }
    if points.len() < 3 || radius <= 0.0 {
        return path_from_points_straight(points);
    }

    let mut d = String::new();
    let (x0, y0) = points[0];
    let _ = write!(d, "M{},{}", fmt_f64(x0), fmt_f64(y0));

    for i in 1..points.len() - 1 {
        let (px, py) = points[i - 1];
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];

        let v1x = cx - px;
        let v1y = cy - py;
        let v2x = nx - cx;
        let v2y = ny - cy;

        let len1 = (v1x * v1x + v1y * v1y).sqrt();
        let len2 = (v2x * v2x + v2y * v2y).sqrt();
        if len1 <= f64::EPSILON || len2 <= f64::EPSILON {
            let _ = write!(d, " L{},{}", fmt_f64(cx), fmt_f64(cy));
            continue;
        }

        let v1nx = v1x / len1;
        let v1ny = v1y / len1;
        let v2nx = v2x / len2;
        let v2ny = v2y / len2;

        let cross = v1nx * v2ny - v1ny * v2nx;
        let dot = v1nx * v2nx + v1ny * v2ny;
        if cross.abs() < 1e-3 && dot.abs() > 0.999 {
            let _ = write!(d, " L{},{}", fmt_f64(cx), fmt_f64(cy));
            continue;
        }

        let r = radius.min(len1 / 2.0).min(len2 / 2.0);
        if r <= f64::EPSILON {
            let _ = write!(d, " L{},{}", fmt_f64(cx), fmt_f64(cy));
            continue;
        }

        let p1x = cx - v1nx * r;
        let p1y = cy - v1ny * r;
        let p2x = cx + v2nx * r;
        let p2y = cy + v2ny * r;

        let _ = write!(d, " L{},{}", fmt_f64(p1x), fmt_f64(p1y));
        let _ = write!(
            d,
            " Q{},{} {},{}",
            fmt_f64(cx),
            fmt_f64(cy),
            fmt_f64(p2x),
            fmt_f64(p2y)
        );
    }

    let (lx, ly) = points[points.len() - 1];
    let _ = write!(d, " L{},{}", fmt_f64(lx), fmt_f64(ly));
    d
}

/// Generate sine wave as SVG path L-segments (matching Mermaid's generateFullSineWavePoints).
/// The wave starts at (x_start, y_center) with sin(0)=0 and traverses `width` pixels
/// using 0.8 cycles and 50 line segments.
fn sine_wave_segments(x_start: f64, y_center: f64, width: f64, amplitude: f64) -> String {
    let steps = 50usize;
    let freq = std::f64::consts::TAU * 0.8 / width;
    let mut d = String::new();
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = x_start + t * width;
        let y = y_center + amplitude * (freq * t * width).sin();
        let _ = write!(d, " L{},{}", fmt_f64(x), fmt_f64(y));
    }
    d
}

/// Build a closed SVG path for a document shape (straight top/sides, sine wave bottom).
pub(super) fn document_svg_path(x: f64, y: f64, w: f64, h: f64, wave_amp: f64) -> String {
    let wave_y = y + h - wave_amp;
    let mut d = format!("M{},{}", fmt_f64(x), fmt_f64(wave_y));
    d.push_str(&sine_wave_segments(x, wave_y, w, wave_amp));
    let _ = write!(d, " L{},{}", fmt_f64(x + w), fmt_f64(y));
    let _ = write!(d, " L{},{}", fmt_f64(x), fmt_f64(y));
    d.push_str(" Z");
    d
}

pub(super) fn polygon_points(points: &[(f64, f64)]) -> String {
    let mut out = String::new();
    for (idx, (x, y)) in points.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{x},{y}", x = fmt_f64(*x), y = fmt_f64(*y));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        Point, path_from_points_curved, path_from_points_rounded, simplify_orthogonal_points,
    };
    use crate::graph::Direction;

    #[test]
    fn path_from_points_curved_emits_cubic_commands_for_multi_point_path() {
        let path = path_from_points_curved(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('C'));
        assert!(path.ends_with("L10.00,10.00"));
    }

    #[test]
    fn path_from_points_rounded_emits_quadratic_corner_commands() {
        let path = path_from_points_rounded(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], 4.0);

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('Q'));
        assert!(path.ends_with("L10.00,10.00"));
    }

    #[test]
    fn simplify_orthogonal_points_collapses_to_single_elbow_when_allowed() {
        let points = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 0.0, y: 1.0 },
            Point { x: 0.0, y: 2.0 },
            Point { x: 1.0, y: 2.0 },
            Point { x: 1.0, y: 3.0 },
            Point { x: 2.0, y: 3.0 },
        ];

        assert_eq!(
            simplify_orthogonal_points(&points, Direction::TopDown, false),
            vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 0.0, y: 3.0 },
                Point { x: 2.0, y: 3.0 },
            ]
        );
    }

    #[test]
    fn simplify_orthogonal_points_preserves_compacted_topology_when_requested() {
        let points = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 0.0, y: 1.0 },
            Point { x: 0.0, y: 2.0 },
            Point { x: 1.0, y: 2.0 },
            Point { x: 1.0, y: 3.0 },
            Point { x: 2.0, y: 3.0 },
        ];

        assert_eq!(
            simplify_orthogonal_points(&points, Direction::TopDown, true),
            vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 0.0, y: 2.0 },
                Point { x: 1.0, y: 2.0 },
                Point { x: 1.0, y: 3.0 },
                Point { x: 2.0, y: 3.0 },
            ]
        );
    }
}
