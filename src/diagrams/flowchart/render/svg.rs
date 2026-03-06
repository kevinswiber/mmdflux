//! SVG rendering for flowchart diagrams.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::super::geometry::{self, EngineHints, FPoint, GraphGeometry};
use super::layout_building::{build_layered_layout_with_config, layered_config_for_layout};
use super::orthogonal_router::{OrthogonalRoutingOptions, route_edges_orthogonal};
use super::route_policy::effective_edge_direction;
use super::svg_metrics::SvgTextMetrics;
use super::svg_router;
use super::text_layout::{
    center_override_subgraphs, compute_sublayouts, expand_parent_bounds, reconcile_sublayouts,
    resolve_sublayout_overlaps,
};
use super::text_routing_core::{
    build_orthogonal_path_float, hexagon_vertices, intersect_convex_polygon,
};
use crate::diagram::{CornerStyle, Curve, EdgeRouting, PathSimplification};
use crate::graph::{Arrow, Diagram, Direction, Edge, Node, Shape, Stroke};
use crate::layered::{LayoutResult, Point, Rect};
use crate::render::{RenderOptions, layout_config_for_diagram};

const STROKE_COLOR: &str = "#333";
const SUBGRAPH_STROKE: &str = "#888";
const NODE_FILL: &str = "white";
const TEXT_COLOR: &str = "#333";
const MIN_BASIS_VISIBLE_STEM_PX: f64 = 8.0;

#[derive(Clone, Copy)]
struct ResolvedSvgNodeStyle<'a> {
    fill: Option<&'a str>,
    stroke: Option<&'a str>,
    text: Option<&'a str>,
}

impl<'a> ResolvedSvgNodeStyle<'a> {
    fn from_node(node: &'a Node) -> Self {
        Self {
            fill: node.style.fill.as_ref().map(|color| color.raw()),
            stroke: node.style.stroke.as_ref().map(|color| color.raw()),
            text: node.style.color.as_ref().map(|color| color.raw()),
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
}

pub fn render_svg(diagram: &Diagram, options: &RenderOptions) -> String {
    let svg_options = &options.svg;
    let metrics = SvgTextMetrics::new(
        svg_options.font_size,
        svg_options.node_padding_x,
        svg_options.node_padding_y,
    );

    let mut config = layout_config_for_diagram(diagram, options);
    config.ranker = options.ranker;
    if options.cluster_ranksep.is_none() {
        // Mermaid's renderer does not add extra rank separation for clusters.
        // Keep the default for text output but disable it for SVG unless overridden.
        config.cluster_rank_sep = 0.0;
    }

    // Use the canonical flux-layered profile from the engine, ensuring parity
    // with FluxLayeredEngine::solve() (the CLI path).
    let edge_routing = options.edge_routing.unwrap_or({
        // Derive from routing_style (same mapping as flux-layered engine).
        match options.svg.routing_style {
            crate::diagram::RoutingStyle::Direct => EdgeRouting::DirectRoute,
            crate::diagram::RoutingStyle::Polyline => EdgeRouting::PolylineRoute,
            crate::diagram::RoutingStyle::Orthogonal => EdgeRouting::OrthogonalRoute,
        }
    });
    let input_cfg = crate::layered::LayoutConfig::default();
    let mut flux_flags = super::super::engine::flux_layout_profile(&input_cfg, edge_routing);
    // Apply crowding adaptation for large diagrams (same threshold as engine).
    if diagram.nodes.len() >= 10 {
        let mode = super::super::engine::MeasurementMode::Svg(metrics.clone());
        if let Ok(adapted) = super::super::engine::adapt_flux_profile_for_reversed_chain_crowding(
            &mode,
            diagram,
            edge_routing,
            &flux_flags,
        ) {
            flux_flags = adapted;
        }
    }
    let geom = build_svg_layout_with_flags(
        diagram,
        &config,
        &metrics,
        edge_routing,
        false,
        Some(&flux_flags),
    );
    let rerouted_edges = geom.rerouted_edges.clone();
    let override_nodes = svg_router::build_override_node_map(diagram);
    render_svg_with_geometry_context(
        diagram,
        options,
        &geom,
        &rerouted_edges,
        &override_nodes,
        edge_routing,
    )
}

/// Build SVG layout with optional engine layout config for enhancement flags.
///
/// When `engine_flags` is provided, engine-specific layout flags are overlaid
/// onto the internal LayoutConfig. Without it, flags default to false.
pub(crate) fn build_svg_layout_with_flags(
    diagram: &Diagram,
    config: &super::text_layout::TextLayoutConfig,
    metrics: &SvgTextMetrics,
    edge_routing: EdgeRouting,
    skip_non_isolated_overrides: bool,
    engine_flags: Option<&crate::layered::LayoutConfig>,
) -> GraphGeometry {
    let direction = diagram.direction;
    let mut layered_config = layered_config_for_layout(diagram, config);
    if let Some(flags) = engine_flags {
        layered_config.greedy_switch = flags.greedy_switch;
        layered_config.model_order_tiebreak = flags.model_order_tiebreak;
        layered_config.variable_rank_spacing = flags.variable_rank_spacing;
        layered_config.always_compound_ordering = flags.always_compound_ordering;
        layered_config.track_reversed_chains = flags.track_reversed_chains;
        layered_config.per_edge_label_spacing = flags.per_edge_label_spacing;
        layered_config.label_side_selection = flags.label_side_selection;
        layered_config.label_dummy_strategy = flags.label_dummy_strategy;
    }
    let mut layout = build_layered_layout_with_config(
        diagram,
        &layered_config,
        |node| svg_node_dimensions(metrics, node, direction),
        |edge| {
            edge.label
                .as_ref()
                .map(|label| metrics.edge_label_dimensions(label))
        },
    );
    let sublayouts = compute_sublayouts(
        diagram,
        &layered_config,
        |node| svg_node_dimensions(metrics, node, direction),
        |edge| {
            edge.label
                .as_ref()
                .map(|label| metrics.edge_label_dimensions(label))
        },
        skip_non_isolated_overrides,
    );
    let title_pad_y = metrics.font_size;
    let content_pad_y = metrics.font_size * 0.3;
    reconcile_sublayouts(
        diagram,
        &mut layout,
        &sublayouts,
        title_pad_y,
        content_pad_y,
    );

    // Shift external predecessors of direction-override subgraphs to align with
    // the subgraph center.  Must happen after reconciliation (sublayout positions
    // finalized) but before overlap resolution and edge rerouting.
    center_override_subgraphs(diagram, &mut layout);

    // Expand parent subgraph bounds to encompass repositioned children.
    let child_margin = metrics.node_padding_x.max(metrics.node_padding_y);
    let title_margin = metrics.font_size;
    expand_parent_bounds(diagram, &mut layout, child_margin, title_margin);

    // Push external nodes that now overlap with reconciled subgraph bounds.
    // Account for post-padding expansion (2 * node_padding_y for adjacent
    // subgraphs) plus visual breathing room (font_size).
    let overlap_gap = metrics.node_padding_y * 2.0 + metrics.font_size;
    resolve_sublayout_overlaps(diagram, &mut layout, overlap_gap);

    // Align sibling nodes with their cross-boundary edge targets on the
    // cross-axis of the parent direction.  Must run after reconciliation
    // and overlap resolution but before edge rerouting.
    svg_router::align_cross_boundary_siblings(diagram, &mut layout);
    expand_parent_bounds(diagram, &mut layout, child_margin, title_margin);

    // Reroute edges affected by direction-override subgraphs.
    // This must happen after reconciliation moves nodes but before padding,
    // so routes use the reconciled node positions.
    let node_directions = svg_router::build_node_directions_svg(diagram);

    // Push cross-boundary edge endpoints apart before rerouting so that the
    // fresh orthogonal paths have enough room for a visible edge stem.
    svg_router::ensure_cross_boundary_edge_spacing(
        diagram,
        &mut layout,
        &node_directions,
        config.rank_sep,
    );

    let (_stats, rerouted_edges) =
        svg_router::reroute_override_edges(diagram, &mut layout, &node_directions);

    // Add padding to subgraph bounds for breathing room around nodes.
    apply_subgraph_svg_padding(
        diagram,
        &mut layout,
        metrics.node_padding_x,
        metrics.node_padding_y,
    );

    // Push external nodes away from subgraph borders so that subgraph-as-node
    // edges have visible length comparable to normal edges.
    ensure_subgraph_edge_spacing(diagram, &mut layout, config.rank_sep);

    // Reroute subgraph-as-node edges with fresh orthogonal paths computed from
    // padded subgraph bounds.  Must run after padding so endpoints land on the
    // visible subgraph border.
    let sg_node_rerouted = svg_router::reroute_subgraph_node_edges(diagram, &mut layout);
    let mut rerouted_edges = rerouted_edges;
    rerouted_edges.extend(sg_node_rerouted);

    // Convert post-processed LayoutResult to engine-agnostic GraphGeometry.
    let has_enhancements = engine_flags
        .map(|f| f.greedy_switch || f.model_order_tiebreak || f.variable_rank_spacing)
        .unwrap_or(false);
    let mut geom = geometry::from_layered_layout(&layout, diagram);
    geom.enhanced_backward_routing = has_enhancements;
    if matches!(edge_routing, EdgeRouting::DirectRoute) {
        geom = inject_routed_paths(diagram, &geom, EdgeRouting::DirectRoute);
        // Direct mode should use standard endpoint adjustment behavior.
        rerouted_edges.clear();
    } else if matches!(edge_routing, EdgeRouting::PolylineRoute) {
        geom = inject_routed_paths(diagram, &geom, EdgeRouting::PolylineRoute);
    } else if matches!(edge_routing, EdgeRouting::OrthogonalRoute) {
        geom = inject_orthogonal_route_paths(diagram, &geom);
        rerouted_edges.extend(geom.edges.iter().map(|e| e.index));
    }
    geom.rerouted_edges = rerouted_edges;
    geom
}

/// Render SVG directly from precomputed graph geometry.
///
/// This is used by runtime-selected engines that already produce `GraphGeometry`.
pub fn render_svg_from_geometry(
    diagram: &Diagram,
    options: &RenderOptions,
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> String {
    // Merge mode-derived rerouted edges with any engine-provided rerouted edges
    // (e.g., direction-override subgraph edges set by build_svg_layout).
    let mut rerouted_edges = rerouted_edge_indexes_for_mode(geom, edge_routing);
    if !matches!(edge_routing, EdgeRouting::DirectRoute) {
        rerouted_edges.extend(geom.rerouted_edges.iter().copied());
    }
    let override_nodes = svg_router::build_override_node_map(diagram);
    render_svg_with_geometry_context(
        diagram,
        options,
        geom,
        &rerouted_edges,
        &override_nodes,
        edge_routing,
    )
}

fn rerouted_edge_indexes_for_mode(
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> HashSet<usize> {
    match edge_routing {
        // Pass-through paths are already positioned by the layout engine
        // and should not receive extra shape clipping.
        EdgeRouting::EngineProvided => geom.edges.iter().map(|e| e.index).collect(),
        // Orthgonal routes already encode endpoint intent and should not
        // be shape-adjusted again in SVG (all path styles).
        EdgeRouting::OrthogonalRoute => geom.edges.iter().map(|e| e.index).collect(),
        // Direct and polyline routes need normal endpoint adjustment.
        EdgeRouting::DirectRoute => HashSet::new(),
        EdgeRouting::PolylineRoute => HashSet::new(),
    }
}

fn inject_routed_paths(
    diagram: &Diagram,
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> GraphGeometry {
    let routed =
        crate::diagrams::flowchart::routing::route_graph_geometry(diagram, geom, edge_routing);
    let mut updated = geom.clone();
    for edge in routed.edges {
        if let Some(layout_edge) = updated.edges.iter_mut().find(|e| e.index == edge.index) {
            layout_edge.layout_path_hint = Some(edge.path);
            layout_edge.label_position = edge.label_position;
        }
    }
    updated
}

fn inject_orthogonal_route_paths(diagram: &Diagram, geom: &GraphGeometry) -> GraphGeometry {
    let routed = route_edges_orthogonal(diagram, geom, OrthogonalRoutingOptions::preview());
    let mut updated = geom.clone();
    for edge in routed {
        if let Some(layout_edge) = updated.edges.iter_mut().find(|e| e.index == edge.index) {
            layout_edge.layout_path_hint = Some(edge.path);
            layout_edge.label_position = edge.label_position;
        }
    }
    updated
}

fn render_svg_with_geometry_context(
    diagram: &Diagram,
    options: &RenderOptions,
    geom: &GraphGeometry,
    rerouted_edges: &HashSet<usize>,
    override_nodes: &HashMap<String, String>,
    edge_routing: EdgeRouting,
) -> String {
    let svg_options = &options.svg;
    let scale = svg_options.scale;
    let metrics = SvgTextMetrics::new(
        svg_options.font_size,
        svg_options.node_padding_x,
        svg_options.node_padding_y,
    );

    let self_edge_paths = compute_self_edge_paths(diagram, geom, &metrics);
    let prepared_edges = prepare_rendered_edge_paths(
        diagram,
        geom,
        override_nodes,
        &self_edge_paths,
        rerouted_edges,
        edge_routing,
        svg_options.curve,
        svg_options.edge_radius,
        options.path_simplification,
    );
    let bounds = compute_svg_bounds(
        diagram,
        geom,
        &metrics,
        &self_edge_paths,
        &prepared_edges.paths,
    );
    let padding = svg_options.diagram_padding;
    let (min_x, min_y, max_x, max_y) = bounds.finalize(geom.bounds.width, geom.bounds.height);
    let width = (max_x - min_x + padding * 2.0) * scale;
    let height = (max_y - min_y + padding * 2.0) * scale;
    let offset_x = (-min_x + padding) * scale;
    let offset_y = (-min_y + padding) * scale;

    let mut writer = SvgWriter::new();
    writer.start_svg(
        width,
        height,
        &svg_options.font_family,
        svg_options.font_size * scale,
    );

    render_defs(&mut writer, scale);
    writer.start_group_transform(offset_x, offset_y);
    render_subgraphs(&mut writer, diagram, geom, &metrics, scale);
    // Render nodes before edges so arrowhead markers draw on top of node fills,
    // preventing the white node background from hiding arrowheads.
    render_nodes(&mut writer, diagram, geom, &metrics, scale);
    render_edges(
        &mut writer,
        diagram,
        &prepared_edges,
        svg_options.curve,
        svg_options.edge_radius,
        scale,
    );
    render_edge_labels(
        &mut writer,
        diagram,
        geom,
        &self_edge_paths,
        &prepared_edges.paths,
        override_nodes,
        &metrics,
        scale,
    );
    writer.end_group();

    writer.end_svg();
    writer.finish()
}

fn apply_subgraph_svg_padding(
    diagram: &Diagram,
    layout: &mut LayoutResult,
    pad_x: f64,
    pad_y: f64,
) {
    if pad_x <= 0.0 && pad_y <= 0.0 {
        return;
    }

    for (id, rect) in layout.subgraph_bounds.iter_mut() {
        rect.x -= pad_x;
        rect.y -= pad_y;
        rect.width = (rect.width + pad_x * 2.0).max(0.0);
        rect.height = (rect.height + pad_y * 2.0).max(0.0);

        if let Some(node_rect) = layout.nodes.get_mut(&crate::layered::NodeId(id.clone())) {
            *node_rect = *rect;
        }
    }

    // Ensure all subgraph IDs exist in nodes map for bounds updates.
    for (id, rect) in layout.subgraph_bounds.iter() {
        if !layout
            .nodes
            .contains_key(&crate::layered::NodeId(id.clone()))
            && diagram.subgraphs.contains_key(id)
        {
            layout
                .nodes
                .insert(crate::layered::NodeId(id.clone()), *rect);
        }
    }
}

/// Push external nodes away from subgraph borders for subgraph-as-node edges.
///
/// After `apply_subgraph_svg_padding` expands subgraph bounds, the gap between
/// external nodes and the visible subgraph border can be much smaller than a
/// normal inter-rank edge.  This function ensures those gaps are at least
/// `min_gap`, matching the visual weight of normal edges.
fn ensure_subgraph_edge_spacing(diagram: &Diagram, layout: &mut LayoutResult, min_gap: f64) {
    for edge in &diagram.edges {
        if edge.stroke == Stroke::Invisible {
            continue;
        }

        // external node → subgraph
        if let Some(sg_id) = &edge.to_subgraph
            && edge.from_subgraph.is_none()
        {
            push_node_from_subgraph(layout, &edge.from, sg_id, diagram.direction, min_gap, true);
        }

        // subgraph → external node
        if let Some(sg_id) = &edge.from_subgraph
            && edge.to_subgraph.is_none()
        {
            push_node_from_subgraph(layout, &edge.to, sg_id, diagram.direction, min_gap, false);
        }

        // subgraph → subgraph
        if let (Some(from_sg), Some(to_sg)) = (&edge.from_subgraph, &edge.to_subgraph) {
            push_subgraph_from_subgraph(
                diagram,
                layout,
                from_sg,
                to_sg,
                diagram.direction,
                min_gap,
            );
        }
    }
}

/// Push a single node away from a subgraph border if the gap is below `min_gap`.
///
/// `node_is_upstream` is true when the node is the source (exits toward the
/// subgraph) and false when it is the target (the subgraph exits toward it).
fn push_node_from_subgraph(
    layout: &mut LayoutResult,
    node_id: &str,
    sg_id: &str,
    direction: Direction,
    min_gap: f64,
    node_is_upstream: bool,
) {
    let node_key = crate::layered::NodeId(node_id.to_string());
    let sg_rect = match layout.subgraph_bounds.get(sg_id) {
        Some(r) => *r,
        None => return,
    };
    let node_rect = match layout.nodes.get(&node_key) {
        Some(r) => *r,
        None => return,
    };

    // Compute the gap between the node face and the subgraph face along the
    // flow axis.  "upstream trailing edge → downstream leading edge".
    let gap = if node_is_upstream {
        // node (source) → subgraph (target)
        match direction {
            Direction::TopDown => sg_rect.y - (node_rect.y + node_rect.height),
            Direction::BottomTop => node_rect.y - (sg_rect.y + sg_rect.height),
            Direction::LeftRight => sg_rect.x - (node_rect.x + node_rect.width),
            Direction::RightLeft => node_rect.x - (sg_rect.x + sg_rect.width),
        }
    } else {
        // subgraph (source) → node (target)
        match direction {
            Direction::TopDown => node_rect.y - (sg_rect.y + sg_rect.height),
            Direction::BottomTop => sg_rect.y - (node_rect.y + node_rect.height),
            Direction::LeftRight => node_rect.x - (sg_rect.x + sg_rect.width),
            Direction::RightLeft => sg_rect.x - (node_rect.x + node_rect.width),
        }
    };

    if gap >= min_gap {
        return;
    }

    let shift = min_gap - gap;
    let node_rect = layout.nodes.get_mut(&node_key).unwrap();

    // Push the node away from the subgraph (against flow for upstream,
    // with flow for downstream).
    if node_is_upstream {
        match direction {
            Direction::TopDown => node_rect.y -= shift,
            Direction::BottomTop => node_rect.y += shift,
            Direction::LeftRight => node_rect.x -= shift,
            Direction::RightLeft => node_rect.x += shift,
        }
    } else {
        match direction {
            Direction::TopDown => node_rect.y += shift,
            Direction::BottomTop => node_rect.y -= shift,
            Direction::LeftRight => node_rect.x += shift,
            Direction::RightLeft => node_rect.x -= shift,
        }
    }
}

/// Push the downstream subgraph (and all its member nodes) away from the
/// upstream subgraph so the visible gap between their borders is at least
/// `min_gap`.
fn push_subgraph_from_subgraph(
    diagram: &Diagram,
    layout: &mut LayoutResult,
    from_sg: &str,
    to_sg: &str,
    direction: Direction,
    min_gap: f64,
) {
    let from_rect = match layout.subgraph_bounds.get(from_sg) {
        Some(r) => *r,
        None => return,
    };
    let to_rect = match layout.subgraph_bounds.get(to_sg) {
        Some(r) => *r,
        None => return,
    };

    let gap = match direction {
        Direction::TopDown => to_rect.y - (from_rect.y + from_rect.height),
        Direction::BottomTop => from_rect.y - (to_rect.y + to_rect.height),
        Direction::LeftRight => to_rect.x - (from_rect.x + from_rect.width),
        Direction::RightLeft => from_rect.x - (to_rect.x + to_rect.width),
    };

    if gap >= min_gap {
        return;
    }

    let shift = min_gap - gap;

    // Collect all node IDs in the downstream subgraph (including nested).
    let mut member_nodes = Vec::new();
    let mut sg_stack = vec![to_sg.to_string()];
    while let Some(sg_id) = sg_stack.pop() {
        if let Some(sg) = diagram.subgraphs.get(&sg_id) {
            for node_id in &sg.nodes {
                if diagram.is_subgraph(node_id) {
                    sg_stack.push(node_id.clone());
                } else {
                    member_nodes.push(node_id.clone());
                }
            }
        }
    }

    // Shift each member node.
    for node_id in &member_nodes {
        let key = crate::layered::NodeId(node_id.clone());
        if let Some(rect) = layout.nodes.get_mut(&key) {
            match direction {
                Direction::TopDown => rect.y += shift,
                Direction::BottomTop => rect.y -= shift,
                Direction::LeftRight => rect.x += shift,
                Direction::RightLeft => rect.x -= shift,
            }
        }
    }

    // Shift the downstream subgraph bounds (and any nested subgraph bounds).
    let mut bounds_to_shift = vec![to_sg.to_string()];
    let mut i = 0;
    while i < bounds_to_shift.len() {
        let children = diagram.subgraph_children(&bounds_to_shift[i]);
        for child in children {
            bounds_to_shift.push(child.clone());
        }
        i += 1;
    }
    for sg_id in &bounds_to_shift {
        if let Some(rect) = layout.subgraph_bounds.get_mut(sg_id.as_str()) {
            match direction {
                Direction::TopDown => rect.y += shift,
                Direction::BottomTop => rect.y -= shift,
                Direction::LeftRight => rect.x += shift,
                Direction::RightLeft => rect.x -= shift,
            }
        }
        // Also update the nodes map entry for the subgraph.
        let key = crate::layered::NodeId(sg_id.clone());
        if let Some(rect) = layout.nodes.get_mut(&key) {
            match direction {
                Direction::TopDown => rect.y += shift,
                Direction::BottomTop => rect.y -= shift,
                Direction::LeftRight => rect.x += shift,
                Direction::RightLeft => rect.x -= shift,
            }
        }
    }
}

pub(crate) fn svg_node_dimensions(
    metrics: &SvgTextMetrics,
    node: &Node,
    direction: Direction,
) -> (f64, f64) {
    let (label_w, label_h) = metrics.measure_text_with_padding(&node.label, 0.0, 0.0);

    let (mut width, mut height) = match node.shape {
        Shape::Rectangle => (
            label_w + metrics.node_padding_x * 4.0,
            label_h + metrics.node_padding_y * 2.0,
        ),
        Shape::Diamond => {
            let w = label_w + metrics.node_padding_x;
            let h = label_h + metrics.node_padding_y;
            let size = w + h;
            (size, size)
        }
        Shape::Stadium => {
            // Stadium ends are semicircles that eat into the content area.
            // Add extra width so the text has room between the rounded ends.
            let h = label_h + metrics.node_padding_y * 2.0;
            let radius = h / 2.0;
            (label_w + metrics.node_padding_x * 2.0 + radius, h)
        }
        Shape::Cylinder => {
            // Cylinder needs extra height for the top and bottom ellipse caps.
            let w = label_w + metrics.node_padding_x * 2.0;
            let rx = w / 2.0;
            let ry = rx / (2.5 + w / 50.0);
            (w, label_h + metrics.node_padding_y * 2.0 + ry)
        }
        Shape::Document => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w, h + h / 8.0) // wave amplitude = content_h / 8
        }
        Shape::Documents => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            let offset = 5.0;
            (w + 2.0 * offset, h + h / 4.0 + 2.0 * offset) // wave amp = h/4
        }
        Shape::TaggedDocument => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 3.0; // extra padding for taller shape
            (w * 1.1, h + h / 4.0) // 10% wider, wave amp = h/4
        }
        Shape::Card => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + 12.0, h) // extra width for corner fold
        }
        Shape::TaggedRect => {
            let w = label_w + metrics.node_padding_x * 2.0;
            let h = label_h + metrics.node_padding_y * 2.0;
            (w + 0.2 * h, h) // extra width for tag triangle
        }
        Shape::SmallCircle => {
            // UML initial node: fixed small size (Mermaid uses radius=7)
            (14.0, 14.0)
        }
        Shape::FramedCircle => {
            // UML activity final node: medium fixed size
            (28.0, 28.0)
        }
        Shape::CrossedCircle => {
            // UML flow final node: larger fixed size (Mermaid uses radius=30)
            (60.0, 60.0)
        }
        _ => (
            label_w + metrics.node_padding_x * 2.0,
            label_h + metrics.node_padding_y * 2.0,
        ),
    };

    match node.shape {
        Shape::Hexagon | Shape::Trapezoid | Shape::InvTrapezoid | Shape::Asymmetric => {
            width *= 1.15;
        }
        Shape::Circle
        | Shape::DoubleCircle
        | Shape::SmallCircle
        | Shape::FramedCircle
        | Shape::CrossedCircle => {
            let size = width.max(height);
            width = size;
            height = size;
        }
        _ => {}
    }

    // ForkJoin bars are perpendicular to flow: swap dimensions for LR/RL.
    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        std::mem::swap(&mut width, &mut height);
    }

    (width, height)
}

fn render_defs(writer: &mut SvgWriter, scale: f64) {
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

    // Circle marker (filled circle)
    let circle_size = 10.0;
    let circle_marker_size = 11.0 * scale;
    let marker = format!(
        "<marker id=\"circlehead\" viewBox=\"0 0 {size} {size}\" refX=\"{ref_x}\" refY=\"{ref_y}\" markerWidth=\"{mw}\" markerHeight=\"{mh}\" orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\">",
        size = fmt_f64(circle_size),
        ref_x = fmt_f64(11.0),
        ref_y = fmt_f64(5.0),
        mw = fmt_f64(circle_marker_size),
        mh = fmt_f64(circle_marker_size)
    );
    writer.start_tag(&marker);
    let circle = format!(
        "<circle cx=\"5\" cy=\"5\" r=\"5\" stroke=\"{color}\" stroke-width=\"1\" fill=\"{color}\" />",
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

    writer.end_tag("</defs>");
}

fn render_subgraphs(
    writer: &mut SvgWriter,
    diagram: &Diagram,
    geom: &GraphGeometry,
    metrics: &SvgTextMetrics,
    scale: f64,
) {
    if geom.subgraphs.is_empty() {
        return;
    }

    let mut subgraphs: Vec<_> = geom
        .subgraphs
        .iter()
        .filter_map(|(id, sg_geom)| diagram.subgraphs.get(id).map(|_| (id, sg_geom)))
        .collect();

    subgraphs.sort_by(|a, b| a.1.depth.cmp(&b.1.depth).then_with(|| a.0.cmp(b.0)));

    writer.start_group("clusters");
    for (_id, sg_geom) in subgraphs {
        let rect = scale_rect(&sg_geom.rect.into(), scale);
        let stroke_width = fmt_f64(1.0 * scale);
        let rect_line = format!(
            "<rect class=\"subgraph\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" />",
            x = fmt_f64(rect.x),
            y = fmt_f64(rect.y),
            w = fmt_f64(rect.width),
            h = fmt_f64(rect.height),
            stroke = SUBGRAPH_STROKE,
            stroke_width = stroke_width
        );
        writer.push_line(&rect_line);

        if !sg_geom.title.trim().is_empty() {
            let title_x = rect.x + rect.width / 2.0;
            let title_y = rect.y + metrics.font_size * 0.25;
            let text = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"hanging\" fill=\"{color}\">{label}</text>",
                x = fmt_f64(title_x),
                y = fmt_f64(title_y),
                color = TEXT_COLOR,
                label = escape_text(&sg_geom.title)
            );
            writer.push_line(&text);
        }
    }
    writer.end_group();
}

#[allow(clippy::too_many_arguments)]
struct PreparedRenderedEdges {
    paths: HashMap<usize, Vec<Point>>,
    basis_stem_edge_indexes: HashSet<usize>,
    compact_basis_stem_edge_indexes: HashSet<usize>,
}

#[allow(clippy::too_many_arguments)]
fn prepare_rendered_edge_paths(
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

    let mut edge_paths: Vec<(usize, Vec<Point>)> = geom
        .edges
        .iter()
        .map(|edge| {
            let points: Vec<Point> = edge
                .layout_path_hint
                .as_ref()
                .map(|ps| ps.iter().map(|p| (*p).into()).collect())
                .unwrap_or_default();
            (edge.index, points)
        })
        .collect();
    edge_paths.extend(geom.self_edges.iter().map(|se| {
        let points = self_edge_paths
            .get(&se.edge_index)
            .cloned()
            .unwrap_or_else(|| se.points.iter().map(|p| (*p).into()).collect());
        (se.edge_index, points)
    }));
    edge_paths.sort_by_key(|(index, _)| *index);

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
    for (index, points) in edge_paths {
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
                points = clip_points_to_rect_start(&points, &sg_geom.rect.into());
            }
            if let Some(sg_id) = edge.to_subgraph.as_ref()
                && let Some(sg_geom) = geom.subgraphs.get(sg_id)
            {
                points = clip_points_to_rect_end(&points, &sg_geom.rect.into());
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

        // Only densify corners for direct/orthogonal sharp paths. For engine-provided
        // polyline geometry, this synthetic densification introduces tiny visible jogs
        // on axis-to-diagonal turns (for example ampersand fan-in).
        if is_sharp
            && !preserve_orthogonal_endpoint_contract
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
                preserve_orthogonal: preserve_orthogonal_endpoint_contract,
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
            && !preserve_orthogonal_endpoint_contract
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

fn render_edges(
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
        clamped = clip_points_to_rect_start(&clamped, &sg_geom.rect.into());
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.from), geom.nodes.get(&edge.from))
    {
        let from_rect: Rect = node_geom.rect.into();
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
        clamped = clip_points_to_rect_end(&clamped, &sg_geom.rect.into());
    } else if let (Some(node), Some(node_geom)) =
        (diagram.nodes.get(&edge.to), geom.nodes.get(&edge.to))
    {
        let to_rect: Rect = node_geom.rect.into();
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
    let target_rect: Rect = target_geom.rect.into();
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

const LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE: f64 = 2.0;
const LABEL_POINT_EPS: f64 = 0.000_001;

fn revalidate_svg_label_anchor(candidate: Point, rendered_path: Option<&[Point]>) -> Point {
    let Some(path) = rendered_path else {
        return candidate;
    };
    if path.is_empty() {
        return candidate;
    }

    let drift = distance_point_to_svg_path(candidate, path);
    if drift <= LABEL_ANCHOR_REVALIDATION_MAX_DISTANCE {
        return candidate;
    }
    svg_path_midpoint(path).unwrap_or(candidate)
}

fn point_distance_svg(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn distance_point_to_svg_segment(point: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq <= LABEL_POINT_EPS {
        return point_distance_svg(point, a);
    }
    let projection = ((point.x - a.x) * dx + (point.y - a.y) * dy) / seg_len_sq;
    let t = projection.clamp(0.0, 1.0);
    let closest = Point {
        x: a.x + t * dx,
        y: a.y + t * dy,
    };
    point_distance_svg(point, closest)
}

fn distance_point_to_svg_path(point: Point, path: &[Point]) -> f64 {
    if path.is_empty() {
        return f64::INFINITY;
    }
    if path.len() == 1 {
        return point_distance_svg(point, path[0]);
    }
    path.windows(2)
        .map(|segment| distance_point_to_svg_segment(point, segment[0], segment[1]))
        .fold(f64::INFINITY, f64::min)
}

fn svg_path_midpoint(path: &[Point]) -> Option<Point> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return path.first().copied();
    }
    let total_len: f64 = path
        .windows(2)
        .map(|segment| point_distance_svg(segment[0], segment[1]))
        .sum();
    if total_len <= LABEL_POINT_EPS {
        return path.get(path.len() / 2).copied();
    }

    let target = total_len / 2.0;
    let mut traversed = 0.0;
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let seg_len = point_distance_svg(a, b);
        if seg_len <= LABEL_POINT_EPS {
            continue;
        }
        if traversed + seg_len >= target {
            let t = (target - traversed) / seg_len;
            return Some(Point {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
            });
        }
        traversed += seg_len;
    }
    path.last().copied()
}

#[allow(clippy::too_many_arguments)]
fn render_edge_labels(
    writer: &mut SvgWriter,
    diagram: &Diagram,
    geom: &GraphGeometry,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
    override_nodes: &HashMap<String, String>,
    metrics: &SvgTextMetrics,
    scale: f64,
) {
    // Pre-build label position lookup from GraphGeometry edges.
    let label_positions: HashMap<usize, Point> = geom
        .edges
        .iter()
        .filter_map(|e| e.label_position.map(|p| (e.index, p.into())))
        .collect();

    writer.start_group("edgeLabels");

    for edge in diagram.edges.iter() {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(label) = edge.label.as_ref() else {
            continue;
        };
        let edge_idx = edge.index;
        let cross_boundary = if edge.from_subgraph.is_none() && edge.to_subgraph.is_none() {
            let from_override = override_nodes.get(&edge.from);
            let to_override = override_nodes.get(&edge.to);
            matches!(
                (from_override, to_override),
                (Some(a), Some(b)) if a != b
            ) || matches!(
                (from_override, to_override),
                (Some(_), None) | (None, Some(_))
            )
        } else {
            false
        };
        let use_precomputed =
            edge.from_subgraph.is_none() && edge.to_subgraph.is_none() && !cross_boundary;
        let position = if use_precomputed {
            label_positions.get(&edge_idx).copied()
        } else {
            None
        }
        .or_else(|| fallback_label_position(geom, edge_idx, self_edge_paths, rendered_edge_paths))
        .map(|candidate| {
            revalidate_svg_label_anchor(
                candidate,
                rendered_edge_paths
                    .get(&edge_idx)
                    .map(|path| path.as_slice()),
            )
        });
        let Some(point) = position else {
            continue;
        };
        render_text_centered(
            writer,
            point.x * scale,
            point.y * scale,
            label,
            TEXT_COLOR,
            metrics,
            scale,
        );
    }

    // Render head/tail end labels from routed edge paths.
    for edge in diagram.edges.iter() {
        if edge.head_label.is_none() && edge.tail_label.is_none() {
            continue;
        }
        // Get the routed path for this edge from geometry.
        let path: Vec<FPoint> = geom
            .edges
            .iter()
            .find(|e| e.index == edge.index)
            .and_then(|e| e.layout_path_hint.clone())
            .unwrap_or_default();
        if path.len() < 2 {
            continue;
        }
        let (head_pos, tail_pos) =
            crate::diagrams::flowchart::routing::compute_end_label_positions(&path);
        if let (Some(label), Some(pos)) = (&edge.head_label, head_pos) {
            render_text_centered(
                writer,
                pos.x * scale,
                pos.y * scale,
                label,
                TEXT_COLOR,
                metrics,
                scale,
            );
        }
        if let (Some(label), Some(pos)) = (&edge.tail_label, tail_pos) {
            render_text_centered(
                writer,
                pos.x * scale,
                pos.y * scale,
                label,
                TEXT_COLOR,
                metrics,
                scale,
            );
        }
    }

    writer.end_group();
}

fn render_nodes(
    writer: &mut SvgWriter,
    diagram: &Diagram,
    geom: &GraphGeometry,
    metrics: &SvgTextMetrics,
    scale: f64,
) {
    writer.start_group("nodes");

    let mut node_ids: Vec<&String> = diagram.nodes.keys().collect();
    node_ids.sort();

    for node_id in node_ids {
        let node = &diagram.nodes[node_id];
        let Some(pos_node) = geom.nodes.get(node_id) else {
            continue;
        };
        let rect: Rect = pos_node.rect.into();
        let style = ResolvedSvgNodeStyle::from_node(node);
        render_node_shape(writer, node, &rect, scale, diagram.direction, style);

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
            &rect,
            style,
            metrics,
            scale,
        );
    }

    writer.end_group();
}

/// Render a node's label, converting `Node::SEPARATOR` lines into horizontal rules.
fn render_node_label(
    writer: &mut SvgWriter,
    center: Point,
    text: &str,
    rect: &Rect,
    style: ResolvedSvgNodeStyle<'_>,
    metrics: &SvgTextMetrics,
    scale: f64,
) {
    let lines: Vec<&str> = text.split('\n').collect();
    let has_separator = lines.contains(&Node::SEPARATOR);
    let stroke = style.stroke_or(STROKE_COLOR);
    let text_color = style.text_or(TEXT_COLOR);

    if !has_separator {
        render_text_centered(writer, center.x, center.y, text, text_color, metrics, scale);
        return;
    }

    let line_height = metrics.line_height * scale;
    let total_height = line_height * (lines.len().saturating_sub(1) as f64);
    let start_y = center.y - total_height / 2.0;
    let x1 = rect.x * scale;
    let x2 = (rect.x + rect.width) * scale;
    // Left-align x: node left edge + padding (matches text renderer's x+2 convention)
    let left_x = x1 + metrics.node_padding_x * scale;
    let mut past_separator = false;

    for (idx, line_text) in lines.iter().enumerate() {
        let line_y = start_y + line_height * idx as f64;
        if *line_text == Node::SEPARATOR {
            past_separator = true;
            let line = format!(
                "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" />",
                x1 = fmt_f64(x1),
                y = fmt_f64(line_y),
                x2 = fmt_f64(x2),
                stroke = stroke,
                sw = fmt_f64(1.0 * scale),
            );
            writer.push_line(&line);
        } else if past_separator {
            // Members: left-aligned
            let line = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"start\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
                x = fmt_f64(left_x),
                y = fmt_f64(line_y),
                color = text_color,
                text = escape_text(line_text)
            );
            writer.push_line(&line);
        } else {
            // Class name: centered
            let line = format!(
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
                x = fmt_f64(center.x),
                y = fmt_f64(line_y),
                color = text_color,
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
) {
    let rect = scale_rect(rect, scale);
    let stroke_width = fmt_f64(1.0 * scale);
    let fill = node_style.fill_or(NODE_FILL);
    let stroke = node_style.stroke_or(STROKE_COLOR);
    let style = format!(
        " fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" stroke-linejoin=\"round\"",
        fill = fill,
        stroke = stroke,
        stroke_width = stroke_width
    );

    match node.shape {
        Shape::Rectangle => {
            let line = format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{style} />",
                x = fmt_f64(rect.x),
                y = fmt_f64(rect.y),
                w = fmt_f64(rect.width),
                h = fmt_f64(rect.height),
                style = style
            );
            writer.push_line(&line);
        }
        Shape::Round => {
            let radius = 5.0 * scale;
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
            let frect = geometry::FRect::new(rect.x, rect.y, rect.width, rect.height);
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

fn render_text_centered(
    writer: &mut SvgWriter,
    x: f64,
    y: f64,
    text: &str,
    color: &str,
    metrics: &SvgTextMetrics,
    scale: f64,
) {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() == 1 {
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
            x = fmt_f64(x),
            y = fmt_f64(y),
            color = color,
            text = escape_text(text)
        );
        writer.push_line(&line);
        return;
    }

    let line_height = metrics.line_height * scale;
    let total_height = line_height * (lines.len().saturating_sub(1) as f64);
    let start_y = y - total_height / 2.0;

    for (idx, line_text) in lines.iter().enumerate() {
        let line_y = start_y + line_height * idx as f64;
        let line = format!(
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{color}\">{text}</text>",
            x = fmt_f64(x),
            y = fmt_f64(line_y),
            color = color,
            text = escape_text(line_text)
        );
        writer.push_line(&line);
    }
}

struct SvgBounds {
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

    fn finalize(&self, fallback_width: f64, fallback_height: f64) -> (f64, f64, f64, f64) {
        if !self.min_x.is_finite() || !self.min_y.is_finite() {
            return (0.0, 0.0, fallback_width, fallback_height);
        }
        (self.min_x, self.min_y, self.max_x, self.max_y)
    }
}

fn compute_svg_bounds(
    diagram: &Diagram,
    geom: &GraphGeometry,
    metrics: &SvgTextMetrics,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
) -> SvgBounds {
    let mut bounds = SvgBounds::new();

    for pos_node in geom.nodes.values() {
        bounds.update_rect(&pos_node.rect.into());
    }

    for sg_geom in geom.subgraphs.values() {
        bounds.update_rect(&sg_geom.rect.into());
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

    // Pre-build label position lookup from GraphGeometry edges.
    let label_positions: HashMap<usize, Point> = geom
        .edges
        .iter()
        .filter_map(|e| e.label_position.map(|p| (e.index, p.into())))
        .collect();

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
        let start: geometry::FPoint = points[0].into();
        let end: geometry::FPoint = points.last().copied().unwrap_or(points[0]).into();
        let waypoints: Vec<geometry::FPoint> = points
            .iter()
            .copied()
            .skip(1)
            .take(points.len().saturating_sub(2))
            .map(Into::into)
            .collect();
        build_orthogonal_path_float(start, end, direction, &waypoints)
            .into_iter()
            .map(Into::into)
            .collect()
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
            simplify_orthogonal_points(&points, direction)
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

fn simplify_orthogonal_points(points: &[Point], direction: Direction) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let start = points[0];
    let end = points[points.len() - 1];
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
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect.into();
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.from)?.rect.into();
        let node = diagram.nodes.get(&edge.from)?;
        (node_rect, node.shape)
    };

    let to = if let Some(sg_id) = edge.to_subgraph.as_ref() {
        let sg_rect: Rect = geom.subgraphs.get(sg_id)?.rect.into();
        (sg_rect, Shape::Rectangle)
    } else {
        let node_rect: Rect = geom.nodes.get(&edge.to)?.rect.into();
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
        // Save original endpoints before support extension so we can detect
        // when extension shifts the source/target off the node boundary.
        let original_start = points[0];
        let original_end = points[points.len() - 1];
        points = enforce_min_orthogonal_endpoint_support(
            &points,
            start_offset + min_endpoint_support,
            end_offset + min_endpoint_support,
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
        start_offset = start_offset.min((start_support - min_endpoint_support).max(0.0));
        end_offset = end_offset.min((end_support - min_endpoint_support).max(0.0));
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
    use crate::diagrams::flowchart::geometry::{FPoint, FRect};
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
fn document_svg_path(x: f64, y: f64, w: f64, h: f64, wave_amp: f64) -> String {
    let wave_y = y + h - wave_amp;
    let mut d = format!("M{},{}", fmt_f64(x), fmt_f64(wave_y));
    d.push_str(&sine_wave_segments(x, wave_y, w, wave_amp));
    let _ = write!(d, " L{},{}", fmt_f64(x + w), fmt_f64(y));
    let _ = write!(d, " L{},{}", fmt_f64(x), fmt_f64(y));
    d.push_str(" Z");
    d
}

fn polygon_points(points: &[(f64, f64)]) -> String {
    let mut out = String::new();
    for (idx, (x, y)) in points.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{x},{y}", x = fmt_f64(*x), y = fmt_f64(*y));
    }
    out
}

fn scale_rect(rect: &Rect, scale: f64) -> Rect {
    Rect {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}

fn compute_self_edge_paths(
    diagram: &Diagram,
    geom: &GraphGeometry,
    metrics: &SvgTextMetrics,
) -> HashMap<usize, Vec<Point>> {
    let pad = metrics.node_padding_x.max(metrics.node_padding_y).max(4.0);
    let mut paths = HashMap::new();

    for se in &geom.self_edges {
        let Some(pos_node) = geom.nodes.get(&se.node_id) else {
            continue;
        };
        if se.points.is_empty() {
            continue;
        }
        let layout_rect: Rect = pos_node.rect.into();
        let layout_points: Vec<Point> = se.points.iter().map(|p| (*p).into()).collect();
        let adjusted =
            adjust_self_edge_points(&layout_rect, &layout_points, diagram.direction, pad);
        paths.insert(se.edge_index, adjusted);
    }

    paths
}

fn adjust_self_edge_points(
    rect: &Rect,
    points: &[Point],
    direction: Direction,
    pad: f64,
) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    match direction {
        Direction::TopDown => {
            let loop_x = points
                .iter()
                .map(|point| point.x)
                .fold(right, f64::max)
                .max(right + pad);
            vec![
                Point { x: right, y: top },
                Point { x: loop_x, y: top },
                Point {
                    x: loop_x,
                    y: bottom,
                },
                Point {
                    x: right,
                    y: bottom,
                },
            ]
        }
        Direction::BottomTop => {
            let loop_x = points
                .iter()
                .map(|point| point.x)
                .fold(right, f64::max)
                .max(right + pad);
            vec![
                Point {
                    x: right,
                    y: bottom,
                },
                Point {
                    x: loop_x,
                    y: bottom,
                },
                Point { x: loop_x, y: top },
                Point { x: right, y: top },
            ]
        }
        Direction::LeftRight => {
            let loop_y = points
                .iter()
                .map(|point| point.y)
                .fold(bottom, f64::max)
                .max(bottom + pad);
            vec![
                Point {
                    x: right,
                    y: bottom,
                },
                Point {
                    x: right,
                    y: loop_y,
                },
                Point { x: left, y: loop_y },
                Point { x: left, y: bottom },
            ]
        }
        Direction::RightLeft => {
            let loop_y = points
                .iter()
                .map(|point| point.y)
                .fold(bottom, f64::max)
                .max(bottom + pad);
            vec![
                Point { x: left, y: bottom },
                Point { x: left, y: loop_y },
                Point {
                    x: right,
                    y: loop_y,
                },
                Point {
                    x: right,
                    y: bottom,
                },
            ]
        }
    }
}

fn fallback_label_position(
    geom: &GraphGeometry,
    edge_index: usize,
    self_edge_paths: &HashMap<usize, Vec<Point>>,
    rendered_edge_paths: &HashMap<usize, Vec<Point>>,
) -> Option<Point> {
    if let Some(points) = self_edge_paths.get(&edge_index) {
        return svg_path_midpoint(points).or_else(|| points.get(points.len() / 2).copied());
    }

    // Try regular edges via layout_path_hint
    if let Some(layout_edge) = geom.edges.iter().find(|e| e.index == edge_index)
        && let Some(path) = &layout_edge.layout_path_hint
    {
        return path.get(path.len() / 2).map(|p| (*p).into());
    }

    // Try self-edges
    if let Some(se) = geom.self_edges.iter().find(|e| e.edge_index == edge_index) {
        return se.points.get(se.points.len() / 2).map(|p| (*p).into());
    }

    if let Some(points) = rendered_edge_paths.get(&edge_index) {
        return svg_path_midpoint(points).or_else(|| points.get(points.len() / 2).copied());
    }

    None
}

fn fmt_f64(value: f64) -> String {
    let mut v = value;
    if v.abs() < 0.005 {
        v = 0.0;
    }
    format!("{:.2}", v)
}

fn escape_text(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

struct SvgWriter {
    buf: String,
    indent: usize,
}

impl SvgWriter {
    fn new() -> Self {
        Self {
            buf: String::new(),
            indent: 0,
        }
    }

    fn start_svg(&mut self, width: f64, height: f64, font_family: &str, font_size: f64) {
        let view_width = fmt_f64(width);
        let view_height = fmt_f64(height);
        let view_box = format!("0 0 {view_width} {view_height}");
        let style = format!("max-width: {view_width}px; background-color: transparent;");
        let line = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" viewBox=\"{view_box}\" style=\"{style}\" font-family=\"{font}\" font-size=\"{font_size}\">",
            view_box = view_box,
            style = style,
            font = escape_text(font_family),
            font_size = fmt_f64(font_size)
        );
        self.push_line(&line);
        self.indent += 1;
    }

    fn end_svg(&mut self) {
        self.indent = self.indent.saturating_sub(1);
        self.push_line("</svg>");
    }

    fn start_tag(&mut self, line: &str) {
        self.push_line(line);
        self.indent += 1;
    }

    fn end_tag(&mut self, line: &str) {
        self.indent = self.indent.saturating_sub(1);
        self.push_line(line);
    }

    fn start_group(&mut self, class_name: &str) {
        let line = format!("<g class=\"{class}\">", class = escape_text(class_name));
        self.start_tag(&line);
    }

    fn start_group_transform(&mut self, dx: f64, dy: f64) {
        let line = format!(
            "<g transform=\"translate({x},{y})\">",
            x = fmt_f64(dx),
            y = fmt_f64(dy)
        );
        self.start_tag(&line);
    }

    fn end_group(&mut self) {
        self.end_tag("</g>");
    }

    fn push_line(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.buf.push_str("  ");
        }
        self.buf.push_str(line);
        self.buf.push('\n');
    }

    fn finish(self) -> String {
        self.buf
    }
}
