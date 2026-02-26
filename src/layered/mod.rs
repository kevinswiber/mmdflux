//! Hierarchical graph layout (Sugiyama framework).
//!
//! Implements the Sugiyama framework:
//! 1. Cycle removal (make graph acyclic)
//! 2. Layer assignment (rank nodes)
//! 3. Crossing reduction (order nodes within layers)
//! 4. Coordinate assignment (x, y positions)
//!
//! # Example
//!
//! ```
//! use mmdflux::layered::{DiGraph, layout, LayoutConfig, Direction};
//!
//! // Create a graph with node dimensions
//! let mut graph = DiGraph::new();
//! graph.add_node("A", (100.0, 50.0)); // (width, height)
//! graph.add_node("B", (100.0, 50.0));
//! graph.add_edge("A", "B");
//!
//! // Configure and run layout
//! let config = LayoutConfig {
//!     direction: Direction::TopBottom,
//!     ..Default::default()
//! };
//!
//! let result = layout(&graph, &config, |_, dims| *dims);
//!
//! // Access positioned nodes
//! for (node_id, rect) in &result.nodes {
//!     println!("{}: ({}, {})", node_id, rect.x, rect.y);
//! }
//! ```

mod acyclic;
mod bk;
pub(crate) mod border;
mod graph;
pub(crate) mod nesting;
pub(crate) mod network_simplex;
pub mod normalize;
mod order;
mod parent_dummy_chains;
mod position;
mod rank;
pub mod types;

use std::collections::{HashMap, HashSet};
use std::io::Write;

pub use graph::DiGraph;
use graph::LayoutGraph;
pub use types::{
    Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Ranker,
    Rect, SelfEdge, SelfEdgeLayout,
};

/// Double all edge minlens to create a uniform rank grid.
///
/// Matches dagre.js's `makeSpaceForEdgeLabels()` behavior, which always doubles
/// minlens (and later compensates via ranksep scaling). The uniform grid ensures
/// downstream Sugiyama phases (normalization, ordering, positioning) see consistent
/// rank spacing, even when no labels are present.
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
    edge_labels: &HashMap<usize, normalize::EdgeLabelInfo>,
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
                    d.dummy_type = normalize::DummyType::Edge;
                    d.width = 0.0;
                    d.height = 0.0;
                }
                if let Some(d) = lg.dummy_nodes.get_mut(&target_id) {
                    d.dummy_type = normalize::DummyType::EdgeLabel;
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
    use normalize::{DummyType, LabelSide};

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

fn debug_pipeline_target() -> Option<String> {
    std::env::var("MMDFLUX_DEBUG_PIPELINE").ok()
}

fn debug_layout_target() -> Option<String> {
    std::env::var("MMDFLUX_DEBUG_LAYOUT").ok()
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn fmt_f64_json(value: f64) -> String {
    if value.is_finite() {
        format!("{}", value)
    } else {
        "null".to_string()
    }
}

fn debug_dump_pipeline(lg: &LayoutGraph, stage: &str) {
    let Some(target) = debug_pipeline_target() else {
        return;
    };

    let mut entries: Vec<(i32, usize, usize)> = lg
        .ranks
        .iter()
        .enumerate()
        .map(|(idx, &rank)| (rank, lg.order[idx], idx))
        .collect();
    entries.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| lg.node_ids[a.2].0.cmp(&lg.node_ids[b.2].0))
    });

    let mut buf = String::new();
    for (rank, order, idx) in entries {
        let id = &lg.node_ids[idx].0;
        let parent = lg.parents[idx].map(|p| lg.node_ids[p].0.clone());
        let dummy = lg
            .dummy_nodes
            .get(&lg.node_ids[idx])
            .map(|d| match d.dummy_type {
                normalize::DummyType::Edge => "edge",
                normalize::DummyType::EdgeLabel => "edge_label",
            });
        let dummy_edge = lg.dummy_nodes.get(&lg.node_ids[idx]).map(|d| d.edge_index);
        let border = lg.border_type.get(&idx).map(|b| match b {
            graph::BorderType::Left => "left",
            graph::BorderType::Right => "right",
        });
        let is_position = lg.is_position_node(idx);
        let is_compound = lg.compound_nodes.contains(&idx);
        let is_excluded = lg.position_excluded_nodes.contains(&idx);

        let parent_json = match parent.as_deref() {
            Some(p) => format!("\"{}\"", json_escape(p)),
            None => "null".to_string(),
        };
        let dummy_json = match dummy {
            Some(d) => format!("\"{}\"", d),
            None => "null".to_string(),
        };
        let dummy_edge_json = match dummy_edge {
            Some(d) => d.to_string(),
            None => "null".to_string(),
        };
        let border_json = match border {
            Some(b) => format!("\"{}\"", b),
            None => "null".to_string(),
        };

        buf.push_str(&format!(
            "{{\"stage\":\"{}\",\"id\":\"{}\",\"rank\":{},\"order\":{},\"parent\":{},\"dummy\":{},\"dummy_edge\":{},\"border\":{},\"is_position\":{},\"is_compound\":{},\"is_excluded\":{}}}\n",
            json_escape(stage),
            json_escape(id),
            rank,
            order,
            parent_json,
            dummy_json,
            dummy_edge_json,
            border_json,
            is_position,
            is_compound,
            is_excluded
        ));
    }

    if target == "1" {
        eprint!("{buf}");
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
    {
        let _ = file.write_all(buf.as_bytes());
    }
}

fn debug_dump_layout_result(result: &LayoutResult, original_edge_count: usize) {
    let Some(target) = debug_layout_target() else {
        return;
    };

    let mut nodes: Vec<(&NodeId, &Rect)> = result.nodes.iter().collect();
    nodes.sort_by(|a, b| a.0.0.cmp(&b.0.0));

    let mut edges: Vec<EdgeLayout> = result
        .edges
        .iter()
        .filter(|e| e.index < original_edge_count)
        .cloned()
        .collect();
    edges.sort_by_key(|e| e.index);

    let mut subgraphs: Vec<(&String, &Rect)> = result.subgraph_bounds.iter().collect();
    subgraphs.sort_by(|a, b| a.0.cmp(b.0));

    let mut buf = String::new();
    buf.push_str("{\"nodes\":[");
    for (i, (id, rect)) in nodes.iter().enumerate() {
        let center_x = rect.x + rect.width / 2.0;
        let center_y = rect.y + rect.height / 2.0;
        let suffix = if i + 1 == nodes.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"id\":\"{}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"center_x\":{},\"center_y\":{}}}{}",
            json_escape(&id.0),
            fmt_f64_json(rect.x),
            fmt_f64_json(rect.y),
            fmt_f64_json(rect.width),
            fmt_f64_json(rect.height),
            fmt_f64_json(center_x),
            fmt_f64_json(center_y),
            suffix
        ));
    }
    buf.push_str("],\"edges\":[");
    for (i, edge) in edges.iter().enumerate() {
        let suffix = if i + 1 == edges.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"index\":{},\"from\":\"{}\",\"to\":\"{}\",\"points\":[",
            edge.index,
            json_escape(&edge.from.0),
            json_escape(&edge.to.0)
        ));
        for (p_idx, point) in edge.points.iter().enumerate() {
            let p_suffix = if p_idx + 1 == edge.points.len() {
                ""
            } else {
                ","
            };
            buf.push_str(&format!(
                "[{},{}]{}",
                fmt_f64_json(point.x),
                fmt_f64_json(point.y),
                p_suffix
            ));
        }
        buf.push_str(&format!("]}}{}", suffix));
    }
    buf.push_str("],\"subgraph_bounds\":[");
    for (i, (id, rect)) in subgraphs.iter().enumerate() {
        let suffix = if i + 1 == subgraphs.len() { "" } else { "," };
        buf.push_str(&format!(
            "{{\"id\":\"{}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}{}",
            json_escape(id),
            fmt_f64_json(rect.x),
            fmt_f64_json(rect.y),
            fmt_f64_json(rect.width),
            fmt_f64_json(rect.height),
            suffix
        ));
    }
    buf.push_str("],\"graph\":{");
    buf.push_str(&format!(
        "\"width\":{},\"height\":{}",
        fmt_f64_json(result.width),
        fmt_f64_json(result.height)
    ));
    buf.push_str("}}\n");

    if target == "1" {
        eprint!("{buf}");
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&target)
    {
        let _ = file.write_all(buf.as_bytes());
    }
}

/// Remove self-edges (from == to) from the graph before the Sugiyama pipeline.
///
/// Self-edges confuse cycle detection and ranking. They are stashed on
/// `lg.self_edges` and removed from `lg.edges` (and parallel arrays).
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
fn insert_self_edge_dummies(lg: &mut LayoutGraph) {
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
        lg.dummy_nodes.insert(
            dummy_id,
            normalize::DummyNode::edge(se.orig_edge_index, node_rank),
        );

        lg.self_edges[i].dummy_index = Some(dummy_idx);
    }
}

/// Compute 6-point orthogonal loop paths for self-edges using positioned node/dummy coordinates.
fn position_self_edges(lg: &LayoutGraph, config: &LayoutConfig) -> Vec<SelfEdgeLayout> {
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
fn translate_layout_result(
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
fn assign_node_intersects(result: &mut LayoutResult) {
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

/// Main entry point for layout computation.
///
/// Takes a directed graph, configuration options, and a function to get node dimensions.
/// Returns a `LayoutResult` with positioned nodes and edge paths.
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
    edge_labels: &HashMap<usize, normalize::EdgeLabelInfo>,
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
        let skip_titles = std::env::var("MMDFLUX_SKIP_TITLE_NODES").is_ok_and(|v| v == "1");
        if !skip_titles {
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

/// Count forward edges crossing each rank gap.
///
/// Returns a map from rank to the number of forward edges crossing the gap
/// between that rank and the next occupied rank. An edge from rank `r_src`
/// to rank `r_tgt` (where `r_src < r_tgt`) contributes to every gap in
/// [r_src, r_tgt - 1].
///
/// Only counts non-reversed edges (forward edges). Reversed edges use outer
/// lanes and don't consume inter-layer routing slots. Edges involving border
/// nodes are also excluded since those are structural compound-graph edges
/// that don't need routing space.
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
fn label_dummy_gap_ranks(lg: &LayoutGraph) -> HashSet<i32> {
    let mut gaps = HashSet::new();
    for (id, dummy) in &lg.dummy_nodes {
        if dummy.dummy_type == normalize::DummyType::EdgeLabel
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: run the layout pipeline up to rank assignment and count forward edges per gap.
    fn count_edges_per_gap_for_test(
        graph: &DiGraph<(f64, f64)>,
        config: &LayoutConfig,
    ) -> HashMap<i32, usize> {
        let mut lg = LayoutGraph::from_digraph(graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if config.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_edge_labels(&mut lg);
        let mut config = config.clone();
        config.rank_sep /= 2.0;
        rank::run(&mut lg, &config);
        rank::normalize(&mut lg);
        count_forward_edges_per_gap(&lg)
    }

    #[test]
    fn count_edges_per_gap_linear_chain() {
        // A -> B -> C: 1 edge in each gap
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let config = LayoutConfig::default();
        let counts = count_edges_per_gap_for_test(&graph, &config);
        // Each gap should have exactly 1 edge
        assert!(
            counts.values().all(|&c| c <= 1),
            "Linear chain should have at most 1 edge per gap, got {:?}",
            counts
        );
    }

    #[test]
    fn count_edges_per_gap_fan_out() {
        // A -> B, A -> C, A -> D: 3 edges in gap between A's rank and B/C/D's rank
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_node("D", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("A", "D");

        let config = LayoutConfig::default();
        let counts = count_edges_per_gap_for_test(&graph, &config);
        let max_count = *counts.values().max().unwrap_or(&0);
        assert!(
            max_count >= 3,
            "fan-out should have 3+ edges in densest gap, got {}",
            max_count
        );
    }

    #[test]
    fn count_edges_per_gap_five_fan_in() {
        // A,B,C,D,E -> F: 5 edges in the gap before F
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E", "F"] {
            graph.add_node(id, (10.0, 10.0));
        }
        for src in ["A", "B", "C", "D", "E"] {
            graph.add_edge(src, "F");
        }

        let config = LayoutConfig::default();
        let counts = count_edges_per_gap_for_test(&graph, &config);
        let max_count = *counts.values().max().unwrap_or(&0);
        assert!(
            max_count >= 5,
            "5-fan-in should have 5 edges in densest gap, got {}",
            max_count
        );
    }

    #[test]
    fn count_edges_per_gap_excludes_backward() {
        // A -> B, B -> A (cycle): only A -> B is forward, B -> A is reversed
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "A");

        let config = LayoutConfig::default();
        let counts = count_edges_per_gap_for_test(&graph, &config);
        let max_count = *counts.values().max().unwrap_or(&0);
        // Only the forward edge (A->B) should be counted; B->A is reversed
        assert!(
            max_count <= 1,
            "backward edge should not be counted, got max {}",
            max_count
        );
    }

    #[test]
    fn count_edges_per_gap_excludes_long_backward_chain() {
        // A -> B -> C -> D, D -> A (reversed long edge spanning 3 ranks).
        // After normalization, D -> A becomes 3 chain edges. These chain edges
        // must NOT be counted as forward edges — they represent a backward edge.
        // Only A->B, B->C, C->D (1 edge per gap) should be counted.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D"] {
            graph.add_node(id, (10.0, 10.0));
        }
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");
        graph.add_edge("C", "D");
        graph.add_edge("D", "A"); // backward edge

        let config = LayoutConfig::default();
        let counts = count_edges_per_gap_for_test(&graph, &config);
        let max_count = *counts.values().max().unwrap_or(&0);
        // Each gap should have exactly 1 forward edge (A->B, B->C, C->D).
        // The reversed D->A chain edges should NOT be counted.
        assert!(
            max_count <= 1,
            "reversed long edge chain should not inflate gap counts, got max {}; counts: {:?}",
            max_count,
            counts
        );
    }

    // Phase 1 (global inflation B1) was prototyped and discarded in favor of
    // per-gap variable spacing (B2). The global approach inflated ALL gaps
    // uniformly, which wasted space in sparse gaps of mixed-density diagrams
    // and caused compound-graph test failures. Per-gap tests are below.

    /// Test helper: run the pipeline up to rank assignment and compute rank_sep overrides.
    fn compute_overrides_for_test(
        graph: &DiGraph<(f64, f64)>,
        config: &LayoutConfig,
    ) -> HashMap<i32, f64> {
        let mut lg = LayoutGraph::from_digraph(graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if config.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_edge_labels(&mut lg);
        let mut config = config.clone();
        config.rank_sep /= 2.0;
        rank::run(&mut lg, &config);
        rank::normalize(&mut lg);
        compute_rank_sep_overrides(&lg, &config)
    }

    #[test]
    fn compute_overrides_five_fan_in_inflates_dense_gap() {
        // A,B,C,D,E -> F: 5 edges in one gap, all other gaps have 0-1
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E", "F"] {
            graph.add_node(id, (10.0, 10.0));
        }
        for src in ["A", "B", "C", "D", "E"] {
            graph.add_edge(src, "F");
        }

        let config = LayoutConfig::default();
        let overrides = compute_overrides_for_test(&graph, &config);

        // There should be at least one override with a value > base rank_sep
        let base = config.rank_sep / 2.0; // after halving in layout_with_labels
        let has_inflation = overrides.values().any(|&v| v > base);
        assert!(
            has_inflation,
            "Should have at least one inflated gap, overrides: {:?}",
            overrides
        );
    }

    #[test]
    fn compute_overrides_linear_chain_no_overrides() {
        // A -> B -> C: max 1 edge per gap, no inflation needed
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let config = LayoutConfig::default();
        let overrides = compute_overrides_for_test(&graph, &config);

        // No gap has more than threshold edges, so no overrides needed
        assert!(
            overrides.is_empty(),
            "Linear chain should have no overrides, got: {:?}",
            overrides
        );
    }

    #[test]
    fn compute_overrides_mixed_density() {
        // A -> B, A -> C (fan-out, 2 edges in gap)
        // B -> D, C -> D (fan-in, 2 edges in gap)
        // Plus E -> D, F -> D (adds to fan-in gap, exceeding threshold)
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E", "F"] {
            graph.add_node(id, (10.0, 10.0));
        }
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");
        graph.add_edge("E", "D");
        graph.add_edge("F", "D");

        let config = LayoutConfig::default();
        let overrides = compute_overrides_for_test(&graph, &config);

        // The fan-in gap before D should have an override (4+ edges),
        // but the fan-out gap after A has only 2 edges (at threshold)
        let has_some_override = !overrides.is_empty();
        assert!(
            has_some_override,
            "Mixed graph should have at least one override"
        );
    }

    #[test]
    fn layout_five_fan_in_denser_gap_than_sparse() {
        // Two-layer graph: A,B,C,D,E -> F (5 edges in one gap).
        // With per-gap spacing, the gap between source and target ranks should
        // be wider than in a simple 1-edge graph.
        let mut graph_dense: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E", "F"] {
            graph_dense.add_node(id, (10.0, 10.0));
        }
        for src in ["A", "B", "C", "D", "E"] {
            graph_dense.add_edge(src, "F");
        }

        let mut graph_sparse: DiGraph<(f64, f64)> = DiGraph::new();
        graph_sparse.add_node("X", (10.0, 10.0));
        graph_sparse.add_node("Y", (10.0, 10.0));
        graph_sparse.add_edge("X", "Y");

        let config = LayoutConfig {
            variable_rank_spacing: true,
            ..LayoutConfig::default()
        };
        let dense_result = layout(&graph_dense, &config, |_, dims| *dims);
        let sparse_result = layout(&graph_sparse, &config, |_, dims| *dims);

        // Dense layout should be taller because its gap is inflated
        assert!(
            dense_result.height > sparse_result.height,
            "Dense 5-fan-in (h={}) should be taller than sparse (h={})",
            dense_result.height,
            sparse_result.height,
        );
    }

    #[test]
    fn layout_mixed_density_selective_inflation() {
        // A -> B -> C, A -> C (long edge), D -> C, E -> C
        // The gap before C should be denser than the gap after A
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E"] {
            graph.add_node(id, (10.0, 10.0));
        }
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");
        graph.add_edge("A", "C"); // long edge, crosses both gaps
        graph.add_edge("D", "C");
        graph.add_edge("E", "C");

        let config = LayoutConfig {
            variable_rank_spacing: true,
            ..LayoutConfig::default()
        };
        let result = layout(&graph, &config, |_, dims| *dims);

        // Should produce a valid layout without panicking
        assert!(result.height > 0.0);
        assert_eq!(result.nodes.len(), 5);
    }

    #[test]
    fn layout_sparse_graph_unchanged_by_variable_spacing() {
        // A -> B -> C: no dense gaps, layout should be identical to fixed rank_sep
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        let a_y = result.nodes.get(&"A".into()).unwrap().y;
        let b_y = result.nodes.get(&"B".into()).unwrap().y;
        let c_y = result.nodes.get(&"C".into()).unwrap().y;

        // Gaps should be equal (no overrides applied)
        let gap_ab = b_y - a_y;
        let gap_bc = c_y - b_y;
        assert!(
            (gap_ab - gap_bc).abs() < 1.0,
            "Sparse chain gaps should be equal: ab={}, bc={}",
            gap_ab,
            gap_bc,
        );
    }

    #[test]
    fn test_simple_layout() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_edge("A", "B");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);

        // A should be above B in TopBottom layout
        let a_rect = result.nodes.get(&"A".into()).unwrap();
        let b_rect = result.nodes.get(&"B".into()).unwrap();
        assert!(a_rect.y < b_rect.y);
    }

    #[test]
    fn test_layout_with_cycle() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "A"); // Cycle

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        // Should still produce a valid layout
        assert_eq!(result.nodes.len(), 2);
        // One edge should be reversed
        assert_eq!(result.reversed_edges.len(), 1);
    }

    #[test]
    fn test_layout_directions() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_edge("A", "B");

        // Test LR direction
        let config = LayoutConfig {
            direction: Direction::LeftRight,
            ..Default::default()
        };
        let result = layout(&graph, &config, |_, dims| *dims);

        let a_rect = result.nodes.get(&"A".into()).unwrap();
        let b_rect = result.nodes.get(&"B".into()).unwrap();
        // A should be left of B in LeftRight layout
        assert!(a_rect.x < b_rect.x);
    }

    #[test]
    fn test_http_request_cycle() {
        // Simulates http_request.mmd graph
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("Client", (100.0, 50.0));
        graph.add_node("Server", (100.0, 50.0));
        graph.add_node("Auth", (100.0, 50.0));
        graph.add_node("Process", (100.0, 50.0));
        graph.add_node("Reject", (100.0, 50.0));
        graph.add_node("Response", (100.0, 50.0));

        // Edges in order from the mmd file
        graph.add_edge("Client", "Server");
        graph.add_edge("Server", "Auth");
        graph.add_edge("Auth", "Process");
        graph.add_edge("Auth", "Reject");
        graph.add_edge("Process", "Response");
        graph.add_edge("Reject", "Response");
        graph.add_edge("Response", "Client"); // Creates cycle

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        // Client should be at the top (smallest y)
        let client_y = result.nodes.get(&"Client".into()).unwrap().y;
        let server_y = result.nodes.get(&"Server".into()).unwrap().y;
        let auth_y = result.nodes.get(&"Auth".into()).unwrap().y;
        let process_y = result.nodes.get(&"Process".into()).unwrap().y;
        let response_y = result.nodes.get(&"Response".into()).unwrap().y;

        assert!(client_y < server_y, "Client should be above Server");
        assert!(server_y < auth_y, "Server should be above Auth");
        assert!(auth_y < process_y, "Auth should be above Process");
        assert!(process_y < response_y, "Process should be above Response");
    }

    #[test]
    fn test_layout_with_long_edge() {
        // A -> B -> C -> D, and A -> D (long edge spanning 3 ranks)
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_node("C", (100.0, 50.0));
        graph.add_node("D", (100.0, 50.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");
        graph.add_edge("C", "D");
        graph.add_edge("A", "D"); // Long edge: spans 3 ranks

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        // Should have 4 nodes
        assert_eq!(result.nodes.len(), 4);

        // Should have 4 edges
        assert_eq!(result.edges.len(), 4);

        // The A->D edge should have waypoints
        let ad_edge = result
            .edges
            .iter()
            .find(|e| e.from.0 == "A" && e.to.0 == "D");
        assert!(ad_edge.is_some(), "Should have A->D edge");

        let ad_edge = ad_edge.unwrap();
        // A->D spans 3 ranks. Points include start, end, and intermediate waypoints.
        assert!(
            ad_edge.points.len() >= 4,
            "A->D edge should have at least 4 points, got {}",
            ad_edge.points.len()
        );

        // Verify waypoints were extracted
        assert!(
            result.edge_waypoints.contains_key(&ad_edge.index),
            "Should have waypoints for long edge"
        );
    }

    #[test]
    fn test_make_space_doubles_all_minlens() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (5.0, 3.0));
        graph.add_node("B", (5.0, 3.0));
        graph.add_node("C", (5.0, 3.0));
        graph.add_edge("A", "B"); // edge 0: labeled
        graph.add_edge("B", "C"); // edge 1: unlabeled

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

        make_space_for_edge_labels(&mut lg);

        // ALL edges should be doubled, not just the labeled one
        assert_eq!(lg.edge_minlens[0], 2); // labeled edge: 1 * 2 = 2
        assert_eq!(lg.edge_minlens[1], 2); // unlabeled edge: 1 * 2 = 2
    }

    #[test]
    fn test_make_space_doubles_without_labels() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (5.0, 3.0));
        graph.add_node("B", (5.0, 3.0));
        graph.add_edge("A", "B");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        make_space_for_edge_labels(&mut lg);

        assert_eq!(lg.edge_minlens[0], 2); // doubled even without labels
    }

    #[test]
    fn make_space_for_labeled_edges_only_bumps_labeled() {
        // 3 edges: edge 0 labeled (minlen=1), edge 1 unlabeled (minlen=1),
        // edge 2 labeled (minlen=2, already sufficient)
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (5.0, 3.0));
        graph.add_node("B", (5.0, 3.0));
        graph.add_node("C", (5.0, 3.0));
        graph.add_node("D", (5.0, 3.0));
        graph.add_edge("A", "B"); // edge 0: labeled
        graph.add_edge("B", "C"); // edge 1: unlabeled
        graph.add_edge("C", "D"); // edge 2: labeled

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        // Set edge 2 minlen to 2 (already sufficient for a label)
        lg.edge_minlens[2] = 2;

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(50.0, 5.0));
        edge_labels.insert(2, normalize::EdgeLabelInfo::new(50.0, 5.0));

        make_space_for_labeled_edges(&mut lg, &edge_labels);

        assert_eq!(lg.edge_minlens[0], 2); // bumped: was 1, needs at least 2
        assert_eq!(lg.edge_minlens[1], 1); // unchanged: no label
        assert_eq!(lg.edge_minlens[2], 2); // unchanged: already >= 2
    }

    // switch_label_dummies tests

    /// Build a LayoutGraph with a long labeled edge spanning `span` ranks.
    /// Returns graph with A at rank 0, B at rank `span`, and `span-1` dummies between.
    /// The label dummy is at the midpoint rank.
    fn build_graph_with_long_labeled_edge(span: usize) -> LayoutGraph {
        use normalize::{DummyChain, DummyNode};

        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

        let a_idx = lg.node_index[&NodeId::from("A")];
        let b_idx = lg.node_index[&NodeId::from("B")];
        lg.ranks[a_idx] = 0;
        lg.ranks[b_idx] = span as i32;

        let midpoint_rank = (span / 2) as i32;

        let mut chain = DummyChain::new(0);
        for r in 1..span {
            let rank = r as i32;
            let dummy_id = NodeId::from(format!("_d{}", r));
            let dummy_idx = lg.node_ids.len();

            let is_label = rank == midpoint_rank;
            let (w, h) = if is_label { (30.0, 14.0) } else { (0.0, 0.0) };

            let dummy_node = if is_label {
                DummyNode::edge_label(0, rank, w, h, normalize::LabelPos::Center)
            } else {
                DummyNode::edge(0, rank)
            };

            lg.node_ids.push(dummy_id.clone());
            lg.node_index.insert(dummy_id.clone(), dummy_idx);
            lg.ranks.push(rank);
            lg.order.push(dummy_idx);
            lg.positions.push(Point::default());
            lg.dimensions.push((w, h));
            lg.original_has_predecessor.push(false);
            lg.parents.push(None);
            lg.model_order.push(None);
            lg.dummy_nodes.insert(dummy_id.clone(), dummy_node);

            if is_label {
                chain.label_dummy_index = Some(chain.dummy_ids.len());
            }
            chain.dummy_ids.push(dummy_id);
        }
        lg.dummy_chains.push(chain);
        lg
    }

    #[test]
    fn switch_midpoint_strategy_is_noop() {
        let mut lg = build_graph_with_long_labeled_edge(6);
        let original_idx = lg.dummy_chains[0].label_dummy_index;
        switch_label_dummies(&mut lg, LabelDummyStrategy::Midpoint);
        assert_eq!(lg.dummy_chains[0].label_dummy_index, original_idx);
    }

    #[test]
    fn switch_to_widest_layer_stays_if_midpoint_is_widest() {
        let mut lg = build_graph_with_long_labeled_edge(6);
        // Label is at midpoint rank 3 (chain index 2). Make rank 3 the widest
        // by adding a wide "real" node at that rank.
        let wide_id = NodeId::from("Wide");
        let wide_idx = lg.node_ids.len();
        lg.node_ids.push(wide_id.clone());
        lg.node_index.insert(wide_id, wide_idx);
        lg.ranks.push(3);
        lg.order.push(wide_idx);
        lg.positions.push(Point::default());
        lg.dimensions.push((200.0, 20.0));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);

        switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);
        // Should stay at the same position since rank 3 is widest
        assert_eq!(lg.dummy_chains[0].label_dummy_index, Some(2));
    }

    #[test]
    fn switch_moves_label_to_wider_layer() {
        let mut lg = build_graph_with_long_labeled_edge(6);
        // Label is at midpoint rank 3 (chain index 2). Make rank 4 wider.
        let wide_id = NodeId::from("Wide");
        let wide_idx = lg.node_ids.len();
        lg.node_ids.push(wide_id.clone());
        lg.node_index.insert(wide_id, wide_idx);
        lg.ranks.push(4);
        lg.order.push(wide_idx);
        lg.positions.push(Point::default());
        lg.dimensions.push((200.0, 20.0));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);

        switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);
        // Label should move to chain index 3 (rank 4)
        assert_eq!(lg.dummy_chains[0].label_dummy_index, Some(3));

        // Verify the old label dummy is now Edge type with 0x0 dimensions
        let old_label_id = &lg.dummy_chains[0].dummy_ids[2];
        let old_dummy = lg.dummy_nodes.get(old_label_id).unwrap();
        assert_eq!(old_dummy.dummy_type, normalize::DummyType::Edge);
        assert_eq!(lg.dimensions[lg.node_index[old_label_id]], (0.0, 0.0));

        // Verify the new label dummy is EdgeLabel type with label dimensions
        let new_label_id = &lg.dummy_chains[0].dummy_ids[3];
        let new_dummy = lg.dummy_nodes.get(new_label_id).unwrap();
        assert_eq!(new_dummy.dummy_type, normalize::DummyType::EdgeLabel);
        assert_eq!(lg.dimensions[lg.node_index[new_label_id]], (30.0, 14.0));
    }

    #[test]
    fn get_label_position_uses_updated_chain_index() {
        let mut lg = build_graph_with_long_labeled_edge(6);
        // Make rank 4 wider to trigger a switch from midpoint (rank 3) to rank 4
        let wide_id = NodeId::from("Wide");
        let wide_idx = lg.node_ids.len();
        lg.node_ids.push(wide_id.clone());
        lg.node_index.insert(wide_id, wide_idx);
        lg.ranks.push(4);
        lg.order.push(wide_idx);
        lg.positions.push(Point::default());
        lg.dimensions.push((200.0, 20.0));
        lg.original_has_predecessor.push(false);
        lg.parents.push(None);
        lg.model_order.push(None);

        switch_label_dummies(&mut lg, LabelDummyStrategy::WidestLayer);

        // Set positions for all dummies so get_label_position returns meaningful values
        for (i, id) in lg.dummy_chains[0].dummy_ids.iter().enumerate() {
            let idx = lg.node_index[id];
            lg.positions[idx] = Point {
                x: 10.0,
                y: (i as f64) * 50.0,
            };
        }

        let pos = normalize::get_label_position(&lg, 0).unwrap();
        // After switch, label is at chain index 3 (rank 4), positioned at y=150.0
        // With label dimensions 30x14, center would be at y=150.0+7.0=157.0
        // (actually get_label_position uses LabelSide which defaults to Center)
        let new_label_idx = lg.node_index[&lg.dummy_chains[0].dummy_ids[3]];
        let expected_y = lg.positions[new_label_idx].y + lg.dimensions[new_label_idx].1 / 2.0;
        assert!(
            (pos.point.y - expected_y).abs() < 0.01,
            "Label position y={} should match switched dummy center y={}",
            pos.point.y,
            expected_y
        );
    }

    // select_label_sides tests

    #[test]
    fn select_label_sides_single_label_stays_center() {
        // A -> B -> C with only A->B labeled.
        // After normalization, there's one label dummy at the intermediate rank.
        // Single label in a layer should stay Center.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(30.0, 10.0));

        let config = LayoutConfig {
            per_edge_label_spacing: true,
            ..Default::default()
        };
        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if config.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_labeled_edges(&mut lg, &edge_labels);
        let mut config = config.clone();
        config.rank_sep /= 2.0;
        rank::run(&mut lg, &config);
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &edge_labels, false);
        order::run(&mut lg, false);
        select_label_sides(&mut lg);

        // Find the label dummy and check its side
        for dummy in lg.dummy_nodes.values() {
            if dummy.is_label() {
                assert_eq!(
                    dummy.label_side,
                    normalize::LabelSide::Center,
                    "single label dummy should stay Center"
                );
            }
        }
    }

    #[test]
    fn select_label_sides_two_parallel_labels_get_above_below() {
        // A -> C and B -> C, both labeled. The label dummies for both edges
        // should be in the same layer. One gets Above, the other Below.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "C");
        graph.add_edge("B", "C");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(30.0, 10.0));
        edge_labels.insert(1, normalize::EdgeLabelInfo::new(30.0, 10.0));

        let config = LayoutConfig {
            per_edge_label_spacing: true,
            ..Default::default()
        };
        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        extract_self_edges(&mut lg);
        if config.acyclic {
            acyclic::run(&mut lg);
        }
        make_space_for_labeled_edges(&mut lg, &edge_labels);
        let mut config = config.clone();
        config.rank_sep /= 2.0;
        rank::run(&mut lg, &config);
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &edge_labels, false);
        order::run(&mut lg, false);
        select_label_sides(&mut lg);

        // Collect label dummy sides
        let mut sides: Vec<normalize::LabelSide> = lg
            .dummy_nodes
            .values()
            .filter(|d| d.is_label())
            .map(|d| d.label_side)
            .collect();
        sides.sort_by_key(|s| match s {
            normalize::LabelSide::Above => 0,
            normalize::LabelSide::Center => 1,
            normalize::LabelSide::Below => 2,
        });

        assert_eq!(
            sides,
            vec![normalize::LabelSide::Above, normalize::LabelSide::Below],
            "two label dummies in same layer should get Above and Below"
        );
    }

    #[test]
    fn per_edge_spacing_unlabeled_edge_has_no_dummy() {
        // Graph: A -> B -> C, only A->B has a label.
        // With per_edge_label_spacing=true: B->C keeps minlen=1, so no dummy node
        // (edge_waypoints empty for edge 1). rank_sep is halved for both modes.
        // With per_edge_label_spacing=false: B->C is doubled to minlen=2, creating
        // a dummy node (edge_waypoints populated for edge 1).
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_node("C", (10.0, 10.0));
        graph.add_edge("A", "B"); // edge 0: labeled
        graph.add_edge("B", "C"); // edge 1: unlabeled

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(50.0, 5.0));

        // Per-edge mode: unlabeled edge should NOT get a dummy node
        let config_per_edge = LayoutConfig {
            per_edge_label_spacing: true,
            ..Default::default()
        };
        let result_per_edge =
            layout_with_labels(&graph, &config_per_edge, |_, dims| *dims, &edge_labels);

        // Edge 1 (B->C, unlabeled) should be a short edge — no waypoints
        assert!(
            !result_per_edge.edge_waypoints.contains_key(&1),
            "per-edge: unlabeled B->C should have no waypoints (short edge)"
        );
        // Edge 0 (A->B, labeled) should have waypoints from label dummy
        assert!(
            result_per_edge.edge_waypoints.contains_key(&0)
                || result_per_edge.label_positions.contains_key(&0),
            "per-edge: labeled A->B should have label position"
        );

        // Global mode: unlabeled edge DOES get a dummy node
        let config_global = LayoutConfig::default();
        let result_global =
            layout_with_labels(&graph, &config_global, |_, dims| *dims, &edge_labels);

        // Edge 1 (B->C, unlabeled) gets doubled to minlen=2, creating a dummy
        assert!(
            result_global.edge_waypoints.contains_key(&1),
            "global: unlabeled B->C should have waypoints (minlen doubled)"
        );
    }

    #[test]
    fn test_ranksep_compensates_for_doubled_minlen() {
        // dagre.js halves ranksep when it doubles minlen (makeSpaceForEdgeLabels).
        // With doubled minlen, A→B spans 2 internal ranks with a gap rank between.
        // Halved ranksep (25) means the total spacing = height + 2*(ranksep/2) = 10 + 50 = 60.
        // Without halving, spacing would be height + 2*ranksep = 10 + 100 = 110.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_edge("A", "B");

        let config = LayoutConfig::default(); // rank_sep = 50, margin = 10
        let result = layout(&graph, &config, |_, dims| *dims);

        let a = result.nodes.get(&"A".into()).unwrap();
        let b = result.nodes.get(&"B".into()).unwrap();

        // Expected: dy = height + 2*(rank_sep/2) = 10 + 2*25 = 60
        let dy = b.y - a.y;
        assert!(
            (dy - 60.0).abs() < 0.01,
            "Expected dy=60 (ranksep halved to 25, 2 rank gaps), got dy={}",
            dy
        );
    }

    #[test]
    fn test_bk_allocates_space_for_label_dummy() {
        // Verify that label dummies with non-zero width influence layout spacing
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 3.0));
        graph.add_node("B", (10.0, 3.0));
        graph.add_node("C", (10.0, 3.0));
        // A -> B and A -> C: two parallel edges on same ranks
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");

        // Label on A->B with significant width
        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(50.0, 5.0));

        let config = LayoutConfig::default();
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        // The label position should exist and have a valid x coordinate
        assert!(result.label_positions.contains_key(&0));
        let label_pos = result.label_positions.get(&0).unwrap();

        // Label dummy width (50.0) should be accounted for — the label
        // position should be at a reasonable x coordinate
        let a_rect = result.nodes.get(&"A".into()).unwrap();
        // Label should be in the general vicinity of the edge path
        assert!(
            label_pos.point.x >= 0.0,
            "Label x should be non-negative, got {}",
            label_pos.point.x
        );
        assert!(
            label_pos.point.y > a_rect.y,
            "Label should be below A in TD layout"
        );
    }

    #[test]
    fn test_denorm_extracts_label_position_between_nodes() {
        // A -> B with label: verify label position is geometrically between A and B
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_edge("A", "B");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(50.0, 20.0));

        let config = LayoutConfig::default();
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        assert!(result.label_positions.contains_key(&0));
        let label_pos = result.label_positions.get(&0).unwrap();

        let a_y = result.nodes.get(&"A".into()).unwrap().y;
        let b_y = result.nodes.get(&"B".into()).unwrap().y;
        assert!(
            label_pos.point.y > a_y && label_pos.point.y < b_y,
            "Label y={} should be between A y={} and B y={}",
            label_pos.point.y,
            a_y,
            b_y
        );
    }

    #[test]
    fn test_layout_with_labels_short_edge_gets_label_position() {
        // A -> B (short edge, 1-rank span) with label
        // After make_space, it should span 2 ranks and get a label dummy
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_edge("A", "B"); // Edge 0 - labeled

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(50.0, 20.0));

        let config = LayoutConfig::default();
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        // Short labeled edge should now have a label position
        assert!(
            result.label_positions.contains_key(&0),
            "Short labeled edge should have a label position"
        );
    }

    #[test]
    fn test_layout_with_labels() {
        // A -> B -> C, and A -> C with label
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_node("C", (100.0, 50.0));
        graph.add_edge("A", "B"); // Edge 0
        graph.add_edge("B", "C"); // Edge 1
        graph.add_edge("A", "C"); // Edge 2 - long edge with label

        let mut edge_labels = HashMap::new();
        edge_labels.insert(2, normalize::EdgeLabelInfo::new(50.0, 20.0));

        let config = LayoutConfig::default();
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        // Should have label position for edge 2
        assert!(
            result.label_positions.contains_key(&2),
            "Should have label position for edge 2"
        );

        let label_pos = result.label_positions.get(&2).unwrap();
        // Label should be at an intermediate y position
        let a_y = result.nodes.get(&"A".into()).unwrap().y;
        let c_y = result.nodes.get(&"C".into()).unwrap().y;
        assert!(
            label_pos.point.y > a_y && label_pos.point.y < c_y,
            "Label should be between A and C"
        );
    }

    #[test]
    fn parallel_labeled_edges_get_distinct_sides() {
        // Two edges landing on the same target, both labeled.
        // A -->|left| C
        // B -->|right| C
        // With side selection enabled, labels should have different y offsets.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_node("C", (40.0, 20.0));
        graph.add_edge("A", "C"); // Edge 0 - labeled "left"
        graph.add_edge("B", "C"); // Edge 1 - labeled "right"

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(30.0, 14.0));
        edge_labels.insert(1, normalize::EdgeLabelInfo::new(30.0, 14.0));

        let config = LayoutConfig {
            per_edge_label_spacing: true,
            label_side_selection: true,
            ..Default::default()
        };
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        let left_pos = result
            .label_positions
            .get(&0)
            .expect("edge 0 should have label position");
        let right_pos = result
            .label_positions
            .get(&1)
            .expect("edge 1 should have label position");

        // Labels should have different y-coordinates (one above, one below)
        assert!(
            (left_pos.point.y - right_pos.point.y).abs() > 1.0,
            "labels should be offset from each other: left y={}, right y={}",
            left_pos.point.y,
            right_pos.point.y,
        );

        // Without label_side_selection, labels should be at the same y (both Center)
        let config_no_side = LayoutConfig {
            per_edge_label_spacing: true,
            label_side_selection: false,
            ..Default::default()
        };
        let result_no_side =
            layout_with_labels(&graph, &config_no_side, |_, dims| *dims, &edge_labels);
        let left_no = result_no_side.label_positions.get(&0).unwrap();
        let right_no = result_no_side.label_positions.get(&1).unwrap();
        // Both Center: same rank → same y (within tolerance for different x positions)
        assert!(
            (left_no.point.y - right_no.point.y).abs() < 1.0,
            "without side selection, labels should be near same y: left={}, right={}",
            left_no.point.y,
            right_no.point.y,
        );
    }

    #[test]
    fn test_layout_compound_graph_end_to_end() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("sg1", (0.0, 0.0));
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.set_parent("A", "sg1");
        graph.set_parent("B", "sg1");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        // Nodes should be laid out
        assert!(result.nodes.contains_key(&"A".into()));
        assert!(result.nodes.contains_key(&"B".into()));

        // Subgraph bounds should exist
        assert!(
            result.subgraph_bounds.contains_key("sg1"),
            "Should have subgraph bounds for sg1"
        );
        let bounds = &result.subgraph_bounds["sg1"];
        assert!(
            bounds.width > 0.0,
            "Subgraph width should be positive, got bounds={:?}, all_nodes={:?}",
            bounds,
            result.nodes
        );
        assert!(bounds.height > 0.0, "Subgraph height should be positive");
    }

    #[test]
    fn test_layout_compound_titled_end_to_end() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("sg1", (0.0, 0.0));
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.set_parent("A", "sg1");
        graph.set_parent("B", "sg1");
        graph.set_has_title("sg1");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        assert!(result.nodes.contains_key(&"A".into()));
        assert!(result.nodes.contains_key(&"B".into()));
        assert!(result.subgraph_bounds.contains_key("sg1"));
    }

    #[test]
    fn test_layout_multi_level_compound_nesting() {
        let mut g: DiGraph<(f64, f64)> = DiGraph::new();
        g.add_node("A", (40.0, 20.0));
        g.add_node("B", (40.0, 20.0));
        g.add_node("inner", (0.0, 0.0));
        g.add_node("outer", (0.0, 0.0));
        g.add_edge("A", "B");
        g.set_parent("A", "inner");
        g.set_parent("B", "inner");
        g.set_parent("inner", "outer");

        let config = LayoutConfig::default();
        let result = layout(&g, &config, |_, dims| *dims);

        assert!(result.nodes.contains_key(&"A".into()));
        assert!(result.nodes.contains_key(&"B".into()));
    }

    #[test]
    fn test_layout_simple_graph_no_subgraph_bounds() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        assert!(
            result.subgraph_bounds.is_empty(),
            "Simple graph should have no subgraph bounds"
        );
    }

    // --- Self-edge extraction tests ---

    fn build_lg_from_edges(edges: &[(&str, &str)]) -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        let mut seen = std::collections::HashSet::new();
        for (from, to) in edges {
            if seen.insert(*from) {
                graph.add_node(*from, (10.0, 5.0));
            }
            if seen.insert(*to) {
                graph.add_node(*to, (10.0, 5.0));
            }
            graph.add_edge(*from, *to);
        }
        LayoutGraph::from_digraph(&graph, |_, dims| *dims)
    }

    #[test]
    fn test_extract_self_edges_single() {
        let mut lg = build_lg_from_edges(&[("A", "A")]);
        assert_eq!(lg.edges.len(), 1);
        extract_self_edges(&mut lg);
        assert_eq!(lg.self_edges.len(), 1);
        assert_eq!(lg.self_edges[0].node_index, lg.node_index[&"A".into()]);
        assert!(lg.edges.is_empty(), "self-edge should be removed");
    }

    #[test]
    fn test_extract_self_edges_mixed() {
        let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A"), ("B", "C")]);
        assert_eq!(lg.edges.len(), 3);
        extract_self_edges(&mut lg);
        assert_eq!(lg.self_edges.len(), 1);
        assert_eq!(lg.edges.len(), 2, "only non-self edges remain");
        // Parallel arrays should be in sync
        assert_eq!(lg.edge_weights.len(), 2);
        assert_eq!(lg.edge_minlens.len(), 2);
    }

    #[test]
    fn test_extract_self_edges_none() {
        let mut lg = build_lg_from_edges(&[("A", "B")]);
        extract_self_edges(&mut lg);
        assert!(lg.self_edges.is_empty());
        assert_eq!(lg.edges.len(), 1);
    }

    #[test]
    fn test_extract_self_edges_multiple() {
        let mut lg = build_lg_from_edges(&[("A", "A"), ("B", "B"), ("A", "B")]);
        extract_self_edges(&mut lg);
        assert_eq!(lg.self_edges.len(), 2);
        assert_eq!(lg.edges.len(), 1);
    }

    #[test]
    fn test_layout_with_self_edge_does_not_panic() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 5.0));
        graph.add_edge("A", "A");
        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);
        assert!(result.nodes.contains_key(&"A".into()));
    }

    #[test]
    fn test_insert_self_edge_dummy_creates_node() {
        let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
        extract_self_edges(&mut lg);
        // Simulate ranking and ordering
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        order::run(&mut lg, false);

        let node_count_before = lg.node_ids.len();
        assert_eq!(lg.self_edges.len(), 1);

        insert_self_edge_dummies(&mut lg);

        assert_eq!(lg.node_ids.len(), node_count_before + 1);
        assert!(lg.self_edges[0].dummy_index.is_some());
    }

    #[test]
    fn test_insert_self_edge_dummy_same_rank() {
        let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
        let a_idx = lg.node_index[&"A".into()];
        extract_self_edges(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        order::run(&mut lg, false);

        let node_rank = lg.ranks[a_idx];
        insert_self_edge_dummies(&mut lg);

        let dummy_idx = lg.self_edges[0].dummy_index.unwrap();
        assert_eq!(lg.ranks[dummy_idx], node_rank);
    }

    #[test]
    fn test_insert_self_edge_dummy_order_after_node() {
        let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
        let a_idx = lg.node_index[&"A".into()];
        extract_self_edges(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        order::run(&mut lg, false);

        let node_order = lg.order[a_idx];
        insert_self_edge_dummies(&mut lg);

        let dummy_idx = lg.self_edges[0].dummy_index.unwrap();
        assert_eq!(lg.order[dummy_idx], node_order + 1);
    }

    #[test]
    fn test_layout_result_contains_self_edge_layout() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 5.0));
        graph.add_edge("A", "A");
        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
        assert_eq!(result.self_edges.len(), 1);
        assert_eq!(result.self_edges[0].node, "A".into());
        assert_eq!(result.self_edges[0].points.len(), 6);
    }

    #[test]
    fn test_layout_result_no_self_edges_when_none_exist() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 5.0));
        graph.add_node("B", (10.0, 5.0));
        graph.add_edge("A", "B");
        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
        assert!(result.self_edges.is_empty());
    }

    #[test]
    fn test_position_self_edges_td_produces_6_points() {
        let mut lg = build_lg_from_edges(&[("A", "B"), ("A", "A")]);
        let a_idx = lg.node_index[&"A".into()];
        extract_self_edges(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        order::run(&mut lg, false);
        insert_self_edge_dummies(&mut lg);
        let config = LayoutConfig::default(); // TopBottom
        position::run(&mut lg, &config);
        let layouts = position_self_edges(&lg, &config);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].points.len(), 6);

        // Verify exit from bottom, enter from top (TD)
        let node_pos = lg.positions[a_idx];
        let (_nw, nh) = lg.dimensions[a_idx];
        let bot = node_pos.y + nh;
        let top = node_pos.y;
        assert!(
            layouts[0].points[0].y >= bot - 0.1,
            "first point should exit bottom"
        );
        assert!(
            layouts[0].points[5].y <= top + 0.1,
            "last point should enter top"
        );
    }

    #[test]
    fn test_layout_self_edge_dummy_not_in_result_nodes() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 5.0));
        graph.add_node("B", (10.0, 5.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "A");
        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
        // Dummy should not appear in result nodes
        assert_eq!(result.nodes.len(), 2);
        assert!(result.nodes.contains_key(&"A".into()));
        assert!(result.nodes.contains_key(&"B".into()));
    }

    #[test]
    fn test_layout_self_edge_not_in_reversed_edges() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 5.0));
        graph.add_node("B", (10.0, 5.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "A");
        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);
        // Self-edge orig index is 1. It should not appear in reversed_edges.
        assert!(
            !result.reversed_edges.contains(&1),
            "self-edge should not be in reversed_edges"
        );
    }

    #[test]
    fn test_reversed_edge_endpoints_match_original_direction() {
        // Edge 0: A→B (forward), Edge 1: B→A (reversed for acyclic).
        // After layout, the reversed edge should have from=B, to=A
        // (original direction) and points going from B toward A.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (10.0, 10.0));
        graph.add_node("B", (10.0, 10.0));
        graph.add_edge("A", "B"); // edge 0
        graph.add_edge("B", "A"); // edge 1: will be reversed

        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);

        assert!(!result.reversed_edges.is_empty());
        let rev_idx = result.reversed_edges[0];
        let edge = result.edges.iter().find(|e| e.index == rev_idx).unwrap();

        // The reversed edge's original direction is B→A.
        // After acyclic undo, from/to should reflect that.
        assert_eq!(
            edge.from,
            "B".into(),
            "reversed edge from should be B (original source)"
        );
        assert_eq!(
            edge.to,
            "A".into(),
            "reversed edge to should be A (original target)"
        );

        // Points should be oriented from B toward A.
        // In TD layout, A is above B. B→A goes upward, so first point
        // should be near B (lower y) and last point near A (higher y).
        let b_rect = result.nodes.get(&"B".into()).unwrap();
        let a_rect = result.nodes.get(&"A".into()).unwrap();
        let p_first = edge.points.first().unwrap();
        let p_last = edge.points.last().unwrap();
        let b_cy = b_rect.y + b_rect.height / 2.0;
        let a_cy = a_rect.y + a_rect.height / 2.0;
        assert!(
            (p_first.y - b_cy).abs() < (p_first.y - a_cy).abs(),
            "first point should be closer to B (original source), \
             p_first.y={}, b_cy={}, a_cy={}",
            p_first.y,
            b_cy,
            a_cy
        );
        assert!(
            (p_last.y - a_cy).abs() < (p_last.y - b_cy).abs(),
            "last point should be closer to A (original target), \
             p_last.y={}, a_cy={}, b_cy={}",
            p_last.y,
            a_cy,
            b_cy
        );
    }

    #[test]
    fn test_translate_layout_result_uses_nodes_not_edge_points() {
        // dagre.js translateGraph uses node bounding boxes and edge labels for
        // min/max, NOT individual edge points. Edge points still get shifted,
        // but don't influence the bounding box calculation.
        let mut result = LayoutResult {
            nodes: HashMap::from([(
                "A".into(),
                Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 10.0,
                    height: 10.0,
                },
            )]),
            edges: vec![EdgeLayout {
                from: "A".into(),
                to: "A".into(),
                points: vec![Point { x: -5.0, y: 12.0 }],
                index: 0,
            }],
            reversed_edges: vec![],
            width: 0.0,
            height: 0.0,
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
            label_sides: HashMap::new(),
            subgraph_bounds: HashMap::new(),
            self_edges: vec![],
            rank_to_position: HashMap::new(),
            node_ranks: HashMap::new(),
        };

        translate_layout_result(&mut result, 10.0, 10.0, Direction::TopBottom);

        // Min X = 10 (from node, not edge point at -5). dagre: minX -= marginX => 0.
        // dx = -minX = -0 = 0. Wait: minX=10, minX -= 10 => 0, dx = -0 = 0.
        // Actually: minX=10 (node left), marginX=10. minX -= marginX => 0. dx = 0.
        // Node stays at x=10, edge point stays at -5.
        let rect = result.nodes.get(&"A".into()).unwrap();
        assert!(
            (rect.x - 10.0).abs() < 0.001,
            "node x should stay at 10.0 (min already at margin), got {}",
            rect.x
        );
        // Edge point shifts by same dx=0
        assert!(
            (result.edges[0].points[0].x - (-5.0)).abs() < 0.001,
            "edge point x should stay at -5.0, got {}",
            result.edges[0].points[0].x
        );

        // Width: maxX=20 (node right), minX after margin reduction = 0.
        // width = maxX - minX + marginX = 20 - 0 + 10 = 30
        assert!(
            (result.width - 30.0).abs() < 0.001,
            "width should be 30.0 (margin on both sides), got {}",
            result.width
        );
    }

    #[test]
    fn test_translate_layout_result_shifts_all_fields() {
        use super::normalize::WaypointWithRank;

        let mut result = LayoutResult {
            nodes: HashMap::from([(
                "A".into(),
                Rect {
                    x: 5.0,
                    y: 5.0,
                    width: 10.0,
                    height: 10.0,
                },
            )]),
            edges: vec![EdgeLayout {
                from: "A".into(),
                to: "A".into(),
                points: vec![Point { x: 5.0, y: 5.0 }, Point { x: 20.0, y: 20.0 }],
                index: 0,
            }],
            reversed_edges: vec![],
            width: 0.0,
            height: 0.0,
            edge_waypoints: HashMap::from([(
                0,
                vec![WaypointWithRank {
                    point: Point { x: 12.0, y: 12.0 },
                    rank: 1,
                }],
            )]),
            label_positions: HashMap::from([(
                0,
                WaypointWithRank {
                    point: Point { x: 12.0, y: 12.0 },
                    rank: 1,
                },
            )]),
            label_sides: HashMap::new(),
            subgraph_bounds: HashMap::from([(
                "sg1".to_string(),
                Rect {
                    x: 3.0,
                    y: 3.0,
                    width: 20.0,
                    height: 20.0,
                },
            )]),
            self_edges: vec![],
            rank_to_position: HashMap::new(),
            node_ranks: HashMap::new(),
        };

        translate_layout_result(&mut result, 10.0, 10.0, Direction::TopBottom);

        // Min from nodes: x=5, subgraph: x=3 => minX=3. label: x=12 (not smaller).
        // dagre-style: minX -= marginX => 3 - 10 = -7. dx = -minX = 7.
        // All coords += 7.
        let sg = result.subgraph_bounds.get("sg1").unwrap();
        assert!(
            (sg.x - 10.0).abs() < 0.001,
            "subgraph x should be 10.0, got {}",
            sg.x
        );
        assert!(
            (sg.y - 10.0).abs() < 0.001,
            "subgraph y should be 10.0, got {}",
            sg.y
        );

        // Edge waypoints should be shifted by dx=7
        let wp = &result.edge_waypoints[&0][0];
        assert!(
            (wp.point.x - 19.0).abs() < 0.001,
            "waypoint x should be 19.0, got {}",
            wp.point.x
        );

        // Label position should be shifted by dx=7
        let lp = &result.label_positions[&0];
        assert!(
            (lp.point.x - 19.0).abs() < 0.001,
            "label x should be 19.0, got {}",
            lp.point.x
        );

        // Width/height with margin on both sides:
        // maxX from node = 5+10=15, subgraph = 3+20=23, label = 12. Max=23.
        // minX = 3 (before margin reduction).
        // width = maxX - (minX - marginX) + marginX = 23 - (3 - 10) + 10 = 23 + 7 + 10 = 40
        assert!(
            (result.width - 40.0).abs() < 0.001,
            "width should be 40.0 (margin on both sides), got {}",
            result.width
        );
        assert!(
            (result.height - 40.0).abs() < 0.001,
            "height should be 40.0 (margin on both sides), got {}",
            result.height
        );
    }

    #[test]
    fn test_compound_node_rect_matches_subgraph_bounds() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("sg1", (0.0, 0.0));
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.set_parent("A", "sg1");
        graph.set_parent("B", "sg1");
        graph.set_has_title("sg1");

        let result = layout(&graph, &LayoutConfig::default(), |_, dims| *dims);

        let bounds = &result.subgraph_bounds["sg1"];
        let rect = result.nodes.get(&"sg1".into()).unwrap();

        assert!(
            (rect.x - bounds.x).abs() < 1e-6,
            "compound node x={} should match bounds x={}",
            rect.x,
            bounds.x
        );
        assert!(
            (rect.y - bounds.y).abs() < 1e-6,
            "compound node y={} should match bounds y={}",
            rect.y,
            bounds.y
        );
        assert!(
            (rect.width - bounds.width).abs() < 1e-6,
            "compound node width={} should match bounds width={}",
            rect.width,
            bounds.width
        );
        assert!(
            (rect.height - bounds.height).abs() < 1e-6,
            "compound node height={} should match bounds height={}",
            rect.height,
            bounds.height
        );
    }

    #[test]
    fn test_assign_node_intersects_updates_edge_endpoints() {
        let mut result = LayoutResult {
            nodes: HashMap::from([
                (
                    "A".into(),
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                ),
                (
                    "B".into(),
                    Rect {
                        x: 0.0,
                        y: 30.0,
                        width: 10.0,
                        height: 10.0,
                    },
                ),
            ]),
            edges: vec![EdgeLayout {
                from: "A".into(),
                to: "B".into(),
                points: vec![
                    Point { x: 5.0, y: 5.0 },
                    Point { x: 5.0, y: 20.0 },
                    Point { x: 5.0, y: 35.0 },
                ],
                index: 0,
            }],
            reversed_edges: vec![],
            width: 0.0,
            height: 0.0,
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
            label_sides: HashMap::new(),
            subgraph_bounds: HashMap::new(),
            self_edges: vec![],
            rank_to_position: HashMap::new(),
            node_ranks: HashMap::new(),
        };

        assign_node_intersects(&mut result);

        let edge = &result.edges[0];
        let p_first = edge.points.first().unwrap();
        let p_last = edge.points.last().unwrap();

        // Bottom of A (center y=5, h/2=5) and top of B (center y=35, h/2=5).
        assert!((p_first.x - 5.0).abs() < 0.001);
        assert!((p_first.y - 10.0).abs() < 0.001);
        assert!((p_last.x - 5.0).abs() < 0.001);
        assert!((p_last.y - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_layout_multi_edge_produces_two_edge_layouts() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "B");

        let config = LayoutConfig::default();
        let result = layout(&graph, &config, |_, dims| *dims);

        assert_eq!(
            result.edges.len(),
            2,
            "Should have 2 edge layouts for 2 edges between A and B"
        );
        assert_ne!(
            result.edges[0].index, result.edges[1].index,
            "Edge indices should differ"
        );
    }

    #[test]
    fn test_layout_multi_edge_with_labels() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (40.0, 20.0));
        graph.add_node("B", (40.0, 20.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "B");

        let mut edge_labels = HashMap::new();
        edge_labels.insert(0, normalize::EdgeLabelInfo::new(30.0, 10.0));
        edge_labels.insert(1, normalize::EdgeLabelInfo::new(30.0, 10.0));

        let config = LayoutConfig::default();
        let result = layout_with_labels(&graph, &config, |_, dims| *dims, &edge_labels);

        assert_eq!(result.edges.len(), 2);
        assert!(
            result.label_positions.contains_key(&0),
            "Edge 0 should have a label position"
        );
        assert!(
            result.label_positions.contains_key(&1),
            "Edge 1 should have a label position"
        );
    }
}
