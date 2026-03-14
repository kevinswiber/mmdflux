use std::fmt::Write;

use super::super::writer::fmt_f64;
use super::super::{MIN_BASIS_VISIBLE_STEM_PX, Point};
use super::basis::enforce_basis_visible_terminal_stems;
use super::{
    collapse_immediate_axis_turnbacks, compact_visual_staircases, dedup_consecutive_svg_points,
    points_are_axis_aligned, segment_axis,
};
use crate::format::{CornerStyle, Curve};
use crate::graph::routing::{EdgeRouting, build_orthogonal_path_float};
use crate::graph::{Direction, Edge};
use crate::simplification::PathSimplification;

pub(super) fn points_for_svg_path(
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
        let start = points[0];
        let end = points[points.len() - 1];
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

pub(super) fn path_from_prepared_points(
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
            let scaled = scaled_points(&basis_points, scale);
            if enforce_basis_visible_stems {
                path_from_points_curved_with_explicit_caps(&scaled)
            } else {
                path_from_points_curved(&scaled)
            }
        }
        Curve::Linear(corner_style) => {
            let scaled = scaled_points(points, scale);
            match corner_style {
                CornerStyle::Rounded => path_from_points_rounded(&scaled, curve_radius * scale),
                CornerStyle::Sharp => path_from_points_straight(&scaled),
            }
        }
    }
}

fn scaled_points(points: &[Point], scale: f64) -> Vec<(f64, f64)> {
    points
        .iter()
        .map(|point| (point.x * scale, point.y * scale))
        .collect()
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
pub(in super::super) fn document_svg_path(x: f64, y: f64, w: f64, h: f64, wave_amp: f64) -> String {
    let wave_y = y + h - wave_amp;
    let mut d = format!("M{},{}", fmt_f64(x), fmt_f64(wave_y));
    d.push_str(&sine_wave_segments(x, wave_y, w, wave_amp));
    let _ = write!(d, " L{},{}", fmt_f64(x + w), fmt_f64(y));
    let _ = write!(d, " L{},{}", fmt_f64(x), fmt_f64(y));
    d.push_str(" Z");
    d
}

pub(in super::super) fn polygon_points(points: &[(f64, f64)]) -> String {
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
        CornerStyle, Curve, Direction, Edge, Point, path_from_points_curved,
        path_from_points_rounded, path_from_prepared_points, simplify_orthogonal_points,
    };

    #[test]
    fn curved_path_helpers_stay_local_to_path_emit() {
        let path = path_from_points_curved(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('C'));
        assert!(path.ends_with("L10.00,10.00"));
    }

    #[test]
    fn rounded_path_helpers_stay_local_to_path_emit() {
        let path = path_from_points_rounded(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], 4.0);

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('Q'));
        assert!(path.ends_with("L10.00,10.00"));
    }

    #[test]
    fn path_from_prepared_points_supports_curved_mode() {
        let path = path_from_prepared_points(
            &[
                Point { x: 0.0, y: 0.0 },
                Point { x: 10.0, y: 0.0 },
                Point { x: 10.0, y: 10.0 },
            ],
            &Edge::new("A", "B"),
            1.0,
            Curve::Basis,
            4.0,
            false,
            false,
        );

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('C'));
    }

    #[test]
    fn path_from_prepared_points_supports_rounded_mode() {
        let path = path_from_prepared_points(
            &[
                Point { x: 0.0, y: 0.0 },
                Point { x: 10.0, y: 0.0 },
                Point { x: 10.0, y: 10.0 },
            ],
            &Edge::new("A", "B"),
            1.0,
            Curve::Linear(CornerStyle::Rounded),
            4.0,
            false,
            false,
        );

        assert!(path.starts_with("M0.00,0.00"));
        assert!(path.contains('Q'));
    }

    #[test]
    fn path_from_prepared_points_supports_straight_mode() {
        let path = path_from_prepared_points(
            &[
                Point { x: 0.0, y: 0.0 },
                Point { x: 10.0, y: 0.0 },
                Point { x: 10.0, y: 10.0 },
            ],
            &Edge::new("A", "B"),
            1.0,
            Curve::Linear(CornerStyle::Sharp),
            4.0,
            false,
            false,
        );

        assert!(path.starts_with("M0.00,0.00"));
        assert!(!path.contains('Q'));
        assert!(!path.contains('C'));
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
