use std::collections::HashMap;
use std::fs;
use std::path::Path;

use mmdflux::diagram::{
    CornerStyle, EdgeRouting, InterpolationStyle, OutputFormat, PathSimplification, RenderConfig,
    RoutingStyle,
};
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::routing::route_graph_geometry;
use mmdflux::graph::Stroke;
use mmdflux::registry::DiagramInstance;
use mmdflux::render::{RenderOptions, render_svg};
use mmdflux::{EngineConfig, build_diagram, parse_flowchart};

/// Extract SVG node center x-coordinates by label text.
///
/// Scans the SVG for `<text ...>Label</text>` elements and returns a map of label -> x coordinate.
fn extract_node_x_positions(svg: &str) -> HashMap<String, f64> {
    let mut positions = HashMap::new();
    for line in svg.lines() {
        let line = line.trim();
        if !line.starts_with("<text") || !line.contains("dominant-baseline") {
            continue;
        }
        // Extract x value from x="..."
        let x_val = line.find("x=\"").and_then(|start| {
            let rest = &line[start + 3..];
            rest.find('"')
                .and_then(|end| rest[..end].parse::<f64>().ok())
        });
        // Extract text content between >...</text>
        let label = line.find("</text>").and_then(|end| {
            let before = &line[..end];
            before
                .rfind('>')
                .map(|start| before[start + 1..].to_string())
        });
        if let (Some(x), Some(label)) = (x_val, label)
            && !label.is_empty()
        {
            positions.insert(label, x);
        }
    }
    positions
}

fn edge_path_data(svg: &str) -> Vec<String> {
    svg.lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("<path d=\"")
                && (line.contains("marker-end=") || line.contains("marker-start="))
        })
        .filter_map(|line| {
            let start = line.find("d=\"")?;
            let after = &line[start + 3..];
            let end = after.find('"')?;
            Some(after[..end].to_string())
        })
        .collect()
}

fn parse_svg_path_points(path_data: &str) -> Vec<(f64, f64)> {
    path_data
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            let (x, y) = token.split_once(',')?;
            let x = x.parse::<f64>().ok()?;
            let y = y.parse::<f64>().ok()?;
            Some((x, y))
        })
        .collect()
}

fn parse_svg_text_position_and_value(line: &str) -> Option<(f64, f64, String)> {
    let line = line.trim();
    if !line.starts_with("<text") {
        return None;
    }
    let x = parse_attr_f64(line, "x")?;
    let y = parse_attr_f64(line, "y")?;
    let end = line.find("</text>")?;
    let before = &line[..end];
    let start = before.rfind('>')?;
    let value = before[start + 1..].to_string();
    Some((x, y, value))
}

fn extract_edge_label_positions(
    svg: &str,
    diagram: &mmdflux::Diagram,
) -> Vec<(String, (f64, f64))> {
    let mut remaining: HashMap<String, usize> = HashMap::new();
    for edge in &diagram.edges {
        if let Some(label) = &edge.label {
            *remaining.entry(label.clone()).or_insert(0) += 1;
        }
    }

    let mut labels = Vec::new();
    for line in svg.lines() {
        let Some((x, y, value)) = parse_svg_text_position_and_value(line) else {
            continue;
        };
        let Some(count) = remaining.get_mut(&value) else {
            continue;
        };
        if *count == 0 {
            continue;
        }
        *count -= 1;
        labels.push((value, (x, y)));
    }
    labels
}

fn euclidean_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn distance_point_to_svg_segment(point: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= 0.000_001 {
        return euclidean_distance(point, a);
    }

    let projection = ((point.0 - a.0) * dx + (point.1 - a.1) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = (a.0 + t * dx, a.1 + t * dy);
    euclidean_distance(point, closest)
}

fn distance_point_to_svg_path(point: (f64, f64), path: &[(f64, f64)]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return euclidean_distance(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_svg_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

fn svg_label_drift_failures(
    svg: &str,
    diagram: &mmdflux::Diagram,
    max_distance: f64,
) -> Vec<String> {
    let expected_labels = diagram
        .edges
        .iter()
        .filter(|edge| edge.label.is_some())
        .count();
    let label_positions = extract_edge_label_positions(svg, diagram);
    let paths: Vec<Vec<(f64, f64)>> = edge_path_data(svg)
        .iter()
        .map(|path| parse_svg_path_points(path))
        .collect();

    let mut failures = Vec::new();
    if label_positions.len() != expected_labels {
        failures.push(format!(
            "edge-label extraction mismatch: expected={expected_labels}, extracted={}",
            label_positions.len()
        ));
    }

    for (label, point) in label_positions {
        let drift = paths
            .iter()
            .map(|path| distance_point_to_svg_path(point, path))
            .fold(f64::INFINITY, f64::min);
        if drift > max_distance {
            failures.push(format!(
                "label {label:?} at ({:.2}, {:.2}) drift={drift:.2} exceeds {max_distance:.2}",
                point.0, point.1
            ));
        }
    }

    failures
}

fn total_svg_edge_segments(svg: &str) -> usize {
    edge_path_data(svg)
        .iter()
        .map(|d| parse_svg_path_points(d).len().saturating_sub(1))
        .sum()
}

fn svg_point_face(rect: (f64, f64, f64, f64), point: (f64, f64)) -> &'static str {
    let eps = 0.5;
    let (x, y, w, h) = rect;
    let left = x;
    let right = x + w;
    let top = y;
    let bottom = y + h;

    let on_right = (point.0 - right).abs() <= eps;
    let on_left = (point.0 - left).abs() <= eps;
    let on_top = (point.1 - top).abs() <= eps;
    let on_bottom = (point.1 - bottom).abs() <= eps;

    if on_right && point.1 > top + eps && point.1 < bottom - eps {
        "right"
    } else if on_left && point.1 > top + eps && point.1 < bottom - eps {
        "left"
    } else if on_top && point.0 > left + eps && point.0 < right - eps {
        "top"
    } else if on_bottom && point.0 > left + eps && point.0 < right - eps {
        "bottom"
    } else if on_right {
        "right"
    } else if on_left {
        "left"
    } else {
        "interior_or_corner"
    }
}

fn svg_terminal_approach_face(rect: (f64, f64, f64, f64), points: &[(f64, f64)]) -> &'static str {
    if points.is_empty() {
        return "interior_or_corner";
    }

    let end = *points.last().expect("path should have at least one point");
    let direct_face = svg_point_face(rect, end);
    if direct_face != "interior_or_corner" {
        return direct_face;
    }

    if points.len() < 2 {
        return direct_face;
    }

    let prev = points[points.len() - 2];
    let dx = end.0 - prev.0;
    let dy = end.1 - prev.1;
    let (x, y, w, h) = rect;
    let left = x;
    let right = x + w;
    let top = y;
    let bottom = y + h;
    const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;

    // SVG marker pullback can leave the terminal path point just outside the
    // node border. Treat that as the attached face when the terminal tangent
    // points inward toward the node.
    if end.0 > right
        && end.0 - right <= MARKER_PULLBACK_TOLERANCE
        && end.1 >= top - MARKER_PULLBACK_TOLERANCE
        && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
        && dy.abs() <= 0.5
        && dx < 0.0
    {
        return "right";
    }
    if end.0 < left
        && left - end.0 <= MARKER_PULLBACK_TOLERANCE
        && end.1 >= top - MARKER_PULLBACK_TOLERANCE
        && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
        && dy.abs() <= 0.5
        && dx > 0.0
    {
        return "left";
    }
    if end.1 > bottom
        && end.1 - bottom <= MARKER_PULLBACK_TOLERANCE
        && end.0 >= left - MARKER_PULLBACK_TOLERANCE
        && end.0 <= right + MARKER_PULLBACK_TOLERANCE
        && dx.abs() <= 0.5
        && dy < 0.0
    {
        return "bottom";
    }
    if end.1 < top
        && top - end.1 <= MARKER_PULLBACK_TOLERANCE
        && end.0 >= left - MARKER_PULLBACK_TOLERANCE
        && end.0 <= right + MARKER_PULLBACK_TOLERANCE
        && dx.abs() <= 0.5
        && dy > 0.0
    {
        return "top";
    }

    if dx.abs() >= dy.abs() {
        if dx > 0.0 {
            "right"
        } else if dx < 0.0 {
            "left"
        } else {
            "interior_or_corner"
        }
    } else if dy > 0.0 {
        "bottom"
    } else if dy < 0.0 {
        "top"
    } else {
        "interior_or_corner"
    }
}

fn svg_terminal_approach_face_relaxed(
    rect: (f64, f64, f64, f64),
    points: &[(f64, f64)],
) -> &'static str {
    if points.is_empty() {
        return "interior_or_corner";
    }

    let end = *points.last().expect("path should have at least one point");
    let direct_face = svg_point_face(rect, end);
    if direct_face != "interior_or_corner" {
        return direct_face;
    }
    if points.len() < 2 {
        return direct_face;
    }

    let prev = points[points.len() - 2];
    let dx = end.0 - prev.0;
    let dy = end.1 - prev.1;
    let (x, y, w, h) = rect;
    let left = x;
    let right = x + w;
    let top = y;
    let bottom = y + h;
    const MARKER_PULLBACK_TOLERANCE: f64 = 6.0;

    if end.0 > right
        && end.0 - right <= MARKER_PULLBACK_TOLERANCE
        && end.1 >= top - MARKER_PULLBACK_TOLERANCE
        && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
        && dx < 0.0
    {
        return "right";
    }
    if end.0 < left
        && left - end.0 <= MARKER_PULLBACK_TOLERANCE
        && end.1 >= top - MARKER_PULLBACK_TOLERANCE
        && end.1 <= bottom + MARKER_PULLBACK_TOLERANCE
        && dx > 0.0
    {
        return "left";
    }
    if end.1 > bottom
        && end.1 - bottom <= MARKER_PULLBACK_TOLERANCE
        && end.0 >= left - MARKER_PULLBACK_TOLERANCE
        && end.0 <= right + MARKER_PULLBACK_TOLERANCE
        && dy < 0.0
    {
        return "bottom";
    }
    if end.1 < top
        && top - end.1 <= MARKER_PULLBACK_TOLERANCE
        && end.0 >= left - MARKER_PULLBACK_TOLERANCE
        && end.0 <= right + MARKER_PULLBACK_TOLERANCE
        && dy > 0.0
    {
        return "top";
    }

    svg_terminal_approach_face(rect, points)
}

fn svg_source_departure_face(rect: (f64, f64, f64, f64), points: &[(f64, f64)]) -> &'static str {
    if points.is_empty() {
        return "interior_or_corner";
    }

    let start = points[0];
    let direct_face = svg_point_face(rect, start);
    if direct_face != "interior_or_corner" {
        return direct_face;
    }
    if points.len() < 2 {
        return direct_face;
    }

    let next = points[1];
    let dx = next.0 - start.0;
    let dy = next.1 - start.1;
    if dx.abs() >= dy.abs() {
        if dx > 0.0 {
            "right"
        } else if dx < 0.0 {
            "left"
        } else {
            "interior_or_corner"
        }
    } else if dy > 0.0 {
        "bottom"
    } else if dy < 0.0 {
        "top"
    } else {
        "interior_or_corner"
    }
}

fn manhattan_segment_len(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

fn horizontal_span(points: &[(f64, f64)]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    max_x - min_x
}

fn segment_axis(a: (f64, f64), b: (f64, f64)) -> Option<char> {
    if (a.0 - b.0).abs() < 0.001 && (a.1 - b.1).abs() >= 0.001 {
        Some('V')
    } else if (a.1 - b.1).abs() < 0.001 && (a.0 - b.0).abs() >= 0.001 {
        Some('H')
    } else {
        None
    }
}

fn trailing_segment_run_len(points: &[(f64, f64)], segment_count: usize) -> f64 {
    if points.len() < 2 || segment_count == 0 {
        return 0.0;
    }
    points
        .windows(2)
        .rev()
        .take(segment_count)
        .map(|segment| manhattan_segment_len(segment[0], segment[1]))
        .sum()
}

fn terminal_collinear_run_len(points: &[(f64, f64)]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut segments = points.windows(2).rev();
    let Some(last) = segments.next() else {
        return 0.0;
    };
    let Some(axis) = segment_axis(last[0], last[1]) else {
        return manhattan_segment_len(last[0], last[1]);
    };

    let mut run = manhattan_segment_len(last[0], last[1]);
    for segment in segments {
        if segment_axis(segment[0], segment[1]) != Some(axis) {
            break;
        }
        run += manhattan_segment_len(segment[0], segment[1]);
    }
    run
}

fn has_immediate_axis_backtrack(points: &[(f64, f64)]) -> bool {
    points.windows(3).any(|triple| {
        let a = triple[0];
        let b = triple[1];
        let c = triple[2];
        match (segment_axis(a, b), segment_axis(b, c)) {
            (Some('V'), Some('V')) => {
                let dy1 = b.1 - a.1;
                let dy2 = c.1 - b.1;
                dy1.abs() > 0.001 && dy2.abs() > 0.001 && dy1.signum() != dy2.signum()
            }
            (Some('H'), Some('H')) => {
                let dx1 = b.0 - a.0;
                let dx2 = c.0 - b.0;
                dx1.abs() > 0.001 && dx2.abs() > 0.001 && dx1.signum() != dx2.signum()
            }
            _ => false,
        }
    })
}

fn has_primary_axis_backtrack(points: &[(f64, f64)], direction: mmdflux::Direction) -> bool {
    const EPS: f64 = 0.001;
    if points.len() < 2 {
        return false;
    }

    match direction {
        mmdflux::Direction::TopDown => points.windows(2).any(|seg| seg[1].1 < seg[0].1 - EPS),
        mmdflux::Direction::BottomTop => points.windows(2).any(|seg| seg[1].1 > seg[0].1 + EPS),
        mmdflux::Direction::LeftRight => points.windows(2).any(|seg| seg[1].0 < seg[0].0 - EPS),
        mmdflux::Direction::RightLeft => points.windows(2).any(|seg| seg[1].0 > seg[0].0 + EPS),
    }
}

#[derive(Debug)]
struct SvgStyleMonitorReport {
    scanned_styled_paths: usize,
    violations: Vec<String>,
    summary_line: String,
}

fn min_svg_segment_len(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|segment| {
            let dx = segment[1].0 - segment[0].0;
            let dy = segment[1].1 - segment[0].1;
            (dx * dx + dy * dy).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

fn style_segment_monitor_report_for_svg(
    fixtures: &[&str],
    min_segment_threshold: f64,
) -> SvgStyleMonitorReport {
    let mut scanned_styled_paths = 0usize;
    let mut violations = Vec::new();

    for fixture in fixtures {
        let diagram = load_flowchart_fixture_diagram(fixture);
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = RoutingStyle::Polyline;
        options.svg.interpolation_style = InterpolationStyle::Linear;
        options.svg.corner_style = CornerStyle::Sharp;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);

        for line in svg.lines().map(str::trim) {
            if !line.starts_with("<path d=\"")
                || !(line.contains("marker-end=") || line.contains("marker-start="))
            {
                continue;
            }
            let is_styled =
                line.contains("stroke-dasharray") || line.contains("stroke-width=\"2.00\"");
            if !is_styled {
                continue;
            }

            let Some(start) = line.find("d=\"") else {
                continue;
            };
            let after = &line[start + 3..];
            let Some(end) = after.find('"') else {
                continue;
            };
            let points = parse_svg_path_points(&after[..end]);
            if points.len() < 2 {
                continue;
            }

            let min_segment = min_svg_segment_len(&points);
            scanned_styled_paths += 1;
            if min_segment < min_segment_threshold {
                violations.push(format!(
                    "{fixture} styled_path min_segment={min_segment:.2} threshold={min_segment_threshold:.2} path={points:?}"
                ));
            }
        }
    }

    SvgStyleMonitorReport {
        scanned_styled_paths,
        summary_line: format!(
            "style_monitor_svg scanned={} violations={} threshold={:.2}",
            scanned_styled_paths,
            violations.len(),
            min_segment_threshold
        ),
        violations,
    }
}

fn parse_attr_f64(line: &str, attr: &str) -> Option<f64> {
    let marker = format!("{attr}=\"");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    rest[..end].parse::<f64>().ok()
}

fn node_rect_for_label(svg: &str, label: &str) -> Option<(f64, f64, f64, f64)> {
    let (text_x, text_y) = svg.lines().find_map(|line| {
        if !line.contains("<text") || !line.contains(&format!(">{label}<")) {
            return None;
        }
        Some((parse_attr_f64(line, "x")?, parse_attr_f64(line, "y")?))
    })?;

    svg.lines().find_map(|line| {
        if !line.contains("<rect ")
            || !line.contains("stroke=\"#333\"")
            || !line.contains("fill=\"white\"")
        {
            return None;
        }
        let x = parse_attr_f64(line, "x")?;
        let y = parse_attr_f64(line, "y")?;
        let width = parse_attr_f64(line, "width")?;
        let height = parse_attr_f64(line, "height")?;
        let inside = text_x >= x && text_x <= x + width && text_y >= y && text_y <= y + height;
        if inside {
            Some((x, y, width, height))
        } else {
            None
        }
    })
}

fn axis_aligned_segment_crosses_rect_interior(
    a: (f64, f64),
    b: (f64, f64),
    rect: (f64, f64, f64, f64),
    margin: f64,
) -> bool {
    let (x, y, w, h) = rect;
    let left = x + margin;
    let right = x + w - margin;
    let top = y + margin;
    let bottom = y + h - margin;
    if left >= right || top >= bottom {
        return false;
    }

    let eps = 0.5;
    if (a.1 - b.1).abs() <= eps {
        let seg_y = a.1;
        if seg_y <= top || seg_y >= bottom {
            return false;
        }
        let seg_min_x = a.0.min(b.0);
        let seg_max_x = a.0.max(b.0);
        return seg_max_x > left && seg_min_x < right;
    }

    if (a.0 - b.0).abs() <= eps {
        let seg_x = a.0;
        if seg_x <= left || seg_x >= right {
            return false;
        }
        let seg_min_y = a.1.min(b.1);
        let seg_max_y = a.1.max(b.1);
        return seg_max_y > top && seg_min_y < bottom;
    }

    false
}

fn path_crosses_rect_interior(
    path: &[(f64, f64)],
    rect: (f64, f64, f64, f64),
    margin: f64,
) -> bool {
    path.windows(2).any(|segment| {
        axis_aligned_segment_crosses_rect_interior(segment[0], segment[1], rect, margin)
    })
}

fn vertical_lane_x_at_y(path: &[(f64, f64)], probe_y: f64) -> Option<f64> {
    let eps = 0.5;
    path.windows(2).find_map(|segment| {
        let a = segment[0];
        let b = segment[1];
        if (a.0 - b.0).abs() > eps {
            return None;
        }
        let min_y = a.1.min(b.1);
        let max_y = a.1.max(b.1);
        if probe_y >= min_y - eps && probe_y <= max_y + eps {
            Some(a.0)
        } else {
            None
        }
    })
}

fn edge_path_for_svg_order(
    diagram: &mmdflux::Diagram,
    svg: &str,
    edge_index: usize,
) -> Vec<(f64, f64)> {
    let mut visible_edge_indexes: Vec<usize> = diagram
        .edges
        .iter()
        .filter(|edge| edge.stroke != Stroke::Invisible)
        .map(|edge| edge.index)
        .collect();
    visible_edge_indexes.sort_unstable();

    let svg_position = visible_edge_indexes
        .iter()
        .position(|idx| *idx == edge_index)
        .expect("edge index should be visible in SVG");
    let paths = edge_path_data(svg);
    parse_svg_path_points(
        paths
            .get(svg_position)
            .expect("edge path should exist at visible edge position"),
    )
}

fn edge_path_d_for_svg_order(diagram: &mmdflux::Diagram, svg: &str, edge_index: usize) -> String {
    let mut visible_edge_indexes: Vec<usize> = diagram
        .edges
        .iter()
        .filter(|edge| edge.stroke != Stroke::Invisible)
        .map(|edge| edge.index)
        .collect();
    visible_edge_indexes.sort_unstable();

    let svg_position = visible_edge_indexes
        .iter()
        .position(|idx| *idx == edge_index)
        .expect("edge index should be visible in SVG");
    edge_path_data(svg)
        .get(svg_position)
        .expect("edge path should exist at visible edge position")
        .to_string()
}

fn load_flowchart_fixture_diagram(name: &str) -> mmdflux::Diagram {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    build_diagram(&flowchart)
}

/// Style tuple: (RoutingStyle, InterpolationStyle, CornerStyle)
/// Equivalents: SHARP = Polyline+Linear+Sharp, SMOOTH = Orthogonal+Bezier+Sharp, ROUNDED = Orthogonal+Linear+Rounded
type StyleTuple = (RoutingStyle, InterpolationStyle, CornerStyle);
const SHARP: StyleTuple = (
    RoutingStyle::Polyline,
    InterpolationStyle::Linear,
    CornerStyle::Sharp,
);
const SMOOTH: StyleTuple = (
    RoutingStyle::Orthogonal,
    InterpolationStyle::Bezier,
    CornerStyle::Sharp,
);
const ROUNDED: StyleTuple = (
    RoutingStyle::Orthogonal,
    InterpolationStyle::Linear,
    CornerStyle::Rounded,
);

fn render_fixture_svg(
    diagram: &mmdflux::Diagram,
    edge_routing: EdgeRouting,
    style: StyleTuple,
) -> String {
    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(edge_routing);
    options.svg.routing_style = style.0;
    options.svg.interpolation_style = style.1;
    options.svg.corner_style = style.2;
    options.path_simplification = PathSimplification::None;
    render_svg(diagram, &options)
}

fn edge_index(diagram: &mmdflux::Diagram, from: &str, to: &str) -> usize {
    diagram
        .edges
        .iter()
        .find(|edge| edge.from == from && edge.to == to)
        .unwrap_or_else(|| panic!("expected edge {from} -> {to}"))
        .index
}

fn node_center_for_id(diagram: &mmdflux::Diagram, node_id: &str) -> (f64, f64) {
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, diagram, &config)
        .expect("layout should succeed for center lookup");
    let node = geom
        .nodes
        .get(node_id)
        .unwrap_or_else(|| panic!("expected node `{node_id}` in layout geometry"));
    (
        node.rect.x + node.rect.width / 2.0,
        node.rect.y + node.rect.height / 2.0,
    )
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[test]
fn render_svg_basic_flowchart_has_svg_root() {
    let input = "graph TD\nA[Start] --> B[End]\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);

    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<text"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("End"));
}

#[test]
fn svg_direct_route_straight_uses_source_and_target_ports() {
    let diagram = load_flowchart_fixture_diagram("chain.mmd");
    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::DirectRoute);
    options.svg.routing_style = RoutingStyle::Direct;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Sharp;
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let edge = diagram
        .edges
        .iter()
        .find(|edge| edge.stroke != Stroke::Invisible)
        .expect("chain fixture should contain at least one visible edge");
    let points = edge_path_for_svg_order(&diagram, &svg, edge.index);

    let source_label = &diagram
        .nodes
        .get(&edge.from)
        .expect("source node should exist")
        .label;
    let target_label = &diagram
        .nodes
        .get(&edge.to)
        .expect("target node should exist")
        .label;
    let source_rect =
        node_rect_for_label(&svg, source_label).expect("source rect should exist in rendered SVG");
    let target_rect =
        node_rect_for_label(&svg, target_label).expect("target rect should exist in rendered SVG");

    let source_face = svg_source_departure_face(source_rect, &points);
    let target_face = svg_terminal_approach_face_relaxed(target_rect, &points);

    assert_eq!(
        source_face, "bottom",
        "direct/straight source should depart from the TD bottom face: points={points:?}"
    );
    assert_eq!(
        target_face, "top",
        "direct/straight target should attach on the TD top face: points={points:?}"
    );
}

#[test]
fn svg_direct_route_double_skip_uses_avoidance_path_for_long_skip_edges() {
    let diagram = load_flowchart_fixture_diagram("double_skip.mmd");
    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::DirectRoute);
    options.svg.routing_style = RoutingStyle::Direct;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Sharp;
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let skip_edge_index = edge_index(&diagram, "A", "D");
    let points = edge_path_for_svg_order(&diagram, &svg, skip_edge_index);
    assert!(
        points.len() > 2,
        "direct mode should preserve avoidance geometry when the straight skip edge would cut through intermediate nodes: points={points:?}"
    );
}

#[test]
fn svg_orthogonal_mode_renders_axis_aligned_path_commands() {
    let input = "graph TD\nA --> B\nA --> C\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    let svg = render_svg(&diagram, &options);

    assert!(!svg.contains("NaN"));

    let edge_paths = edge_path_data(&svg);
    assert!(
        !edge_paths.is_empty(),
        "expected edge path data in SVG output"
    );
    for d in edge_paths {
        let points = parse_svg_path_points(&d);
        assert!(
            points.windows(2).all(|segment| {
                (segment[0].0 - segment[1].0).abs() < 0.001
                    || (segment[0].1 - segment[1].1).abs() < 0.001
            }),
            "orthogonal path should be axis-aligned, got {d}"
        );
    }
}

#[test]
fn svg_lossless_path_simplification_sits_between_none_and_lossy_for_orthogonal_route() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);

    let render_with = |path_simplification: PathSimplification| {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = RoutingStyle::Orthogonal;
        options.svg.interpolation_style = InterpolationStyle::Linear;
        options.svg.corner_style = CornerStyle::Rounded;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = path_simplification;
        render_svg(&diagram, &options)
    };

    let full = render_with(PathSimplification::None);
    let compact = render_with(PathSimplification::Lossless);
    let simplified = render_with(PathSimplification::Lossy);

    let full_segments = total_svg_edge_segments(&full);
    let compact_segments = total_svg_edge_segments(&compact);
    let simplified_segments = total_svg_edge_segments(&simplified);

    assert!(
        full_segments >= compact_segments,
        "compact should not increase total segments: full={full_segments}, compact={compact_segments}"
    );
    assert!(
        full_segments != simplified_segments,
        "simplified should change segment density compared to full: full={full_segments}, simplified={simplified_segments}"
    );
}

#[test]
fn routed_svg_defaults_to_none_path_simplification() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let mut default_options = RenderOptions::default_svg();
    default_options.svg.routing_style = RoutingStyle::Orthogonal;
    default_options.svg.interpolation_style = InterpolationStyle::Linear;
    default_options.svg.corner_style = CornerStyle::Rounded;
    default_options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    let default_svg = render_svg(&diagram, &default_options);
    let default_points = edge_path_for_svg_order(&diagram, &default_svg, edge_index);

    let mut full_options = default_options;
    full_options.path_simplification = PathSimplification::None;
    let full_svg = render_svg(&diagram, &full_options);
    let full_points = edge_path_for_svg_order(&diagram, &full_svg, edge_index);

    let mut simplified_options = full_options;
    simplified_options.path_simplification = PathSimplification::Lossy;
    let simplified_svg = render_svg(&diagram, &simplified_options);
    let simplified_points = edge_path_for_svg_order(&diagram, &simplified_svg, edge_index);

    assert_eq!(
        default_points, full_points,
        "default routed SVG path detail should match full output"
    );
    assert!(
        default_points.len() >= simplified_points.len(),
        "default full detail should not have fewer points than simplified: default={}, simplified={}",
        default_points.len(),
        simplified_points.len()
    );
    if default_points.len() == simplified_points.len() {
        assert!(
            default_points.len() <= 3,
            "default/simplified point counts should only match when the routed path is already minimal: default={}, simplified={}, points={:?}",
            default_points.len(),
            simplified_points.len(),
            default_points
        );
    }
}

const SVG_LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT: f64 = 2.0;

#[test]
fn svg_orthogonal_orthogonal_route_labeled_edges_labels_remain_attached_to_active_segments() {
    let diagram = load_flowchart_fixture_diagram("labeled_edges.mmd");
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let failures = svg_label_drift_failures(
        &svg,
        &diagram,
        SVG_LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
    );
    assert!(
        failures.is_empty(),
        "Label revalidation regression: labeled_edges rendered off-path edge labels:\n{}",
        failures.join("\n")
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_inline_label_flowchart_labels_remain_attached_to_active_segments()
 {
    let diagram = load_flowchart_fixture_diagram("inline_label_flowchart.mmd");
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let failures = svg_label_drift_failures(
        &svg,
        &diagram,
        SVG_LABEL_REVALIDATION_MAX_DISTANCE_TO_ACTIVE_SEGMENT,
    );
    assert!(
        failures.is_empty(),
        "Label revalidation regression: inline_label_flowchart rendered off-path edge labels:\n{}",
        failures.join("\n")
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_inline_label_flowchart_avoids_known_node_intrusions() {
    let diagram = load_flowchart_fixture_diagram("inline_label_flowchart.mmd");
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let cache_to_validate = edge_index(&diagram, "cache", "validate");
    let reject_to_metrics = edge_index(&diagram, "reject", "metrics");
    let retry_to_queue = edge_index(&diagram, "retry", "queue");
    let fastpath_to_metrics = edge_index(&diagram, "fastpath", "metrics");
    let audit_to_metrics = edge_index(&diagram, "audit", "metrics");
    let cache_to_validate_points = edge_path_for_svg_order(&diagram, &svg, cache_to_validate);
    let reject_to_metrics_points = edge_path_for_svg_order(&diagram, &svg, reject_to_metrics);
    let retry_to_queue_points = edge_path_for_svg_order(&diagram, &svg, retry_to_queue);
    let fastpath_to_metrics_points = edge_path_for_svg_order(&diagram, &svg, fastpath_to_metrics);
    let audit_to_metrics_points = edge_path_for_svg_order(&diagram, &svg, audit_to_metrics);

    let serve_cached_rect =
        node_rect_for_label(&svg, "Serve Cached").expect("missing Serve Cached rect");
    let notify_user_rect =
        node_rect_for_label(&svg, "Notify User").expect("missing Notify User rect");
    let page_on_call_rect =
        node_rect_for_label(&svg, "Page On-call").expect("missing Page On-call rect");

    assert!(
        !path_crosses_rect_interior(&cache_to_validate_points, serve_cached_rect, 1.0),
        "Lookup Cache -> Valid? should not pass through Serve Cached interior in orthogonal mode; path={cache_to_validate_points:?}, serve_cached_rect={serve_cached_rect:?}"
    );
    assert!(
        !path_crosses_rect_interior(&reject_to_metrics_points, notify_user_rect, 1.0),
        "Reject -> Emit Metrics should not pass through Notify User interior in orthogonal mode; path={reject_to_metrics_points:?}, notify_user_rect={notify_user_rect:?}"
    );
    assert!(
        !path_crosses_rect_interior(&reject_to_metrics_points, page_on_call_rect, 1.0),
        "Reject -> Emit Metrics should not pass through Page On-call interior in orthogonal mode; path={reject_to_metrics_points:?}, page_on_call_rect={page_on_call_rect:?}"
    );
    assert!(
        !path_crosses_rect_interior(&retry_to_queue_points, page_on_call_rect, 1.0),
        "Retry -> Enqueue Job should not pass through Page On-call interior in orthogonal mode; path={retry_to_queue_points:?}, page_on_call_rect={page_on_call_rect:?}"
    );

    let fast_support = *fastpath_to_metrics_points
        .get(fastpath_to_metrics_points.len().saturating_sub(2))
        .expect("Serve Cached -> Emit Metrics should include terminal support point");
    let audit_support = *audit_to_metrics_points
        .get(audit_to_metrics_points.len().saturating_sub(2))
        .expect("Audit Log -> Emit Metrics should include terminal support point");
    assert!(
        (fast_support.1 - audit_support.1).abs() >= 1.0,
        "Serve Cached -> Emit Metrics and Audit Log -> Emit Metrics should stagger terminal horizontal support lanes into Emit Metrics; fast_support={fast_support:?}, audit_support={audit_support:?}, fast_path={fastpath_to_metrics_points:?}, audit_path={audit_to_metrics_points:?}"
    );

    let probe_y = 1000.0;
    let retry_lane_x = vertical_lane_x_at_y(&retry_to_queue_points, probe_y)
        .expect("Retry -> Enqueue Job should expose a vertical lane through probe y");
    let fastpath_lane_x = vertical_lane_x_at_y(&fastpath_to_metrics_points, probe_y)
        .expect("Serve Cached -> Emit Metrics should expose a vertical lane through probe y");
    assert!(
        (retry_lane_x - fastpath_lane_x).abs() >= 1.0,
        "Retry -> Enqueue Job should not share the same vertical lane as Serve Cached -> Emit Metrics around y={probe_y}; retry_x={retry_lane_x}, fastpath_x={fastpath_lane_x}, retry_path={retry_to_queue_points:?}, fast_path={fastpath_to_metrics_points:?}"
    );
}

#[test]
fn path_simplification_monotonicity_holds_none_lossless_lossy() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let render_for = |path_simplification: PathSimplification| {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = RoutingStyle::Orthogonal;
        options.svg.interpolation_style = InterpolationStyle::Linear;
        options.svg.corner_style = CornerStyle::Rounded;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = path_simplification;
        let svg = render_svg(&diagram, &options);
        edge_path_for_svg_order(&diagram, &svg, edge_index).len()
    };

    let full = render_for(PathSimplification::None);
    let compact = render_for(PathSimplification::Lossless);
    let simplified = render_for(PathSimplification::Lossy);

    assert!(
        full >= compact && compact >= simplified,
        "path-detail monotonicity violated for SVG: full={full}, compact={compact}, simplified={simplified}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_preserves_clear_terminal_stem_into_arrowhead() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
    assert!(
        points.len() >= 2,
        "expected routed SVG points for Bmid -> F"
    );

    let prev = points[points.len() - 2];
    let end = points[points.len() - 1];
    let axis = segment_axis(prev, end).expect("terminal segment should be axis-aligned");
    let stem_len = manhattan_segment_len(prev, end);
    assert_eq!(
        axis, 'V',
        "Bmid -> F terminal segment should be vertical in TD layout: {points:?}"
    );
    assert!(
        end.1 > prev.1,
        "Bmid -> F terminal segment should point downward into F (arrow-support direction), got prev={prev:?}, end={end:?}, points={points:?}"
    );
    assert!(
        !has_immediate_axis_backtrack(&points),
        "Bmid -> F path should not include an immediate axis backtrack near the elbow: {points:?}"
    );
    assert!(
        stem_len >= 8.0,
        "Bmid -> F terminal stem should retain extra buffer beyond arrow pullback (>= 8px), got {stem_len} with {points:?}"
    );

    let (_fx, fy, _fw, _fh) = node_rect_for_label(&svg, "f").expect("expected SVG rect for node f");
    let expected_endpoint_y = fy - 4.0;
    assert!(
        (end.1 - expected_endpoint_y).abs() <= 0.5,
        "Bmid -> F endpoint should be pulled back so arrow tip touches F border: endpoint_y={}, expected_y={} (f_top={fy}) points={points:?}",
        end.1,
        expected_endpoint_y
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_does_not_add_short_staircase_jogs_after_adjustment() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);

    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config)
        .expect("layout should succeed");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let routed_edge = routed
        .edges
        .iter()
        .find(|edge| edge.index == edge_index)
        .expect("orthogonal routed edge should exist");
    let routed_segments = routed_edge.path.len().saturating_sub(1);

    let mut options = RenderOptions::default_svg();
    // Sharp renders straight-line segments without arc corners, so segment
    // counts are directly comparable to routed waypoints.
    options.svg.routing_style = RoutingStyle::Polyline;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
    let svg_segments = points.len().saturating_sub(1);
    assert!(
        svg_segments <= routed_segments + 2,
        "SVG conversion should not add staircase jogs for Bmid -> F: routed_segments={routed_segments}, svg_segments={svg_segments}, svg_points={points:?}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_multiple_cycles_avoids_tiny_terminal_staircase_jogs() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multiple_cycles.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edges = [
        edge_index(&diagram, "C", "A"),
        edge_index(&diagram, "C", "B"),
    ];

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for edge_idx in edges {
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 2,
            "multiple_cycles edge should keep at least one terminal segment in orthogonal mode: {points:?}"
        );
        let terminal_support =
            manhattan_segment_len(points[points.len() - 2], points[points.len() - 1]);
        // A perfectly straight terminal (2 points) is acceptable as long as it is not tiny.
        // If there is an elbow near the terminal (>= 3 points), also require the
        // pre-terminal leg to be non-trivial to avoid staircase artifacts.
        if points.len() >= 3 {
            let pre_terminal =
                manhattan_segment_len(points[points.len() - 3], points[points.len() - 2]);
            assert!(
                terminal_support >= 10.0 && pre_terminal >= 3.0,
                "multiple_cycles orthogonal tail should avoid tiny terminal staircase jogs; terminal_support={terminal_support}, pre_terminal={pre_terminal}, points={points:?}"
            );
        } else {
            assert!(
                terminal_support >= 10.0,
                "multiple_cycles orthogonal straight terminal should remain substantial (>= 10px): terminal_support={terminal_support}, points={points:?}"
            );
        }
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_double_skip_avoids_tiny_leading_lateral_jog() {
    let diagram = load_flowchart_fixture_diagram("double_skip.mmd");
    let edge_idx = edge_index(&diagram, "A", "C");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
    assert!(
        points.len() >= 2,
        "double_skip A -> C should render with at least one segment: {points:?}"
    );

    if points.len() >= 4 {
        let p0 = points[0];
        let p1 = points[1];
        let p2 = points[2];
        let p3 = points[3];
        let first_vertical = segment_axis(p0, p1) == Some('V');
        let middle_horizontal = segment_axis(p1, p2) == Some('H');
        let terminal_vertical = segment_axis(p2, p3) == Some('V');
        if first_vertical && middle_horizontal && terminal_vertical {
            let jog = manhattan_segment_len(p1, p2);
            assert!(
                jog >= 3.0,
                "double_skip A -> C should not keep a tiny leading lateral shim in orthogonal mode; jog={jog}, points={points:?}"
            );
        }
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_decision_diamond_outbound_prefers_horizontal_departure() {
    let diagram = load_flowchart_fixture_diagram("decision.mmd");
    let edges = [
        edge_index(&diagram, "B", "C"),
        edge_index(&diagram, "B", "D"),
    ];

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for edge_idx in edges {
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 3,
            "decision branch should keep at least one bend after horizontal departure preference: {points:?}"
        );
        assert_eq!(
            segment_axis(points[0], points[1]),
            Some('H'),
            "decision branch should depart diamond horizontally in TD orthogonal orthogonal mode: {points:?}"
        );
        assert_eq!(
            segment_axis(points[points.len() - 2], points[points.len() - 1]),
            Some('V'),
            "decision branch should arrive at target with vertical support in TD orthogonal orthogonal mode: {points:?}"
        );
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_hexagon_outbound_departure_insets_from_bottom_border() {
    let diagram = load_flowchart_fixture_diagram("hexagon_flow.mmd");
    let edges = [
        edge_index(&diagram, "A", "B"),
        edge_index(&diagram, "A", "D"),
    ];

    let measurement_mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&measurement_mode, &diagram, &config)
        .expect("layout should succeed for hexagon_flow fixture");
    let source_rect = geom
        .nodes
        .get("A")
        .expect("hexagon_flow fixture should contain source node A")
        .rect;
    let source_bottom = source_rect.y + source_rect.height;

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for edge_idx in edges {
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 3,
            "hexagon outbound edge should keep at least one bend: {points:?}"
        );
        assert_eq!(
            segment_axis(points[0], points[1]),
            Some('H'),
            "hexagon outbound edge should depart laterally first in TD orthogonal orthogonal mode: {points:?}"
        );
        assert!(
            points[0].1 <= source_bottom - 2.0,
            "hexagon outbound edge start should be inset above the bottom border to avoid border-aligned stems: start={:?}, source_bottom={}, points={points:?}",
            points[0],
            source_bottom
        );
        assert_eq!(
            segment_axis(points[points.len() - 2], points[points.len() - 1]),
            Some('V'),
            "hexagon outbound edge should arrive with a vertical terminal support: {points:?}"
        );
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_nested_subgraph_edge_avoids_large_lateral_detour() {
    let diagram = load_flowchart_fixture_diagram("nested_subgraph_edge.mmd");
    let edges = [
        edge_index(&diagram, "Client", "Server1"),
        edge_index(&diagram, "Server1", "Monitoring"),
    ];

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for edge_idx in edges {
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        let span = horizontal_span(&points);
        assert!(
            span <= 20.0,
            "nested_subgraph_edge orthogonal path should not make a large horizontal detour: span={span}, points={points:?}"
        );
    }
}

#[test]
fn svg_curved_orthogonal_route_ampersand_avoids_tiny_terminal_hook_before_arrow() {
    let diagram = load_flowchart_fixture_diagram("ampersand.mmd");
    let merge_in_edges = [
        edge_index(&diagram, "A", "C"),
        edge_index(&diagram, "B", "C"),
    ];

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for edge_idx in merge_in_edges {
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 2,
            "ampersand edge should contain at least two path points: {points:?}"
        );
        let terminal = terminal_collinear_run_len(&points);
        assert!(
            terminal >= 3.5,
            "curved orthogonal terminal approach should avoid tiny hook before marker; collinear_terminal_run={terminal}, points={points:?}"
        );
    }
}

#[test]
fn svg_non_orth_orthogonal_route_backward_edges_terminal_tangent_points_toward_target() {
    let cases = [
        ("decision.mmd", "D", "A"),
        ("git_workflow.mmd", "Remote", "Working"),
        ("http_request.mmd", "Response", "Client"),
        ("labeled_edges.mmd", "Error", "Setup"),
    ];
    let styles = [SHARP, ROUNDED, SMOOTH];

    for (fixture_name, from, to) in cases {
        let diagram = load_flowchart_fixture_diagram(fixture_name);
        let edge_idx = edge_index(&diagram, from, to);
        let target_center = node_center_for_id(&diagram, to);

        for style in styles {
            let mut options = RenderOptions::default_svg();
            options.svg.routing_style = style.0;

            options.svg.interpolation_style = style.1;

            options.svg.corner_style = style.2;
            options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
            options.path_simplification = PathSimplification::None;
            let svg = render_svg(&diagram, &options);
            let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);

            assert!(
                points.len() >= 2,
                "{fixture_name} {from}->{to} should have at least two SVG path points for {style:?}: {points:?}"
            );

            let prev = points[points.len() - 2];
            let end = points[points.len() - 1];
            let toward_target = distance(end, target_center) < distance(prev, target_center);
            assert!(
                toward_target,
                "{fixture_name} {from}->{to} terminal tangent should point toward target center for {style:?}: prev={prev:?}, end={end:?}, target_center={target_center:?}, points={points:?}"
            );
        }
    }
}

#[test]
fn svg_straight_orthogonal_route_avoids_primary_axis_backtrack_for_bmid_to_f() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Polyline;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_index);

    assert!(
        !has_primary_axis_backtrack(&points, diagram.direction),
        "Bmid -> F should not backtrack along TD primary axis in straight SVG: {points:?}"
    );
}

#[test]
fn svg_curved_orthogonal_route_avoids_primary_axis_backtrack_for_bmid_to_f() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_index);

    assert!(
        !has_primary_axis_backtrack(&points, diagram.direction),
        "Bmid -> F should not backtrack along TD primary axis in curved SVG: {points:?}"
    );
}

#[test]
fn svg_rounded_orthogonal_route_avoids_primary_axis_backtrack_for_bmid_to_f() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_index);

    assert!(
        !has_primary_axis_backtrack(&points, diagram.direction),
        "Bmid -> F should not backtrack along TD primary axis in rounded SVG: {points:?}"
    );
}

#[test]
fn svg_non_orth_orthogonal_route_keeps_endpoint_pulled_back_for_visible_arrow_tip() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("multi_subgraph_direction_override.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_index = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Bmid" && edge.to == "F")
        .expect("fixture should contain edge Bmid -> F")
        .index;

    let styles = [SHARP, ROUNDED, SMOOTH];

    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
        let end = points
            .last()
            .copied()
            .expect("Bmid -> F should have SVG path points");
        let (_fx, fy, _fw, _fh) =
            node_rect_for_label(&svg, "f").expect("expected SVG rect for node f");
        let expected_endpoint_y = fy - 4.0;

        assert!(
            (end.1 - expected_endpoint_y).abs() <= 0.5,
            "non-orth {style:?} endpoint should be pulled back so arrow tip lands on F border: endpoint_y={}, expected_y={} (f_top={fy}) points={points:?}",
            end.1,
            expected_endpoint_y
        );
    }
}

#[test]
fn svg_non_orth_orthogonal_route_fan_in_lr_terminal_arrowheads_do_not_end_inside_target() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("fan_in_lr.mmd");
    let input = fs::read_to_string(fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);

    let top_edge = edge_index(&diagram, "A", "D");
    let bottom_edge = edge_index(&diagram, "C", "D");
    let styles = [SHARP, ROUNDED, SMOOTH];

    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let (tx, ty, tw, th) =
            node_rect_for_label(&svg, "Target").expect("target rect should exist");

        for edge_idx in [top_edge, bottom_edge] {
            let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
            let end = points
                .last()
                .copied()
                .expect("edge should have path points");
            let inside = end.0 > tx + 0.5
                && end.0 < tx + tw - 0.5
                && end.1 > ty + 0.5
                && end.1 < ty + th - 0.5;

            assert!(
                !inside,
                "fan_in_lr edge endpoint should not be inside target interior for {style:?}: end={end:?}, target_rect=({tx}, {ty}, {tw}, {th}), points={points:?}"
            );
        }
    }
}

#[test]
fn svg_non_orth_orthogonal_route_backward_edges_keep_terminal_arrowheads_visible() {
    let cases = [
        ("decision.mmd", "D", "A", "Start"),
        ("labeled_edges.mmd", "Error", "Setup", "Setup"),
        ("http_request.mmd", "Response", "Client", "Client"),
        ("complex.mmd", "E", "A", "Input"),
    ];
    let styles = [SHARP, ROUNDED, SMOOTH];

    for (fixture_name, from, to, target_label) in cases {
        let diagram = load_flowchart_fixture_diagram(fixture_name);
        let edge_idx = edge_index(&diagram, from, to);

        for style in styles {
            let mut options = RenderOptions::default_svg();
            options.svg.routing_style = style.0;

            options.svg.interpolation_style = style.1;

            options.svg.corner_style = style.2;
            options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
            options.path_simplification = PathSimplification::None;
            let svg = render_svg(&diagram, &options);
            let (tx, ty, tw, th) = node_rect_for_label(&svg, target_label)
                .unwrap_or_else(|| panic!("target rect should exist for {target_label}"));
            let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
            let end = points
                .last()
                .copied()
                .expect("edge should have path points");
            let inside = end.0 > tx + 0.5
                && end.0 < tx + tw - 0.5
                && end.1 > ty + 0.5
                && end.1 < ty + th - 0.5;

            assert!(
                !inside,
                "{fixture_name} {from}->{to} endpoint should stay outside target interior for {style:?}: end={end:?}, target_rect=({tx}, {ty}, {tw}, {th}), points={points:?}"
            );
        }
    }
}

#[test]
fn svg_non_orth_orthogonal_route_backward_in_subgraph_avoids_tiny_terminal_tail_hooks() {
    const MIN_TERMINAL_SUPPORT: f64 = 3.5;
    let diagram = load_flowchart_fixture_diagram("backward_in_subgraph.mmd");
    let edge_idx = edge_index(&diagram, "B", "A");
    let styles = [SHARP, ROUNDED, SMOOTH];

    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 2,
            "backward_in_subgraph B->A should have at least two points for {style:?}: {points:?}"
        );

        let rect = node_rect_for_label(&svg, "Node").expect("target rect should exist for Node");
        let end_face = svg_terminal_approach_face_relaxed(rect, &points);
        assert_eq!(
            end_face, "bottom",
            "backward_in_subgraph B->A should enter Node on bottom face for {style:?}: points={points:?}"
        );

        let terminal_support =
            manhattan_segment_len(points[points.len() - 2], points[points.len() - 1]);
        let min_terminal_support = if style.1 == InterpolationStyle::Bezier {
            // Curved rendering intentionally tapers the final straight cap segment.
            1.0
        } else {
            MIN_TERMINAL_SUPPORT
        };
        assert!(
            terminal_support >= min_terminal_support,
            "backward_in_subgraph B->A should avoid tiny terminal tail hooks before the arrowhead for {style:?}: terminal_support={terminal_support}, min={min_terminal_support}, points={points:?}"
        );
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_complex_backward_edge_keeps_arrowhead_visible() {
    let diagram = load_flowchart_fixture_diagram("complex.mmd");
    let edge_idx = edge_index(&diagram, "E", "A");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let (tx, ty, tw, th) =
        node_rect_for_label(&svg, "Input").expect("target rect should exist for Input");
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
    let end = points
        .last()
        .copied()
        .expect("complex E->A should have SVG path points");

    let ends_on_target_border_or_inside =
        end.0 >= tx - 0.5 && end.0 <= tx + tw + 0.5 && end.1 >= ty - 0.5 && end.1 <= ty + th + 0.5;
    assert!(
        !ends_on_target_border_or_inside,
        "complex E->A orthogonal endpoint should be pulled outside the Input node envelope so arrowhead remains visible; end={end:?}, target_rect=({tx}, {ty}, {tw}, {th}), points={points:?}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_complex_backward_edge_terminal_tangent_points_toward_target() {
    let diagram = load_flowchart_fixture_diagram("complex.mmd");
    let edge_idx = edge_index(&diagram, "E", "A");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let rect = node_rect_for_label(&svg, "Input").expect("target rect should exist for Input");
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
    assert!(
        points.len() >= 2,
        "complex E->A should have at least two path points in orthogonal mode: {points:?}"
    );
    let prev = points[points.len() - 2];
    let end = points[points.len() - 1];
    let end_face = svg_terminal_approach_face_relaxed(rect, &points);

    match end_face {
        "right" => assert!(
            (end.1 - prev.1).abs() <= 0.5 && end.0 < prev.0,
            "complex E->A orthogonal terminal tangent on right face should point left into Input; prev={prev:?}, end={end:?}, points={points:?}"
        ),
        "left" => assert!(
            (end.1 - prev.1).abs() <= 0.5 && end.0 > prev.0,
            "complex E->A orthogonal terminal tangent on left face should point right into Input; prev={prev:?}, end={end:?}, points={points:?}"
        ),
        "top" => assert!(
            (end.0 - prev.0).abs() <= 0.5 && end.1 > prev.1,
            "complex E->A orthogonal terminal tangent on top face should point down into Input; prev={prev:?}, end={end:?}, points={points:?}"
        ),
        "bottom" => assert!(
            (end.0 - prev.0).abs() <= 0.5 && end.1 < prev.1,
            "complex E->A orthogonal terminal tangent on bottom face should point up into Input; prev={prev:?}, end={end:?}, points={points:?}"
        ),
        other => panic!(
            "complex E->A orthogonal terminal approach should resolve to a concrete Input face, got {other}; prev={prev:?}, end={end:?}, points={points:?}"
        ),
    }
}

#[test]
fn svg_orthogonal_route_complex_top_diamond_loop_avoids_single_edge_micro_jogs() {
    const MIN_SEGMENT_LEN: f64 = 6.0;

    let diagram = load_flowchart_fixture_diagram("complex.mmd");
    let mut straight_options = RenderOptions::default_svg();
    straight_options.svg.routing_style = RoutingStyle::Polyline;
    straight_options.svg.interpolation_style = InterpolationStyle::Linear;
    straight_options.svg.corner_style = CornerStyle::Sharp;
    straight_options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    straight_options.path_simplification = PathSimplification::None;
    let straight_svg = render_svg(&diagram, &straight_options);

    for (from, to) in [("C", "E"), ("E", "A")] {
        let edge_idx = edge_index(&diagram, from, to);
        let points = edge_path_for_svg_order(&diagram, &straight_svg, edge_idx);
        assert!(
            points.len() >= 2,
            "complex {from}->{to} should emit at least one segment in straight mode: {points:?}"
        );
        let min_segment = min_svg_segment_len(&points);
        assert!(
            min_segment >= MIN_SEGMENT_LEN,
            "complex {from}->{to} should avoid tiny elbow jog segments in orthogonal straight mode (min {MIN_SEGMENT_LEN}): min_segment={min_segment}, points={points:?}"
        );
    }

    let mut orth_options = RenderOptions::default_svg();
    orth_options.svg.routing_style = RoutingStyle::Orthogonal;
    orth_options.svg.interpolation_style = InterpolationStyle::Linear;
    orth_options.svg.corner_style = CornerStyle::Rounded;
    orth_options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    orth_options.path_simplification = PathSimplification::None;
    let orth_svg = render_svg(&diagram, &orth_options);
    let backward_idx = edge_index(&diagram, "E", "A");
    let backward_points = edge_path_for_svg_order(&diagram, &orth_svg, backward_idx);
    assert!(
        !has_immediate_axis_backtrack(&backward_points),
        "complex E->A should not include an immediate axis backtrack in orthogonal orthogonal mode: {backward_points:?}"
    );
}

#[test]
fn svg_non_orth_orthogonal_route_complex_backward_edge_avoids_center_biased_input_attachment() {
    const MIN_CENTER_OFFSET: f64 = 12.0;

    let diagram = load_flowchart_fixture_diagram("complex.mmd");
    let edge_idx = edge_index(&diagram, "E", "A");
    let styles = [SHARP, ROUNDED, SMOOTH];

    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);

        let rect = node_rect_for_label(&svg, "Input").expect("target rect should exist for Input");
        let center_x = rect.0 + rect.2 / 2.0;
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        let end = *points
            .last()
            .expect("complex E->A should have path points for non-orth style");
        let end_face = svg_terminal_approach_face_relaxed(rect, &points);

        if end_face == "bottom" || end_face == "top" {
            let center_offset = (end.0 - center_x).abs();
            assert!(
                center_offset >= MIN_CENTER_OFFSET,
                "complex E->A {style:?} should avoid center-biased vertical attachment on Input when approaching from a backward top-loop lane; end={end:?}, center_x={center_x}, center_offset={center_offset}, min_offset={MIN_CENTER_OFFSET}, points={points:?}"
            );
        }
    }
}

#[test]
fn svg_straight_orthogonal_route_ci_pipeline_diamond_exits_avoid_extra_elbow_jogs() {
    let diagram = load_flowchart_fixture_diagram("ci_pipeline.mmd");
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Polyline;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for (from, to) in [("Deploy", "Staging"), ("Deploy", "Prod")] {
        let edge_idx = edge_index(&diagram, from, to);
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 3,
            "ci_pipeline {from}->{to} should have at least three points for elbow checks: {points:?}"
        );
        let first = points[0];
        let second = points[1];
        let third = points[2];
        let first_axis = segment_axis(first, second);
        let second_axis = segment_axis(second, third);
        if points.len() >= 4 {
            let fourth = points[3];
            let third_axis = segment_axis(third, fourth);
            assert!(
                !(first_axis.is_none() && second_axis.is_some() && third_axis.is_some()),
                "ci_pipeline {from}->{to} should avoid extra elbow jogs right after Deploy? in orthogonal straight mode (prefer direct diagonal-to-lane): points={points:?}"
            );
        }
    }
}

#[test]
fn svg_orthogonal_route_backward_edges_preserve_selected_non_orth_style() {
    let diagram = load_flowchart_fixture_diagram("simple_cycle.mmd");
    let edge_idx = edge_index(&diagram, "C", "A");

    let curved_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SMOOTH);
    let curved_d = edge_path_d_for_svg_order(&diagram, &curved_svg, edge_idx);
    assert!(
        curved_d.contains('C'),
        "simple_cycle C->A backward edge should use curved-style cubic segments in orthogonal routing: d={curved_d}"
    );

    let rounded_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, ROUNDED);
    let rounded_d = edge_path_d_for_svg_order(&diagram, &rounded_svg, edge_idx);
    assert!(
        rounded_d.contains('Q'),
        "simple_cycle C->A backward edge should use rounded corner commands in orthogonal routing: d={rounded_d}"
    );
    let rounded_points = edge_path_for_svg_order(&diagram, &rounded_svg, edge_idx);
    assert!(
        rounded_points.len() >= 2,
        "simple_cycle C->A backward edge should expose at least two rounded points: {rounded_points:?}"
    );
    let rounded_prev = rounded_points[rounded_points.len() - 2];
    let rounded_end = rounded_points[rounded_points.len() - 1];
    let rounded_dx = (rounded_end.0 - rounded_prev.0).abs();
    let rounded_dy = (rounded_end.1 - rounded_prev.1).abs();
    assert!(
        rounded_dx <= 0.5 || rounded_dy <= 0.5,
        "simple_cycle C->A rounded backward terminal approach should stay axis-aligned (no diagonal terminal tail): prev={rounded_prev:?}, end={rounded_end:?}, d={rounded_d}"
    );

    let straight_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SHARP);
    let straight_d = edge_path_d_for_svg_order(&diagram, &straight_svg, edge_idx);
    assert!(
        !straight_d.contains('Q') && !straight_d.contains('C'),
        "simple_cycle C->A backward edge should remain polyline in straight mode: d={straight_d}"
    );
    let straight_points = edge_path_for_svg_order(&diagram, &straight_svg, edge_idx);
    assert!(
        straight_points.len() >= 2,
        "simple_cycle C->A backward edge should expose at least two straight points: {straight_points:?}"
    );
    let straight_prev = straight_points[straight_points.len() - 2];
    let straight_end = straight_points[straight_points.len() - 1];
    let straight_dx = (straight_end.0 - straight_prev.0).abs();
    let straight_dy = (straight_end.1 - straight_prev.1).abs();
    assert!(
        straight_dx <= 0.5 || straight_dy <= 0.5,
        "simple_cycle C->A straight backward terminal approach should stay axis-aligned (no diagonal terminal tail): prev={straight_prev:?}, end={straight_end:?}, d={straight_d}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_label_spacing_keeps_td_departure_stems_from_source() {
    let diagram = load_flowchart_fixture_diagram("label_spacing.mmd");
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    for (from, to) in [("A", "B"), ("A", "C")] {
        let edge_idx = edge_index(&diagram, from, to);
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        assert!(
            points.len() >= 2,
            "label_spacing {from}->{to} should expose at least two points in orthogonal mode: {points:?}"
        );
        let start = points[0];
        let next = points[1];
        assert!(
            (next.0 - start.0).abs() <= 0.5 && (next.1 - start.1).abs() > 0.5,
            "label_spacing {from}->{to} orthogonal route should depart A along TD primary axis (vertical stem first), not lateral-first: start={start:?}, next={next:?}, points={points:?}"
        );
    }
}

#[test]
fn svg_non_orth_orthogonal_route_fan_in_backward_channel_conflict_keeps_backward_canonical_face() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join("fan_in_backward_channel_conflict.mmd");
    let input = fs::read_to_string(&fixture).expect("fixture should load");
    let flowchart = parse_flowchart(&input).expect("fixture should parse");
    let diagram = build_diagram(&flowchart);
    let edge_idx = edge_index(&diagram, "Loop", "B");

    let styles = [SHARP, ROUNDED, SMOOTH];

    let mut rect = None;

    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let (tx, ty, tw, th) = match rect {
            Some(rect) => rect,
            None => {
                let parsed = node_rect_for_label(&svg, "Target")
                    .expect("expected target rect for fan_in_backward_channel_conflict fixture");
                rect = Some(parsed);
                parsed
            }
        };
        let rect = (tx, ty, tw, th);

        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        let end = points
            .last()
            .copied()
            .expect("backward edge should have path points");
        let end_face = svg_terminal_approach_face_relaxed(rect, &points);

        assert_eq!(
            end_face, "bottom",
            "Loop-conflict edge should follow TD parity target entry (bottom face) for {style:?}: end={end:?}, rect={rect:?}, points={points:?}"
        );
    }
}

#[test]
fn svg_curved_orthogonal_route_fan_in_backward_channel_conflict_avoids_tiny_terminal_hook_before_arrow()
 {
    let diagram = load_flowchart_fixture_diagram("fan_in_backward_channel_conflict.mmd");
    let edge_idx = edge_index(&diagram, "Loop", "B");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);

    assert!(
        points.len() >= 3,
        "fan_in_backward_channel_conflict backward edge should keep at least one terminal support segment in curved mode: points={points:?}"
    );

    let terminal = manhattan_segment_len(points[points.len() - 2], points[points.len() - 1]);
    let trailing_run = trailing_segment_run_len(&points, 4);
    assert!(
        terminal >= 1.0 && trailing_run >= 6.0,
        "curved orthogonal backward terminal hook should avoid tiny elbow before marker; terminal={terminal}, trailing_run={trailing_run}, points={points:?}"
    );
}

#[test]
fn svg_non_orth_orthogonal_route_fan_in_backward_channel_conflict_preserves_lower_terminal_lane() {
    let diagram = load_flowchart_fixture_diagram("fan_in_backward_channel_conflict.mmd");
    let edge_idx = edge_index(&diagram, "Loop", "B");
    let styles = [SHARP, ROUNDED, SMOOTH];

    let mut rect = None;
    for style in styles {
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = style.0;

        options.svg.interpolation_style = style.1;

        options.svg.corner_style = style.2;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let (_tx, ty, _tw, th) = match rect {
            Some(rect) => rect,
            None => {
                let parsed = node_rect_for_label(&svg, "Target")
                    .expect("expected target rect for fan_in_backward_channel_conflict fixture");
                rect = Some(parsed);
                parsed
            }
        };
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        let end = points
            .last()
            .copied()
            .expect("fan_in_backward_channel_conflict backward edge should have path points");

        assert!(
            end.1 >= ty + th - 2.0,
            "Loop-conflict non-orth terminal lane should stay near lower right-face channel for {style:?}: end={end:?}, target_rect_y={ty}, target_rect_h={th}, points={points:?}"
        );
    }
}

#[test]
fn svg_orthogonal_orthogonal_route_fan_in_backward_channel_conflict_avoids_terminal_axis_backtrack()
{
    let diagram = load_flowchart_fixture_diagram("fan_in_backward_channel_conflict.mmd");
    let edge_idx = edge_index(&diagram, "Loop", "B");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);

    assert!(
        !has_immediate_axis_backtrack(&points),
        "fan_in_backward_channel_conflict orthogonal backward edge should not axis-backtrack near the terminal hook; points={points:?}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_decision_backward_edge_avoids_source_elbow_axis_backtrack() {
    let diagram = load_flowchart_fixture_diagram("decision.mmd");
    let edge_idx = edge_index(&diagram, "D", "A");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);

    assert!(
        !has_immediate_axis_backtrack(&points),
        "decision D->A orthogonal backward edge should avoid source-elbow axis backtrack spikes; points={points:?}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_decision_backward_edge_uses_right_face_to_avoid_crossing() {
    // D is to the right of A; the crossing-avoidance heuristic bypasses TD
    // top/bottom parity so the backward edge uses side-channel (right-face)
    // routing instead, avoiding a crossing with the forward A->D edge.
    let diagram = load_flowchart_fixture_diagram("decision.mmd");
    let edge_idx = edge_index(&diagram, "D", "A");

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
    let start_rect =
        node_rect_for_label(&svg, "Start").expect("missing Start rect in decision fixture");
    let target_face = svg_terminal_approach_face_relaxed(start_rect, &points);

    assert_eq!(
        target_face, "right",
        "decision D->A orthogonal backward edge should enter Start from the right face (crossing avoided); face={target_face}, points={points:?}"
    );
}

#[test]
fn svg_orthogonal_orthogonal_route_decision_backward_edge_preserves_routed_terminal_lane_x() {
    const MAX_TERMINAL_LANE_X_DRIFT: f64 = 8.0;

    let diagram = load_flowchart_fixture_diagram("decision.mmd");
    let edge_idx = edge_index(&diagram, "D", "A");

    let measurement_mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&measurement_mode, &diagram, &config)
        .expect("layout should succeed for decision fixture");
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);
    let routed_edge = routed
        .edges
        .iter()
        .find(|edge| edge.from == "D" && edge.to == "A")
        .expect("decision fixture should contain backward edge D -> A");
    assert!(
        routed_edge.path.len() >= 3,
        "routed decision D->A should keep at least one terminal support segment: path={:?}",
        routed_edge.path
    );
    let routed_terminal_support = routed_edge.path[routed_edge.path.len() - 2];

    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Linear;
    options.svg.corner_style = CornerStyle::Rounded;
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);
    let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
    assert!(
        points.len() >= 3,
        "rendered decision D->A should keep at least one terminal support segment: points={points:?}"
    );
    let svg_terminal_support = points[points.len() - 2];
    let drift = (svg_terminal_support.0 - routed_terminal_support.x).abs();

    assert!(
        drift <= MAX_TERMINAL_LANE_X_DRIFT,
        "decision D->A orthogonal SVG endpoint adjustment should preserve routed terminal lane x (drift <= {MAX_TERMINAL_LANE_X_DRIFT}); routed_terminal_support={routed_terminal_support:?}, svg_terminal_support={svg_terminal_support:?}, drift={drift}, routed_path={:?}, svg_points={points:?}",
        routed_edge.path
    );
}

#[test]
fn svg_straight_fan_in_backward_channel_interaction_fixture_matrix_matches_documented_faces() {
    let fan_in_cases = [
        ("stacked_fan_in.mmd", "C", "Bot", 0usize),
        ("fan_in.mmd", "D", "Target", 0usize),
        ("five_fan_in.mmd", "F", "Target", 0usize),
    ];

    for (fixture_name, target_id, target_label, min_side_faces) in fan_in_cases {
        let diagram = load_flowchart_fixture_diagram(fixture_name);
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = RoutingStyle::Polyline;
        options.svg.interpolation_style = InterpolationStyle::Linear;
        options.svg.corner_style = CornerStyle::Sharp;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let rect = node_rect_for_label(&svg, target_label)
            .unwrap_or_else(|| panic!("missing target rect for {target_label} in {fixture_name}"));
        let inbound_indices: Vec<usize> = diagram
            .edges
            .iter()
            .filter(|edge| edge.to == target_id)
            .map(|edge| edge.index)
            .collect();
        assert!(
            !inbound_indices.is_empty(),
            "fixture {fixture_name} should have inbound edges to {target_id}"
        );

        let mut side_face_count = 0usize;
        let mut interior_or_corner_count = 0usize;
        for edge_index in inbound_indices {
            let points = edge_path_for_svg_order(&diagram, &svg, edge_index);
            let face = svg_terminal_approach_face(rect, &points);
            if face == "interior_or_corner" {
                interior_or_corner_count += 1;
            }
            if matches!(face, "left" | "right") {
                side_face_count += 1;
            }
        }

        assert_eq!(
            interior_or_corner_count, 0,
            "fixture {fixture_name} should keep inbound endpoints on a concrete target face under Fan-in overflow policy"
        );
        if min_side_faces == 0 {
            assert_eq!(
                side_face_count, 0,
                "fixture {fixture_name} should stay on primary TD incoming face when overflow is not required"
            );
        } else {
            assert!(
                side_face_count >= min_side_faces,
                "fixture {fixture_name} should spill overflow arrivals to side faces under Fan-in overflow policy: expected >= {min_side_faces}, actual={side_face_count}"
            );
        }
    }

    let backward_channel_cases = [
        (
            "simple_cycle.mmd",
            "C",
            "A",
            "End",
            "Start",
            "top",
            "bottom",
        ),
        (
            "multiple_cycles.mmd",
            "C",
            "A",
            "Bottom",
            "Top",
            "top",
            "bottom",
        ),
        (
            "fan_in_backward_channel_conflict.mmd",
            "Loop",
            "B",
            "Sink",
            "Target",
            "top",
            "bottom",
        ),
        (
            "http_request.mmd",
            "Response",
            "Client",
            "Send Response",
            "Client",
            "right",
            "right",
        ),
        (
            "git_workflow.mmd",
            "Remote",
            "Working",
            "Remote Repo",
            "Working Dir",
            "bottom",
            "bottom",
        ),
    ];

    for (
        fixture_name,
        from,
        to,
        source_label,
        target_label,
        expected_source_face,
        expected_target_face,
    ) in backward_channel_cases
    {
        let diagram = load_flowchart_fixture_diagram(fixture_name);
        let mut options = RenderOptions::default_svg();
        options.svg.routing_style = RoutingStyle::Polyline;
        options.svg.interpolation_style = InterpolationStyle::Linear;
        options.svg.corner_style = CornerStyle::Sharp;
        options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
        options.path_simplification = PathSimplification::None;
        let svg = render_svg(&diagram, &options);
        let source_rect = node_rect_for_label(&svg, source_label)
            .unwrap_or_else(|| panic!("missing source rect for {source_label} in {fixture_name}"));
        let target_rect = node_rect_for_label(&svg, target_label)
            .unwrap_or_else(|| panic!("missing target rect for {target_label} in {fixture_name}"));
        let edge_idx = edge_index(&diagram, from, to);
        let points = edge_path_for_svg_order(&diagram, &svg, edge_idx);
        let source_face = svg_source_departure_face(source_rect, &points);
        assert_eq!(
            source_face, expected_source_face,
            "fixture {fixture_name} edge {from}->{to} should keep expected backward source face {expected_source_face}; points={points:?}"
        );
        let target_face = svg_terminal_approach_face_relaxed(target_rect, &points);
        assert_eq!(
            target_face, expected_target_face,
            "fixture {fixture_name} edge {from}->{to} should keep expected backward target face {expected_target_face}; points={points:?}"
        );
    }
}

#[test]
fn svg_orthogonal_route_five_fan_in_keeps_e_terminal_not_left_of_d() {
    let diagram = load_flowchart_fixture_diagram("five_fan_in.mmd");
    let d_edge = edge_index(&diagram, "D", "F");
    let e_edge = edge_index(&diagram, "E", "F");

    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    let svg = render_svg(&diagram, &options);

    let d_points = edge_path_for_svg_order(&diagram, &svg, d_edge);
    let e_points = edge_path_for_svg_order(&diagram, &svg, e_edge);
    let d_end = d_points[d_points.len() - 1];
    let e_end = e_points[e_points.len() - 1];

    assert!(
        e_end.0 + 1.0 >= d_end.0,
        "five_fan_in orthogonal routing should not place E->Target terminal left of D->Target: d_end={d_end:?}, e_end={e_end:?}, d_points={d_points:?}, e_points={e_points:?}"
    );
}

#[test]
fn svg_curved_orthogonal_route_five_fan_in_keeps_mirrored_pairs_visually_symmetric() {
    let diagram = load_flowchart_fixture_diagram("five_fan_in.mmd");
    let b_edge = edge_index(&diagram, "B", "F");
    let d_edge = edge_index(&diagram, "D", "F");
    let a_edge = edge_index(&diagram, "A", "F");
    let e_edge = edge_index(&diagram, "E", "F");

    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let b_points = edge_path_for_svg_order(&diagram, &svg, b_edge);
    let d_points = edge_path_for_svg_order(&diagram, &svg, d_edge);
    let a_points = edge_path_for_svg_order(&diagram, &svg, a_edge);
    let e_points = edge_path_for_svg_order(&diagram, &svg, e_edge);

    assert!(
        b_points.len() >= 2 && d_points.len() >= 2 && a_points.len() >= 2 && e_points.len() >= 2,
        "curved fan-in edges should each include at least one segment: B={b_points:?} D={d_points:?} A={a_points:?} E={e_points:?}"
    );
    let b_prev = b_points[b_points.len() - 2];
    let d_prev = d_points[d_points.len() - 2];
    let a_prev = a_points[a_points.len() - 2];
    let e_prev = e_points[e_points.len() - 2];

    assert!(
        (b_prev.1 - d_prev.1).abs() <= 1.0,
        "curved B->Target and D->Target should have mirrored terminal approach depth after fan-in channel collapse: B_prev={b_prev:?}, D_prev={d_prev:?}, B={b_points:?}, D={d_points:?}"
    );
    assert!(
        (a_prev.1 - e_prev.1).abs() <= 1.0,
        "curved A->Target and E->Target should have mirrored terminal approach depth after fan-in channel collapse: A_prev={a_prev:?}, E_prev={e_prev:?}, A={a_points:?}, E={e_points:?}"
    );
}

#[test]
fn svg_curved_orthogonal_route_git_workflow_backward_edge_keeps_terminal_support_into_working_dir()
{
    let diagram = load_flowchart_fixture_diagram("git_workflow.mmd");
    let backward_edge = edge_index(&diagram, "Remote", "Working");

    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.svg.routing_style = RoutingStyle::Orthogonal;
    options.svg.interpolation_style = InterpolationStyle::Bezier;
    options.svg.corner_style = CornerStyle::Sharp;
    options.path_simplification = PathSimplification::None;
    let svg = render_svg(&diagram, &options);

    let points = edge_path_for_svg_order(&diagram, &svg, backward_edge);
    assert!(
        points.len() >= 2,
        "git_workflow backward curved edge should include a terminal segment: {points:?}"
    );

    let prev = points[points.len() - 2];
    let end = points[points.len() - 1];
    let terminal_support = (prev.0 - end.0).abs() + (prev.1 - end.1).abs();
    assert!(
        terminal_support >= 3.0,
        "git_workflow backward curved edge should keep at least ~3px terminal support into Working Dir: support={terminal_support}, prev={prev:?}, end={end:?}, points={points:?}"
    );
}

#[test]
fn style_segment_monitor_reports_actionable_summary_for_svg() {
    let report =
        style_segment_monitor_report_for_svg(&["edge_styles.mmd", "inline_edge_labels.mmd"], 12.0);
    assert!(
        report.scanned_styled_paths > 0,
        "style monitor should scan at least one styled path; report={report:?}"
    );
    assert!(
        !report.summary_line.is_empty(),
        "style monitor should emit a stable summary line for CI parsing"
    );
    assert!(
        report.violations.is_empty(),
        "style monitor detected styled-segment violations: {:#?}",
        report
    );
}

#[test]
fn svg_straight_orthogonal_route_self_loop_tail_does_not_collapse_upward_before_arrow() {
    let diagram = load_flowchart_fixture_diagram("self_loop_labeled.mmd");
    let edge_idx = edge_index(&diagram, "B", "B");

    let full_svg = render_fixture_svg(&diagram, EdgeRouting::PolylineRoute, SHARP);
    let orthogonal_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SHARP);

    let full_points = edge_path_for_svg_order(&diagram, &full_svg, edge_idx);
    let orthogonal_points = edge_path_for_svg_order(&diagram, &orthogonal_svg, edge_idx);

    assert!(
        full_points.len() >= 4 && orthogonal_points.len() >= 4,
        "expected self-loop to contain at least 4 points; full={full_points:?}, orthogonal={orthogonal_points:?}"
    );

    // Compare the bottom loop lane instead of relying on a fixed elbow index.
    // Polyline cleanup can reduce intermediate points while preserving loop shape.
    let full_tail_lane_y = full_points
        .iter()
        .take(full_points.len().saturating_sub(1))
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let orthogonal_tail_lane_y = orthogonal_points
        .iter()
        .take(orthogonal_points.len().saturating_sub(1))
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let delta_y = (full_tail_lane_y - orthogonal_tail_lane_y).abs();

    assert!(
        delta_y <= 12.0,
        "self-loop tail lane should remain near polyline routing in orthogonal straight mode (avoid upward collapse); full_tail_lane_y={full_tail_lane_y}, orthogonal_tail_lane_y={orthogonal_tail_lane_y}, delta_y={delta_y}, full_points={full_points:?}, orthogonal_points={orthogonal_points:?}"
    );
}

#[test]
fn orthogonal_route_diamond_boundary_clipping_matches_shape_boundary() {
    let diagram = load_flowchart_fixture_diagram("decision.mmd");

    let mut options = RenderOptions::default_svg();
    options.edge_routing = Some(EdgeRouting::OrthogonalRoute);
    options.path_simplification = PathSimplification::None;

    let mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&mode, &diagram, &config).unwrap();
    let routed = route_graph_geometry(&diagram, &geom, EdgeRouting::OrthogonalRoute);

    // B is a diamond; B->D is a forward edge — verify source endpoint is on diamond boundary
    let edge = routed
        .edges
        .iter()
        .find(|e| e.from == "B" && e.to == "D")
        .expect("missing B->D edge");
    let start = edge.path.first().unwrap();
    let b_rect = geom.nodes.get("B").unwrap().rect;
    let cx = b_rect.x + b_rect.width / 2.0;
    let cy = b_rect.y + b_rect.height / 2.0;
    let w = b_rect.width / 2.0;
    let h = b_rect.height / 2.0;
    let boundary = (start.x - cx).abs() / w + (start.y - cy).abs() / h;
    assert!(
        (boundary - 1.0).abs() < 0.05,
        "orthogonal B->D source should be on diamond boundary: boundary={boundary}, start={start:?}"
    );
}

#[test]
fn orthogonal_route_subgraph_to_subgraph_edge_keeps_terminal_attachment() {
    let diagram = load_flowchart_fixture_diagram("subgraph_to_subgraph_edge.mmd");
    let edge_index = edge_index(&diagram, "API", "DB");

    let full_svg = render_fixture_svg(&diagram, EdgeRouting::PolylineRoute, SMOOTH);
    let orthogonal_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SMOOTH);

    let full_points = edge_path_for_svg_order(&diagram, &full_svg, edge_index);
    let orthogonal_points = edge_path_for_svg_order(&diagram, &orthogonal_svg, edge_index);
    let full_start = full_points[0];
    let orthogonal_start = orthogonal_points[0];
    let full_end = full_points[full_points.len() - 1];
    let orthogonal_end = orthogonal_points[orthogonal_points.len() - 1];

    assert!(
        (full_start.1 - orthogonal_start.1).abs() <= 1.0
            && (full_end.1 - orthogonal_end.1).abs() <= 1.0,
        "API -> DB should keep vertical attachment parity with polyline routing; full_points={full_points:?}, orthogonal_points={orthogonal_points:?}"
    );
}

#[test]
fn orthogonal_route_inner_bt_subgraph_edge_does_not_collapse() {
    let diagram = load_flowchart_fixture_diagram("subgraph_direction_nested_both.mmd");
    let edge_index = edge_index(&diagram, "A", "B");

    let full_svg = render_fixture_svg(&diagram, EdgeRouting::PolylineRoute, SMOOTH);
    let orthogonal_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SMOOTH);

    let full_points = edge_path_for_svg_order(&diagram, &full_svg, edge_index);
    let orthogonal_points = edge_path_for_svg_order(&diagram, &orthogonal_svg, edge_index);
    let full_start = full_points[0];
    let orthogonal_start = orthogonal_points[0];
    let full_end = full_points[full_points.len() - 1];
    let orthogonal_end = orthogonal_points[orthogonal_points.len() - 1];
    let full_span = (full_start.1 - full_end.1).abs();
    let orthogonal_span = (orthogonal_start.1 - orthogonal_end.1).abs();

    assert!(
        (full_start.1 - orthogonal_start.1).abs() <= 1.0
            && (full_end.1 - orthogonal_end.1).abs() <= 1.0
            && orthogonal_span >= full_span - 1.0,
        "A -> B in inner BT subgraph should preserve polyline span; full_points={full_points:?}, orthogonal_points={orthogonal_points:?}, full_span={full_span}, orthogonal_span={orthogonal_span}"
    );
}

#[test]
fn orthogonal_route_nested_override_cross_boundary_edge_keeps_lr_side_faces() {
    let diagram = load_flowchart_fixture_diagram("subgraph_direction_nested_both.mmd");
    let edge_index = edge_index(&diagram, "C", "A");

    let full_svg = render_fixture_svg(&diagram, EdgeRouting::PolylineRoute, ROUNDED);
    let orthogonal_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, ROUNDED);

    let full_points = edge_path_for_svg_order(&diagram, &full_svg, edge_index);
    let orthogonal_points = edge_path_for_svg_order(&diagram, &orthogonal_svg, edge_index);

    let source_rect = node_rect_for_label(&full_svg, "C")
        .expect("subgraph_direction_nested_both should render node rect for C");
    let target_rect = node_rect_for_label(&full_svg, "A")
        .expect("subgraph_direction_nested_both should render node rect for A");

    let full_source_face = svg_source_departure_face(source_rect, &full_points);
    let full_target_face = svg_terminal_approach_face_relaxed(target_rect, &full_points);
    let orthogonal_source_face = svg_source_departure_face(source_rect, &orthogonal_points);
    let orthogonal_target_face =
        svg_terminal_approach_face_relaxed(target_rect, &orthogonal_points);

    assert_eq!(
        full_source_face, "right",
        "fixture contract invalid: polyline C->A should depart C from east/right face: points={full_points:?}"
    );
    assert_eq!(
        full_target_face, "left",
        "fixture contract invalid: polyline C->A should enter A from west/left face: points={full_points:?}"
    );
    assert_eq!(
        orthogonal_source_face, full_source_face,
        "orthogonal C->A should preserve source face parity with polyline in nested override cross-boundary routing: full={full_source_face}, orthogonal={orthogonal_source_face}, full_points={full_points:?}, orthogonal_points={orthogonal_points:?}"
    );
    assert_eq!(
        orthogonal_target_face, full_target_face,
        "orthogonal C->A should preserve target face parity with polyline in nested override cross-boundary routing: full={full_target_face}, orthogonal={orthogonal_target_face}, full_points={full_points:?}, orthogonal_points={orthogonal_points:?}"
    );
}

#[test]
fn render_svg_edge_styles_and_labels() {
    let input = "graph TD\nA ==>|yes| B\nB -.->|no| C\nC <--> D\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    assert!(svg.contains("stroke-dasharray"));
    assert!(svg.contains("stroke-width"));
    assert!(svg.contains("marker-end"));
    assert!(svg.contains("marker-start"));
    assert!(svg.contains("yes"));
    assert!(svg.contains("no"));
}

#[test]
fn render_svg_subgraphs_and_self_edges() {
    let input = "graph TD\nsubgraph Group\nA-->A\nend\n";
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    assert!(svg.contains("Group"));
    assert!(svg.contains("class=\"subgraph\""));
    assert!(svg.matches("<path").count() >= 2);
}

#[test]
fn render_svg_direction_override_lr_node_positions() {
    // subgraph_direction_lr.mmd: TD graph with LR subgraph containing Step 1 -> Step 2 -> Step 3
    // After direction override, these nodes should be arranged horizontally (increasing x).
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_lr.mmd").unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    let positions = extract_node_x_positions(&svg);
    let x_step1 = positions.get("Step 1").expect("Step 1 not found in SVG");
    let x_step2 = positions.get("Step 2").expect("Step 2 not found in SVG");
    let x_step3 = positions.get("Step 3").expect("Step 3 not found in SVG");

    assert!(
        x_step1 < x_step2 && x_step2 < x_step3,
        "LR direction override: Step 1 ({x_step1}) < Step 2 ({x_step2}) < Step 3 ({x_step3}) expected"
    );
}

#[test]
fn render_svg_direction_override_cross_boundary() {
    // subgraph_direction_cross_boundary.mmd: TD graph with LR subgraph, cross-boundary edges
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd")
            .unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    // A and B are inside the LR subgraph, should be horizontal
    let positions = extract_node_x_positions(&svg);
    let x_a = positions.get("A").expect("A not found in SVG");
    let x_b = positions.get("B").expect("B not found in SVG");

    assert!(
        x_a < x_b,
        "LR direction override: A ({x_a}) should be left of B ({x_b})"
    );

    // SVG should not contain NaN values
    assert!(!svg.contains("NaN"), "SVG should not contain NaN values");
}

#[test]
fn render_svg_direction_override_cross_boundary_remains_nan_free() {
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd")
            .unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    assert!(!svg.contains("NaN"), "SVG should not contain NaN values");
    assert!(
        !svg.contains("inf"),
        "SVG should not contain infinite values"
    );
}

#[test]
fn cross_boundary_direction_override_edges_still_render_without_nan() {
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd")
            .unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    assert!(!svg.contains("NaN"));
}

#[test]
fn render_svg_direction_override_mixed() {
    // subgraph_direction_mixed.mmd: Two subgraphs with different direction overrides
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_mixed.mmd").unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    let positions = extract_node_x_positions(&svg);

    // LR group: A should be left of B
    let x_a = positions.get("A").expect("A not found");
    let x_b = positions.get("B").expect("B not found");
    assert!(x_a < x_b, "LR: A ({x_a}) should be left of B ({x_b})");

    // BT group: C and D should be vertically arranged (same x or close x)
    let x_c = positions.get("C").expect("C not found");
    let x_d = positions.get("D").expect("D not found");
    assert!(
        (x_c - x_d).abs() < 1.0,
        "BT: C ({x_c}) and D ({x_d}) should have similar x (vertically stacked)"
    );

    assert!(!svg.contains("NaN"), "SVG should not contain NaN");
}

#[test]
fn render_svg_direction_override_nested() {
    // subgraph_direction_nested.mmd: Outer (no override) with inner LR subgraph
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_nested.mmd").unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    let positions = extract_node_x_positions(&svg);

    // Inner LR: A -> B -> C should be horizontal
    let x_a = positions.get("A").expect("A not found");
    let x_b = positions.get("B").expect("B not found");
    let x_c = positions.get("C").expect("C not found");
    assert!(
        x_a < x_b && x_b < x_c,
        "Inner LR: A ({x_a}) < B ({x_b}) < C ({x_c})"
    );

    assert!(!svg.contains("NaN"), "SVG should not contain NaN");
}

#[test]
fn render_svg_direction_override_nested_both() {
    // subgraph_direction_nested_both.mmd: Outer LR with inner BT
    let input =
        std::fs::read_to_string("tests/fixtures/flowchart/subgraph_direction_nested_both.mmd")
            .unwrap();
    let flowchart = parse_flowchart(&input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    let positions = extract_node_x_positions(&svg);

    // Inner BT: A and B should be vertically arranged (similar x)
    let x_a = positions.get("A").expect("A not found");
    let x_b = positions.get("B").expect("B not found");
    assert!(
        (x_a - x_b).abs() < 1.0,
        "Inner BT: A ({x_a}) and B ({x_b}) should have similar x"
    );

    // Outer LR: C should be to the side of the inner subgraph
    assert!(positions.contains_key("C"), "C should be present");

    assert!(!svg.contains("NaN"), "SVG should not contain NaN");
}

#[test]
fn render_svg_all_direction_override_fixtures_valid() {
    // Run all direction override fixtures and verify no NaN and valid SVG
    let fixtures = [
        "subgraph_direction_lr.mmd",
        "subgraph_direction_cross_boundary.mmd",
        "subgraph_direction_mixed.mmd",
        "subgraph_direction_nested.mmd",
        "subgraph_direction_nested_both.mmd",
    ];
    for fixture in &fixtures {
        let path = format!("tests/fixtures/flowchart/{fixture}");
        let input =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
        let flowchart =
            parse_flowchart(&input).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
        let diagram = build_diagram(&flowchart);
        let svg = render_svg(&diagram, &RenderOptions::default_svg());

        assert!(
            svg.starts_with("<svg"),
            "{fixture}: SVG should start with <svg"
        );
        assert!(
            !svg.contains("NaN"),
            "{fixture}: SVG should not contain NaN"
        );
        // Every fixture should have at least one edge path
        assert!(
            svg.contains("<path"),
            "{fixture}: SVG should contain at least one <path element"
        );
    }
}

#[test]
fn render_svg_direction_override_backward_edge() {
    // Backward edge (B -> Start) crossing subgraph boundary
    let input = r#"graph TD
    Start --> A
    subgraph sg1[Loop Section]
        direction LR
        A --> B
    end
    B --> Start
"#;
    let flowchart = parse_flowchart(input).unwrap();
    let diagram = build_diagram(&flowchart);
    let svg = render_svg(&diagram, &RenderOptions::default_svg());

    let positions = extract_node_x_positions(&svg);

    // LR nodes A and B should be horizontal
    let x_a = positions.get("A").expect("A not found");
    let x_b = positions.get("B").expect("B not found");
    assert!(x_a < x_b, "LR: A ({x_a}) should be left of B ({x_b})");

    assert!(!svg.contains("NaN"), "SVG should not contain NaN");
    assert!(svg.contains("<path"), "SVG should have edge paths");
}

#[test]
fn render_svg_positioned_mmds_routed_basic_includes_paths_and_subgraph() {
    let input = std::fs::read_to_string("tests/fixtures/mmds/positioned/routed-basic.json")
        .expect("positioned fixture should exist");
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    instance.parse(&input).expect("MMDS parse should succeed");

    let svg = instance
        .render(OutputFormat::Svg, &RenderConfig::default())
        .expect("routed MMDS should render SVG");

    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("class=\"subgraph\""));
    assert!(svg.contains("<path"));
    assert!(svg.contains("Start"));
    assert!(svg.contains("Group"));
}

/// Assert MMDS and SVG endpoints agree within `tolerance` for a given edge.
fn assert_mmds_svg_endpoint_convergence(
    diagram: &mmdflux::Diagram,
    from: &str,
    to: &str,
    tolerance: f64,
) {
    // MMDS path (no SVG post-adjustment)
    let mode = MeasurementMode::for_format(OutputFormat::Svg, &RenderConfig::default());
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let geom = run_layered_layout(&mode, diagram, &config).unwrap();
    let routed = route_graph_geometry(diagram, &geom, EdgeRouting::OrthogonalRoute);
    let mmds_edge = routed
        .edges
        .iter()
        .find(|e| e.from == from && e.to == to)
        .unwrap_or_else(|| panic!("MMDS should have edge {from}->{to}"));
    let mmds_start = mmds_edge.path.first().unwrap();
    let mmds_end = mmds_edge.path.last().unwrap();

    // SVG path (with SVG post-adjustment pipeline)
    let svg = render_fixture_svg(diagram, EdgeRouting::OrthogonalRoute, SMOOTH);
    let edge_idx = edge_index(diagram, from, to);
    let svg_points = edge_path_for_svg_order(diagram, &svg, edge_idx);
    let svg_start = svg_points[0];
    let svg_end = svg_points[svg_points.len() - 1];

    // Source convergence
    let dx = (mmds_start.x - svg_start.0).abs();
    let dy = (mmds_start.y - svg_start.1).abs();
    assert!(
        dx <= tolerance && dy <= tolerance,
        "MMDS/SVG source convergence failed for {from}->{to}: mmds={mmds_start:?}, svg={svg_start:?}, delta=({dx:.2}, {dy:.2})"
    );

    // Target convergence
    let dx = (mmds_end.x - svg_end.0).abs();
    let dy = (mmds_end.y - svg_end.1).abs();
    assert!(
        dx <= tolerance && dy <= tolerance,
        "MMDS/SVG target convergence failed for {from}->{to}: mmds=({:.2}, {:.2}), svg={svg_end:?}, delta=({dx:.2}, {dy:.2})",
        mmds_end.x,
        mmds_end.y
    );
}

#[test]
fn mmds_svg_diamond_endpoint_convergence_decision() {
    let diagram = load_flowchart_fixture_diagram("decision.mmd");

    // Tolerance accounts for SVG marker offsets (~3-4px for arrow markers).
    // Before single-sourcing, diamond endpoints diverged by 30-40+px.
    let tolerance = 5.0;

    // Test edges from diamond node B (source convergence)
    for (from, to) in [("B", "C"), ("B", "D")] {
        assert_mmds_svg_endpoint_convergence(&diagram, from, to, tolerance);
    }

    // Test edge into diamond node B (target convergence)
    assert_mmds_svg_endpoint_convergence(&diagram, "A", "B", tolerance);
}

#[test]
fn mmds_svg_diamond_endpoint_convergence_diamond_fan_out() {
    let diagram = load_flowchart_fixture_diagram("diamond_fan_out.mmd");
    let tolerance = 5.0;
    for to in ["B", "C", "D"] {
        assert_mmds_svg_endpoint_convergence(&diagram, "A", to, tolerance);
    }
}

#[test]
fn mmds_svg_hexagon_endpoint_convergence_hexagon_flow() {
    let diagram = load_flowchart_fixture_diagram("hexagon_flow.mmd");
    let tolerance = 5.0;
    // Fan-out from hexagon A
    for to in ["B", "D"] {
        assert_mmds_svg_endpoint_convergence(&diagram, "A", to, tolerance);
    }
    // Fan-in to hexagon A
    assert_mmds_svg_endpoint_convergence(&diagram, "C", "A", tolerance);
}

#[test]
fn mmds_svg_diamond_backward_endpoint_convergence() {
    let diagram = load_flowchart_fixture_diagram("diamond_backward.mmd");
    let tolerance = 5.0;
    // Forward edges to/from diamond B
    assert_mmds_svg_endpoint_convergence(&diagram, "A", "B", tolerance);
    assert_mmds_svg_endpoint_convergence(&diagram, "B", "C", tolerance);
    // Backward edge C->B (target is diamond)
    assert_mmds_svg_endpoint_convergence(&diagram, "C", "B", tolerance);
}

#[test]
fn mmds_svg_mixed_shape_chain_endpoint_convergence() {
    let diagram = load_flowchart_fixture_diagram("mixed_shape_chain.mmd");
    let tolerance = 5.0;
    // A[rect]->B{diamond}->C{{hexagon}}->D[rect]
    assert_mmds_svg_endpoint_convergence(&diagram, "A", "B", tolerance);
    assert_mmds_svg_endpoint_convergence(&diagram, "B", "C", tolerance);
    assert_mmds_svg_endpoint_convergence(&diagram, "C", "D", tolerance);
}

// --- Task 4.3: SVG style/topology decoupling ---

/// Extract (start, end) endpoint coordinates for each edge path in the SVG.
fn extract_edge_endpoints(svg: &str) -> Vec<((f64, f64), (f64, f64))> {
    edge_path_data(svg)
        .iter()
        .filter_map(|d| {
            let pts = parse_svg_path_points(d);
            if pts.len() >= 2 {
                Some((pts[0], pts[pts.len() - 1]))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn svg_style_does_not_alter_edge_path_topology() {
    // Style (Sharp vs Smooth) should not change which points edges connect —
    // only how segments are drawn.
    let diagram = load_flowchart_fixture_diagram("fan_in.mmd");

    let sharp_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SHARP);
    let smooth_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SMOOTH);

    let sharp_endpoints = extract_edge_endpoints(&sharp_svg);
    let smooth_endpoints = extract_edge_endpoints(&smooth_svg);

    assert_eq!(
        sharp_endpoints.len(),
        smooth_endpoints.len(),
        "same number of edge paths"
    );
    for (i, (se, sme)) in sharp_endpoints
        .iter()
        .zip(smooth_endpoints.iter())
        .enumerate()
    {
        let (sharp_start, sharp_end) = se;
        let (smooth_start, smooth_end) = sme;
        assert!(
            (sharp_start.0 - smooth_start.0).abs() <= 1.0
                && (sharp_start.1 - smooth_start.1).abs() <= 1.0,
            "edge {i} start should match: sharp={sharp_start:?} smooth={smooth_start:?}"
        );
        assert!(
            (sharp_end.0 - smooth_end.0).abs() <= 1.0 && (sharp_end.1 - smooth_end.1).abs() <= 1.0,
            "edge {i} end should match: sharp={sharp_end:?} smooth={smooth_end:?}"
        );
    }
}

#[test]
fn svg_rounded_style_does_not_force_orthogonal_topology() {
    // Rounded applies arc corners to existing engine-provided paths.
    // It must not alter how endpoints connect to nodes.
    let diagram = load_flowchart_fixture_diagram("fan_in.mmd");

    let rounded_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, ROUNDED);
    let smooth_svg = render_fixture_svg(&diagram, EdgeRouting::OrthogonalRoute, SMOOTH);

    let rounded_endpoints = extract_edge_endpoints(&rounded_svg);
    let smooth_endpoints = extract_edge_endpoints(&smooth_svg);

    assert_eq!(
        rounded_endpoints.len(),
        smooth_endpoints.len(),
        "same number of edge paths"
    );
    for (i, (re, sme)) in rounded_endpoints
        .iter()
        .zip(smooth_endpoints.iter())
        .enumerate()
    {
        let (r_start, r_end) = re;
        let (s_start, s_end) = sme;
        assert!(
            (r_start.0 - s_start.0).abs() <= 1.0 && (r_start.1 - s_start.1).abs() <= 1.0,
            "edge {i} start should match: rounded={r_start:?} smooth={s_start:?}"
        );
        assert!(
            (r_end.0 - s_end.0).abs() <= 1.0 && (r_end.1 - s_end.1).abs() <= 1.0,
            "edge {i} end should match: rounded={r_end:?} smooth={s_end:?}"
        );
    }
}
