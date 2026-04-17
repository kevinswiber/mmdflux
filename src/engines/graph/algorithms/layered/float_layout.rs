//! Engine-owned float-space layout construction for graph-family diagrams.

use super::kernel::{LayoutConfig, LayoutResult, NodeId};
use super::layout_building::{
    build_layered_layout_with_config, compute_sublayouts, layered_config_for_layout,
};
use super::layout_subgraph_ops::{
    center_override_subgraphs, expand_parent_bounds, rearrange_concurrent_regions,
    reconcile_sublayouts, resolve_sublayout_overlaps,
};
use super::{float_router, from_layered_layout};
use crate::graph::direction_policy::build_node_directions;
use crate::graph::geometry::{GraphGeometry, RoutedEdgeGeometry};
use crate::graph::grid::GridLayoutConfig;
use crate::graph::measure::{ProportionalTextMetrics, proportional_node_dimensions};
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{Direction, Edge, Graph, Stroke};

/// Edge-label sizing that honours the pre-engine wrap artifact when present
/// (plan 0147 Task 1.6). Falls back to single-line measurement otherwise.
fn edge_label_dims_proportional(
    metrics: &ProportionalTextMetrics,
    edge: &Edge,
) -> Option<(f64, f64)> {
    if let Some(lines) = edge.wrapped_label_lines.as_deref() {
        return Some(metrics.edge_label_dimensions_wrapped(lines));
    }
    edge.label
        .as_deref()
        .map(|label| metrics.edge_label_dimensions(label))
}

/// Stroke thickness in layout units used for ELK label-dummy padding.
/// Matches the `info.thickness` assignment at `layout_building.rs:355` so
/// layout sizing and `EdgeLabelInfo` stay consistent.
pub(super) fn edge_thickness(edge: &Edge) -> f64 {
    match edge.stroke {
        Stroke::Thick => 3.0,
        _ => 1.0,
    }
}

/// Apply ELK `LabelDummyInserter` padding — inflate the rank-axis extent
/// of the label dummy by `edge_label_spacing + thickness`. Plan 0147
/// Task 2.5 / ELK `LabelDummyInserter.java:80-110`. Rank axis is vertical
/// for TB/BT and horizontal for LR/RL.
pub(super) fn pad_edge_label_dims(
    dims: (f64, f64),
    spacing: f64,
    thickness: f64,
    direction: Direction,
) -> (f64, f64) {
    let pad = spacing + thickness;
    if matches!(direction, Direction::TopDown | Direction::BottomTop) {
        (dims.0, dims.1 + pad)
    } else {
        (dims.0 + pad, dims.1)
    }
}

/// Grid-mode variant of [`pad_edge_label_dims`] (#238 / Plan 0148).
///
/// Grid-mode cell-valued dims are fed to the layered solver as plain
/// `f64`s alongside pixel-valued separations (`rank_sep = 50` etc.),
/// and the solver's output is scaled by `compute_grid_scale_factors`
/// (~0.075 for a typical TD fixture) before being rendered. So a pad
/// expressed in the same float units as `pad_edge_label_dims` widens
/// the rendered gap by `round(pad * scale)` cells.
///
/// The default spacing (`2.0`) + default thickness (`1.0`) = 3.0 is
/// subtracted so the default config contributes 0 padding and existing
/// Text snapshots stay byte-identical. Above-default spacings widen
/// the label-dummy rank-axis extent by `(spacing + thickness - 3.0)`
/// units, producing additional row/column cells between labelled ranks
/// once the delta crosses the scale threshold.
pub(super) fn pad_edge_label_dims_grid(
    dims: (f64, f64),
    spacing: f64,
    thickness: f64,
    direction: Direction,
) -> (f64, f64) {
    const DEFAULT_BASELINE_PX: f64 = 3.0;
    let pad = (spacing + thickness - DEFAULT_BASELINE_PX).max(0.0);
    if matches!(direction, Direction::TopDown | Direction::BottomTop) {
        (dims.0, dims.1 + pad)
    } else {
        (dims.0 + pad, dims.1)
    }
}

pub(crate) fn build_float_layout_with_flags(
    diagram: &Graph,
    config: &GridLayoutConfig,
    metrics: &ProportionalTextMetrics,
    edge_routing: EdgeRouting,
    skip_non_isolated_overrides: bool,
    engine_flags: Option<&LayoutConfig>,
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
        layered_config.edge_label_spacing = flags.edge_label_spacing;
        layered_config.label_side_selection = flags.label_side_selection;
        layered_config.label_side_strategy = flags.label_side_strategy;
        layered_config.label_dummy_placement = flags.label_dummy_placement;
        layered_config.label_dummy_routing = flags.label_dummy_routing;
        layered_config.backward_edge_side_grouping = flags.backward_edge_side_grouping;
    }
    let edge_label_spacing = layered_config.edge_label_spacing;
    let mut layout = build_layered_layout_with_config(
        diagram,
        &layered_config,
        |node| proportional_node_dimensions(metrics, node, direction),
        |edge| {
            edge_label_dims_proportional(metrics, edge).map(|dims| {
                pad_edge_label_dims(dims, edge_label_spacing, edge_thickness(edge), direction)
            })
        },
    );
    let sublayouts = compute_sublayouts(
        diagram,
        &layered_config,
        |node| proportional_node_dimensions(metrics, node, direction),
        |edge| {
            edge_label_dims_proportional(metrics, edge).map(|dims| {
                pad_edge_label_dims(dims, edge_label_spacing, edge_thickness(edge), direction)
            })
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
    float_router::align_cross_boundary_siblings(diagram, &mut layout);
    expand_parent_bounds(diagram, &mut layout, child_margin, title_margin);

    // Shift external predecessors/successors of subgraph-as-node and
    // direction-override subgraphs to align with the subgraph center.
    // Runs after all bound expansions so the centering uses final bounds.
    center_override_subgraphs(diagram, &mut layout);

    // Rearrange concurrent region subgraphs from vertical stacking (produced
    // by compound layout with invisible inter-region edges) to horizontal
    // side-by-side arrangement (UML convention).
    rearrange_concurrent_regions(diagram, &mut layout, config.node_sep);

    // Reroute edges affected by direction-override subgraphs.
    // This must happen after reconciliation moves nodes but before padding,
    // so routes use the reconciled node positions.
    let node_directions = build_node_directions(diagram);

    // Push cross-boundary edge endpoints apart before rerouting so that the
    // fresh orthogonal paths have enough room for a visible edge stem.
    float_router::ensure_cross_boundary_edge_spacing(
        diagram,
        &mut layout,
        &node_directions,
        config.rank_sep,
    );

    let (_stats, rerouted_edges) =
        float_router::reroute_override_edges(diagram, &mut layout, &node_directions);

    // Add padding to subgraph bounds for breathing room around nodes.
    apply_subgraph_float_padding(
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
    let sg_node_rerouted = float_router::reroute_subgraph_node_edges(diagram, &mut layout);
    let mut rerouted_edges = rerouted_edges;
    rerouted_edges.extend(sg_node_rerouted);

    // Convert post-processed LayoutResult to engine-agnostic GraphGeometry.
    let has_enhancements = engine_flags
        .map(|f| f.greedy_switch || f.model_order_tiebreak || f.variable_rank_spacing)
        .unwrap_or(false);
    let mut geom = from_layered_layout(&layout, diagram);
    geom.enhanced_backward_routing = has_enhancements;
    match edge_routing {
        EdgeRouting::DirectRoute => {
            geom = inject_routed_paths(diagram, &geom, EdgeRouting::DirectRoute, metrics);
            // Direct mode should use standard endpoint adjustment behavior.
            rerouted_edges.clear();
        }
        EdgeRouting::PolylineRoute => {
            geom = inject_routed_paths(diagram, &geom, EdgeRouting::PolylineRoute, metrics);
        }
        EdgeRouting::OrthogonalRoute => {
            // Route through the full pipeline so the label-lane pass and
            // bounds extension run for orthogonal preset variants too
            // (step, smooth-step, curved-step). Without this, orthogonal
            // edges get routed but labels never get lane-shifted.
            geom = inject_routed_paths(diagram, &geom, EdgeRouting::OrthogonalRoute, metrics);
            rerouted_edges.extend(geom.edges.iter().map(|e| e.index));
        }
        EdgeRouting::EngineProvided => {}
    }
    geom.rerouted_edges = rerouted_edges;
    geom
}

fn inject_routed_paths(
    diagram: &Graph,
    geom: &GraphGeometry,
    edge_routing: EdgeRouting,
    metrics: &ProportionalTextMetrics,
) -> GraphGeometry {
    let routed = route_graph_geometry(diagram, geom, edge_routing, metrics);
    let mut updated = geom.clone();
    apply_routed_edge_paths(&mut updated, routed.edges);
    updated
}

/// **Load-bearing downgrade.** Copies routed-stage geometry back onto `LayoutEdge`
/// so `Visual` SVG solve paths (where the engine returns `routed: None`) still see
/// authoritative paths and label rectangles. Changes to `RoutedEdgeGeometry` fields
/// that SVG / MMDS / bounds consume MUST be reflected here. See plan 0145
/// `architecture/design.md` §2.2 for the data-flow contract.
fn apply_routed_edge_paths(
    updated: &mut GraphGeometry,
    routed_edges: impl IntoIterator<Item = RoutedEdgeGeometry>,
) {
    for edge in routed_edges {
        if let Some(layout_edge) = updated.edges.iter_mut().find(|e| e.index == edge.index) {
            layout_edge.layout_path_hint = Some(edge.path);
            layout_edge.label_position = edge.label_position;
            layout_edge.preserve_orthogonal_topology = edge.preserve_orthogonal_topology;
            layout_edge.label_geometry = edge.label_geometry;
        }
    }
}

fn apply_subgraph_float_padding(
    diagram: &Graph,
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

        if let Some(node_rect) = layout.nodes.get_mut(&NodeId(id.clone())) {
            *node_rect = *rect;
        }
    }

    // Ensure all subgraph IDs exist in nodes map for bounds updates.
    for (id, rect) in layout.subgraph_bounds.iter() {
        if !layout.nodes.contains_key(&NodeId(id.clone())) && diagram.subgraphs.contains_key(id) {
            layout.nodes.insert(NodeId(id.clone()), *rect);
        }
    }
}

/// Push external nodes away from subgraph borders for subgraph-as-node edges.
///
/// After `apply_subgraph_float_padding` expands subgraph bounds, the gap between
/// external nodes and the visible subgraph border can be much smaller than a
/// normal inter-rank edge.  This function ensures those gaps are at least
/// `min_gap`, matching the visual weight of normal edges.
fn ensure_subgraph_edge_spacing(diagram: &Graph, layout: &mut LayoutResult, min_gap: f64) {
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
    let node_key = NodeId(node_id.to_string());
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
    diagram: &Graph,
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
        let key = NodeId(node_id.clone());
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
        let key = NodeId(sg_id.clone());
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

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::graph::geometry::{EdgeLabelGeometry, EdgeLabelSide, LayoutEdge};
    use crate::graph::space::{FPoint, FRect};

    #[test]
    fn apply_routed_edge_paths_propagates_label_geometry_to_layout_edge() {
        let label_geom = EdgeLabelGeometry {
            center: FPoint::new(50.0, 60.0),
            rect: FRect::new(40.0, 55.0, 20.0, 10.0),
            padding: (4.0, 2.0),
            side: EdgeLabelSide::Above,
            track: 0,
        };

        let mut geometry = GraphGeometry {
            nodes: HashMap::new(),
            edges: vec![LayoutEdge {
                index: 0,
                from: "A".into(),
                to: "B".into(),
                waypoints: vec![],
                label_position: None,
                label_side: None,
                from_subgraph: None,
                to_subgraph: None,
                layout_path_hint: None,
                preserve_orthogonal_topology: false,
                label_geometry: None,
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

        let routed_edges = vec![RoutedEdgeGeometry {
            index: 0,
            from: "A".into(),
            to: "B".into(),
            path: vec![FPoint::new(0.0, 0.0), FPoint::new(100.0, 100.0)],
            label_position: Some(FPoint::new(50.0, 50.0)),
            label_side: None,
            head_label_position: None,
            tail_label_position: None,
            is_backward: false,
            from_subgraph: None,
            to_subgraph: None,
            source_port: None,
            target_port: None,
            preserve_orthogonal_topology: false,
            label_geometry: Some(label_geom),
        }];

        apply_routed_edge_paths(&mut geometry, routed_edges);

        assert_eq!(geometry.edges[0].label_geometry, Some(label_geom));
    }
}
