//! Graph-family SVG marker definitions.

use std::collections::BTreeSet;

use super::{GraphSvgPalette, MarkerDef, dynamic_css_attrs};
use crate::render::svg::{SvgWriter, fmt_f64};

pub(super) fn render_defs(
    writer: &mut SvgWriter,
    scale: f64,
    palette: &GraphSvgPalette,
    used_markers: &BTreeSet<MarkerDef>,
) {
    if used_markers.is_empty() {
        return;
    }

    let base = 10.0;
    let half = base / 2.0;
    let marker_size = 8.0 * scale;

    writer.start_tag("<defs>");

    let fill_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "graph-marker-fill",
        &["fill:var(--_arrow);"],
    );
    let stroke_dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "graph-marker-stroke",
        &["stroke:var(--_arrow);"],
    );
    let circle_vb = 12.0;
    let circle_marker_size = 12.0 * scale;
    let diamond_size = 12.0;
    let diamond_half = diamond_size / 2.0;
    let diamond_marker_size = 12.0 * scale;

    for marker in used_markers {
        let marker_color = marker.color.as_deref().unwrap_or(&palette.marker_color);
        let fill_dynamic_attrs = if marker.color.is_none() {
            fill_dynamic_attrs.as_str()
        } else {
            ""
        };
        let stroke_dynamic_attrs = if marker.color.is_none() {
            stroke_dynamic_attrs.as_str()
        } else {
            ""
        };
        let circle_dynamic_attrs = if marker.color.is_none() {
            dynamic_css_attrs(
                palette.dynamic_css,
                "graph-circle-marker",
                &["stroke:var(--_arrow);fill:var(--bg);"],
            )
        } else {
            String::new()
        };

        match marker.kind {
            "arrowhead" => {
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {base} {base}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    base = fmt_f64(base),
                    ref_x = fmt_f64(half),
                    ref_y = fmt_f64(half),
                    mw = fmt_f64(marker_size),
                    mh = fmt_f64(marker_size)
                );
                writer.start_tag(&marker);
                let path = format!(
                    "<path d=\"M 0 0 L {tip} {mid} L 0 {size} z\" fill=\"{color}\"{dynamic_attrs} />",
                    tip = fmt_f64(base),
                    mid = fmt_f64(half),
                    size = fmt_f64(base),
                    color = marker_color,
                    dynamic_attrs = fill_dynamic_attrs
                );
                writer.push_line(&path);
                writer.end_tag("</marker>");
            }
            "crosshead" => {
                let cross_size = 11.0;
                let cross_marker_size = 11.0 * scale;
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    size = fmt_f64(cross_size),
                    ref_x = fmt_f64(12.0),
                    ref_y = fmt_f64(5.2),
                    mw = fmt_f64(cross_marker_size),
                    mh = fmt_f64(cross_marker_size)
                );
                writer.start_tag(&marker);
                let path = format!(
                    "<path d=\"M 1,1 l 9,9 M 10,1 l -9,9\" stroke=\"{color}\" stroke-width=\"2\"{dynamic_attrs} />",
                    color = marker_color,
                    dynamic_attrs = stroke_dynamic_attrs
                );
                writer.push_line(&path);
                writer.end_tag("</marker>");
            }
            "circlehead" => {
                let circle_fill = palette
                    .root_style
                    .background_color
                    .as_deref()
                    .unwrap_or("white");
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    size = fmt_f64(circle_vb),
                    ref_x = fmt_f64(11.0),
                    ref_y = fmt_f64(6.0),
                    mw = fmt_f64(circle_marker_size),
                    mh = fmt_f64(circle_marker_size)
                );
                writer.start_tag(&marker);
                let circle = format!(
                    "<circle cx=\"6\" cy=\"6\" r=\"5\" stroke=\"{color}\" stroke-width=\"1\" fill=\"{fill}\"{dynamic_attrs} />",
                    color = marker_color,
                    fill = circle_fill,
                    dynamic_attrs = circle_dynamic_attrs
                );
                writer.push_line(&circle);
                writer.end_tag("</marker>");
            }
            "open-arrowhead" => {
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {base} {base}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    base = fmt_f64(base),
                    ref_x = fmt_f64(half),
                    ref_y = fmt_f64(half),
                    mw = fmt_f64(marker_size),
                    mh = fmt_f64(marker_size)
                );
                writer.start_tag(&marker);
                let polygon = format!(
                    "<polygon points=\"0,0 {tip},{mid} 0,{size}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1\"{dynamic_attrs} />",
                    tip = fmt_f64(base),
                    mid = fmt_f64(half),
                    size = fmt_f64(base),
                    color = marker_color,
                    dynamic_attrs = stroke_dynamic_attrs
                );
                writer.push_line(&polygon);
                writer.end_tag("</marker>");
            }
            "diamondhead" => {
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    size = fmt_f64(diamond_size),
                    ref_x = fmt_f64(diamond_half),
                    ref_y = fmt_f64(diamond_half),
                    mw = fmt_f64(diamond_marker_size),
                    mh = fmt_f64(diamond_marker_size)
                );
                writer.start_tag(&marker);
                let polygon = format!(
                    "<polygon points=\"0,{mid} {mid},0 {size},{mid} {mid},{size}\" fill=\"{color}\"{dynamic_attrs} />",
                    mid = fmt_f64(diamond_half),
                    size = fmt_f64(diamond_size),
                    color = marker_color,
                    dynamic_attrs = fill_dynamic_attrs
                );
                writer.push_line(&polygon);
                writer.end_tag("</marker>");
            }
            "open-diamondhead" => {
                let marker = format!(
                    "<marker id=\"{id}\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
                    id = marker.id,
                    size = fmt_f64(diamond_size),
                    ref_x = fmt_f64(diamond_half),
                    ref_y = fmt_f64(diamond_half),
                    mw = fmt_f64(diamond_marker_size),
                    mh = fmt_f64(diamond_marker_size)
                );
                writer.start_tag(&marker);
                let polygon = format!(
                    "<polygon points=\"0,{mid} {mid},0 {size},{mid} {mid},{size}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1\"{dynamic_attrs} />",
                    mid = fmt_f64(diamond_half),
                    size = fmt_f64(diamond_size),
                    color = marker_color,
                    dynamic_attrs = stroke_dynamic_attrs
                );
                writer.push_line(&polygon);
                writer.end_tag("</marker>");
            }
            _ => {}
        }
    }

    writer.end_tag("</defs>");
}
