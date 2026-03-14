//! Edge path preparation, shaping, and SVG path emission for graph rendering.

mod basis;
mod endpoints;
mod markers;
mod path_emit;

use std::collections::{HashMap, HashSet};

use basis::{
    adapt_basis_anchor_points, apply_reciprocal_lane_offsets,
    clamp_basis_edge_endpoints_to_boundaries, collapse_primary_face_fan_channel_for_curved,
    edge_rank_span_for_svg, is_simple_two_node_reciprocal_pair, synthesize_bezier_control_points,
    synthesize_reciprocal_bezier_control_points,
};
use endpoints::{
    adjust_edge_points_for_shapes, clip_points_to_rect_end, clip_points_to_rect_start,
    edge_endpoint_shape_rects, fix_corner_points, intersect_svg_node,
    orthogonal_route_edge_direction, should_adjust_rerouted_edge_endpoints,
};
use markers::{
    MarkerOffsetOptions, apply_marker_offsets, collapse_tiny_straight_smoothing_jogs,
    curve_adaptive_orthogonal_terminal_support, edge_marker_attrs, edge_style_attrs,
    enforce_primary_axis_tail_contracts_if_primary_terminal,
};
pub(super) use path_emit::{document_svg_path, polygon_points};
use path_emit::{path_from_prepared_points, points_for_svg_path};

use super::writer::SvgWriter;
use super::{Point, Rect};
use crate::format::{CornerStyle, Curve};
use crate::graph::geometry::GraphGeometry;
use crate::graph::routing::EdgeRouting;
use crate::graph::{Graph, Shape, Stroke};
use crate::simplification::PathSimplification;

#[allow(clippy::too_many_arguments)]
pub(super) struct PreparedRenderedEdges {
    pub(super) paths: HashMap<usize, Vec<Point>>,
    basis_stem_edge_indexes: HashSet<usize>,
    compact_basis_stem_edge_indexes: HashSet<usize>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_rendered_edge_paths(
    diagram: &Graph,
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
    diagram: &Graph,
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

fn segment_manhattan_len(start: Point, end: Point) -> f64 {
    (start.x - end.x).abs() + (start.y - end.y).abs()
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
