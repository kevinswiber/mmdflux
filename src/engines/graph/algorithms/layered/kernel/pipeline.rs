//! Layered engine orchestration pipeline.

#![allow(dead_code)]

use std::collections::HashMap;

use super::debug::{debug_dump_layout_result, debug_dump_pipeline, skip_title_nodes};
use super::graph::LayoutGraph;
use super::support::{
    assign_node_intersects, compute_rank_sep_overrides, extract_self_edges,
    insert_self_edge_dummies, label_dummy_gap_ranks, make_space_for_edge_labels,
    make_space_for_labeled_edges, position_self_edges, select_label_sides, switch_label_dummies,
    translate_layout_result,
};
use super::types::EdgeLabelInfo;
use super::{
    DiGraph, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Rect,
    acyclic, border, nesting, normalize, order, parent_dummy_chains, position, rank,
};

/// Main entry point for layout computation.
///
/// Takes a directed graph, configuration options, and a function to get node
/// dimensions. Returns a `LayoutResult` with positioned nodes and edge paths.
pub fn run_layered_layout<N, F>(
    graph: &DiGraph<N>,
    config: &LayoutConfig,
    get_dimensions: F,
) -> LayoutResult
where
    F: Fn(&NodeId, &N) -> (f64, f64),
{
    layout(graph, config, get_dimensions)
}

pub fn layout<N, F>(graph: &DiGraph<N>, config: &LayoutConfig, get_dimensions: F) -> LayoutResult
where
    F: Fn(&NodeId, &N) -> (f64, f64),
{
    layout_with_labels(graph, config, get_dimensions, &HashMap::new())
}

/// Layout computation with edge label support.
///
/// This variant allows specifying label dimensions for edges, which will be
/// used during normalization to create label dummies with appropriate sizes.
pub fn layout_with_labels<N, F>(
    graph: &DiGraph<N>,
    config: &LayoutConfig,
    get_dimensions: F,
    edge_labels: &HashMap<usize, EdgeLabelInfo>,
) -> LayoutResult
where
    F: Fn(&NodeId, &N) -> (f64, f64),
{
    // Build internal layout graph
    let mut lg = LayoutGraph::from_digraph(graph, get_dimensions);
    let original_node_count = lg.node_ids.len();
    let has_compound = !lg.compound_nodes.is_empty();

    // Phase 0: Remove self-edges before acyclic detection
    extract_self_edges(&mut lg);

    // Phase 1: Make graph acyclic
    if config.acyclic {
        acyclic::run(&mut lg);
    }

    // Phase 1.5: Create space for edge label dummies.
    // Two strategies (both halve rank_sep to compensate for the doubled minlen model):
    // - per_edge_label_spacing: only bump labeled edges to minlen >= 2.
    //   Unlabeled edges keep minlen=1, so they span only 1 rank gap (rank_sep/2),
    //   making layouts more compact when few edges have labels.
    // - global (dagre-compatible): double ALL minlens. Every edge spans 2+ ranks,
    //   so the halved rank_sep cancels out uniformly. Matches dagre.js behavior.
    // Must be before nesting::run so nesting minlen multiplication applies to these too.
    let mut config = config.clone();
    let original_rank_sep = config.rank_sep;
    if config.per_edge_label_spacing {
        make_space_for_labeled_edges(&mut lg, edge_labels);
    } else {
        make_space_for_edge_labels(&mut lg);
    }
    config.rank_sep /= 2.0;

    // Compound: add nesting structure (border top/bottom, nesting edges).
    // Multiplies all existing edge minlens by nodeSep = 2*height+1.
    if has_compound {
        nesting::run(&mut lg);
    }

    // Phase 2: Assign ranks (layers)
    rank::run(&mut lg, &config);
    debug_dump_pipeline(&lg, "after_rank");

    // Compound: remove empty ranks created by nesting minlen multiplication.
    // Must run after ranking to compress the expanded rank space, and before
    // nesting cleanup so border nodes are still present.
    if has_compound {
        rank::remove_empty_ranks(&mut lg);
        debug_dump_pipeline(&lg, "after_remove_empty_ranks");
    }

    // Compound: cleanup nesting edges, normalize, insert title nodes, compute rank spans
    if has_compound {
        nesting::cleanup(&mut lg);
        debug_dump_pipeline(&lg, "after_nesting_cleanup");
    }

    rank::normalize(&mut lg);
    debug_dump_pipeline(&lg, "after_rank_normalize");

    if has_compound {
        if !skip_title_nodes() {
            nesting::insert_title_nodes(&mut lg);
            debug_dump_pipeline(&lg, "after_insert_title_nodes");
            // Re-normalize after title nodes may have introduced rank -1
            rank::normalize(&mut lg);
            debug_dump_pipeline(&lg, "after_rank_normalize_titles");
        }
        nesting::assign_rank_minmax(&mut lg);
        debug_dump_pipeline(&lg, "after_rank_minmax");
    }

    // Capture original edge indices of reversed edges BEFORE normalization,
    // because normalization removes long edges (and their reversed_edges entries).
    // The rendering layer needs these to identify backward edges for waypoint routing.
    let reversed_orig_edges: Vec<usize> = lg
        .reversed_edges
        .iter()
        .map(|&pos| lg.edges[pos].2)
        .collect();

    // Phase 2.5: Normalize long edges (insert dummy nodes)
    normalize::run(&mut lg, edge_labels, config.track_reversed_chains);
    debug_dump_pipeline(&lg, "after_normalize");

    // Phase 2.6: Optionally move label dummies to widest layer
    if config.label_dummy_strategy != LabelDummyStrategy::Midpoint {
        switch_label_dummies(&mut lg, config.label_dummy_strategy);
    }

    // Compound: assign dummy chain parents to match compound hierarchy.
    if has_compound {
        parent_dummy_chains::run(&mut lg);
        debug_dump_pipeline(&lg, "after_parent_dummy_chains");
    }

    // Compound: add border segments (left/right border nodes per rank)
    if has_compound {
        border::add_segments(&mut lg);
        debug_dump_pipeline(&lg, "after_border_segments");
    }

    // Clear model_order when tie-breaking is disabled so that it has no effect
    // on barycenter sorting (None.cmp(&None) == Equal).
    if !config.model_order_tiebreak {
        lg.model_order.fill(None);
    }

    // Phase 3: Reduce crossings (now includes dummy nodes and border segments)
    if config.always_compound_ordering {
        order::run_with_options(&mut lg, config.greedy_switch, true);
    } else {
        order::run(&mut lg, config.greedy_switch);
    }
    debug_dump_pipeline(&lg, "after_order");

    // Phase 3.6: Assign Above/Below sides to label dummies to reduce overlaps
    if config.label_side_selection {
        select_label_sides(&mut lg);
    }

    // Phase 3.5: Insert self-edge dummies (after ordering, before positioning)
    insert_self_edge_dummies(&mut lg);

    // Compute per-gap variable spacing overrides from edge density.
    // Dense gaps (3+ forward edges) get extra space proportional to edge count
    // above the threshold. Sparse gaps use base rank_sep (no override).
    // Based on research 0056 Q5 (routing-driven spacing, option B2).
    if config.variable_rank_spacing {
        config.rank_sep_overrides = compute_rank_sep_overrides(&lg, &config);
    }

    // Per-edge label spacing: restore full rank_sep for gaps that don't contain
    // label dummies. The halved rank_sep was needed for ranking (so labeled edges
    // with minlen=2 span ~original_rank_sep total), but unlabeled edges with
    // minlen=1 would only get half the normal spacing. Override those gaps back
    // to the original rank_sep so unlabeled edges maintain normal spacing.
    if config.per_edge_label_spacing {
        let label_gap_ranks = label_dummy_gap_ranks(&lg);
        let min_rank = lg.ranks.iter().copied().min().unwrap_or(0);
        let max_rank = lg.ranks.iter().copied().max().unwrap_or(0);
        for rank in min_rank..max_rank {
            if !label_gap_ranks.contains(&rank) {
                config
                    .rank_sep_overrides
                    .entry(rank)
                    .and_modify(|v| *v = (*v).max(original_rank_sep))
                    .or_insert(original_rank_sep);
            }
        }
    }

    // Phase 4: Assign coordinates (now uses per-gap overrides via rank_sep_for_gap)
    position::run(&mut lg, &config);

    // Phase 4.5: Compute self-edge loop paths
    let self_edge_layouts = position_self_edges(&lg, &config);

    // Compound: extract subgraph bounding boxes from border node positions
    let subgraph_bounds = if has_compound {
        border::remove_nodes(&mut lg)
    } else {
        HashMap::new()
    };

    // Extract waypoints from dummy positions
    let edge_waypoints = normalize::denormalize(&lg);

    // Extract label positions and sides
    let mut label_positions = HashMap::new();
    let mut label_sides = HashMap::new();
    for chain in &lg.dummy_chains {
        let edge_info = edge_labels.get(&chain.edge_index);
        let thickness = edge_info.map(|i| i.thickness).unwrap_or(1.0);
        if let Some(pos) = normalize::get_label_position_with_thickness(
            &lg,
            chain.edge_index,
            thickness,
            config.edge_label_spacing,
        ) {
            label_positions.insert(chain.edge_index, pos);
        }
        if let Some(label_idx) = chain.label_dummy_index {
            let dummy_id = &chain.dummy_ids[label_idx];
            if let Some(dummy) = lg.dummy_nodes.get(dummy_id) {
                label_sides.insert(chain.edge_index, dummy.label_side);
            }
        }
    }

    // Only include real nodes (not dummies) in the output
    let mut nodes: HashMap<NodeId, Rect> = lg
        .node_ids
        .iter()
        .enumerate()
        .take(original_node_count)
        .map(|(i, id)| {
            let pos = lg.positions[i];
            let (w, h) = lg.dimensions[i];
            (
                id.clone(),
                Rect {
                    x: pos.x,
                    y: pos.y,
                    width: w,
                    height: h,
                },
            )
        })
        .collect();

    // Apply subgraph bounds to compound nodes (dagre.js exposes compound bounds in node layout)
    if has_compound {
        for (id, rect) in &subgraph_bounds {
            let node_id = NodeId(id.clone());
            if let Some(existing) = nodes.get_mut(&node_id) {
                *existing = *rect;
            }
        }
    }

    // Build edge layouts, using waypoints for normalized edges
    let mut edges_by_orig_idx: HashMap<usize, EdgeLayout> = HashMap::new();

    for &(from, to, orig_idx) in &lg.edges {
        // Skip if this edge is already processed (part of a chain)
        if edges_by_orig_idx.contains_key(&orig_idx) {
            continue;
        }

        // Check if this edge has waypoints (was normalized)
        if let Some(waypoints) = edge_waypoints.get(&orig_idx) {
            // Find the original source and target nodes
            // For normalized edges, we need to find the chain endpoints
            // The source is the non-dummy node in the first segment
            // The target is the non-dummy node in the last segment
            let first_segment = lg
                .edges
                .iter()
                .find(|&&(f, _, idx)| idx == orig_idx && !lg.is_dummy_index(f));

            let last_segment = lg
                .edges
                .iter()
                .rev()
                .find(|&&(_, t, idx)| idx == orig_idx && !lg.is_dummy_index(t));

            if let (Some(&(src, _, _)), Some(&(_, tgt, _))) = (first_segment, last_segment) {
                let src_pos = lg.positions[src];
                let src_dim = lg.dimensions[src];
                let tgt_pos = lg.positions[tgt];
                let tgt_dim = lg.dimensions[tgt];

                let mut points = Vec::new();

                // Start point (center of source)
                points.push(Point {
                    x: src_pos.x + src_dim.0 / 2.0,
                    y: src_pos.y + src_dim.1 / 2.0,
                });

                // Add waypoints (extract just the point, not the rank info)
                points.extend(waypoints.iter().map(|wp| wp.point));

                // End point (center of target)
                points.push(Point {
                    x: tgt_pos.x + tgt_dim.0 / 2.0,
                    y: tgt_pos.y + tgt_dim.1 / 2.0,
                });

                edges_by_orig_idx.insert(
                    orig_idx,
                    EdgeLayout {
                        from: lg.node_ids[src].clone(),
                        to: lg.node_ids[tgt].clone(),
                        points,
                        index: orig_idx,
                    },
                );
            }
        } else {
            // Direct edge (not normalized)
            let from_pos = lg.positions[from];
            let to_pos = lg.positions[to];
            let from_dim = lg.dimensions[from];
            let to_dim = lg.dimensions[to];

            let from_center = Point {
                x: from_pos.x + from_dim.0 / 2.0,
                y: from_pos.y + from_dim.1 / 2.0,
            };
            let to_center = Point {
                x: to_pos.x + to_dim.0 / 2.0,
                y: to_pos.y + to_dim.1 / 2.0,
            };

            edges_by_orig_idx.insert(
                orig_idx,
                EdgeLayout {
                    from: lg.node_ids[from].clone(),
                    to: lg.node_ids[to].clone(),
                    points: vec![from_center, to_center],
                    index: orig_idx,
                },
            );
        }
    }

    // Sort edges by original index to maintain consistent ordering
    let mut edges: Vec<EdgeLayout> = edges_by_orig_idx.into_values().collect();
    edges.sort_by_key(|e| e.index);

    // Reverse points and swap from/to for reversed edges.
    // Matches dagre.js reversePointsForReversedEdges + acyclic.undo:
    // internally, reversed edges are laid out in the flipped direction;
    // this restores original source->target orientation.
    for edge in &mut edges {
        if reversed_orig_edges.contains(&edge.index) {
            edge.points.reverse();
            if let Some((orig_from, orig_to)) = graph.edges().get(edge.index) {
                edge.from = orig_from.clone();
                edge.to = orig_to.clone();
            }
        }
    }

    // Build rank-to-position mapping.
    // Contains user nodes and border nodes (position nodes), excluding dummies.
    // Note: The render layer now computes layer_starts from node_bounds + node_ranks,
    // so this mapping is retained for potential future use but not currently needed
    // for waypoint transformation.
    let is_vertical = config.direction.is_vertical();
    let rank_to_position: HashMap<i32, (f64, f64)> = lg
        .node_ids
        .iter()
        .enumerate()
        .filter(|&(i, _)| lg.is_position_node(i) && !lg.is_dummy_index(i))
        .fold(HashMap::new(), |mut acc, (i, _)| {
            let rank = lg.ranks[i];
            let pos = lg.positions[i];
            let (w, h) = lg.dimensions[i];
            let (start, end) = if is_vertical {
                (pos.y, pos.y + h)
            } else {
                (pos.x, pos.x + w)
            };
            acc.entry(rank)
                .and_modify(|(s, e)| {
                    *s = s.min(start);
                    *e = e.max(end);
                })
                .or_insert((start, end));
            acc
        });

    // Build node_ranks mapping for user nodes only (excluding dummies and compounds).
    // This allows the render layer to compute layer_starts from actual node bounds.
    // Compounds are excluded because they don't have rendered bounds in node_bounds;
    // including them would cause waypoint drift when compound bounds are surfaced.
    let node_ranks: HashMap<NodeId, i32> = lg
        .node_ids
        .iter()
        .enumerate()
        .take(original_node_count)
        .filter(|&(i, _)| !lg.is_dummy_index(i) && !lg.compound_nodes.contains(&i))
        .map(|(i, id)| (id.clone(), lg.ranks[i]))
        .collect();

    let mut result = LayoutResult {
        nodes,
        edges,
        reversed_edges: reversed_orig_edges,
        width: 0.0,
        height: 0.0,
        edge_waypoints,
        label_positions,
        label_sides,
        subgraph_bounds,
        self_edges: self_edge_layouts,
        rank_to_position,
        node_ranks,
    };

    // Post-layout translation: shift all coordinates so min corner = (margin, margin).
    // Matches dagre.js translateGraph (layout.js:215-264).
    translate_layout_result(&mut result, config.margin, config.margin, config.direction);
    // Adjust edge endpoints to node borders (dagre.js assignNodeIntersects).
    assign_node_intersects(&mut result);

    debug_dump_layout_result(&result, lg.original_edge_count);

    result
}
