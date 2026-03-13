//! Support utilities for the layered engine pipeline.

use std::collections::{HashMap, HashSet};

use super::graph::LayoutGraph;
use super::types::{DummyNode, DummyType, EdgeLabelInfo, LabelSide};
use super::{
    Direction, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Rect, SelfEdge,
    SelfEdgeLayout, rank,
};

pub(crate) fn make_space_for_edge_labels(lg: &mut LayoutGraph) {
    for minlen in &mut lg.edge_minlens {
        *minlen *= 2;
    }
}

/// Bump minlen only for edges that carry labels.
///
/// Unlike `make_space_for_edge_labels` (which doubles ALL minlens), this only
/// ensures labeled edges have minlen >= 2 so a label dummy can be inserted.
/// Unlabeled edges keep their original minlen. This produces more compact layouts
/// when only a few edges have labels.
pub(crate) fn make_space_for_labeled_edges(
    lg: &mut LayoutGraph,
    edge_labels: &HashMap<usize, EdgeLabelInfo>,
) {
    for (edge_idx, minlen) in lg.edge_minlens.iter_mut().enumerate() {
        if edge_labels.contains_key(&edge_idx) {
            *minlen = (*minlen).max(2);
        }
    }
}

/// Move label dummies to the widest layer in their chain to minimize width increase.
///
/// For each labeled edge's dummy chain, computes the total node width per layer
/// and swaps the label dummy to the layer with the most existing width. This
/// avoids adding label width to a narrow layer that would widen the layout.
///
/// Runs after normalization, before crossing reduction.
pub(crate) fn switch_label_dummies(lg: &mut LayoutGraph, strategy: LabelDummyStrategy) {
    if strategy == LabelDummyStrategy::Midpoint {
        return;
    }

    // Pre-compute total width per rank
    let mut rank_widths: HashMap<i32, f64> = HashMap::new();
    for (idx, &rank) in lg.ranks.iter().enumerate() {
        *rank_widths.entry(rank).or_default() += lg.dimensions[idx].0;
    }

    for chain in &mut lg.dummy_chains {
        let Some(label_idx) = chain.label_dummy_index else {
            continue;
        };

        // Find the widest layer among all dummies in this chain
        let best_idx = chain
            .dummy_ids
            .iter()
            .enumerate()
            .max_by(|(_, a_id), (_, b_id)| {
                let a_rank = lg.node_index.get(a_id).map(|&i| lg.ranks[i]).unwrap_or(0);
                let b_rank = lg.node_index.get(b_id).map(|&i| lg.ranks[i]).unwrap_or(0);
                let a_w = rank_widths.get(&a_rank).copied().unwrap_or(0.0);
                let b_w = rank_widths.get(&b_rank).copied().unwrap_or(0.0);
                a_w.partial_cmp(&b_w).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(label_idx);

        if best_idx != label_idx {
            // Swap: label dummy becomes Edge dummy, target becomes EdgeLabel dummy
            let label_id = chain.dummy_ids[label_idx].clone();
            let target_id = chain.dummy_ids[best_idx].clone();

            if let (Some(label_dummy), Some(_target_dummy)) = (
                lg.dummy_nodes.get(&label_id).cloned(),
                lg.dummy_nodes.get(&target_id).cloned(),
            ) {
                // Swap types and dimensions in dummy_nodes
                let label_w = label_dummy.width;
                let label_h = label_dummy.height;
                let label_pos = label_dummy.label_pos;

                if let Some(d) = lg.dummy_nodes.get_mut(&label_id) {
                    d.dummy_type = DummyType::Edge;
                    d.width = 0.0;
                    d.height = 0.0;
                }
                if let Some(d) = lg.dummy_nodes.get_mut(&target_id) {
                    d.dummy_type = DummyType::EdgeLabel;
                    d.width = label_w;
                    d.height = label_h;
                    d.label_pos = label_pos;
                }

                // Swap dimensions in the graph arrays
                let label_graph_idx = lg.node_index[&label_id];
                let target_graph_idx = lg.node_index[&target_id];
                lg.dimensions[label_graph_idx] = (0.0, 0.0);
                lg.dimensions[target_graph_idx] = (label_w, label_h);

                chain.label_dummy_index = Some(best_idx);
            }
        }
    }
}

/// Assign Above/Below sides to label dummies to reduce label-label overlaps.
///
/// After crossing reduction, label dummies sharing a layer are differentiated:
/// - Single label in layer → Center (unchanged)
/// - Two labels → first gets Above, last gets Below
/// - Three+ labels → first Above, last Below, rest Center
///
/// Runs after `order::run()` which establishes the definitive node ordering.
pub(crate) fn select_label_sides(lg: &mut LayoutGraph) {
    // Build layers from current ranks and ordering
    let layers = rank::by_rank_filtered(lg, |node| lg.ranks[node] >= 0);
    // Sort each layer by order
    let mut sorted_layers = layers;
    for layer in &mut sorted_layers {
        layer.sort_by_key(|&node| lg.order[node]);
    }

    for layer in &sorted_layers {
        // Collect label dummy node indices in layer order
        let label_dummies: Vec<usize> = layer
            .iter()
            .filter(|&&node_idx| {
                let node_id = &lg.node_ids[node_idx];
                lg.dummy_nodes
                    .get(node_id)
                    .map(|d| d.dummy_type == DummyType::EdgeLabel)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        let count = label_dummies.len();
        if count <= 1 {
            continue; // Single or no label: keep Center
        }
        for (i, &node_idx) in label_dummies.iter().enumerate() {
            let node_id = &lg.node_ids[node_idx];
            if let Some(dummy) = lg.dummy_nodes.get_mut(node_id) {
                dummy.label_side = if i == 0 {
                    LabelSide::Above
                } else if i == count - 1 {
                    LabelSide::Below
                } else {
                    LabelSide::Center
                };
            }
        }
    }
}

pub(crate) fn extract_self_edges(lg: &mut LayoutGraph) {
    debug_assert!(
        lg.reversed_edges.is_empty(),
        "extract_self_edges must run before acyclic::run()"
    );

    let mut to_remove = Vec::new();
    for (pos, &(from, to, orig_idx)) in lg.edges.iter().enumerate() {
        if from == to {
            lg.self_edges.push(SelfEdge {
                node_index: from,
                orig_edge_index: orig_idx,
                dummy_index: None,
            });
            to_remove.push(pos);
        }
    }

    // Remove in reverse order to preserve indices
    for &pos in to_remove.iter().rev() {
        lg.edges.remove(pos);
        lg.edge_weights.remove(pos);
        lg.edge_minlens.remove(pos);
    }
}

/// Insert dummy nodes for self-edges after ordering, before positioning.
///
/// Each self-edge gets a small dummy at the same rank, ordered right after
/// the self-edge's node. The BK algorithm will position it adjacent to the
/// node, establishing the loop extent.
pub(crate) fn insert_self_edge_dummies(lg: &mut LayoutGraph) {
    for (i, se) in lg.self_edges.clone().iter().enumerate() {
        let node_rank = lg.ranks[se.node_index];
        let node_order = lg.order[se.node_index];
        let dummy_id: NodeId = format!("_self_edge_dummy_{}", i).into();
        let dummy_idx = lg.node_ids.len();

        // Add dummy node to all parallel arrays
        lg.node_ids.push(dummy_id.clone());
        lg.node_index.insert(dummy_id.clone(), dummy_idx);
        lg.ranks.push(node_rank);
        lg.positions.push(Point::default());
        lg.dimensions.push((1.0, 1.0));
        lg.original_has_predecessor.push(false);
        lg.parents.push(lg.parents[se.node_index]);
        lg.model_order.push(None);

        // Insert into ordering: place dummy right after the node
        // Shift all nodes at this rank with order > node_order
        for idx in 0..lg.order.len() - 1 {
            // -1 because we haven't pushed yet
            if lg.ranks[idx] == node_rank && lg.order[idx] > node_order {
                lg.order[idx] += 1;
            }
        }
        lg.order.push(node_order + 1);

        // Register as dummy
        lg.dummy_nodes
            .insert(dummy_id, DummyNode::edge(se.orig_edge_index, node_rank));

        lg.self_edges[i].dummy_index = Some(dummy_idx);
    }
}

/// Compute 6-point orthogonal loop paths for self-edges using positioned node/dummy coordinates.
pub(crate) fn position_self_edges(lg: &LayoutGraph, config: &LayoutConfig) -> Vec<SelfEdgeLayout> {
    let gap = 1.0;

    lg.self_edges
        .iter()
        .filter_map(|se| {
            let dummy_idx = se.dummy_index?;
            let node_pos = lg.positions[se.node_index];
            let (nw, nh) = lg.dimensions[se.node_index];
            let dummy_pos = lg.positions[dummy_idx];

            let node_id = lg.node_ids[se.node_index].clone();
            let node_cx = node_pos.x + nw / 2.0;
            let node_cy = node_pos.y + nh / 2.0;

            let points = match config.direction {
                Direction::TopBottom => {
                    let loop_x = dummy_pos.x + 0.5;
                    let bot = node_pos.y + nh;
                    let top = node_pos.y;
                    vec![
                        Point { x: node_cx, y: bot },
                        Point {
                            x: node_cx,
                            y: bot + gap,
                        },
                        Point {
                            x: loop_x,
                            y: bot + gap,
                        },
                        Point {
                            x: loop_x,
                            y: top - gap,
                        },
                        Point {
                            x: node_cx,
                            y: top - gap,
                        },
                        Point { x: node_cx, y: top },
                    ]
                }
                Direction::BottomTop => {
                    let loop_x = dummy_pos.x + 0.5;
                    let top = node_pos.y;
                    let bot = node_pos.y + nh;
                    vec![
                        Point { x: node_cx, y: top },
                        Point {
                            x: node_cx,
                            y: top - gap,
                        },
                        Point {
                            x: loop_x,
                            y: top - gap,
                        },
                        Point {
                            x: loop_x,
                            y: bot + gap,
                        },
                        Point {
                            x: node_cx,
                            y: bot + gap,
                        },
                        Point { x: node_cx, y: bot },
                    ]
                }
                Direction::LeftRight => {
                    let loop_y = dummy_pos.y + 0.5;
                    let right = node_pos.x + nw;
                    let left = node_pos.x;
                    vec![
                        Point {
                            x: right,
                            y: node_cy,
                        },
                        Point {
                            x: right + gap,
                            y: node_cy,
                        },
                        Point {
                            x: right + gap,
                            y: loop_y,
                        },
                        Point {
                            x: left - gap,
                            y: loop_y,
                        },
                        Point {
                            x: left - gap,
                            y: node_cy,
                        },
                        Point {
                            x: left,
                            y: node_cy,
                        },
                    ]
                }
                Direction::RightLeft => {
                    let loop_y = dummy_pos.y + 0.5;
                    let left = node_pos.x;
                    let right = node_pos.x + nw;
                    vec![
                        Point {
                            x: left,
                            y: node_cy,
                        },
                        Point {
                            x: left - gap,
                            y: node_cy,
                        },
                        Point {
                            x: left - gap,
                            y: loop_y,
                        },
                        Point {
                            x: right + gap,
                            y: loop_y,
                        },
                        Point {
                            x: right + gap,
                            y: node_cy,
                        },
                        Point {
                            x: right,
                            y: node_cy,
                        },
                    ]
                }
            };

            Some(SelfEdgeLayout {
                node: node_id,
                edge_index: se.orig_edge_index,
                points,
            })
        })
        .collect()
}

/// Translate all layout coordinates so the minimum corner aligns with margins.
///
/// Matches dagre.js `translateGraph` (layout.js:215-264): computes min/max across
/// node bounding boxes and edge labels (not edge points), then shifts all coordinates
/// (including edge points) so the minimum is at (margin_x, margin_y). Width/height
/// include margin on both sides, matching dagre's `minX -= marginX` before the
/// `width = maxX - minX + marginX` calculation.
pub(crate) fn translate_layout_result(
    result: &mut LayoutResult,
    margin_x: f64,
    margin_y: f64,
    direction: Direction,
) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    macro_rules! update_rect {
        ($x:expr, $y:expr, $w:expr, $h:expr) => {
            min_x = min_x.min($x);
            max_x = max_x.max($x + $w);
            min_y = min_y.min($y);
            max_y = max_y.max($y + $h);
        };
    }

    // Nodes (dagre: g.nodes().forEach(v => getExtremes(g.node(v))))
    for rect in result.nodes.values() {
        update_rect!(rect.x, rect.y, rect.width, rect.height);
    }

    // Edge labels — dagre only includes edges with edge.x (i.e. labels),
    // not individual edge points. We don't store edge-level label rects in
    // EdgeLayout, but label_positions serve the same role.
    // (label_positions are point-sized, so they only affect min/max as points.)
    for lp in result.label_positions.values() {
        min_x = min_x.min(lp.point.x);
        max_x = max_x.max(lp.point.x);
        min_y = min_y.min(lp.point.y);
        max_y = max_y.max(lp.point.y);
    }

    // Subgraph bounds (mmdflux-specific, no dagre equivalent in translateGraph)
    for rect in result.subgraph_bounds.values() {
        update_rect!(rect.x, rect.y, rect.width, rect.height);
    }

    if min_x == f64::INFINITY {
        return; // empty result
    }

    // dagre.js: minX -= marginX; minY -= marginY;
    // Then: node.x -= minX (which adds marginX since minX is now smaller).
    // Net shift per coordinate: -(originalMinX - marginX) = marginX - originalMinX.
    // This places the leftmost extent at marginX, with width including margin on both sides.
    min_x -= margin_x;
    min_y -= margin_y;

    let dx = -min_x;
    let dy = -min_y;

    // Shift nodes
    for rect in result.nodes.values_mut() {
        rect.x += dx;
        rect.y += dy;
    }

    // Shift edge points
    for edge in &mut result.edges {
        for p in &mut edge.points {
            p.x += dx;
            p.y += dy;
        }
    }

    // Shift subgraph bounds
    for rect in result.subgraph_bounds.values_mut() {
        rect.x += dx;
        rect.y += dy;
    }

    // Shift edge waypoints
    for wps in result.edge_waypoints.values_mut() {
        for wp in wps {
            wp.point.x += dx;
            wp.point.y += dy;
        }
    }

    // Shift label positions
    for lp in result.label_positions.values_mut() {
        lp.point.x += dx;
        lp.point.y += dy;
    }

    // Shift self-edge points
    for se in &mut result.self_edges {
        for p in &mut se.points {
            p.x += dx;
            p.y += dy;
        }
    }

    // Shift rank_to_position: the primary axis is Y for vertical, X for horizontal
    let primary_delta = if direction.is_vertical() { dy } else { dx };
    for (start, end) in result.rank_to_position.values_mut() {
        *start += primary_delta;
        *end += primary_delta;
    }

    // dagre.js: graphLabel.width = maxX - minX + marginX
    // Since minX was already reduced by marginX, this adds margin on both sides.
    result.width = max_x - min_x + margin_x;
    result.height = max_y - min_y + margin_y;
}

/// Adjust edge endpoints to intersect node borders.
///
/// Mirrors dagre.js `assignNodeIntersects` (layout.js:269-276).
/// Uses the first/last waypoint (not the node center) as the direction vector
/// so intersections are computed toward the edge path.
pub(crate) fn assign_node_intersects(result: &mut LayoutResult) {
    fn rect_center(rect: &Rect) -> Point {
        Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        }
    }

    fn intersect_rect(rect: &Rect, point: Point) -> Point {
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let dx = point.x - cx;
        let dy = point.y - cy;
        let w = rect.width / 2.0;
        let h = rect.height / 2.0;

        if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
            // Edge case: point equals center, return bottom-center.
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

    for edge in &mut result.edges {
        if edge.points.is_empty() {
            continue;
        }
        let Some(from_rect) = result.nodes.get(&edge.from) else {
            continue;
        };
        let Some(to_rect) = result.nodes.get(&edge.to) else {
            continue;
        };

        let from_center = rect_center(from_rect);
        let to_center = rect_center(to_rect);
        let last_idx = edge.points.len() - 1;

        let from_target = if edge.points.len() >= 2 {
            edge.points[1]
        } else {
            to_center
        };
        let to_target = if edge.points.len() >= 2 {
            edge.points[last_idx - 1]
        } else {
            from_center
        };

        edge.points[0] = intersect_rect(from_rect, from_target);
        edge.points[last_idx] = intersect_rect(to_rect, to_target);
    }
}

/// Count forward edges crossing each rank gap.
///
/// Returns a map from rank to the number of forward edges crossing the gap
/// between that rank and the next occupied rank.
pub(crate) fn count_forward_edges_per_gap(lg: &LayoutGraph) -> HashMap<i32, usize> {
    let mut counts: HashMap<i32, usize> = HashMap::new();

    for (edge_pos, &(from, to, _orig_idx)) in lg.edges.iter().enumerate() {
        // Skip reversed (backward) edges
        if lg.reversed_edges.contains(&edge_pos) {
            continue;
        }

        // Skip excluded edges (nesting edges)
        if lg.excluded_edges.contains(&edge_pos) {
            continue;
        }

        // Skip edges involving border nodes (structural compound graph edges)
        if lg.border_type.contains_key(&from) || lg.border_type.contains_key(&to) {
            continue;
        }

        let r_from = lg.ranks[from];
        let r_to = lg.ranks[to];

        // Forward edge: source rank < target rank
        let (r_lo, r_hi) = if r_from < r_to {
            (r_from, r_to)
        } else if r_to < r_from {
            (r_to, r_from)
        } else {
            continue; // same-rank edge, no gap crossing
        };

        // This edge crosses every gap from r_lo to r_hi - 1
        for rank in r_lo..r_hi {
            *counts.entry(rank).or_insert(0) += 1;
        }
    }

    counts
}

/// Compute per-rank-gap spacing overrides based on edge density.
///
/// For each rank gap, counts the number of forward edges crossing it.
/// Gaps with more than `threshold` edges get inflated spacing to give
/// the orthogonal router more room for edge separation.
///
/// Returns a map from rank to the override rank_sep for the gap after
/// that rank. Only contains entries for gaps that exceed the threshold;
/// gaps at or below the threshold use the base `config.rank_sep`.
///
/// Based on research 0056 Q5 (routing-driven spacing, option B2).
pub(crate) fn compute_rank_sep_overrides(
    lg: &LayoutGraph,
    config: &LayoutConfig,
) -> HashMap<i32, f64> {
    let edge_counts = count_forward_edges_per_gap(lg);
    let threshold: usize = 3;
    let mut overrides = HashMap::new();

    // Use half of edge_sep as the per-edge inflation multiplier.
    // Full edge_sep (20.0) produces excessive inflation because each edge
    // contributes to every intermediate gap (doubled minlen means 2 gaps
    // per direct edge). Half the multiplier keeps the total inflation
    // proportional to a single routing channel width per extra edge.
    let edge_inflation = config.edge_sep / 2.0;

    for (&rank, &count) in &edge_counts {
        if count > threshold {
            let inflation = (count - threshold) as f64 * edge_inflation;
            overrides.insert(rank, config.rank_sep + inflation);
        }
    }

    overrides
}

/// Identify rank gaps that contain label dummy nodes.
///
/// A label dummy sits between two ranks; the "gap" is identified by the
/// lower rank number. For example, a label dummy at rank 1 sits in the
/// gap between rank 0 and rank 1, so rank 0 is in the returned set.
pub(crate) fn label_dummy_gap_ranks(lg: &LayoutGraph) -> HashSet<i32> {
    let mut gaps = HashSet::new();
    for (id, dummy) in &lg.dummy_nodes {
        if dummy.dummy_type == DummyType::EdgeLabel
            && let Some(&idx) = lg.node_index.get(id)
        {
            let rank = lg.ranks[idx];
            // Label dummy at rank R sits in the gap R-1..R.
            // Only that gap needs the halved rank_sep; the gap R..R+1
            // may contain unlabeled edges that need full rank_sep.
            gaps.insert(rank - 1);
        }
    }
    gaps
}
