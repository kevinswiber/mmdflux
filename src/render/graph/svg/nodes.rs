//! Node and subgraph SVG drawing for graph rendering.

use std::fmt::Write;

use super::bounds::scale_rect;
use super::edges::{document_svg_path, polygon_points};
use super::text::{TextRenderStyle, render_text_centered};
use super::{GraphSvgPalette, Point, Rect, dynamic_css_attrs};
use crate::graph::geometry::{FRect, GraphGeometry};
use crate::graph::measure::ProportionalTextMetrics;
use crate::graph::routing::hexagon_vertices;
use crate::graph::{Direction, Graph, Node, Shape};
use crate::render::svg::{SvgWriter, escape_text, fmt_f64};

#[derive(Clone, Copy)]
struct ResolvedSvgNodeStyle<'a> {
    fill: Option<&'a str>,
    stroke: Option<&'a str>,
    text: Option<&'a str>,
    font_style: Option<&'a str>,
    font_weight: Option<&'a str>,
    stroke_width: Option<&'a str>,
    stroke_dasharray: Option<&'a str>,
    rx: Option<&'a str>,
}

impl<'a> ResolvedSvgNodeStyle<'a> {
    fn from_node(node: &'a Node) -> Self {
        Self {
            fill: node.style.fill.as_ref().map(|color| color.raw()),
            stroke: node.style.stroke.as_ref().map(|color| color.raw()),
            text: node.style.color.as_ref().map(|color| color.raw()),
            font_style: node.style.font_style.as_deref(),
            font_weight: node.style.font_weight.as_deref(),
            stroke_width: node.style.stroke_width.as_deref(),
            stroke_dasharray: node.style.stroke_dasharray.as_deref(),
            rx: node.style.rx.as_deref(),
        }
    }

    fn fill_or(self, default: &'a str) -> &'a str {
        self.fill.unwrap_or(default)
    }

    fn stroke_or(self, default: &'a str) -> &'a str {
        self.stroke.unwrap_or(default)
    }

    fn text_or(self, default: &'a str) -> &'a str {
        self.text.unwrap_or(default)
    }

    fn fill_is_overridden(self) -> bool {
        self.fill.is_some()
    }

    fn stroke_is_overridden(self) -> bool {
        self.stroke.is_some()
    }

    fn text_is_overridden(self) -> bool {
        self.text.is_some()
    }
}

#[derive(Clone, Copy)]
struct NodeLabelRenderContext<'a> {
    rect: &'a Rect,
    style: ResolvedSvgNodeStyle<'a>,
    metrics: &'a ProportionalTextMetrics,
    scale: f64,
    palette: &'a GraphSvgPalette,
}

pub(super) fn render_subgraphs(
    writer: &mut SvgWriter,
    diagram: &Graph,
    geom: &GraphGeometry,
    metrics: &ProportionalTextMetrics,
    scale: f64,
    palette: &GraphSvgPalette,
) {
    if geom.subgraphs.is_empty() {
        return;
    }

    let mut subgraphs: Vec<_> = geom
        .subgraphs
        .iter()
        .filter_map(|(id, sg_geom)| {
            diagram
                .subgraphs
                .get(id)
                .filter(|sg| !sg.invisible)
                .map(|_| (id, sg_geom))
        })
        .collect();

    subgraphs.sort_by(|a, b| a.1.depth.cmp(&b.1.depth).then_with(|| a.0.cmp(b.0)));

    writer.start_group("clusters");
    for (_id, sg_geom) in subgraphs {
        let rect = scale_rect(&sg_geom.rect, scale);
        let stroke_width = fmt_f64(1.0 * scale);
        let dynamic_attrs = dynamic_css_attrs(
            palette.dynamic_css,
            "graph-subgraph-stroke",
            &["stroke:var(--_inner-stroke);"],
        );
        let rect_line = format!(
            "<rect class=\"subgraph\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\"{dynamic_attrs} />",
            x = fmt_f64(rect.x),
            y = fmt_f64(rect.y),
            w = fmt_f64(rect.width),
            h = fmt_f64(rect.height),
            stroke = palette.subgraph_stroke,
            stroke_width = stroke_width,
            dynamic_attrs = dynamic_attrs
        );
        writer.push_line(&rect_line);

        if !sg_geom.title.trim().is_empty() {
            let title_x = rect.x + rect.width / 2.0;
            let title_y = rect.y + metrics.font_size * 0.25;
            let dynamic_attrs = dynamic_css_attrs(
                palette.dynamic_css,
                "graph-subgraph-text",
                &["fill:var(--_group-hdr);"],
            );
            let text = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"hanging\" fill=\"{color}\"{dynamic_attrs}>{label}</text>",
                x = fmt_f64(title_x),
                y = fmt_f64(title_y),
                color = palette.subgraph_title_text,
                dynamic_attrs = dynamic_attrs,
                label = escape_text(&sg_geom.title)
            );
            writer.push_line(&text);
        }
    }
    writer.end_group();
}

pub(super) fn render_nodes(
    writer: &mut SvgWriter,
    diagram: &Graph,
    geom: &GraphGeometry,
    metrics: &ProportionalTextMetrics,
    scale: f64,
    palette: &GraphSvgPalette,
) {
    writer.start_group("nodes");

    let mut node_ids: Vec<&String> = diagram.nodes.keys().collect();
    node_ids.sort();

    for node_id in node_ids {
        let node = &diagram.nodes[node_id];
        let Some(pos_node) = geom.nodes.get(node_id) else {
            continue;
        };
        let rect: Rect = pos_node.rect;
        let style = ResolvedSvgNodeStyle::from_node(node);
        render_node_shape(
            writer,
            node,
            &rect,
            scale,
            diagram.direction,
            style,
            palette,
        );

        let center = rect.center();
        let mut text_x = center.x;
        let mut text_y = center.y;
        // Offset text downward for cylinders so it centers in the body below the top cap.
        if node.shape == Shape::Cylinder {
            let rx = rect.width / 2.0;
            let ry = rx / (2.5 + rect.width / 50.0);
            text_y += ry / 2.0;
        }
        // Offset text upward for document shapes to center in content area above wave.
        if node.shape == Shape::Document {
            let wave_amp = rect.height / 9.0;
            text_y -= wave_amp / 2.0;
        }
        if node.shape == Shape::TaggedDocument {
            let wave_amp = rect.height / 5.0;
            text_y -= wave_amp / 2.0;
        }
        if node.shape == Shape::Documents {
            let offset = 5.0;
            let front_h = rect.height - 2.0 * offset;
            let wave_amp = front_h / 5.0;
            text_y += offset - wave_amp / 2.0;
            text_x -= offset; // front doc is shifted left
        }
        render_node_label(
            writer,
            Point {
                x: text_x * scale,
                y: text_y * scale,
            },
            &node.label,
            NodeLabelRenderContext {
                rect: &rect,
                style,
                metrics,
                scale,
                palette,
            },
        );
    }

    writer.end_group();
}

/// Render a node's label, converting `Node::SEPARATOR` lines into horizontal rules.
fn render_node_label(
    writer: &mut SvgWriter,
    center: Point,
    text: &str,
    context: NodeLabelRenderContext<'_>,
) {
    let lines: Vec<&str> = text.split('\n').collect();
    let has_separator = lines.contains(&Node::SEPARATOR);
    let stroke = context.style.stroke_or(&context.palette.node_stroke);
    let text_color = context.style.text_or(&context.palette.node_text);
    let text_dynamic_attrs = if context.style.text_is_overridden() {
        String::new()
    } else {
        dynamic_css_attrs(
            context.palette.dynamic_css,
            "graph-node-text",
            &["fill:var(--_text);"],
        )
    };
    let mut font_attrs = text_dynamic_attrs;
    if let Some(fw) = context.style.font_weight {
        write!(font_attrs, " font-weight=\"{fw}\"").unwrap();
    }
    if let Some(fs) = context.style.font_style {
        write!(font_attrs, " font-style=\"{fs}\"").unwrap();
    }

    if !has_separator {
        render_text_centered(
            writer,
            center,
            text,
            context.metrics,
            context.scale,
            TextRenderStyle {
                color: text_color,
                extra_attrs: font_attrs.as_str(),
                background: None,
            },
        );
        return;
    }

    let line_height = context.metrics.line_height * context.scale;
    let total_height = line_height * (lines.len().saturating_sub(1) as f64);
    let start_y = center.y - total_height / 2.0;
    let x1 = context.rect.x * context.scale;
    let x2 = (context.rect.x + context.rect.width) * context.scale;
    // Left-align x: node left edge + padding (matches text renderer's x+2 convention)
    let left_x = x1 + context.metrics.node_padding_x * context.scale;
    let mut past_separator = false;

    for (idx, line_text) in lines.iter().enumerate() {
        let line_y = start_y + line_height * idx as f64;
        if *line_text == Node::SEPARATOR {
            past_separator = true;
            let line = format!(
                "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"{stroke}\" stroke-width=\"{sw}\"{dynamic_attrs} />",
                x1 = fmt_f64(x1),
                y = fmt_f64(line_y),
                x2 = fmt_f64(x2),
                stroke = stroke,
                sw = fmt_f64(context.scale),
                dynamic_attrs = if context.style.stroke_is_overridden() {
                    String::new()
                } else {
                    dynamic_css_attrs(
                        context.palette.dynamic_css,
                        "graph-node-stroke",
                        &["stroke:var(--_node-stroke);"],
                    )
                },
            );
            writer.push_line(&line);
        } else if past_separator {
            // Members: left-aligned
            let line = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"start\" dominant-baseline=\"middle\" fill=\"{color}\"{dynamic_attrs}>{text}</text>",
                x = fmt_f64(left_x),
                y = fmt_f64(line_y),
                color = text_color,
                dynamic_attrs = font_attrs.as_str(),
                text = escape_text(line_text)
            );
            writer.push_line(&line);
        } else {
            // Class name: centered
            let line = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\"{dynamic_attrs}>{text}</text>",
                x = fmt_f64(center.x),
                y = fmt_f64(line_y),
                color = text_color,
                dynamic_attrs = font_attrs.as_str(),
                text = escape_text(line_text)
            );
            writer.push_line(&line);
        }
    }
}

fn render_node_shape(
    writer: &mut SvgWriter,
    node: &Node,
    rect: &Rect,
    scale: f64,
    direction: Direction,
    node_style: ResolvedSvgNodeStyle<'_>,
    palette: &GraphSvgPalette,
) {
    let rect = scale_rect(rect, scale);
    let default_stroke_width = fmt_f64(1.0 * scale);
    let stroke_width = node_style.stroke_width.unwrap_or(&default_stroke_width);
    let fill = node_style.fill_or(&palette.node_fill);
    let stroke = node_style.stroke_or(&palette.node_stroke);
    let mut dynamic_declarations = Vec::new();
    if !node_style.fill_is_overridden() {
        dynamic_declarations.push("fill:var(--_node-fill);");
    }
    if !node_style.stroke_is_overridden() {
        dynamic_declarations.push("stroke:var(--_node-stroke);");
    }
    let dynamic_attrs = dynamic_css_attrs(
        palette.dynamic_css,
        "graph-node-shape",
        &dynamic_declarations,
    );
    let dasharray_attr = node_style
        .stroke_dasharray
        .map(|v| format!(" stroke-dasharray=\"{v}\""))
        .unwrap_or_default();
    let style = format!(
        " fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" stroke-linejoin=\"round\"{dasharray_attr}{dynamic_attrs}",
        fill = fill,
        stroke = stroke,
        stroke_width = stroke_width,
        dasharray_attr = dasharray_attr,
        dynamic_attrs = dynamic_attrs
    );

    match node.shape {
        Shape::Rectangle => {
            let rx_attr = node_style
                .rx
                .map(|v| format!(" rx=\"{v}\" ry=\"{v}\""))
                .unwrap_or_default();
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{rx_attr}{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                rx_attr = rx_attr,
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Round => {
            let default_radius = fmt_f64(5.0 * scale);
            let rx_val = node_style.rx.unwrap_or(&default_radius);
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"{rx}\" ry=\"{ry}\"{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                rx = rx_val,
                ry = rx_val,
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Stadium => {
            let radius = rect.height / 2.0;
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"{rx}\" ry=\"{ry}\"{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                rx = fmt_f64(radius),
                ry = fmt_f64(radius),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Document => {
            // Single closed path with sine wave bottom (matching Mermaid waveEdgedRectangle).
            // wave_amp = content_h/8; total_h = content_h + wave_amp = 9/8 * content_h
            let wave_amp = rect.height / 9.0;
            let d = document_svg_path(rect.x, rect.y, rect.width, rect.height, wave_amp);
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));
        }
        Shape::Documents => {
            // Three stacked document paths (back → middle → front), each filled white.
            // Front doc covers the others; back docs peek out at top-right.
            let offset = 5.0 * scale;
            let doc_w = rect.width - 2.0 * offset;
            let doc_h = rect.height - 2.0 * offset;
            // wave_amp = content_h/4; doc_h = content_h + wave_amp = 5/4 * content_h
            let wave_amp = doc_h / 5.0;
            // Back document (top-right)
            let d = document_svg_path(rect.x + 2.0 * offset, rect.y, doc_w, doc_h, wave_amp);
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));
            // Middle document
            let d = document_svg_path(rect.x + offset, rect.y + offset, doc_w, doc_h, wave_amp);
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));
            // Front document
            let d = document_svg_path(rect.x, rect.y + 2.0 * offset, doc_w, doc_h, wave_amp);
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));
        }
        Shape::TaggedDocument => {
            // Document with sine wave bottom + page fold at bottom-right.
            // wave_amp = content_h/4; total_h = content_h + wave_amp = 5/4 * content_h
            let wave_amp = rect.height / 5.0;
            let wave_y = rect.y + rect.height - wave_amp;
            let freq = std::f64::consts::TAU * 0.8 / rect.width;

            // Main document path with wave bottom.
            let d = document_svg_path(rect.x, rect.y, rect.width, rect.height, wave_amp);
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));

            // Fold at bottom-right corner: a white-filled shape that covers the
            // wave in that area and shows a diagonal fold line.
            let content_h = rect.height - wave_amp;
            let fold_w = 0.2 * rect.width;
            let fold_h = 0.25 * content_h;
            let right_x = rect.x + rect.width;
            let fold_left_x = right_x - fold_w;

            // Wave Y at the fold's left edge.
            let t_left = (fold_left_x - rect.x) / rect.width;
            let y_fold_left = wave_y + wave_amp * (freq * t_left * rect.width).sin();
            let fold_top_y = y_fold_left - fold_h;

            // Build fold shape: follow the wave from fold_left to right edge,
            // then up to fold_top; Z closes with the diagonal (the fold line).
            let steps = 50usize;
            let i_start = (t_left * steps as f64).ceil() as usize;
            let mut fold_d = format!("M{},{}", fmt_f64(fold_left_x), fmt_f64(y_fold_left));
            for i in i_start..=steps {
                let t = i as f64 / steps as f64;
                let x = rect.x + t * rect.width;
                let y = wave_y + wave_amp * (freq * t * rect.width).sin();
                let _ = write!(fold_d, " L{},{}", fmt_f64(x), fmt_f64(y));
            }
            let _ = write!(fold_d, " L{},{}", fmt_f64(right_x), fmt_f64(fold_top_y));
            fold_d.push_str(" Z");
            writer.push_line(&format!(
                "<path d=\"{fold_d}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" />"
            ));
        }
        Shape::Card => {
            // Polygon with cut corner at top-left (matching Mermaid card shape).
            let fold = 12.0 * scale;
            let x = rect.x;
            let y = rect.y;
            let w = rect.width;
            let h = rect.height;
            let d = format!(
                "M{},{} L{},{} L{},{} L{},{} L{},{} Z",
                fmt_f64(x + fold),
                fmt_f64(y),
                fmt_f64(x + w),
                fmt_f64(y),
                fmt_f64(x + w),
                fmt_f64(y + h),
                fmt_f64(x),
                fmt_f64(y + h),
                fmt_f64(x),
                fmt_f64(y + fold),
            );
            writer.push_line(&format!("<path d=\"{d}\"{style} />"));
        }
        Shape::TaggedRect => {
            // Rectangle with triangle tag at bottom-right (matching Mermaid taggedRect).
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                style = style
            );
            writer.push_line(&line);
            // Triangle tag at bottom-right
            let tag = 0.2 * rect.height;
            let tag_d = format!(
                "M{},{} L{},{} L{},{} Z",
                fmt_f64(rect.x + rect.width - tag),
                fmt_f64(rect.y + rect.height),
                fmt_f64(rect.x + rect.width),
                fmt_f64(rect.y + rect.height),
                fmt_f64(rect.x + rect.width),
                fmt_f64(rect.y + rect.height - tag),
            );
            writer.push_line(&format!(
                "<path d=\"{tag_d}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" />"
            ));
        }
        Shape::Diamond => {
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let points = vec![
                (cx, rect.y),
                (rect.x + rect.width, cy),
                (cx, rect.y + rect.height),
                (rect.x, cy),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Hexagon => {
            let frect = FRect::new(rect.x, rect.y, rect.width, rect.height);
            let verts = hexagon_vertices(frect);
            let points: Vec<(f64, f64)> = verts.iter().map(|v| (v.x, v.y)).collect();
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Asymmetric => {
            let indent = rect.width * 0.2;
            let cy = rect.y + rect.height / 2.0;
            let points = vec![
                (rect.x + indent, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x + indent, rect.y + rect.height),
                (rect.x, cy),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Parallelogram => {
            let indent = rect.width * 0.2;
            let points = vec![
                (rect.x + indent, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width - indent, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::InvParallelogram => {
            let indent = rect.width * 0.2;
            let points = vec![
                (rect.x, rect.y),
                (rect.x + rect.width - indent, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x + indent, rect.y + rect.height),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::ManualInput => {
            let slant = rect.height * 0.25;
            let points = vec![
                (rect.x + slant, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Trapezoid => {
            let indent = rect.width * 0.2;
            let points = vec![
                (rect.x + indent, rect.y),
                (rect.x + rect.width - indent, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::InvTrapezoid => {
            let indent = rect.width * 0.2;
            let points = vec![
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width - indent, rect.y + rect.height),
                (rect.x + indent, rect.y + rect.height),
            ];
            let line = format!(
                "<polygon points=\"{points}\"{style} />",
                points = polygon_points(&points),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Circle => {
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let rx = rect.width / 2.0;
            let ry = rect.height / 2.0;
            let line = format!(
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\"{style} />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                rx = fmt_f64(rx),
                ry = fmt_f64(ry),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::DoubleCircle => {
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let rx = rect.width / 2.0;
            let ry = rect.height / 2.0;
            let line = format!(
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\"{style} />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                rx = fmt_f64(rx),
                ry = fmt_f64(ry),
                style = style
            );
            writer.push_line(&line);

            let inset = (rect.width.min(rect.height) * 0.12).max(3.0 * scale);
            let inner_rx = (rx - inset).max(0.0);
            let inner_ry = (ry - inset).max(0.0);
            let inner = format!(
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\"{style} />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                rx = fmt_f64(inner_rx),
                ry = fmt_f64(inner_ry),
                style = style
            );
            writer.push_line(&inner);
        }
        Shape::SmallCircle => {
            // UML initial node: small filled circle
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let radius = rect.width.min(rect.height) / 2.0;
            let circle = format!(
                "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" stroke-linejoin=\"round\" />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                r = fmt_f64(radius),
                fill = node_style.fill_or(stroke),
                stroke = stroke,
                sw = stroke_width
            );
            writer.push_line(&circle);
        }
        Shape::FramedCircle => {
            // UML activity final node: outer circle with filled inner circle
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let outer_radius = rect.width.min(rect.height) / 2.0;
            let gap = 5.0 * scale;
            let inner_radius = outer_radius - gap;
            let outer = format!(
                "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\"{style} />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                r = fmt_f64(outer_radius),
                style = style
            );
            writer.push_line(&outer);
            let inner = format!(
                "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" stroke-linejoin=\"round\" />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                r = fmt_f64(inner_radius),
                fill = node_style.fill_or(stroke),
                stroke = stroke,
                sw = stroke_width
            );
            writer.push_line(&inner);
        }
        Shape::CrossedCircle => {
            // UML flow final node: circle with diagonal cross
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            let radius = rect.width.min(rect.height) / 2.0;
            let circle = format!(
                "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\"{style} />",
                cx = fmt_f64(cx),
                cy = fmt_f64(cy),
                r = fmt_f64(radius),
                style = style
            );
            writer.push_line(&circle);
            let stroke_attr = format!(
                " stroke=\"{stroke}\" stroke-width=\"{stroke_width}\"",
                stroke = stroke,
                stroke_width = stroke_width
            );
            // Cross lines span the full radius at 45 degrees
            let d = radius * std::f64::consts::FRAC_1_SQRT_2;
            let line1 = format!(
                "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{stroke} />",
                x1 = fmt_f64(cx - d),
                y1 = fmt_f64(cy - d),
                x2 = fmt_f64(cx + d),
                y2 = fmt_f64(cy + d),
                stroke = stroke_attr
            );
            let line2 = format!(
                "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{stroke} />",
                x1 = fmt_f64(cx - d),
                y1 = fmt_f64(cy + d),
                x2 = fmt_f64(cx + d),
                y2 = fmt_f64(cy - d),
                stroke = stroke_attr
            );
            writer.push_line(&line1);
            writer.push_line(&line2);
        }
        Shape::Subroutine => {
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                style = style
            );
            writer.push_line(&line);

            let inset = 8.0 * scale;
            let x1 = rect.x + inset;
            let x2 = rect.x + rect.width - inset;
            let stroke = format!(
                " stroke=\"{stroke}\" stroke-width=\"{stroke_width}\"",
                stroke = stroke,
                stroke_width = stroke_width
            );
            let left_line = format!(
                "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x1}\" y2=\"{y2}\"{stroke} />",
                x1 = fmt_f64(x1),
                y1 = fmt_f64(rect.y),
                y2 = fmt_f64(rect.y + rect.height),
                stroke = stroke
            );
            let right_line = format!(
                "<line x1=\"{x2}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{stroke} />",
                x2 = fmt_f64(x2),
                y1 = fmt_f64(rect.y),
                y2 = fmt_f64(rect.y + rect.height),
                stroke = stroke
            );
            writer.push_line(&left_line);
            writer.push_line(&right_line);
        }
        Shape::Cylinder => {
            // 3D cylinder: full ellipse at top, straight sides, half-ellipse at bottom.
            let rx = rect.width / 2.0;
            let ry = rx / (2.5 + rect.width / 50.0);
            let x0 = rect.x;
            let x1 = rect.x + rect.width;
            let top = rect.y + ry;
            let bot = rect.y + rect.height - ry;

            // Outer path: top ellipse (back + front arcs), sides, bottom arc
            let d = format!(
                "M{x0},{top} A{rx},{ry} 0 0,0 {x1},{top} A{rx},{ry} 0 0,0 {x0},{top} L{x0},{bot} A{rx},{ry} 0 0,0 {x1},{bot} L{x1},{top}",
                x0 = fmt_f64(x0),
                x1 = fmt_f64(x1),
                top = fmt_f64(top),
                bot = fmt_f64(bot),
                rx = fmt_f64(rx),
                ry = fmt_f64(ry),
            );
            let body = format!("<path d=\"{d}\"{style} />", d = d, style = style);
            writer.push_line(&body);

            // Inner line: front edge of top ellipse (creates the 3D rim)
            let inner_d = format!(
                "M{x0},{top} A{rx},{ry} 0 0,1 {x1},{top}",
                x0 = fmt_f64(x0),
                x1 = fmt_f64(x1),
                top = fmt_f64(top),
                rx = fmt_f64(rx),
                ry = fmt_f64(ry),
            );
            let inner_style = format!(
                " fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{sw}\"",
                stroke = stroke,
                sw = stroke_width,
            );
            let inner = format!("<path d=\"{inner_d}\"{inner_style} />");
            writer.push_line(&inner);
        }
        Shape::NoteRect => {
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#fff5ad\" stroke=\"#aaaa33\" stroke-width=\"{sw}\" />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                sw = stroke_width
            );
            writer.push_line(&line);
        }
        Shape::TextBlock => {
            // Borderless: only text will be drawn.
        }
        Shape::ForkJoin => {
            if matches!(direction, Direction::LeftRight | Direction::RightLeft) {
                // Vertical bar for horizontal flow
                let x = rect.x + rect.width / 2.0;
                let stroke = format!(
                    " stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" stroke-linecap=\"square\"",
                    stroke = stroke,
                    stroke_width = fmt_f64((rect.width * 0.3).max(3.0 * scale))
                );
                let line = format!(
                    "<line x1=\"{x}\" y1=\"{y1}\" x2=\"{x}\" y2=\"{y2}\"{stroke} />",
                    x = fmt_f64(x),
                    y1 = fmt_f64(rect.y),
                    y2 = fmt_f64(rect.y + rect.height),
                    stroke = stroke
                );
                writer.push_line(&line);
            } else {
                // Horizontal bar for vertical flow
                let y = rect.y + rect.height / 2.0;
                let stroke = format!(
                    " stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" stroke-linecap=\"square\"",
                    stroke = stroke,
                    stroke_width = fmt_f64((rect.height * 0.3).max(3.0 * scale))
                );
                let line = format!(
                    "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\"{stroke} />",
                    x1 = fmt_f64(rect.x),
                    x2 = fmt_f64(rect.x + rect.width),
                    y = fmt_f64(y),
                    stroke = stroke
                );
                writer.push_line(&line);
            }
        }
    }
}
