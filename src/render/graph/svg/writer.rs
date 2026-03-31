//! Graph-family SVG marker definitions.

use super::STROKE_COLOR;
use crate::render::svg::{SvgWriter, fmt_f64};

pub(super) fn render_defs(writer: &mut SvgWriter, scale: f64) {
    let base = 10.0;
    let half = base / 2.0;
    let marker_size = 8.0 * scale;

    writer.start_tag("<defs>");

    // Normal arrowhead (triangle)
    let marker = format!(
        "<marker id=\"arrowhead\" viewBox=\"0 0 {base} {base}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        base = fmt_f64(base),
        ref_x = fmt_f64(half),
        ref_y = fmt_f64(half),
        mw = fmt_f64(marker_size),
        mh = fmt_f64(marker_size)
    );
    writer.start_tag(&marker);
    let path = format!(
        "<path d=\"M 0 0 L {tip} {mid} L 0 {size} z\" fill=\"{color}\" />",
        tip = fmt_f64(base),
        mid = fmt_f64(half),
        size = fmt_f64(base),
        color = STROKE_COLOR
    );
    writer.push_line(&path);
    writer.end_tag("</marker>");

    // Cross marker (X shape)
    let cross_size = 11.0;
    let cross_marker_size = 11.0 * scale;
    let marker = format!(
        "<marker id=\"crosshead\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(cross_size),
        ref_x = fmt_f64(12.0),
        ref_y = fmt_f64(5.2),
        mw = fmt_f64(cross_marker_size),
        mh = fmt_f64(cross_marker_size)
    );
    writer.start_tag(&marker);
    let path = format!(
        "<path d=\"M 1,1 l 9,9 M 10,1 l -9,9\" stroke=\"{color}\" stroke-width=\"2\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&path);
    writer.end_tag("</marker>");

    // Circle marker (hollow circle for lollipop interfaces)
    // ViewBox 12x12 with circle r=5 centered at (6,6) leaves room for the stroke.
    // refX at the circle's right edge (cx+r=11) so the edge line terminates
    // at the circle boundary rather than penetrating into it.
    let circle_vb = 12.0;
    let circle_marker_size = 12.0 * scale;
    let marker = format!(
        "<marker id=\"circlehead\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(circle_vb),
        ref_x = fmt_f64(11.0),
        ref_y = fmt_f64(6.0),
        mw = fmt_f64(circle_marker_size),
        mh = fmt_f64(circle_marker_size)
    );
    writer.start_tag(&marker);
    let circle = format!(
        "<circle cx=\"6\" cy=\"6\" r=\"5\" stroke=\"{color}\" stroke-width=\"1\" fill=\"white\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&circle);
    writer.end_tag("</marker>");

    // Open arrowhead (hollow triangle for inheritance)
    let marker = format!(
        "<marker id=\"open-arrowhead\" viewBox=\"0 0 {base} {base}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        base = fmt_f64(base),
        ref_x = fmt_f64(half),
        ref_y = fmt_f64(half),
        mw = fmt_f64(marker_size),
        mh = fmt_f64(marker_size)
    );
    writer.start_tag(&marker);
    let polygon = format!(
        "<polygon points=\"0,0 {tip},{mid} 0,{size}\" fill=\"white\" stroke=\"{color}\" stroke-width=\"1\" />",
        tip = fmt_f64(base),
        mid = fmt_f64(half),
        size = fmt_f64(base),
        color = STROKE_COLOR
    );
    writer.push_line(&polygon);
    writer.end_tag("</marker>");

    // Diamond marker (filled diamond for composition)
    let diamond_size = 12.0;
    let diamond_half = diamond_size / 2.0;
    let diamond_marker_size = 12.0 * scale;
    let marker = format!(
        "<marker id=\"diamondhead\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(diamond_size),
        ref_x = fmt_f64(diamond_half),
        ref_y = fmt_f64(diamond_half),
        mw = fmt_f64(diamond_marker_size),
        mh = fmt_f64(diamond_marker_size)
    );
    writer.start_tag(&marker);
    let polygon = format!(
        "<polygon points=\"0,{mid} {mid},0 {size},{mid} {mid},{size}\" fill=\"{color}\" />",
        mid = fmt_f64(diamond_half),
        size = fmt_f64(diamond_size),
        color = STROKE_COLOR
    );
    writer.push_line(&polygon);
    writer.end_tag("</marker>");

    // Open diamond marker (hollow diamond for aggregation)
    let marker = format!(
        "<marker id=\"open-diamondhead\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(diamond_size),
        ref_x = fmt_f64(diamond_half),
        ref_y = fmt_f64(diamond_half),
        mw = fmt_f64(diamond_marker_size),
        mh = fmt_f64(diamond_marker_size)
    );
    writer.start_tag(&marker);
    let polygon = format!(
        "<polygon points=\"0,{mid} {mid},0 {size},{mid} {mid},{size}\" fill=\"white\" stroke=\"{color}\" stroke-width=\"1\" />",
        mid = fmt_f64(diamond_half),
        size = fmt_f64(diamond_size),
        color = STROKE_COLOR
    );
    writer.push_line(&polygon);
    writer.end_tag("</marker>");

    // ER: Only-one marker (two parallel vertical lines)
    let er_vb = 18.0;
    let er_marker_size = 18.0 * scale;
    let marker = format!(
        "<marker id=\"er-only-one\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(er_vb),
        ref_x = fmt_f64(15.0),
        ref_y = fmt_f64(9.0),
        mw = fmt_f64(er_marker_size),
        mh = fmt_f64(er_marker_size)
    );
    writer.start_tag(&marker);
    let path = format!(
        "<path d=\"M 3,0 L 3,18 M 9,0 L 9,18\" stroke=\"{color}\" stroke-width=\"1\" fill=\"none\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&path);
    writer.end_tag("</marker>");

    // ER: Zero-or-one marker (circle + vertical line)
    let zo_vb_w = 30.0;
    let zo_vb_h = 18.0;
    let zo_marker_w = 30.0 * scale;
    let zo_marker_h = 18.0 * scale;
    let marker = format!(
        "<marker id=\"er-zero-or-one\" viewBox=\"0 0 {w} {h}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        w = fmt_f64(zo_vb_w),
        h = fmt_f64(zo_vb_h),
        ref_x = fmt_f64(27.0),
        ref_y = fmt_f64(9.0),
        mw = fmt_f64(zo_marker_w),
        mh = fmt_f64(zo_marker_h)
    );
    writer.start_tag(&marker);
    let circle = format!(
        "<circle cx=\"9\" cy=\"9\" r=\"6\" stroke=\"{color}\" stroke-width=\"1\" fill=\"white\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&circle);
    let line = format!(
        "<path d=\"M 21,0 L 21,18\" stroke=\"{color}\" stroke-width=\"1\" fill=\"none\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&line);
    writer.end_tag("</marker>");

    // ER: One-or-more marker (crow's foot + vertical line)
    let om_vb_w = 45.0;
    let om_vb_h = 36.0;
    let om_marker_w = 45.0 * scale;
    let om_marker_h = 36.0 * scale;
    let marker = format!(
        "<marker id=\"er-one-or-more\" viewBox=\"0 0 {w} {h}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        w = fmt_f64(om_vb_w),
        h = fmt_f64(om_vb_h),
        ref_x = fmt_f64(27.0),
        ref_y = fmt_f64(18.0),
        mw = fmt_f64(om_marker_w),
        mh = fmt_f64(om_marker_h)
    );
    writer.start_tag(&marker);
    let path = format!(
        "<path d=\"M 3,9 L 3,27 M 9,18 Q 27,0 45,18 Q 27,36 9,18\" stroke=\"{color}\" stroke-width=\"1\" fill=\"none\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&path);
    writer.end_tag("</marker>");

    // ER: Zero-or-more marker (crow's foot + circle)
    let zm_vb_w = 57.0;
    let zm_vb_h = 36.0;
    let zm_marker_w = 57.0 * scale;
    let zm_marker_h = 36.0 * scale;
    let marker = format!(
        "<marker id=\"er-zero-or-more\" viewBox=\"0 0 {w} {h}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        w = fmt_f64(zm_vb_w),
        h = fmt_f64(zm_vb_h),
        ref_x = fmt_f64(39.0),
        ref_y = fmt_f64(18.0),
        mw = fmt_f64(zm_marker_w),
        mh = fmt_f64(zm_marker_h)
    );
    writer.start_tag(&marker);
    let circle = format!(
        "<circle cx=\"9\" cy=\"18\" r=\"6\" stroke=\"{color}\" stroke-width=\"1\" fill=\"white\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&circle);
    let path = format!(
        "<path d=\"M 21,18 Q 39,0 57,18 Q 39,36 21,18\" stroke=\"{color}\" stroke-width=\"1\" fill=\"none\" />",
        color = STROKE_COLOR
    );
    writer.push_line(&path);
    writer.end_tag("</marker>");

    writer.end_tag("</defs>");
}
