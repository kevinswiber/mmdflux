//! Layout computation for flowchart diagrams.
//!
//! Translates layout float coordinates into ASCII character-grid positions using
//! uniform scale factors, collision repair, and waypoint transformation.

use std::collections::{HashMap, HashSet};

use super::shape::{NodeBounds, node_dimensions};
use crate::graph::{Diagram, Direction, Edge, Node, Shape, Stroke};
#[cfg(test)]
use crate::layered::Point;
use crate::layered::normalize::WaypointWithRank;
use crate::layered::{self, Direction as LayeredDirection, LayoutConfig as LayeredConfig, Rect};

/// Bounding box for a subgraph border in draw coordinates.
#[derive(Debug, Clone)]
pub struct SubgraphBounds {
    /// Left edge x coordinate.
    pub x: usize,
    /// Top edge y coordinate.
    pub y: usize,
    /// Total width including border.
    pub width: usize,
    /// Total height including border.
    pub height: usize,
    /// Display title for the subgraph.
    pub title: String,
    /// Nesting depth (0 = top-level, 1 = nested once, etc.)
    pub depth: usize,
}

/// Draw-coordinate data for a self-edge loop.
#[derive(Debug, Clone)]
pub struct SelfEdgeDrawData {
    /// Node ID the self-edge loops on.
    pub node_id: String,
    /// Original edge index.
    pub edge_index: usize,
    /// Draw-coordinate points for the orthogonal loop.
    pub points: Vec<(usize, usize)>,
}

/// Grid position of a node (layer/column in abstract grid coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPos {
    /// Layer (row for TD/BT, column for LR/RL).
    pub layer: usize,
    /// Position within layer.
    pub pos: usize,
}

/// Coordinate transformation context from layout float coordinates to draw coordinates.
///
/// Encapsulates the scaling, offset, and padding parameters needed to convert
/// the layout engine's floating-point coordinates to integer character-grid positions.
struct CoordTransform<'a> {
    scale_x: f64,
    scale_y: f64,
    layout_min_x: f64,
    layout_min_y: f64,
    max_overhang_x: usize,
    max_overhang_y: usize,
    config: &'a LayoutConfig,
}

impl CoordTransform<'_> {
    /// Convert layout coordinates to draw coordinates.
    fn to_draw(&self, x: f64, y: f64) -> (usize, usize) {
        let dx = ((x - self.layout_min_x) * self.scale_x).round() as isize;
        let dy = ((y - self.layout_min_y) * self.scale_y).round() as isize;
        let draw_x = dx.max(0) as usize
            + self.max_overhang_x
            + self.config.padding
            + self.config.left_label_margin;
        let draw_y = dy.max(0) as usize + self.max_overhang_y + self.config.padding;
        (draw_x, draw_y)
    }
}

/// Layout result containing node positions and canvas dimensions.
#[derive(Debug)]
pub struct Layout {
    /// Node positions in grid coordinates.
    pub grid_positions: HashMap<String, GridPos>,
    /// Node positions in draw coordinates (x, y pixels/chars).
    pub draw_positions: HashMap<String, (usize, usize)>,
    /// Node bounding boxes in draw coordinates.
    pub node_bounds: HashMap<String, NodeBounds>,
    /// Total canvas width needed.
    pub width: usize,
    /// Total canvas height needed.
    pub height: usize,
    /// Spacing between nodes horizontally.
    pub h_spacing: usize,
    /// Spacing between nodes vertically.
    pub v_spacing: usize,

    // --- Edge routing data from normalization ---
    /// Waypoints for each edge, derived from dummy node positions.
    /// Key: edge index in `Diagram::edges`, Value: list of waypoint coordinates.
    /// Empty for short edges (span 1 rank), populated for long edges.
    pub edge_waypoints: HashMap<usize, Vec<(usize, usize)>>,

    /// Pre-computed label positions for edges with labels.
    /// Key: edge index in `Diagram::edges`, Value: (x, y) position for the label center.
    /// Only populated for edges that have labels.
    pub edge_label_positions: HashMap<usize, (usize, usize)>,

    /// Node shapes for intersection calculation.
    /// Maps node ID to its shape for computing dynamic attachment points.
    pub node_shapes: HashMap<String, Shape>,

    /// Subgraph bounding boxes in draw coordinates.
    /// Key: subgraph ID, Value: bounds with title.
    /// Empty for diagrams without subgraphs.
    pub subgraph_bounds: HashMap<String, SubgraphBounds>,

    /// Self-edge loop data in draw coordinates.
    pub self_edges: Vec<SelfEdgeDrawData>,

    /// Effective layout direction per node.
    /// Nodes inside a direction-override subgraph use the subgraph's direction;
    /// other nodes use the diagram's root direction.
    pub node_directions: HashMap<String, Direction>,
}

impl Layout {
    /// Get the bounding box for a node.
    pub fn get_bounds(&self, node_id: &str) -> Option<&NodeBounds> {
        self.node_bounds.get(node_id)
    }

    /// Get the effective layout direction for an edge.
    ///
    /// If both endpoints share the same direction override (e.g. both are in an LR
    /// subgraph), returns that override direction.  Otherwise returns the fallback
    /// (typically the diagram's root direction).
    pub fn effective_edge_direction(&self, from: &str, to: &str, fallback: Direction) -> Direction {
        let src_dir = self.node_directions.get(from).copied().unwrap_or(fallback);
        let tgt_dir = self.node_directions.get(to).copied().unwrap_or(fallback);
        if src_dir == tgt_dir {
            return src_dir;
        }
        // If either endpoint uses the root direction, the edge crosses from an
        // override subgraph to the root part of the diagram — use the root direction.
        if src_dir == fallback || tgt_dir == fallback {
            return fallback;
        }
        // Both endpoints have non-root direction overrides (e.g., LR and BT in
        // nested subgraphs).  Infer direction from geometry.
        match (self.node_bounds.get(from), self.node_bounds.get(to)) {
            (Some(fb), Some(tb)) => {
                let dx = (fb.center_x() as isize - tb.center_x() as isize).unsigned_abs();
                let dy = (fb.center_y() as isize - tb.center_y() as isize).unsigned_abs();
                if dx > dy {
                    if fb.center_x() <= tb.center_x() {
                        Direction::LeftRight
                    } else {
                        Direction::RightLeft
                    }
                } else if dy > 0 {
                    if fb.center_y() <= tb.center_y() {
                        Direction::TopDown
                    } else {
                        Direction::BottomTop
                    }
                } else {
                    fallback
                }
            }
            _ => fallback,
        }
    }
}

/// Configuration for layout computation.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Horizontal spacing between nodes.
    pub h_spacing: usize,
    /// Vertical spacing between nodes.
    pub v_spacing: usize,
    /// Padding around the entire diagram.
    pub padding: usize,
    /// Extra left margin for edge labels on left branches.
    pub left_label_margin: usize,
    /// Extra right margin for edge labels on right branches.
    pub right_label_margin: usize,
    /// Ranking algorithm override.
    pub ranker: Option<crate::layered::types::Ranker>,
    /// Node spacing (nodesep).
    pub node_sep: f64,
    /// Edge segment spacing (edgesep).
    pub edge_sep: f64,
    /// Rank spacing (ranksep).
    pub rank_sep: f64,
    /// Layout margin (applied in translateGraph).
    pub margin: f64,
    /// Additional ranksep applied when subgraphs are present (Mermaid clusters).
    pub cluster_rank_sep: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            h_spacing: 4,
            v_spacing: 3,
            padding: 1,
            left_label_margin: 0,
            right_label_margin: 0,
            ranker: None,
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            margin: 8.0,
            cluster_rank_sep: 25.0,
        }
    }
}

/// Convert a graph-level Direction to a layered Direction.
fn to_layered_direction(dir: Direction) -> LayeredDirection {
    match dir {
        Direction::TopDown => LayeredDirection::TopBottom,
        Direction::BottomTop => LayeredDirection::BottomTop,
        Direction::LeftRight => LayeredDirection::LeftRight,
        Direction::RightLeft => LayeredDirection::RightLeft,
    }
}

/// Pre-computed sub-layout result for a direction-override subgraph.
pub(crate) struct SubLayoutResult {
    /// The LayoutResult with node positions in the sub-layout coordinate system.
    result: layered::LayoutResult,
    /// Map from sublayout edge index to original diagram edge index.
    edge_index_map: Vec<usize>,
}

/// Mermaid-compatible isolation check for sublayout extraction.
///
/// Treat edges that target/source the subgraph endpoint (`to_subgraph` /
/// `from_subgraph`) as cluster-endpoint links, not direction-tainting
/// node-level cross-boundary edges.
fn subgraph_has_tainting_cross_boundary_edges(diagram: &Diagram, sg_id: &str) -> bool {
    let Some(sg) = diagram.subgraphs.get(sg_id) else {
        return false;
    };
    let sg_nodes: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
    diagram.edges.iter().any(|edge| {
        let from_in = sg_nodes.contains(edge.from.as_str());
        let to_in = sg_nodes.contains(edge.to.as_str());
        if from_in == to_in {
            return false;
        }
        let via_sg_endpoint = edge.to_subgraph.as_deref() == Some(sg_id)
            || edge.from_subgraph.as_deref() == Some(sg_id);
        !via_sg_endpoint
    })
}

/// Compute sub-layouts for subgraphs with direction overrides.
///
/// For each subgraph that has a `dir` override, this creates a standalone layered
/// graph with just the subgraph's internal nodes and edges, and runs layout with
/// the overridden direction. The resulting dimensions can be injected into the
/// parent layout so the compound node is sized correctly.
pub(crate) fn compute_sublayouts<FN, FE>(
    diagram: &Diagram,
    parent_layered_config: &LayeredConfig,
    node_dims: FN,
    edge_label_dims: FE,
    skip_non_isolated_overrides: bool,
) -> HashMap<String, SubLayoutResult>
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let mut sublayouts = HashMap::new();

    for (sg_id, sg) in &diagram.subgraphs {
        let sub_dir = match sg.dir {
            Some(d) => d,
            None => continue,
        };

        if skip_non_isolated_overrides && subgraph_has_tainting_cross_boundary_edges(diagram, sg_id)
        {
            continue;
        }

        let layered_direction = to_layered_direction(sub_dir);

        let mut sub_graph: layered::DiGraph<(f64, f64)> = layered::DiGraph::new();

        // Add leaf nodes (not child subgraphs)
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id)
                && let Some(node) = diagram.nodes.get(node_id)
            {
                let (w, h) = node_dims(node);
                sub_graph.add_node(node_id.as_str(), (w, h));
            }
        }

        // Add internal edges only (both endpoints inside this subgraph)
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        let mut edge_labels: HashMap<usize, layered::normalize::EdgeLabelInfo> = HashMap::new();
        let mut edge_index_map = Vec::new();
        for (edge_idx, edge) in diagram.edges.iter().enumerate() {
            if sg_node_set.contains(edge.from.as_str()) && sg_node_set.contains(edge.to.as_str()) {
                let sub_edge_idx = edge_index_map.len();
                edge_index_map.push(edge_idx);
                sub_graph.add_edge(edge.from.as_str(), edge.to.as_str());
                if let Some((label_width, label_height)) = edge_label_dims(edge) {
                    edge_labels.insert(
                        sub_edge_idx,
                        layered::normalize::EdgeLabelInfo::new(label_width, label_height),
                    );
                }
            }
        }

        // Chain disconnected components with synthetic edges so the layout spreads
        // all nodes along the sub-layout's primary axis.  Without this, nodes
        // with no internal edges collapse into rank 0.
        //
        // Find connected components via union-find, then link the last node of
        // each component (in declaration order) to the first node of the next.
        let leaf_ids: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| !diagram.is_subgraph(n) && diagram.nodes.contains_key(n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if leaf_ids.len() > 1 {
            let id_to_idx: HashMap<&str, usize> = leaf_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect();
            let mut parent: Vec<usize> = (0..leaf_ids.len()).collect();
            let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
                while parent[x] != x {
                    parent[x] = parent[parent[x]];
                    x = parent[x];
                }
                x
            };
            for edge in &diagram.edges {
                if let (Some(&a), Some(&b)) = (
                    id_to_idx.get(edge.from.as_str()),
                    id_to_idx.get(edge.to.as_str()),
                ) {
                    let ra = find(&mut parent, a);
                    let rb = find(&mut parent, b);
                    if ra != rb {
                        parent[ra] = rb;
                    }
                }
            }
            // Walk leaf_ids in declaration order; when the component changes,
            // add a synthetic edge from the previous node to the current one.
            let mut prev_component = find(&mut parent, 0);
            for i in 1..leaf_ids.len() {
                let comp = find(&mut parent, i);
                if comp != prev_component {
                    sub_graph.add_edge(leaf_ids[i - 1], leaf_ids[i]);
                    // Merge so subsequent nodes in the same component don't
                    // create redundant edges.
                    let rc = find(&mut parent, comp);
                    let rp = find(&mut parent, prev_component);
                    parent[rc] = rp;
                }
                prev_component = find(&mut parent, i);
            }
        }

        // Use parent config but override direction
        let sub_config = LayeredConfig {
            direction: layered_direction,
            ..parent_layered_config.clone()
        };

        let result =
            layered::layout_with_labels(&sub_graph, &sub_config, |_, dims| *dims, &edge_labels);

        sublayouts.insert(
            sg_id.clone(),
            SubLayoutResult {
                result,
                edge_index_map,
            },
        );
    }

    sublayouts
}

/// Reconcile direction-override sub-layout positions in draw coordinates.
///
/// For each subgraph with a direction override:
/// 1. Get the current subgraph draw bounds (from the main layout's compound pipeline)
/// 2. Convert sub-layout positions to draw coordinates using simple spacing
/// 3. Center the sub-layout's draw positions within the subgraph bounds
/// 4. Override draw_positions, node_bounds, and subgraph_bounds
#[allow(clippy::too_many_arguments)]
fn reconcile_sublayouts_draw(
    diagram: &Diagram,
    config: &LayoutConfig,
    sublayouts: &HashMap<String, SubLayoutResult>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    canvas_width: &mut usize,
    canvas_height: &mut usize,
) {
    // Process sublayouts in deterministic depth order (shallowest first).
    // This ensures that when both parent and child subgraphs have direction
    // overrides, the deeper override writes last and wins.
    let mut sorted_sg_ids: Vec<&String> = sublayouts.keys().collect();
    sorted_sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });

    // Build cross-boundary edge maps for directional padding.
    let parent_map = build_subgraph_parent_map(&diagram.subgraphs);
    let incoming_map = build_subgraph_incoming_map(&diagram.subgraphs, &diagram.edges, &parent_map);
    let outgoing_map = build_subgraph_outgoing_map(&diagram.subgraphs, &diagram.edges, &parent_map);

    for sg_id in sorted_sg_ids {
        let sublayout = &sublayouts[sg_id];
        let sg = &diagram.subgraphs[sg_id];

        // Get the current subgraph draw bounds as the anchor position
        let sg_draw = match subgraph_bounds.get(sg_id) {
            Some(b) => b.clone(),
            None => continue,
        };

        // Compute draw coordinates for sub-layout nodes.
        // Each node's position in the sub-layout is in layout float coords.
        // We convert them to character positions using a simple approach:
        // node draw (x, y) = layout position scaled to fit draw space.
        //
        // For the sub-layout, we use the node dimensions directly and add spacing.
        let sub_dir = sg.dir.unwrap_or(diagram.direction);
        let sub_is_vertical = matches!(sub_dir, Direction::TopDown | Direction::BottomTop);

        // Collect sub-layout node draw positions relative to (0,0)
        let mut sub_draw_nodes: Vec<(String, usize, usize, usize, usize)> = Vec::new();

        // Compute sub-layout-specific scale factors
        let sub_node_dims: HashMap<String, (usize, usize)> = sublayout
            .result
            .nodes
            .iter()
            .filter_map(|(id, _)| {
                diagram
                    .nodes
                    .get(&id.0)
                    .map(|n| (id.0.clone(), node_dimensions(n, sub_dir)))
            })
            .collect();

        let sub_rank_sep = config.rank_sep + config.cluster_rank_sep;
        let (sub_scale_x, sub_scale_y) = compute_ascii_scale_factors(
            &sub_node_dims,
            sub_rank_sep,
            config.node_sep,
            config.v_spacing,
            config.h_spacing,
            sub_is_vertical,
            false,
        );

        // Find sub-layout bounding box min
        let sub_layout_min_x = sublayout
            .result
            .nodes
            .values()
            .map(|r| r.x)
            .fold(f64::INFINITY, f64::min);
        let sub_layout_min_y = sublayout
            .result
            .nodes
            .values()
            .map(|r| r.y)
            .fold(f64::INFINITY, f64::min);

        // Convert each sub-layout node to draw coordinates (relative)
        for (node_id, rect) in &sublayout.result.nodes {
            let (w, h) = match sub_node_dims.get(&node_id.0) {
                Some(&dims) => dims,
                None => continue,
            };

            let cx =
                ((rect.x + rect.width / 2.0 - sub_layout_min_x) * sub_scale_x).round() as usize;
            let cy =
                ((rect.y + rect.height / 2.0 - sub_layout_min_y) * sub_scale_y).round() as usize;
            let x = cx.saturating_sub(w / 2);
            let y = cy.saturating_sub(h / 2);

            sub_draw_nodes.push((node_id.0.clone(), x, y, w, h));
        }

        // Repair overlapping/touching nodes along the primary axis.
        // After scaling and rounding, the leftmost (or topmost) node can clip
        // to position 0 via saturating_sub, collapsing the gap to its neighbor.
        // Push only the affected nodes apart by the minimum amount needed for
        // edge rendering (2 chars: stem + arrowhead).
        let min_gap = 2;
        if !sub_is_vertical {
            sub_draw_nodes.sort_by_key(|n| n.1); // sort by x
            for i in 1..sub_draw_nodes.len() {
                let prev_right = sub_draw_nodes[i - 1].1 + sub_draw_nodes[i - 1].3;
                let needed = prev_right + min_gap;
                if sub_draw_nodes[i].1 < needed {
                    sub_draw_nodes[i].1 = needed;
                }
            }
        } else {
            sub_draw_nodes.sort_by_key(|n| n.2); // sort by y
            for i in 1..sub_draw_nodes.len() {
                let prev_bottom = sub_draw_nodes[i - 1].2 + sub_draw_nodes[i - 1].4;
                let needed = prev_bottom + min_gap;
                if sub_draw_nodes[i].2 < needed {
                    sub_draw_nodes[i].2 = needed;
                }
            }
        }

        if sub_draw_nodes.is_empty() {
            continue;
        }

        // Find the bounding box of the sub-layout in draw coordinates
        let sub_draw_min_x = sub_draw_nodes
            .iter()
            .map(|(_, x, _, _, _)| *x)
            .min()
            .unwrap_or(0);
        let sub_draw_min_y = sub_draw_nodes
            .iter()
            .map(|(_, _, y, _, _)| *y)
            .min()
            .unwrap_or(0);
        let sub_draw_max_x = sub_draw_nodes
            .iter()
            .map(|(_, x, _, w, _)| x + w)
            .max()
            .unwrap_or(0);
        let sub_draw_max_y = sub_draw_nodes
            .iter()
            .map(|(_, _, y, _, h)| y + h)
            .max()
            .unwrap_or(0);

        let sub_draw_w = sub_draw_max_x - sub_draw_min_x;
        let sub_draw_h = sub_draw_max_y - sub_draw_min_y;

        // Padding around sub-layout content within the subgraph border.
        // Each side gets 1 char for the border itself.  An extra spacing row
        // is added only on sides where cross-boundary edges route through,
        // so blank rows are eliminated on sides with no routing.
        let has_incoming = incoming_map.get(sg_id).copied().unwrap_or(false);
        let has_outgoing = outgoing_map.get(sg_id).copied().unwrap_or(false);
        let (top_pad, bottom_pad) = match diagram.direction {
            Direction::TopDown => (
                if has_incoming { 2 } else { 1 },
                if has_outgoing { 2 } else { 1 },
            ),
            Direction::BottomTop => (
                if has_outgoing { 2 } else { 1 },
                if has_incoming { 2 } else { 1 },
            ),
            _ => (2, 2),
        };
        let left_pad = 2;
        let right_pad = 2;

        // Compute the total subgraph bounds needed
        let sg_needed_w = sub_draw_w + left_pad + right_pad;
        let sg_needed_h = sub_draw_h + top_pad + bottom_pad;

        // Enforce title-width minimum
        let min_title_width = if !sg.title.trim().is_empty() {
            sg.title.len() + 6
        } else {
            0
        };
        let sg_final_w = sg_needed_w.max(min_title_width);

        // Use the current subgraph center as the anchor point
        let sg_cx = sg_draw.x + sg_draw.width / 2;
        let sg_cy = sg_draw.y + sg_draw.height / 2;

        // Compute new subgraph bounds centered on the old center
        let new_sg_x = sg_cx.saturating_sub(sg_final_w / 2);
        let new_sg_y = sg_cy.saturating_sub(sg_needed_h / 2);

        // Offset to place sub-layout content within the new subgraph bounds
        let content_x = new_sg_x + left_pad + (sg_final_w - sg_needed_w) / 2;
        let content_y = new_sg_y + top_pad;

        let offset_x = content_x.saturating_sub(sub_draw_min_x);
        let offset_y = content_y.saturating_sub(sub_draw_min_y);

        // Override node positions
        for (node_id, rel_x, rel_y, w, h) in &sub_draw_nodes {
            let final_x = rel_x + offset_x;
            let final_y = rel_y + offset_y;

            draw_positions.insert(node_id.clone(), (final_x, final_y));
            node_bounds.insert(
                node_id.clone(),
                NodeBounds {
                    x: final_x,
                    y: final_y,
                    width: *w,
                    height: *h,
                    layout_center_x: Some(final_x + w / 2),
                    layout_center_y: Some(final_y + h / 2),
                },
            );
        }

        // Update subgraph bounds
        let depth = diagram.subgraph_depth(sg_id);
        subgraph_bounds.insert(
            sg_id.clone(),
            SubgraphBounds {
                x: new_sg_x,
                y: new_sg_y,
                width: sg_final_w,
                height: sg_needed_h,
                title: sg.title.clone(),
                depth,
            },
        );

        // Expand canvas if needed
        *canvas_width = (*canvas_width).max(new_sg_x + sg_final_w + config.padding);
        *canvas_height = (*canvas_height).max(new_sg_y + sg_needed_h + config.padding);
    }
}

/// After draw-coordinate reconciliation, sibling nodes and child subgraphs
/// within a direction-override parent may overlap.  This happens because the
/// parent's sublayout positions its member nodes individually without knowing
/// the final dimensions of child subgraphs (which are reconciled separately).
///
/// For each direction-override parent, detect nodes that overlap with sibling
/// child subgraph bounds and shift the subgraph (and all its contents) away
/// to create separation.
fn resolve_sibling_overlaps_draw(
    diagram: &Diagram,
    node_bounds: &mut HashMap<String, NodeBounds>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    // Build set of nodes that belong to each child subgraph, so we can
    // identify "direct" children of the parent (nodes not in any grandchild).
    let child_sg_nodes: HashMap<&str, HashSet<&str>> = diagram
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.as_str(), sg.nodes.iter().map(|s| s.as_str()).collect()))
        .collect();

    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let sub_dir = sg.dir.unwrap();

        // Find child subgraphs (subgraphs whose parent is this one).
        let child_sgs: Vec<&str> = diagram
            .subgraphs
            .iter()
            .filter(|(_, child)| child.parent.as_deref() == Some(sg_id.as_str()))
            .map(|(id, _)| id.as_str())
            .collect();

        if child_sgs.is_empty() {
            continue;
        }

        // Find direct member nodes (in this subgraph but not inside any child subgraph).
        let direct_nodes: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| {
                !diagram.is_subgraph(n)
                    && !child_sgs.iter().any(|cs| {
                        child_sg_nodes
                            .get(cs)
                            .is_some_and(|set| set.contains(n.as_str()))
                    })
            })
            .map(|s| s.as_str())
            .collect();

        // For each child subgraph, check if any direct node overlaps.
        for child_sg_id in &child_sgs {
            let Some(sg_b) = subgraph_bounds.get(*child_sg_id).cloned() else {
                continue;
            };

            for node_id in &direct_nodes {
                let Some(nb) = node_bounds.get(*node_id) else {
                    continue;
                };

                // Check overlap based on the parent's direction.
                // For LR/RL, the primary axis is x; check if node and subgraph
                // share y-range (cross axis) and overlap on x (primary axis).
                let (shift_x, shift_y) = match sub_dir {
                    Direction::LeftRight | Direction::RightLeft => {
                        // Check y-range overlap (cross axis).
                        let y_overlap = nb.y < sg_b.y + sg_b.height && nb.y + nb.height > sg_b.y;
                        if !y_overlap {
                            continue;
                        }
                        // Check x overlap.
                        let node_right = nb.x + nb.width;
                        if node_right <= sg_b.x {
                            continue; // no overlap
                        }
                        let node_left = nb.x;
                        if node_left >= sg_b.x + sg_b.width {
                            continue; // no overlap
                        }
                        // Node overlaps with subgraph on x.  Determine which
                        // side the node is on and push the subgraph away.
                        let node_cx = nb.x + nb.width / 2;
                        let sg_cx = sg_b.x + sg_b.width / 2;
                        if node_cx < sg_cx {
                            // Node is to the left — push subgraph right.
                            let shift = node_right + 1 - sg_b.x;
                            (shift, 0)
                        } else {
                            // Node is to the right — push subgraph left (shift node right).
                            let shift = sg_b.x + sg_b.width + 1 - nb.x;
                            // Shift the node instead.
                            if let Some(pos) = draw_positions.get_mut(*node_id) {
                                pos.0 += shift;
                            }
                            if let Some(b) = node_bounds.get_mut(*node_id) {
                                b.x += shift;
                                if let Some(ref mut cx) = b.layout_center_x {
                                    *cx += shift;
                                }
                            }
                            continue;
                        }
                    }
                    Direction::TopDown | Direction::BottomTop => {
                        // Check x-range overlap (cross axis).
                        let x_overlap = nb.x < sg_b.x + sg_b.width && nb.x + nb.width > sg_b.x;
                        if !x_overlap {
                            continue;
                        }
                        // Check y overlap.
                        let node_bottom = nb.y + nb.height;
                        if node_bottom <= sg_b.y {
                            continue;
                        }
                        let node_top = nb.y;
                        if node_top >= sg_b.y + sg_b.height {
                            continue;
                        }
                        let node_cy = nb.y + nb.height / 2;
                        let sg_cy = sg_b.y + sg_b.height / 2;
                        if node_cy < sg_cy {
                            let shift = node_bottom + 1 - sg_b.y;
                            (0, shift)
                        } else {
                            let shift = sg_b.y + sg_b.height + 1 - nb.y;
                            if let Some(pos) = draw_positions.get_mut(*node_id) {
                                pos.1 += shift;
                            }
                            if let Some(b) = node_bounds.get_mut(*node_id) {
                                b.y += shift;
                                if let Some(ref mut cy) = b.layout_center_y {
                                    *cy += shift;
                                }
                            }
                            continue;
                        }
                    }
                };

                if shift_x == 0 && shift_y == 0 {
                    continue;
                }

                // Shift the child subgraph and all its contents.
                if let Some(b) = subgraph_bounds.get_mut(*child_sg_id) {
                    b.x += shift_x;
                    b.y += shift_y;
                }
                // Shift nodes inside the child subgraph.
                let child_sg = &diagram.subgraphs[*child_sg_id];
                for member_id in &child_sg.nodes {
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.0 += shift_x;
                        pos.1 += shift_y;
                    }
                    if let Some(b) = node_bounds.get_mut(member_id) {
                        b.x += shift_x;
                        b.y += shift_y;
                        if let Some(ref mut cx) = b.layout_center_x {
                            *cx += shift_x;
                        }
                        if let Some(ref mut cy) = b.layout_center_y {
                            *cy += shift_y;
                        }
                    }
                }
                // Shift grandchild subgraph bounds too.
                for (gc_id, gc_sg) in &diagram.subgraphs {
                    if gc_sg.parent.as_deref() == Some(*child_sg_id)
                        && let Some(b) = subgraph_bounds.get_mut(gc_id)
                    {
                        b.x += shift_x;
                        b.y += shift_y;
                    }
                }
            }
        }
    }
}

/// After sublayout reconciliation and overlap resolution, align direct sibling
/// nodes with their cross-boundary edge targets on the cross-axis of the parent
/// direction.  Without this, a node like C in an LR subgraph may stay vertically
/// aligned with B (top of a BT child subgraph) instead of A (its actual target at
/// the bottom), forcing the C→A edge to route diagonally through B's area.
fn align_cross_boundary_siblings_draw(
    diagram: &Diagram,
    node_bounds: &mut HashMap<String, NodeBounds>,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    let mut affected_parents: HashSet<String> = HashSet::new();

    for (sg_id, sg) in &diagram.subgraphs {
        let Some(sub_dir) = sg.dir else { continue };
        let is_horizontal = matches!(sub_dir, Direction::LeftRight | Direction::RightLeft);

        // Collect nodes that belong to any child subgraph of this parent.
        let child_sg_nodes: HashSet<&str> = diagram
            .subgraphs
            .iter()
            .filter(|(_, child)| child.parent.as_deref() == Some(sg_id.as_str()))
            .flat_map(|(_, child)| child.nodes.iter().map(|s| s.as_str()))
            .collect();

        if child_sg_nodes.is_empty() {
            continue;
        }

        // Direct member nodes: in this subgraph but not inside any child subgraph.
        let direct_nodes: Vec<&str> = sg
            .nodes
            .iter()
            .filter(|n| !diagram.is_subgraph(n) && !child_sg_nodes.contains(n.as_str()))
            .map(|s| s.as_str())
            .collect();

        for node_id in &direct_nodes {
            // Collect cross-boundary edge targets inside child subgraphs.
            let mut target_cross_positions: Vec<usize> = Vec::new();
            for edge in &diagram.edges {
                let target = if edge.from == *node_id && child_sg_nodes.contains(edge.to.as_str()) {
                    Some(edge.to.as_str())
                } else if edge.to == *node_id && child_sg_nodes.contains(edge.from.as_str()) {
                    Some(edge.from.as_str())
                } else {
                    None
                };

                if let Some(target_id) = target
                    && let Some(tb) = node_bounds.get(target_id)
                {
                    if is_horizontal {
                        target_cross_positions.push(tb.y + tb.height / 2);
                    } else {
                        target_cross_positions.push(tb.x + tb.width / 2);
                    }
                }
            }

            if target_cross_positions.is_empty() {
                continue;
            }

            let avg_target =
                target_cross_positions.iter().sum::<usize>() / target_cross_positions.len();
            let Some(nb) = node_bounds.get(*node_id).cloned() else {
                continue;
            };

            if is_horizontal {
                let node_cy = nb.y + nb.height / 2;
                if avg_target == node_cy {
                    continue;
                }
                let new_y = avg_target.saturating_sub(nb.height / 2);
                if let Some(pos) = draw_positions.get_mut(*node_id) {
                    pos.1 = new_y;
                }
                if let Some(b) = node_bounds.get_mut(*node_id) {
                    b.y = new_y;
                    b.layout_center_y = Some(new_y + nb.height / 2);
                }
            } else {
                let node_cx = nb.x + nb.width / 2;
                if avg_target == node_cx {
                    continue;
                }
                let new_x = avg_target.saturating_sub(nb.width / 2);
                if let Some(pos) = draw_positions.get_mut(*node_id) {
                    pos.0 = new_x;
                }
                if let Some(b) = node_bounds.get_mut(*node_id) {
                    b.x = new_x;
                    b.layout_center_x = Some(new_x + nb.width / 2);
                }
            }
            affected_parents.insert(sg_id.clone());
        }
    }

    if affected_parents.is_empty() {
        return;
    }

    // Re-expand bounds only for parent subgraphs where nodes were moved.
    for sg_id in &affected_parents {
        let Some(sg) = diagram.subgraphs.get(sg_id.as_str()) else {
            continue;
        };
        let Some(sb) = subgraph_bounds.get_mut(sg_id.as_str()) else {
            continue;
        };
        let pad = 2usize; // border + spacing
        for node_id in &sg.nodes {
            if diagram.is_subgraph(node_id) {
                continue;
            }
            let Some(nb) = node_bounds.get(node_id.as_str()) else {
                continue;
            };
            let need_left = nb.x.saturating_sub(pad);
            let need_top = nb.y.saturating_sub(pad);
            let need_right = nb.x + nb.width + pad;
            let need_bottom = nb.y + nb.height + pad;

            let title_rows = if !sg.title.trim().is_empty() { 1 } else { 0 };
            let need_top_with_title = need_top.saturating_sub(title_rows);

            let cur_right = sb.x + sb.width;
            let cur_bottom = sb.y + sb.height;
            let new_left = sb.x.min(need_left);
            let new_top = sb.y.min(need_top_with_title);
            let new_right = cur_right.max(need_right);
            let new_bottom = cur_bottom.max(need_bottom);
            sb.x = new_left;
            sb.y = new_top;
            sb.width = new_right.saturating_sub(new_left);
            sb.height = new_bottom.saturating_sub(new_top);
        }
    }
}

pub(crate) fn layered_config_for_layout(diagram: &Diagram, config: &LayoutConfig) -> LayeredConfig {
    let layered_direction = to_layered_direction(diagram.direction);

    let node_sep = config.node_sep;
    let edge_sep = config.edge_sep;
    let mut rank_sep = config.rank_sep;
    if diagram.has_subgraphs() && config.cluster_rank_sep > 0.0 {
        // Mermaid increases ranksep for cluster graphs (ranksep + 25).
        // We apply the offset when subgraphs are present to approximate that behavior.
        rank_sep += config.cluster_rank_sep;
    }

    LayeredConfig {
        direction: layered_direction,
        node_sep,
        edge_sep,
        rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or_default(),
    }
}

/// Reconcile direction-override sub-layouts into a LayoutResult (SVG pipeline).
///
/// This updates node positions, internal edge paths, label positions, and subgraph bounds
/// for subgraphs that override direction.
pub(crate) fn reconcile_sublayouts(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    sublayouts: &HashMap<String, SubLayoutResult>,
    title_pad_y: f64,
    content_pad_y: f64,
) {
    if sublayouts.is_empty() {
        return;
    }

    // Process sublayouts in deterministic depth order (shallowest first).
    let mut sorted_sg_ids: Vec<&String> = sublayouts.keys().collect();
    sorted_sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });

    for sg_id in sorted_sg_ids {
        let sublayout = &sublayouts[sg_id];
        let Some(parent_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        // Compute sublayout bounds from node rects.
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for rect in sublayout.result.nodes.values() {
            min_x = min_x.min(rect.x);
            min_y = min_y.min(rect.y);
            max_x = max_x.max(rect.x + rect.width);
            max_y = max_y.max(rect.y + rect.height);
        }

        if !min_x.is_finite() || !min_y.is_finite() {
            continue;
        }

        let sub_w = (max_x - min_x).max(0.0);
        let sub_h = (max_y - min_y).max(0.0);

        // Use the center of the layout's internal node positions as anchor,
        // not the oversized parent cluster bounds.  The compound node
        // bounds span many ranks for long cross-boundary edges, but the
        // sublayout should sit where the internal nodes were ranked.
        let sg = &diagram.subgraphs[sg_id];
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        let (nodes_cx, nodes_cy) = {
            let mut nx_min = f64::INFINITY;
            let mut ny_min = f64::INFINITY;
            let mut nx_max = f64::NEG_INFINITY;
            let mut ny_max = f64::NEG_INFINITY;
            for (nid, rect) in &layout.nodes {
                if sg_node_set.contains(nid.0.as_str()) {
                    nx_min = nx_min.min(rect.x);
                    ny_min = ny_min.min(rect.y);
                    nx_max = nx_max.max(rect.x + rect.width);
                    ny_max = ny_max.max(rect.y + rect.height);
                }
            }
            if nx_min.is_finite() {
                ((nx_min + nx_max) / 2.0, (ny_min + ny_max) / 2.0)
            } else {
                (
                    parent_bounds.x + parent_bounds.width / 2.0,
                    parent_bounds.y + parent_bounds.height / 2.0,
                )
            }
        };

        let has_title = diagram
            .subgraphs
            .get(sg_id)
            .is_some_and(|sg| !sg.title.trim().is_empty());
        let title_pad = if has_title { title_pad_y } else { 0.0 };

        let final_w = sub_w;
        let final_h = sub_h + title_pad + content_pad_y * 2.0;

        let new_sg_x = nodes_cx - final_w / 2.0;
        let new_sg_y = nodes_cy - final_h / 2.0;

        let offset_x = new_sg_x + (final_w - sub_w) / 2.0 - min_x;
        let offset_y = new_sg_y + content_pad_y + title_pad - min_y;

        // Update node positions for sublayout nodes.
        for (node_id, rect) in &sublayout.result.nodes {
            if let Some(existing) = layout.nodes.get_mut(node_id) {
                *existing = Rect {
                    x: rect.x + offset_x,
                    y: rect.y + offset_y,
                    width: rect.width,
                    height: rect.height,
                };
            }
        }

        // Update subgraph bounds and compound node rect.
        let new_bounds = Rect {
            x: new_sg_x,
            y: new_sg_y,
            width: final_w,
            height: final_h,
        };
        layout.subgraph_bounds.insert(sg_id.clone(), new_bounds);
        if let Some(existing) = layout.nodes.get_mut(&layered::NodeId(sg_id.clone())) {
            *existing = new_bounds;
        }

        // Remap edge paths for internal edges.
        let mut edge_points_by_orig_idx: HashMap<usize, Vec<layered::Point>> = HashMap::new();
        for edge in &sublayout.result.edges {
            if let Some(orig_idx) = sublayout.edge_index_map.get(edge.index) {
                let points: Vec<layered::Point> = edge
                    .points
                    .iter()
                    .map(|p| layered::Point {
                        x: p.x + offset_x,
                        y: p.y + offset_y,
                    })
                    .collect();
                edge_points_by_orig_idx.insert(*orig_idx, points);
            }
        }

        for edge in layout.edges.iter_mut() {
            if let Some(points) = edge_points_by_orig_idx.get(&edge.index) {
                edge.points = points.clone();
            }
        }

        // Remap label positions for internal edges.
        for (sub_idx, pos) in &sublayout.result.label_positions {
            if let Some(orig_idx) = sublayout.edge_index_map.get(*sub_idx) {
                let mut updated = *pos;
                updated.point = layered::Point {
                    x: pos.point.x + offset_x,
                    y: pos.point.y + offset_y,
                };
                layout.label_positions.insert(*orig_idx, updated);
            }
        }

        // Remap self-edge paths for internal edges.
        let mut self_edge_points_by_idx: HashMap<usize, Vec<layered::Point>> = HashMap::new();
        for edge in &sublayout.result.self_edges {
            if let Some(orig_idx) = sublayout.edge_index_map.get(edge.edge_index) {
                let points: Vec<layered::Point> = edge
                    .points
                    .iter()
                    .map(|p| layered::Point {
                        x: p.x + offset_x,
                        y: p.y + offset_y,
                    })
                    .collect();
                self_edge_points_by_idx.insert(*orig_idx, points);
            }
        }

        for edge in layout.self_edges.iter_mut() {
            if let Some(points) = self_edge_points_by_idx.get(&edge.edge_index) {
                edge.points = points.clone();
            }
        }
    }
}

/// Shift external predecessor and successor nodes of subgraphs on the
/// cross-axis so they align with the subgraph center.
///
/// Applies to two kinds of subgraphs:
/// 1. **Direction overrides** (`dir.is_some()`): internal layout is computed
///    separately, so external nodes should align with the subgraph as a whole.
/// 2. **Subgraph-as-node targets**: edges like `Client --> sg1` conceptually
///    connect to the subgraph, not a specific child — centering the external
///    node over the subgraph is the natural position.
///
/// Only the earliest-rank predecessors and latest-rank successors are shifted —
/// intermediate chain nodes are left in place so their internal edges remain
/// intact.
///
/// Edge points connected to shifted nodes are interpolated: full shift at the
/// shifted endpoint, tapering to zero at the unshifted endpoint.
pub(crate) fn center_override_subgraphs(diagram: &Diagram, layout: &mut layered::LayoutResult) {
    // Cross-axis: x for TD/BT, y for LR/RL.
    let horizontal = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);

    // Subgraphs referenced as edge endpoints (subgraph-as-node).
    let sg_as_node_ids: HashSet<&str> = diagram
        .edges
        .iter()
        .filter_map(|e| e.to_subgraph.as_deref())
        .chain(
            diagram
                .edges
                .iter()
                .filter_map(|e| e.from_subgraph.as_deref()),
        )
        .collect();

    // Nodes belonging to any subgraph — these should never be shifted since
    // they are positioned by their own subgraph's layout.
    let all_sg_members: HashSet<&str> = diagram
        .subgraphs
        .values()
        .flat_map(|sg| sg.nodes.iter().map(|s| s.as_str()))
        .collect();

    // Collect all shifts to apply: node_id → (delta on cross-axis, primary-axis distance).
    // When a node is claimed by multiple subgraphs (e.g. it's a successor
    // of one and a predecessor of another), keep the shift from the
    // subgraph whose members are closest on the primary axis.
    let mut node_shifts: HashMap<String, (f64, f64)> = HashMap::new();

    for sg_id in &diagram.subgraph_order {
        let sg = &diagram.subgraphs[sg_id];
        let is_dir_override = sg.dir.is_some();
        let is_sg_as_node = sg_as_node_ids.contains(sg_id.as_str());

        if !is_dir_override && !is_sg_as_node {
            continue;
        }

        let Some(sg_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();

        let sg_center = if horizontal {
            sg_bounds.x + sg_bounds.width / 2.0
        } else {
            sg_bounds.y + sg_bounds.height / 2.0
        };

        // Collect external predecessors: edges entering the subgraph.
        // Two sources:
        // 1. Edges where `to` is a direct member and `from` is outside (direction overrides)
        // 2. Edges where `to_subgraph` references this subgraph (subgraph-as-node)
        let mut predecessors: Vec<String> = Vec::new();
        for edge in &diagram.edges {
            let is_incoming = (is_dir_override
                && sg_node_set.contains(edge.to.as_str())
                && !sg_node_set.contains(edge.from.as_str()))
                || (is_sg_as_node && edge.to_subgraph.as_deref() == Some(sg_id.as_str()));

            if is_incoming
                && edge.from != *sg_id
                && !sg_node_set.contains(edge.from.as_str())
                && !all_sg_members.contains(edge.from.as_str())
                && !predecessors.contains(&edge.from)
            {
                predecessors.push(edge.from.clone());
            }
        }

        // Collect external successors: edges leaving the subgraph.
        let mut successors: Vec<String> = Vec::new();
        for edge in &diagram.edges {
            let is_outgoing = (is_dir_override
                && sg_node_set.contains(edge.from.as_str())
                && !sg_node_set.contains(edge.to.as_str()))
                || (is_sg_as_node && edge.from_subgraph.as_deref() == Some(sg_id.as_str()));

            if is_outgoing
                && edge.to != *sg_id
                && !sg_node_set.contains(edge.to.as_str())
                && !all_sg_members.contains(edge.to.as_str())
                && !successors.contains(&edge.to)
            {
                successors.push(edge.to.clone());
            }
        }

        if predecessors.is_empty() && successors.is_empty() {
            continue;
        }

        // Filter predecessors to earliest-rank only.  This avoids shifting
        // intermediate chain nodes (e.g. Z in C→X→Y→Z→A) which would break
        // the chain's internal edges.
        if predecessors.len() > 1 {
            let min_rank = predecessors
                .iter()
                .filter_map(|id| layout.node_ranks.get(&layered::NodeId(id.clone())).copied())
                .min();

            if let Some(min_rank) = min_rank {
                predecessors.retain(|id| {
                    layout.node_ranks.get(&layered::NodeId(id.clone())).copied() == Some(min_rank)
                });
            }
        }

        // Filter successors to latest-rank only (symmetric with predecessors).
        if successors.len() > 1 {
            let max_rank = successors
                .iter()
                .filter_map(|id| layout.node_ranks.get(&layered::NodeId(id.clone())).copied())
                .max();

            if let Some(max_rank) = max_rank {
                successors.retain(|id| {
                    layout.node_ranks.get(&layered::NodeId(id.clone())).copied() == Some(max_rank)
                });
            }
        }

        // Compute tight member-node bounds on the primary axis.  The layout engine's
        // compound subgraph bounds span all ranks reachable from border nodes,
        // which can be much larger than the actual member nodes.  Use member
        // bounds for the inside_primary check so external nodes at distant
        // ranks are correctly identified as outside.
        let (member_primary_min, member_primary_max) = {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for nid in &sg_node_set {
                if let Some(r) = layout.nodes.get(&layered::NodeId(nid.to_string())) {
                    if horizontal {
                        lo = lo.min(r.y);
                        hi = hi.max(r.y + r.height);
                    } else {
                        lo = lo.min(r.x);
                        hi = hi.max(r.x + r.width);
                    }
                }
            }
            (lo, hi)
        };

        // Shift predecessors and successors toward the subgraph center,
        // but only those outside the subgraph bounds on the primary axis.
        // Nodes at the same rank as internal nodes (e.g. Logs beside
        // Database) would overlap after a cross-axis shift.
        //
        // To avoid collapsing multiple nodes onto the same position, compute
        // a single uniform delta from the group centroid to the subgraph
        // center.  This preserves relative ordering within the group.
        let member_primary_center = (member_primary_min + member_primary_max) / 2.0;
        let pred_set: HashSet<&str> = predecessors.iter().map(|s| s.as_str()).collect();
        let mut eligible: Vec<(String, f64, f64)> = Vec::new(); // (id, cross_center, primary_pos)
        let mut eligible_pred_crosses: Vec<f64> = Vec::new();
        for node_id in predecessors.iter().chain(successors.iter()) {
            if let Some(rect) = layout.nodes.get(&layered::NodeId(node_id.clone())) {
                let node_cy = rect.y + rect.height / 2.0;
                let node_cx = rect.x + rect.width / 2.0;
                let inside_primary = if horizontal {
                    node_cy >= member_primary_min && node_cy <= member_primary_max
                } else {
                    node_cx >= member_primary_min && node_cx <= member_primary_max
                };
                if inside_primary {
                    continue;
                }
                let cross = if horizontal { node_cx } else { node_cy };
                let primary = if horizontal { node_cy } else { node_cx };
                eligible.push((node_id.clone(), cross, primary));
                if pred_set.contains(node_id.as_str()) {
                    eligible_pred_crosses.push(cross);
                }
            }
        }
        if !eligible.is_empty() {
            let centroid = eligible.iter().map(|(_, c, _)| *c).sum::<f64>() / eligible.len() as f64;

            // Determine shift direction: when the external predecessor
            // centroid falls outside the subgraph's cross-axis bounds, the
            // subgraph is mispositioned and should move toward the centroid.
            // When the predecessor centroid is inside the bounds (or there
            // are no predecessors), the subgraph is well-placed and external
            // nodes should shift toward the subgraph center instead.
            // Use predecessors (not successors) for the inside/outside check
            // because predecessor positions drive incoming-edge routing.
            let (sg_cross_min, sg_cross_max) = if horizontal {
                (sg_bounds.x, sg_bounds.x + sg_bounds.width)
            } else {
                (sg_bounds.y, sg_bounds.y + sg_bounds.height)
            };
            let shift_subgraph = is_dir_override && !eligible_pred_crosses.is_empty() && {
                let pred_centroid =
                    eligible_pred_crosses.iter().sum::<f64>() / eligible_pred_crosses.len() as f64;
                pred_centroid < sg_cross_min || pred_centroid > sg_cross_max
            };

            if shift_subgraph {
                // Direction-override subgraph is mispositioned: shift the
                // subgraph toward the external centroid.
                let delta = centroid - sg_center;
                if delta.abs() >= 1.0 {
                    let primary_dist = eligible
                        .iter()
                        .map(|(_, _, p)| (*p - member_primary_center).abs())
                        .fold(f64::INFINITY, f64::min);
                    // Shift all member nodes.
                    for member_id in &sg.nodes {
                        if let Some(&(_, existing_dist)) = node_shifts.get(member_id) {
                            if primary_dist < existing_dist {
                                node_shifts.insert(member_id.clone(), (delta, primary_dist));
                            }
                        } else {
                            node_shifts.insert(member_id.clone(), (delta, primary_dist));
                        }
                    }
                    // Shift this subgraph's bounds.
                    if let Some(bounds) = layout.subgraph_bounds.get_mut(sg_id) {
                        if horizontal {
                            bounds.x += delta;
                        } else {
                            bounds.y += delta;
                        }
                    }
                    // Shift nested child subgraph bounds recursively.
                    let mut children_to_shift: Vec<String> = diagram
                        .subgraph_children(sg_id)
                        .into_iter()
                        .cloned()
                        .collect();
                    let mut idx = 0;
                    while idx < children_to_shift.len() {
                        let child_id = &children_to_shift[idx];
                        if let Some(cb) = layout.subgraph_bounds.get_mut(child_id) {
                            if horizontal {
                                cb.x += delta;
                            } else {
                                cb.y += delta;
                            }
                        }
                        let grandchildren: Vec<String> = diagram
                            .subgraph_children(child_id)
                            .into_iter()
                            .cloned()
                            .collect();
                        children_to_shift.extend(grandchildren);
                        idx += 1;
                    }
                }
            } else {
                // Subgraph-as-node: shift external nodes toward the subgraph
                // center (existing behavior).
                let delta = sg_center - centroid;
                if delta.abs() >= 1.0 {
                    let primary_dist = eligible
                        .iter()
                        .map(|(_, _, p)| (*p - member_primary_center).abs())
                        .fold(f64::INFINITY, f64::min);
                    for (id, _, _) in &eligible {
                        if let Some(&(_, existing_dist)) = node_shifts.get(id) {
                            if primary_dist < existing_dist {
                                node_shifts.insert(id.clone(), (delta, primary_dist));
                            }
                        } else {
                            node_shifts.insert(id.clone(), (delta, primary_dist));
                        }
                    }
                }
            }
        }
    }

    if node_shifts.is_empty() {
        return;
    }

    // Apply node position shifts.
    for (node_id, &(delta, _)) in &node_shifts {
        if let Some(rect) = layout.nodes.get_mut(&layered::NodeId(node_id.clone())) {
            if horizontal {
                rect.x += delta;
            } else {
                rect.y += delta;
            }
        }
    }

    // Adjust edge points: interpolate shift from shifted endpoint to unshifted endpoint.
    for edge in &mut layout.edges {
        let from_delta = node_shifts.get(edge.from.0.as_str()).map(|&(d, _)| d);
        let to_delta = node_shifts.get(edge.to.0.as_str()).map(|&(d, _)| d);

        let n = edge.points.len();
        if n == 0 {
            continue;
        }

        match (from_delta, to_delta) {
            (Some(d1), Some(d2)) => {
                // Both endpoints shifted — interpolate between the two deltas.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        0.5
                    };
                    let shift = d1 * (1.0 - t) + d2 * t;
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (Some(d), None) => {
                // Only source shifted — taper from full shift to zero.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        0.0
                    };
                    let shift = d * (1.0 - t);
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (None, Some(d)) => {
                // Only target shifted — taper from zero to full shift.
                for (i, p) in edge.points.iter_mut().enumerate() {
                    let t = if n > 1 {
                        i as f64 / (n - 1) as f64
                    } else {
                        1.0
                    };
                    let shift = d * t;
                    if horizontal {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
            (None, None) => {}
        }
    }

    // Adjust self-edge points for shifted nodes.
    for se in &mut layout.self_edges {
        if let Some(&(d, _)) = node_shifts.get(se.node.0.as_str()) {
            for p in &mut se.points {
                if horizontal {
                    p.x += d;
                } else {
                    p.y += d;
                }
            }
        }
    }

    // Adjust label positions for edges with shifted endpoints.
    for (key, pos) in &mut layout.label_positions {
        if let Some(diag_edge) = diagram.edges.get(*key) {
            let from_delta = node_shifts.get(diag_edge.from.as_str()).map(|&(d, _)| d);
            let to_delta = node_shifts.get(diag_edge.to.as_str()).map(|&(d, _)| d);
            let avg_delta = match (from_delta, to_delta) {
                (Some(d1), Some(d2)) => (d1 + d2) / 2.0,
                (Some(d), None) | (None, Some(d)) => d / 2.0,
                (None, None) => continue,
            };
            if horizontal {
                pos.point.x += avg_delta;
            } else {
                pos.point.y += avg_delta;
            }
        }
    }
}

/// Expand parent subgraph bounds to encompass all member nodes and child
/// subgraph bounds.
///
/// After sublayout reconciliation and centering, child subgraphs may have been
/// repositioned (e.g., an LR inner subgraph is wider than the layout predicted).
/// This walks subgraphs inner-first and expands each parent's bounds to be the
/// union of its current bounds and all member content.
///
/// `child_margin` adds space between parent and child subgraph borders.
/// In SVG this should match the subgraph padding so borders don't overlap;
/// in the text pipeline pass 0.0 (draw-coordinate expansion handles padding).
///
/// `title_margin` adds extra top space when the parent has a visible title,
/// so the child border doesn't overlap the parent's title text.
pub(crate) fn expand_parent_bounds(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    child_margin: f64,
    title_margin: f64,
) {
    // Process inner-first so child bounds are finalized before parents.
    let order: Vec<&String> = if !diagram.subgraph_order.is_empty() {
        diagram.subgraph_order.iter().collect()
    } else {
        let mut keys: Vec<&String> = diagram.subgraphs.keys().collect();
        keys.sort();
        keys
    };

    for sg_id in &order {
        let sg = match diagram.subgraphs.get(*sg_id) {
            Some(sg) => sg,
            None => continue,
        };
        let Some(current) = layout.subgraph_bounds.get(*sg_id).copied() else {
            continue;
        };
        let recompute_tight = child_margin > 0.0 || title_margin > 0.0;
        let mut min_x = if recompute_tight {
            f64::INFINITY
        } else {
            current.x
        };
        let mut min_y = if recompute_tight {
            f64::INFINITY
        } else {
            current.y
        };
        let mut max_x = if recompute_tight {
            f64::NEG_INFINITY
        } else {
            current.x + current.width
        };
        let mut max_y = if recompute_tight {
            f64::NEG_INFINITY
        } else {
            current.y + current.height
        };
        let mut found_content = !recompute_tight;

        // Check member nodes.
        for member_id in &sg.nodes {
            if let Some(rect) = layout.nodes.get(&layered::NodeId(member_id.clone())) {
                found_content = true;
                min_x = min_x.min(rect.x);
                min_y = min_y.min(rect.y);
                max_x = max_x.max(rect.x + rect.width);
                max_y = max_y.max(rect.y + rect.height);
            }
        }

        // Check child subgraph bounds (subgraphs whose parent is this subgraph).
        // Add child_margin so the parent border sits outside the child border.
        // In expansion-only mode (text path), preserve the previous title-margin behavior.
        let has_title = !sg.title.trim().is_empty();
        let top_margin = if recompute_tight {
            child_margin
        } else {
            child_margin + if has_title { title_margin } else { 0.0 }
        };
        for (child_sg_id, child_sg) in &diagram.subgraphs {
            if child_sg.parent.as_deref() == Some(sg_id.as_str())
                && let Some(child_bounds) = layout.subgraph_bounds.get(child_sg_id)
            {
                found_content = true;
                min_x = min_x.min(child_bounds.x - child_margin);
                min_y = min_y.min(child_bounds.y - top_margin);
                max_x = max_x.max(child_bounds.x + child_bounds.width + child_margin);
                max_y = max_y.max(child_bounds.y + child_bounds.height + child_margin);
            }
        }

        // Empty subgraph: preserve current bounds if we have nothing to recompute from.
        if !found_content {
            continue;
        }

        if recompute_tight && has_title {
            min_y -= title_margin;
        }

        if let Some(bounds) = layout.subgraph_bounds.get_mut(*sg_id) {
            bounds.x = min_x;
            bounds.y = min_y;
            bounds.width = max_x - min_x;
            bounds.height = max_y - min_y;
        }
        if let Some(node_rect) = layout.nodes.get_mut(&layered::NodeId((*sg_id).clone())) {
            node_rect.x = min_x;
            node_rect.y = min_y;
            node_rect.width = max_x - min_x;
            node_rect.height = max_y - min_y;
        }
    }
}

/// Push external nodes that overlap with reconciled subgraph bounds downward.
///
/// After sublayout reconciliation, the subgraph may now occupy space where the layout
/// placed external nodes.  This shifts those nodes (and everything below them)
/// down to maintain a minimum gap.
pub(crate) fn resolve_sublayout_overlaps(
    diagram: &Diagram,
    layout: &mut layered::LayoutResult,
    min_gap: f64,
) {
    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let Some(sg_bounds) = layout.subgraph_bounds.get(sg_id).copied() else {
            continue;
        };

        let sg_bottom = sg_bounds.y + sg_bounds.height;
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        // Treat nested subgraph compound nodes as internal to this subgraph.
        // `sg.nodes` contains leaf members, so without this descendants are
        // misclassified as external blockers during overlap resolution.
        let mut internal_subgraph_ids: HashSet<&str> = HashSet::new();
        let mut stack = vec![sg_id.as_str()];
        while let Some(parent_id) = stack.pop() {
            for (child_id, child) in &diagram.subgraphs {
                if child.parent.as_deref() == Some(parent_id)
                    && internal_subgraph_ids.insert(child_id.as_str())
                {
                    stack.push(child_id.as_str());
                }
            }
        }

        // Find the maximum shift needed to clear overlapping external nodes.
        let mut max_shift = 0.0f64;
        for (nid, rect) in &layout.nodes {
            if sg_node_set.contains(nid.0.as_str())
                || nid.0 == *sg_id
                || internal_subgraph_ids.contains(nid.0.as_str())
            {
                continue;
            }
            // Only consider nodes whose center is below the subgraph center
            // (i.e., they should be below the subgraph, not above it).
            let node_cy = rect.y + rect.height / 2.0;
            let sg_cy = sg_bounds.y + sg_bounds.height / 2.0;
            if node_cy > sg_cy && rect.y < sg_bottom + min_gap {
                let needed = sg_bottom + min_gap - rect.y;
                if needed > max_shift {
                    max_shift = needed;
                }
            }
        }

        if max_shift < 0.01 {
            continue;
        }

        // Shift all nodes below the subgraph center.
        let sg_cy = sg_bounds.y + sg_bounds.height / 2.0;
        for (nid, rect) in layout.nodes.iter_mut() {
            if sg_node_set.contains(nid.0.as_str())
                || nid.0 == *sg_id
                || internal_subgraph_ids.contains(nid.0.as_str())
            {
                continue;
            }
            if rect.y + rect.height / 2.0 > sg_cy {
                rect.y += max_shift;
            }
        }

        // Shift edge points that are below the subgraph center.
        for edge in &mut layout.edges {
            for point in &mut edge.points {
                if point.y > sg_cy {
                    point.y += max_shift;
                }
            }
        }

        // Shift self-edge points.
        for se in &mut layout.self_edges {
            for point in &mut se.points {
                if point.y > sg_cy {
                    point.y += max_shift;
                }
            }
        }

        // Shift label positions.
        for pos in layout.label_positions.values_mut() {
            if pos.point.y > sg_cy {
                pos.point.y += max_shift;
            }
        }

        // Shift sibling subgraph bounds that are below the pushing subgraph.
        let sibling_ids: Vec<String> = layout
            .subgraph_bounds
            .keys()
            .filter(|id| *id != sg_id)
            .cloned()
            .collect();
        for sibling_id in sibling_ids {
            if internal_subgraph_ids.contains(sibling_id.as_str()) {
                continue;
            }
            if let Some(bounds) = layout.subgraph_bounds.get_mut(&sibling_id)
                && bounds.y + bounds.height / 2.0 > sg_cy
            {
                bounds.y += max_shift;
            }
        }

        // Update layout height.
        layout.height += max_shift;
    }
}

fn build_layered_layout_with_config<FN, FE>(
    diagram: &Diagram,
    layered_config: &LayeredConfig,
    node_dims: FN,
    edge_label_dims: FE,
) -> layered::LayoutResult
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let mut dgraph = layered::DiGraph::new();

    let mut seen = std::collections::HashSet::new();
    let mut ordered_node_ids = Vec::new();
    for edge in &diagram.edges {
        for node_id in [&edge.from, &edge.to] {
            if seen.insert(node_id.clone()) {
                ordered_node_ids.push(node_id.clone());
            }
        }
    }
    let mut node_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_keys.sort();
    for id in node_keys {
        if seen.insert(id.clone()) {
            ordered_node_ids.push(id.clone());
        }
    }

    for id in &ordered_node_ids {
        if let Some(node) = diagram.nodes.get(id) {
            let dims = node_dims(node);
            dgraph.add_node(id.as_str(), dims);
        }
    }

    // Add subgraph compound nodes in reverse parse order (Mermaid parity).
    // subgraph_order is post-order (inner-first); reversing gives outer-first,
    // matching Mermaid's getData() insertion order.
    // Falls back to sorted keys for manually constructed Diagrams without subgraph_order.
    let subgraph_keys: Vec<&String> = if !diagram.subgraph_order.is_empty() {
        diagram.subgraph_order.iter().rev().collect()
    } else {
        let mut keys: Vec<&String> = diagram.subgraphs.keys().collect();
        keys.sort();
        keys
    };
    for sg_id in &subgraph_keys {
        let sg = &diagram.subgraphs[*sg_id];
        dgraph.add_node(sg_id.as_str(), (0.0, 0.0));
        if !sg.title.trim().is_empty() {
            dgraph.set_has_title(sg_id.as_str());
        }
    }

    // Set parent relationships for compound nodes
    let mut node_parent_keys: Vec<&String> = diagram.nodes.keys().collect();
    node_parent_keys.sort();
    for node_id in node_parent_keys {
        let node = &diagram.nodes[node_id];
        if let Some(ref parent) = node.parent {
            dgraph.set_parent(node_id.as_str(), parent.as_str());
        }
    }

    // Set parent relationships for nested subgraphs
    for sg_id in &subgraph_keys {
        let sg = &diagram.subgraphs[*sg_id];
        if let Some(ref parent_id) = sg.parent {
            dgraph.set_parent(sg_id.as_str(), parent_id.as_str());
        }
    }

    // Edges internal to a true direction-override subgraph are handled by the
    // sub-layout, not the main compound layout. Including them here would
    // force the subgraph to span multiple ranks along the root direction,
    // producing excessive spacing that persists even after reconciliation.
    //
    // A subgraph `dir` that equals its effective parent direction is treated as
    // inherited direction, not an override.
    let dir_override_internal: HashSet<usize> = {
        fn effective_parent_direction(diagram: &Diagram, sg: &crate::graph::Subgraph) -> Direction {
            let mut current = sg.parent.as_deref();
            while let Some(parent_id) = current {
                let Some(parent) = diagram.subgraphs.get(parent_id) else {
                    break;
                };
                if let Some(dir) = parent.dir {
                    return dir;
                }
                current = parent.parent.as_deref();
            }
            diagram.direction
        }

        let mut set = HashSet::new();
        for sg in diagram.subgraphs.values() {
            let Some(sub_dir) = sg.dir else {
                continue;
            };
            if sub_dir == effective_parent_direction(diagram, sg) {
                continue;
            }
            let members: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
            for (idx, edge) in diagram.edges.iter().enumerate() {
                if members.contains(edge.from.as_str()) && members.contains(edge.to.as_str()) {
                    set.insert(idx);
                }
            }
        }
        set
    };

    let mut edge_labels: HashMap<usize, layered::normalize::EdgeLabelInfo> = HashMap::new();
    for (edge_idx, edge) in diagram.edges.iter().enumerate() {
        let weight = if edge.stroke == Stroke::Invisible {
            0.0
        } else {
            1.0
        };
        if dir_override_internal.contains(&edge_idx) {
            // Keep connectivity but don't force rank separation.
            dgraph.add_edge_full(edge.from.as_str(), edge.to.as_str(), weight, 0);
        } else {
            dgraph.add_edge_full(edge.from.as_str(), edge.to.as_str(), weight, edge.minlen);
        }
        if let Some((label_width, label_height)) = edge_label_dims(edge) {
            edge_labels.insert(
                edge_idx,
                layered::normalize::EdgeLabelInfo::new(label_width, label_height),
            );
        }
    }

    let result =
        layered::layout_with_labels(&dgraph, layered_config, |_, dims| *dims, &edge_labels);

    if std::env::var("MMDFLUX_DEBUG_NODE_POS").is_ok_and(|v| v == "1") {
        for (id, rect) in &result.nodes {
            eprintln!(
                "[layered_nodes] {} x={:.2} y={:.2} w={:.2} h={:.2}",
                id.0, rect.x, rect.y, rect.width, rect.height
            );
        }
    }

    result
}

pub(crate) fn build_layered_layout<FN, FE>(
    diagram: &Diagram,
    config: &LayoutConfig,
    node_dims: FN,
    edge_label_dims: FE,
) -> layered::LayoutResult
where
    FN: Fn(&Node) -> (f64, f64),
    FE: Fn(&Edge) -> Option<(f64, f64)>,
{
    let layered_config = layered_config_for_layout(diagram, config);
    build_layered_layout_with_config(diagram, &layered_config, node_dims, edge_label_dims)
}

fn text_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Compute the layout using the layered algorithm with direct coordinate translation.
///
/// This uses uniform scale factors to translate the layout's float coordinates to ASCII
/// character cells, replacing the stagger pipeline. The 3-step process:
/// 1. Compute per-axis scale factors
/// 2. Apply uniform scaling + rounding to all layout coordinates
/// 3. Enforce minimum spacing via collision repair
pub fn compute_layout_direct(diagram: &Diagram, config: &LayoutConfig) -> Layout {
    // --- Phase A: Build layered graph ---
    let layered_config = layered_config_for_layout(diagram, config);
    let layered_direction = layered_config.direction;

    // Pre-compute sub-layouts for subgraphs with direction overrides.
    let direction = diagram.direction;
    let sublayouts = compute_sublayouts(
        diagram,
        &layered_config,
        |node| {
            let (w, h) = node_dimensions(node, direction);
            (w as f64, h as f64)
        },
        |edge| {
            edge.label
                .as_ref()
                .map(|label| text_edge_label_dimensions(label))
        },
        false,
    );

    let mut result = build_layered_layout_with_config(
        diagram,
        &layered_config,
        |node| {
            let (w, h) = node_dimensions(node, direction);
            (w as f64, h as f64)
        },
        |edge| {
            edge.label
                .as_ref()
                .map(|label| text_edge_label_dimensions(label))
        },
    );

    // Shift external predecessors of direction-override subgraphs to align
    // with the subgraph center, before coordinate transformation.
    center_override_subgraphs(diagram, &mut result);
    expand_parent_bounds(diagram, &mut result, 0.0, 0.0);

    // Convert post-processed LayoutResult to engine-agnostic GraphGeometry.
    // From this point on, phases read from `geom` (and `engine_hints` for
    // rank-annotated data) instead of the raw `result`.
    let geom = super::super::geometry::from_layered_layout(&result, diagram);
    let engine_hints = match &geom.engine_hints {
        Some(super::super::geometry::EngineHints::Layered(h)) => h,
        _ => unreachable!("layered adapter always produces layered hints"),
    };

    // --- Phase B: Group nodes into layers ---
    let is_vertical = matches!(diagram.direction, Direction::TopDown | Direction::BottomTop);

    // Collect subgraph IDs to exclude from layer grouping (compound nodes are not rendered as nodes)
    let subgraph_ids: std::collections::HashSet<&str> =
        diagram.subgraphs.keys().map(|s| s.as_str()).collect();

    let mut layer_coords: Vec<(String, f64, f64)> = geom
        .nodes
        .iter()
        .filter(|(id, _)| !subgraph_ids.contains(id.as_str()))
        .map(|(id, pos_node)| {
            let primary = if is_vertical {
                pos_node.rect.y
            } else {
                pos_node.rect.x
            };
            let secondary = if is_vertical {
                pos_node.rect.x
            } else {
                pos_node.rect.y
            };
            (id.clone(), primary, secondary)
        })
        .collect();
    layer_coords.sort_by(|a, b| a.1.total_cmp(&b.1));

    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut current_layer: Vec<String> = Vec::new();
    let mut last_primary: Option<f64> = None;
    for (id, primary, _) in &layer_coords {
        if let Some(last) = last_primary
            && (*primary - last).abs() > 25.0
            && !current_layer.is_empty()
        {
            layers.push(std::mem::take(&mut current_layer));
        }
        current_layer.push(id.clone());
        last_primary = Some(*primary);
    }
    if !current_layer.is_empty() {
        layers.push(current_layer);
    }

    let secondary_coord = |id: &String| -> f64 {
        geom.nodes
            .get(id)
            .map(|n| if is_vertical { n.rect.x } else { n.rect.y })
            .unwrap_or(0.0)
    };
    for layer in &mut layers {
        layer.sort_by(|a, b| secondary_coord(a).total_cmp(&secondary_coord(b)));
    }

    let grid_positions = compute_grid_positions(&layers);

    // --- Phase C: Compute node dimensions ---
    let node_dims: HashMap<String, (usize, usize)> = diagram
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node_dimensions(node, direction)))
        .collect();

    // --- Phase D: Scale layout coordinates to ASCII ---
    // The layered layout halves rank_sep when it doubles minlen (matching dagre.js
    // makeSpaceForEdgeLabels), so layout positions are already compact. No
    // render-side scale compensation is needed: pass ranks_doubled=false so the
    // scale formula uses the original rank_sep directly.
    // However, minlen IS still doubled, so waypoints at odd layout ranks still
    // need interpolation in the layer_starts map (ranks_doubled_for_layers=true).
    let ranks_doubled_for_scale = false;
    let ranks_doubled_for_layers = true;
    let (scale_x, scale_y) = compute_ascii_scale_factors(
        &node_dims,
        layered_config.rank_sep,
        layered_config.node_sep,
        config.v_spacing,
        config.h_spacing,
        is_vertical,
        ranks_doubled_for_scale,
    );

    // Find layout bounding box min
    let mut layout_min_x = geom
        .nodes
        .values()
        .map(|n| n.rect.x)
        .fold(f64::INFINITY, f64::min);
    let mut layout_min_y = geom
        .nodes
        .values()
        .map(|n| n.rect.y)
        .fold(f64::INFINITY, f64::min);

    if !geom.subgraphs.is_empty() {
        let sg_min_x = geom
            .subgraphs
            .values()
            .map(|sg| sg.rect.x)
            .fold(f64::INFINITY, f64::min);
        let sg_min_y = geom
            .subgraphs
            .values()
            .map(|sg| sg.rect.y)
            .fold(f64::INFINITY, f64::min);
        layout_min_x = layout_min_x.min(sg_min_x);
        layout_min_y = layout_min_y.min(sg_min_y);
    }

    if std::env::var("MMDFLUX_DEBUG_MIN_X").is_ok_and(|v| v == "1") {
        eprintln!(
            "[min_x] layout_min_x={:.2} layout_min_y={:.2}",
            layout_min_x, layout_min_y
        );
    }

    // Scale each node's center, then compute top-left.
    // First pass: compute raw centers and find the maximum overhang
    // (how much a node's half-width exceeds its raw center coordinate).
    // This prevents clipping to zero, which would destroy the relative
    // separations computed by the layout algorithm (e.g., BK stagger).
    //
    // Lesson: rendering pipeline bugs can silently mask
    // correct layout output. The original saturating_sub here clipped wide
    // left-positioned nodes to x=0, collapsing BK-computed stagger. The fix
    // is a uniform coordinate-space translation that preserves all relative
    // separations. When debugging layout issues, check the rendering pipeline
    // first — the layout algorithm may already be correct.
    let mut raw_centers: Vec<RawCenter> = Vec::new();
    let mut max_overhang_x: usize = 0;
    let mut max_overhang_y: usize = 0;

    for (node_id, pos_node) in &geom.nodes {
        if let Some(&(w, h)) = node_dims.get(node_id) {
            let cx = ((pos_node.rect.x + pos_node.rect.width / 2.0 - layout_min_x) * scale_x)
                .round() as usize;
            let cy = ((pos_node.rect.y + pos_node.rect.height / 2.0 - layout_min_y) * scale_y)
                .round() as usize;
            if w / 2 > cx {
                max_overhang_x = max_overhang_x.max(w / 2 - cx);
            }
            if h / 2 > cy {
                max_overhang_y = max_overhang_y.max(h / 2 - cy);
            }
            raw_centers.push(RawCenter {
                id: node_id.clone(),
                cx,
                cy,
                w,
                h,
            });
        }
    }

    // Second pass: apply overhang offset and compute draw positions
    let mut draw_positions: HashMap<String, (usize, usize)> = HashMap::new();
    let mut node_bounds: HashMap<String, NodeBounds> = HashMap::new();

    for rc in &raw_centers {
        let center_x = rc.cx + max_overhang_x;
        let center_y = rc.cy + max_overhang_y;

        let x = center_x - rc.w / 2 + config.padding + config.left_label_margin;
        let y = center_y - rc.h / 2 + config.padding;

        draw_positions.insert(rc.id.clone(), (x, y));
        node_bounds.insert(
            rc.id.clone(),
            NodeBounds {
                x,
                y,
                width: rc.w,
                height: rc.h,
                layout_center_x: Some(center_x + config.padding + config.left_label_margin),
                layout_center_y: Some(center_y + config.padding),
            },
        );
    }

    // --- Phase E: Collision repair ---
    // Within-layer (cross-axis) repair
    collision_repair(
        &layers,
        &mut draw_positions,
        &node_dims,
        is_vertical,
        if is_vertical {
            config.h_spacing
        } else {
            config.v_spacing
        },
    );
    // Between-layer (primary-axis) repair: ensure minimum gap for edge routing
    rank_gap_repair(
        &layers,
        &mut draw_positions,
        &node_dims,
        is_vertical,
        if is_vertical {
            config.v_spacing
        } else {
            config.h_spacing
        },
    );

    // Update node_bounds after collision repair
    for (id, &(x, y)) in &draw_positions {
        if let Some(&(w, h)) = node_dims.get(id) {
            // Preserve layout center from the initial pass
            let prev = node_bounds.get(id);
            let layout_center_x = prev.and_then(|b| b.layout_center_x);
            let layout_center_y = prev.and_then(|b| b.layout_center_y);
            node_bounds.insert(
                id.clone(),
                NodeBounds {
                    x,
                    y,
                    width: w,
                    height: h,
                    layout_center_x,
                    layout_center_y,
                },
            );
        }
    }

    // --- Phase F: Compute canvas size ---
    // Add margin for synthetic backward-edge routing around nodes
    let has_backward_edges = !geom.reversed_edges.is_empty();
    let backward_margin = if has_backward_edges {
        super::router::BACKWARD_ROUTE_GAP + 2
    } else {
        0
    };

    let base_width = node_bounds
        .values()
        .map(|b| b.x + b.width)
        .max()
        .unwrap_or(0)
        + config.padding
        + config.right_label_margin;
    let base_height = node_bounds
        .values()
        .map(|b| b.y + b.height)
        .max()
        .unwrap_or(0)
        + config.padding;

    // For TD/BT, backward edges route to the right; for LR/RL, below
    let (width, height) = if is_vertical {
        (base_width + backward_margin, base_height)
    } else {
        (base_width, base_height + backward_margin)
    };

    // --- Phase G: Build layout-rank → draw-coordinate mapping ---
    // Use actual node_bounds to compute layer positions, ensuring waypoints are positioned
    // relative to where nodes are actually rendered (not scaled layout positions).
    //
    // For each rank with user nodes, compute the extent (start, end) on the primary axis
    // from the actual node_bounds. For dummy ranks (no user nodes), interpolate between
    // neighboring real node ranks.
    let rank_to_actual_bounds: HashMap<i32, (usize, usize)> = {
        let mut rank_bounds: HashMap<i32, (usize, usize)> = HashMap::new();
        for (node_id, &rank) in &engine_hints.node_ranks {
            if let Some(bounds) = node_bounds.get(node_id) {
                let (start, end) = if is_vertical {
                    (bounds.y, bounds.y + bounds.height)
                } else {
                    (bounds.x, bounds.x + bounds.width)
                };
                rank_bounds
                    .entry(rank)
                    .and_modify(|(s, e)| {
                        *s = (*s).min(start);
                        *e = (*e).max(end);
                    })
                    .or_insert((start, end));
            }
        }
        rank_bounds
    };

    // Build layer_starts as a Vec indexed by layout rank.
    // Real node ranks use the actual node bounds extent.
    // Missing ranks (e.g., dummy/label ranks) interpolate between the nearest neighbors.
    let max_rank = engine_hints
        .node_ranks
        .values()
        .copied()
        .max()
        .unwrap_or(0)
        .max(0) as usize;

    // Helper: find nearest lower rank that has actual bounds
    let find_lower_bound = |rank: i32| -> Option<(i32, usize)> {
        (0..rank)
            .rev()
            .find_map(|r| rank_to_actual_bounds.get(&r).map(|&(_, end)| (r, end)))
    };

    // Helper: find nearest upper rank that has actual bounds
    let find_upper_bound = |rank: i32, max: i32| -> Option<(i32, usize)> {
        ((rank + 1)..=max).find_map(|r| rank_to_actual_bounds.get(&r).map(|&(start, _)| (r, start)))
    };

    let layer_starts: Vec<usize> = (0..=max_rank)
        .map(|rank| {
            let rank_i32 = rank as i32;
            if let Some(&(start, _end)) = rank_to_actual_bounds.get(&rank_i32) {
                // Real node rank — use its actual draw position
                start
            } else {
                // Dummy/label rank — interpolate between nearest actual bounds
                let lower = find_lower_bound(rank_i32);
                let upper = find_upper_bound(rank_i32, max_rank as i32);

                match (lower, upper) {
                    (Some((lower_rank, lower_end)), Some((upper_rank, upper_start))) => {
                        // Linearly interpolate between lower_end and upper_start
                        let rank_span = upper_rank - lower_rank;
                        let rank_offset = rank_i32 - lower_rank;
                        let pos_span = upper_start as i32 - lower_end as i32;
                        (lower_end as i32 + (pos_span * rank_offset) / rank_span) as usize
                    }
                    (Some((_, lower_end)), None) => lower_end,
                    (None, Some((_, upper_start))) => upper_start,
                    (None, None) => 0,
                }
            }
        })
        .collect();

    // --- Phase H: Transform waypoints and labels ---
    let ctx = TransformContext {
        layout_min_x,
        layout_min_y,
        scale_x,
        scale_y,
        padding: config.padding,
        left_label_margin: config.left_label_margin,
        overhang_x: max_overhang_x,
        overhang_y: max_overhang_y,
    };

    // Transient adapter glue: convert LayeredHints back to WaypointWithRank for
    // the transform functions. This conversion will be removed when the transform
    // functions are updated to consume geometry IR types directly (Plan 0055).
    let edge_waypoints_raw: HashMap<usize, Vec<WaypointWithRank>> = engine_hints
        .edge_waypoints
        .iter()
        .map(|(&idx, wps)| {
            (
                idx,
                wps.iter()
                    .map(|(fp, rank)| WaypointWithRank {
                        point: layered::Point { x: fp.x, y: fp.y },
                        rank: *rank,
                    })
                    .collect(),
            )
        })
        .collect();
    let label_positions_raw: HashMap<usize, WaypointWithRank> = engine_hints
        .label_positions
        .iter()
        .map(|(&idx, (fp, rank))| {
            (
                idx,
                WaypointWithRank {
                    point: layered::Point { x: fp.x, y: fp.y },
                    rank: *rank,
                },
            )
        })
        .collect();

    if std::env::var("MMDFLUX_DEBUG_WAYPOINTS").is_ok_and(|v| v == "1") {
        eprintln!("[node_ranks] {:?}", engine_hints.node_ranks);
        eprintln!("[rank_to_actual_bounds] {:?}", rank_to_actual_bounds);
        eprintln!("[layer_starts] {:?}", layer_starts);
        for (edge_idx, wps) in &edge_waypoints_raw {
            if let Some(edge) = diagram.edges.get(*edge_idx) {
                eprintln!(
                    "[raw layout waypoints] {} -> {}: {:?}",
                    edge.from, edge.to, wps
                );
            }
        }
    }

    let edge_waypoints_converted = transform_waypoints_direct(
        &edge_waypoints_raw,
        &diagram.edges,
        &ctx,
        &layer_starts,
        is_vertical,
        width,
        height,
    );

    if std::env::var("MMDFLUX_DEBUG_WAYPOINTS").is_ok_and(|v| v == "1") {
        for (edge_idx, waypoints) in &edge_waypoints_converted {
            if let Some(edge) = diagram.edges.get(*edge_idx) {
                eprintln!("[waypoints] {} -> {}: {:?}", edge.from, edge.to, waypoints);
            }
        }
    }

    let mut edge_label_positions_converted = transform_label_positions_direct(
        &label_positions_raw,
        &diagram.edges,
        &ctx,
        &layer_starts,
        is_vertical,
        width,
        height,
    );

    // --- Phase I: Strip layout waypoints from backward edges ---
    // When ranks are doubled (labels present), backward edges get inflated layout
    // waypoints from normalization dummies that create tall vertical columns.
    // Strip them so the router falls through to synthetic compact routing via
    // generate_backward_waypoints().
    let mut edge_waypoints_final = edge_waypoints_converted;
    const BACKWARD_WAYPOINT_STRIP_THRESHOLD: usize = 6;
    if ranks_doubled_for_layers && is_vertical {
        for edge in &diagram.edges {
            if let (Some(from_b), Some(to_b)) =
                (node_bounds.get(&edge.from), node_bounds.get(&edge.to))
                && super::router::is_backward_edge(from_b, to_b, diagram.direction)
                && edge_waypoints_final
                    .get(&edge.index)
                    .is_some_and(|wps| wps.len() >= BACKWARD_WAYPOINT_STRIP_THRESHOLD)
            {
                edge_waypoints_final.remove(&edge.index);
            }
        }
    }

    // --- Phase I.5: Nudge waypoints that collide with nodes ---
    nudge_colliding_waypoints(
        &mut edge_waypoints_final,
        &node_bounds,
        is_vertical,
        width,
        height,
    );

    // --- Phase J: Collect node shapes ---
    let node_shapes: HashMap<String, Shape> = diagram
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node.shape))
        .collect();

    // --- Phase K: Convert subgraph bounds to draw coordinates ---
    let coord_transform = CoordTransform {
        scale_x,
        scale_y,
        layout_min_x,
        layout_min_y,
        max_overhang_x,
        max_overhang_y,
        config,
    };
    // Transient adapter glue: convert GraphGeometry subgraphs back to layered Rect map
    // for subgraph_bounds_to_draw. Will be removed in Plan 0055.
    let layout_sg_bounds: HashMap<String, Rect> = geom
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.clone(), sg.rect.into()))
        .collect();
    let mut subgraph_bounds =
        subgraph_bounds_to_draw(&diagram.subgraphs, &layout_sg_bounds, &coord_transform);
    debug_compare_subgraph_bounds(
        &diagram.subgraphs,
        &subgraph_bounds,
        &layout_sg_bounds,
        &coord_transform,
    );
    shrink_subgraph_vertical_gaps(
        &diagram.subgraphs,
        &diagram.edges,
        &node_bounds,
        &mut subgraph_bounds,
        diagram.direction,
    );
    shrink_subgraph_horizontal_gaps(
        &diagram.subgraphs,
        &diagram.edges,
        &node_bounds,
        &mut subgraph_bounds,
        diagram.direction,
    );
    debug_subgraph_gaps(&diagram.subgraphs, &node_bounds, &subgraph_bounds);

    // --- Phase L: Compute self-edge loop paths in draw coordinates ---
    // We use node bounds directly rather than transforming layout-space loop points,
    // because the layout gap (1.0) would collapse to 0 after ASCII scaling.
    let self_edges: Vec<SelfEdgeDrawData> = geom
        .self_edges
        .iter()
        .filter_map(|se| {
            let bounds = node_bounds.get(&se.node_id)?;
            let loop_extent = 3; // how far the loop extends beyond the node edge

            // The layout places self-edge loops on the right face (TD/BT) or
            // bottom face (LR/RL), matching the "order" dimension where the
            // dummy node is placed after the self-edge node.
            let points = match layered_direction {
                LayeredDirection::TopBottom => {
                    // Loop on right face: exit top-right, loop right, enter bottom-right
                    let right = bounds.x + bounds.width;
                    let loop_x = right + loop_extent;
                    let top_y = bounds.y;
                    let bot_y = bounds.y + bounds.height - 1;
                    vec![
                        (right, top_y),  // exit right face at top
                        (loop_x, top_y), // go right
                        (loop_x, bot_y), // go down
                        (right, bot_y),  // enter right face at bottom
                    ]
                }
                LayeredDirection::BottomTop => {
                    // Loop on right face: exit bottom-right, loop right, enter top-right
                    let right = bounds.x + bounds.width;
                    let loop_x = right + loop_extent;
                    let top_y = bounds.y;
                    let bot_y = bounds.y + bounds.height - 1;
                    vec![
                        (right, bot_y),  // exit right face at bottom
                        (loop_x, bot_y), // go right
                        (loop_x, top_y), // go up
                        (right, top_y),  // enter right face at top
                    ]
                }
                LayeredDirection::LeftRight => {
                    // Loop on bottom face: exit bottom-right, loop down, enter bottom-left
                    let bot = bounds.y + bounds.height;
                    let loop_y = bot + loop_extent;
                    let left_x = bounds.x;
                    let right_x = bounds.x + bounds.width - 1;
                    vec![
                        (right_x, bot),    // exit bottom face at right
                        (right_x, loop_y), // go down
                        (left_x, loop_y),  // go left
                        (left_x, bot),     // enter bottom face at left
                    ]
                }
                LayeredDirection::RightLeft => {
                    // Loop on bottom face: exit bottom-left, loop down, enter bottom-right
                    let bot = bounds.y + bounds.height;
                    let loop_y = bot + loop_extent;
                    let left_x = bounds.x;
                    let right_x = bounds.x + bounds.width - 1;
                    vec![
                        (left_x, bot),     // exit bottom face at left
                        (left_x, loop_y),  // go down
                        (right_x, loop_y), // go right
                        (right_x, bot),    // enter bottom face at right
                    ]
                }
            };

            Some(SelfEdgeDrawData {
                node_id: se.node_id.clone(),
                edge_index: se.edge_index,
                points,
            })
        })
        .collect();

    // Expand canvas to fit subgraph borders (which extend beyond member nodes)
    let mut width = width;
    let mut height = height;
    for sb in subgraph_bounds.values() {
        width = width.max(sb.x + sb.width + config.padding);
        height = height.max(sb.y + sb.height + config.padding);
    }

    // Expand canvas to fit self-edge loops
    for se in &self_edges {
        for &(x, y) in &se.points {
            width = width.max(x + config.padding + 1);
            height = height.max(y + config.padding + 1);
        }
    }

    // --- Phase M: Direction-override sub-layout reconciliation in draw coordinates ---
    // For subgraphs with direction overrides, compute sub-layout positions in draw
    // coordinates and override the main layout's positions.
    if !sublayouts.is_empty() {
        reconcile_sublayouts_draw(
            diagram,
            config,
            &sublayouts,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
            &mut width,
            &mut height,
        );

        // Re-expand parent bounds after reconciliation repositioned children.
        expand_parent_subgraph_bounds(&diagram.subgraphs, &mut subgraph_bounds);

        // Push sibling nodes and child subgraphs apart when they overlap.
        resolve_sibling_overlaps_draw(
            diagram,
            &mut node_bounds,
            &mut draw_positions,
            &mut subgraph_bounds,
        );

        // Align sibling nodes with their cross-boundary edge targets so
        // edges don't have to route diagonally through child subgraph contents.
        align_cross_boundary_siblings_draw(
            diagram,
            &mut node_bounds,
            &mut draw_positions,
            &mut subgraph_bounds,
        );

        // Re-expand parent bounds after alignment may have moved nodes.
        expand_parent_subgraph_bounds(&diagram.subgraphs, &mut subgraph_bounds);

        // --- Phase N: Ensure external-edge spacing for direction-override subgraphs ---
        // Before clipping waypoints, shift direction-override subgraphs (border +
        // member nodes) so there is room for horizontal edge routing between
        // external predecessor/successor nodes and the subgraph border.
        // Must run before waypoint clipping so the clip uses the shifted border.
        ensure_external_edge_spacing(
            diagram,
            &mut draw_positions,
            &mut node_bounds,
            &mut subgraph_bounds,
        );

        // Invalidate or adjust waypoints and label positions for edges touching
        // direction-override subgraphs. The main layout computed these for
        // the parent direction; after reconciliation the node positions have
        // moved, so the old waypoints are stale.
        //
        // - Internal edges (both endpoints inside the override) have their
        //   waypoints removed so the router uses the subgraph's direction.
        // - Cross-boundary edges keep waypoints but clip them to the subgraph
        //   border so they don't run through internal nodes.
        // - Labels for all touched edges are removed to avoid stale positions.
        for sg in diagram.subgraphs.values() {
            if sg.dir.is_none() {
                continue;
            }
            let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
            for edge in &diagram.edges {
                let from_in = sg_node_set.contains(edge.from.as_str());
                let to_in = sg_node_set.contains(edge.to.as_str());
                if !(from_in || to_in) {
                    continue;
                }
                let key = edge.index;
                if from_in && to_in {
                    edge_waypoints_final.remove(&key);
                } else if let Some(bounds) = subgraph_bounds.get(&sg.id)
                    && let Some(wps) = edge_waypoints_final.get(&key).cloned()
                {
                    let clipped = clip_waypoints_to_subgraph(&wps, bounds, from_in, to_in);
                    // For incoming edges, check if the clipped waypoints
                    // end on the wrong subgraph face. After the centering
                    // shift, waypoints can end up on a side border when
                    // the source is above/below (or vice versa). When the
                    // entry face doesn't match the source position, the
                    // waypoints are stale and should be removed.
                    if to_in && !from_in {
                        let stale = node_bounds.get(&edge.from).is_some_and(|src_nb| {
                            clipped.last().is_some_and(|last| {
                                let src_cy = src_nb.y + src_nb.height / 2;
                                let src_cx = src_nb.x + src_nb.width / 2;
                                let on_top = last.1 == bounds.y;
                                let on_bottom =
                                    last.1 == bounds.y + bounds.height.saturating_sub(1);
                                let on_left = last.0 == bounds.x;
                                let on_right = last.0 == bounds.x + bounds.width.saturating_sub(1);
                                // Source above → expect top entry
                                (src_cy < bounds.y && !on_top)
                                // Source below → expect bottom entry
                                || (src_cy > bounds.y + bounds.height && !on_bottom)
                                // Source left → expect left entry
                                || (src_cx < bounds.x && !on_left)
                                // Source right → expect right entry
                                || (src_cx > bounds.x + bounds.width && !on_right)
                            })
                        });
                        if stale {
                            edge_waypoints_final.remove(&key);
                        } else {
                            edge_waypoints_final.insert(key, clipped);
                        }
                    } else {
                        edge_waypoints_final.insert(key, clipped);
                    }
                }
                edge_label_positions_converted.remove(&key);
            }
        }
    }

    // Re-expand canvas to fit any subgraphs/nodes shifted by Phase N.
    for sb in subgraph_bounds.values() {
        width = width.max(sb.x + sb.width + config.padding);
        height = height.max(sb.y + sb.height + config.padding);
    }
    for nb in node_bounds.values() {
        width = width.max(nb.x + nb.width + config.padding);
        height = height.max(nb.y + nb.height + config.padding);
    }

    let node_directions = geom.node_directions.clone();

    Layout {
        grid_positions,
        draw_positions,
        node_bounds,
        width,
        height,
        h_spacing: config.h_spacing,
        v_spacing: config.v_spacing,
        edge_waypoints: edge_waypoints_final,
        edge_label_positions: edge_label_positions_converted,
        node_shapes,
        subgraph_bounds,
        self_edges,
        node_directions,
    }
}

/// Assign grid positions to nodes based on layers.
fn compute_grid_positions(layers: &[Vec<String>]) -> HashMap<String, GridPos> {
    let mut positions = HashMap::new();

    for (layer_idx, layer) in layers.iter().enumerate() {
        for (pos_idx, node_id) in layer.iter().enumerate() {
            positions.insert(
                node_id.clone(),
                GridPos {
                    layer: layer_idx,
                    pos: pos_idx,
                },
            );
        }
    }

    positions
}

/// Compute per-axis ASCII scale factors for translating layout float coordinates
/// to character grid positions.
///
/// Returns `(scale_x, scale_y)` where each factor maps layout coordinate deltas
/// to ASCII character deltas along that axis.
///
/// For vertical layouts (TD/BT):
///   - scale_y (primary) = (max_h + v_spacing) / (max_h + rank_sep)
///   - scale_x (cross)   = (avg_w + h_spacing) / (avg_w + node_sep)
///
/// For horizontal layouts (LR/RL):
///   - scale_x (primary) = (max_w + h_spacing) / (max_w + rank_sep)
///   - scale_y (cross)   = (avg_h + v_spacing) / (avg_h + node_sep)
fn compute_ascii_scale_factors(
    node_dims: &HashMap<String, (usize, usize)>,
    rank_sep: f64,
    node_sep: f64,
    v_spacing: usize,
    h_spacing: usize,
    is_vertical: bool,
    ranks_doubled: bool,
) -> (f64, f64) {
    let (total_w, total_h, max_w, max_h, count) = node_dims.values().fold(
        (0usize, 0usize, 0usize, 0usize, 0usize),
        |(tw, th, mw, mh, c), &(w, h)| (tw + w, th + h, mw.max(w), mh.max(h), c + 1),
    );
    let count_f = count.max(1) as f64;
    let avg_w = total_w as f64 / count_f;
    let avg_h = total_h as f64 / count_f;

    if is_vertical {
        // When ranks are doubled, the layout positions nodes 2× further apart.
        // To compensate exactly, we need: eff_rs = max_h + 2 * rank_sep
        // This gives scale_primary_new = scale_primary_old / 2, so that
        // (2 * rank_sep) * scale_new = rank_sep * scale_old.
        let effective_rank_sep = if ranks_doubled {
            max_h as f64 + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_h as f64 + v_spacing as f64) / (max_h as f64 + effective_rank_sep);
        let scale_cross = (avg_w + h_spacing as f64) / (avg_w + node_sep);
        (scale_cross, scale_primary)
    } else {
        let effective_rank_sep = if ranks_doubled {
            max_w as f64 + 2.0 * rank_sep
        } else {
            rank_sep
        };
        let scale_primary = (max_w as f64 + h_spacing as f64) / (max_w as f64 + effective_rank_sep);
        let scale_cross = (avg_h + v_spacing as f64) / (avg_h + node_sep);
        (scale_primary, scale_cross)
    }
}

/// Enforce minimum spacing between adjacent nodes within each layer after
/// scaling and rounding.
///
/// Nodes are sorted by their cross-axis position within each layer, then
/// scanned left-to-right (or top-to-bottom for horizontal layouts). If any
/// adjacent pair overlaps or is too close, the later node is pushed forward.
/// This cascades: pushing node B may cause it to overlap C, which also gets pushed.
///
/// For vertical layouts (`is_vertical = true`), the cross-axis is X.
/// For horizontal layouts (`is_vertical = false`), the cross-axis is Y.
fn collision_repair(
    layers: &[Vec<String>],
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_dims: &HashMap<String, (usize, usize)>,
    is_vertical: bool,
    min_gap: usize,
) {
    for layer in layers {
        if layer.len() <= 1 {
            continue;
        }

        let mut sorted: Vec<String> = layer.clone();
        sorted.sort_by_key(|id| {
            let &(x, y) = &draw_positions[id];
            if is_vertical { x } else { y }
        });

        for i in 1..sorted.len() {
            let prev_id = &sorted[i - 1];
            let curr_id = &sorted[i];
            let &(pw, ph) = &node_dims[prev_id];
            let (prev_x, prev_y) = draw_positions[prev_id];
            let (curr_x, curr_y) = draw_positions[curr_id];

            if is_vertical {
                let min_x = prev_x + pw + min_gap;
                if curr_x < min_x {
                    draw_positions.insert(curr_id.clone(), (min_x, curr_y));
                }
            } else {
                let min_y = prev_y + ph + min_gap;
                if curr_y < min_y {
                    draw_positions.insert(curr_id.clone(), (curr_x, min_y));
                }
            }
        }
    }
}

/// Enforce minimum spacing between adjacent layers along the primary axis.
///
/// For vertical layouts, layers stack along Y; for horizontal, along X.
/// If the closest node in the next layer is too close to the farthest node
/// in the previous layer, shift the entire next layer (and all subsequent layers)
/// forward to maintain the minimum gap.
fn rank_gap_repair(
    layers: &[Vec<String>],
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_dims: &HashMap<String, (usize, usize)>,
    is_vertical: bool,
    min_gap: usize,
) {
    if layers.len() <= 1 {
        return;
    }

    for i in 1..layers.len() {
        // Find the maximum primary-axis extent of the previous layer
        let prev_max = layers[i - 1]
            .iter()
            .filter_map(|id| {
                let &(x, y) = draw_positions.get(id)?;
                let &(w, h) = node_dims.get(id)?;
                Some(if is_vertical { y + h } else { x + w })
            })
            .max()
            .unwrap_or(0);

        // Find the minimum primary-axis position in the current layer
        let curr_min = layers[i]
            .iter()
            .filter_map(|id| {
                let &(x, y) = draw_positions.get(id)?;
                Some(if is_vertical { y } else { x })
            })
            .min()
            .unwrap_or(0);

        let required = prev_max + min_gap;
        if curr_min < required {
            let shift = required - curr_min;
            // Shift all nodes in this layer and all subsequent layers
            for layer in &layers[i..] {
                for id in layer {
                    if let Some(&(x, y)) = draw_positions.get(id) {
                        let new_pos = if is_vertical {
                            (x, y + shift)
                        } else {
                            (x + shift, y)
                        };
                        draw_positions.insert(id.clone(), new_pos);
                    }
                }
            }
        }
    }
}

/// Intermediate result for a node's scaled center and dimensions, used between
/// the overhang-detection pass and the draw-position pass.
struct RawCenter {
    id: String,
    cx: usize,
    cy: usize,
    w: usize,
    h: usize,
}

/// Build a map from parent subgraph ID to list of direct child subgraph IDs.
#[cfg(test)]
fn build_children_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
) -> HashMap<String, Vec<String>> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for sg in subgraphs.values() {
        if let Some(ref parent_id) = sg.parent {
            children
                .entry(parent_id.clone())
                .or_default()
                .push(sg.id.clone());
        }
    }
    children
}

/// Convert subgraph member-node positions to draw-coordinate SubgraphBounds.
///
/// Uses inside-out (bottom-up) computation: leaf subgraphs first, then parents
/// expand to contain their children. This ensures proper nesting of bounds.
fn subgraph_bounds_to_draw(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    layout_bounds: &HashMap<String, Rect>,
    transform: &CoordTransform,
) -> HashMap<String, SubgraphBounds> {
    let mut bounds: HashMap<String, SubgraphBounds> = HashMap::new();

    for (sg_id, rect) in layout_bounds {
        let sg = match subgraphs.get(sg_id) {
            Some(sg) => sg,
            None => continue,
        };

        let (x0, y0) = transform.to_draw(rect.x, rect.y);
        let (x1, y1) = transform.to_draw(rect.x + rect.width, rect.y + rect.height);

        let mut final_x = x0;
        let mut final_width = x1.saturating_sub(x0);
        let final_height = y1.saturating_sub(y0);

        // Enforce title-width minimum: ┌─ Title ─┐
        // Overhead: 2 corners + "─ " prefix (2) + " ─" suffix (2) = 6
        let has_visible_title = !sg.title.trim().is_empty();
        let min_title_width = if has_visible_title {
            sg.title.len() + 6
        } else {
            0
        };
        if min_title_width > 0 && final_width < min_title_width {
            let expand = min_title_width - final_width;
            final_x = final_x.saturating_sub(expand / 2);
            final_width = min_title_width;
        }

        // Compute nesting depth by walking parent chain
        let mut depth = 0;
        let mut cur = sg_id.as_str();
        while let Some(s) = subgraphs.get(cur) {
            if let Some(ref p) = s.parent {
                depth += 1;
                cur = p;
            } else {
                break;
            }
        }

        bounds.insert(
            sg_id.clone(),
            SubgraphBounds {
                x: final_x,
                y: y0,
                width: final_width,
                height: final_height,
                title: sg.title.clone(),
                depth,
            },
        );
    }

    expand_parent_subgraph_bounds(subgraphs, &mut bounds);

    bounds
}

fn debug_compare_subgraph_bounds(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    computed: &HashMap<String, SubgraphBounds>,
    layout_bounds: &HashMap<String, Rect>,
    transform: &CoordTransform,
) {
    if !std::env::var("MMDFLUX_DEBUG_SUBGRAPH_BOUNDS").is_ok_and(|v| v == "1") {
        return;
    }

    let mut ids: HashSet<String> = HashSet::new();
    ids.extend(subgraphs.keys().cloned());
    ids.extend(computed.keys().cloned());
    ids.extend(layout_bounds.keys().cloned());

    eprintln!("[subgraph_bounds] comparing computed vs layout-derived");
    let mut ids: Vec<String> = ids.into_iter().collect();
    ids.sort();
    for id in ids {
        let computed_bounds = computed.get(&id);
        let layout_rect = layout_bounds.get(&id);
        if computed_bounds.is_none() && layout_rect.is_none() {
            continue;
        }

        if let Some(rect) = layout_rect {
            eprintln!(
                "[subgraph_bounds] raw {} = ({:.2}, {:.2}, {:.2}, {:.2})",
                id, rect.x, rect.y, rect.width, rect.height
            );
        }

        let layout_draw = layout_rect.map(|rect| {
            let (x0, y0) = transform.to_draw(rect.x, rect.y);
            let (x1, y1) = transform.to_draw(rect.x + rect.width, rect.y + rect.height);
            (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
        });

        let computed_tuple = computed_bounds.map(|b| (b.x, b.y, b.width, b.height));

        eprintln!(
            "[subgraph_bounds] {} computed={:?} layout={:?}",
            id, computed_tuple, layout_draw
        );
    }
}

fn debug_subgraph_gaps(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &HashMap<String, SubgraphBounds>,
) {
    if !std::env::var("MMDFLUX_DEBUG_SUBGRAPH_GAPS").is_ok_and(|v| v == "1") {
        return;
    }

    eprintln!("[subgraph_gaps] top-border to content gaps");

    for (sg_id, sg) in subgraphs {
        let Some(bounds) = subgraph_bounds.get(sg_id) else {
            continue;
        };

        let mut min_y: Option<usize> = None;
        let mut max_y: Option<usize> = None;
        for member in &sg.nodes {
            if let Some(node) = node_bounds.get(member) {
                let node_bottom = node.y.saturating_add(node.height.saturating_sub(1));
                min_y = Some(min_y.map_or(node.y, |cur| cur.min(node.y)));
                max_y = Some(max_y.map_or(node_bottom, |cur| cur.max(node_bottom)));
                continue;
            }
            if let Some(child_bounds) = subgraph_bounds.get(member) {
                let child_bottom = child_bounds
                    .y
                    .saturating_add(child_bounds.height.saturating_sub(1));
                min_y = Some(min_y.map_or(child_bounds.y, |cur| cur.min(child_bounds.y)));
                max_y = Some(max_y.map_or(child_bottom, |cur| cur.max(child_bottom)));
            }
        }

        let (Some(min_y), Some(max_y)) = (min_y, max_y) else {
            continue;
        };

        let content_top = bounds.y.saturating_add(1); // inside top border row
        let content_bottom = bounds.y.saturating_add(bounds.height.saturating_sub(2)); // inside bottom border row
        let top_gap = min_y.saturating_sub(content_top);
        let bottom_gap = content_bottom.saturating_sub(max_y);

        eprintln!(
            "[subgraph_gaps] {} top={} min_y={} max_y={} top_gap={} bottom_gap={}",
            sg_id, bounds.y, min_y, max_y, top_gap, bottom_gap
        );
    }
}

fn shrink_subgraph_vertical_gaps(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    direction: Direction,
) {
    let parent_map = build_subgraph_parent_map(subgraphs);
    let incoming_map = build_subgraph_incoming_map(subgraphs, edges, &parent_map);
    let outgoing_map = build_subgraph_outgoing_map(subgraphs, edges, &parent_map);

    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();

    for sg_id in ids {
        let Some(bounds) = subgraph_bounds.get(&sg_id).cloned() else {
            continue;
        };
        let Some(sg) = subgraphs.get(&sg_id) else {
            continue;
        };

        let mut min_y: Option<usize> = None;
        let mut max_y: Option<usize> = None;
        for member in &sg.nodes {
            if let Some(node) = node_bounds.get(member) {
                let node_bottom = node.y.saturating_add(node.height.saturating_sub(1));
                min_y = Some(min_y.map_or(node.y, |cur| cur.min(node.y)));
                max_y = Some(max_y.map_or(node_bottom, |cur| cur.max(node_bottom)));
                continue;
            }
            if let Some(child_bounds) = subgraph_bounds.get(member) {
                let child_bottom = child_bounds
                    .y
                    .saturating_add(child_bounds.height.saturating_sub(1));
                min_y = Some(min_y.map_or(child_bounds.y, |cur| cur.min(child_bounds.y)));
                max_y = Some(max_y.map_or(child_bottom, |cur| cur.max(child_bottom)));
            }
        }

        let (Some(min_y), Some(max_y)) = (min_y, max_y) else {
            continue;
        };

        let content_top = bounds.y.saturating_add(1);
        let content_bottom = bounds.y.saturating_add(bounds.height.saturating_sub(2));
        let top_gap = min_y.saturating_sub(content_top);
        let bottom_gap = content_bottom.saturating_sub(max_y);

        let has_incoming = incoming_map.get(&sg_id).copied().unwrap_or(false);
        let has_outgoing = outgoing_map.get(&sg_id).copied().unwrap_or(false);

        // Each side needs 1 row of gap only if cross-boundary edges route
        // through it; blank rows without routing should be eliminated.
        let (min_top_gap, min_bottom_gap) = match direction {
            Direction::TopDown => (
                if has_incoming { 1 } else { 0 },
                if has_outgoing { 1 } else { 0 },
            ),
            Direction::BottomTop => (
                if has_outgoing { 1 } else { 0 },
                if has_incoming { 1 } else { 0 },
            ),
            _ => (0, 0),
        };

        // Only shrink; never expand beyond the current gap.
        let desired_top = min_top_gap.min(top_gap);
        let desired_bottom = min_bottom_gap.min(bottom_gap);
        let shrink_top = top_gap.saturating_sub(desired_top);
        let shrink_bottom = bottom_gap.saturating_sub(desired_bottom);
        let expand_top = desired_top.saturating_sub(top_gap);
        let expand_bottom = desired_bottom.saturating_sub(bottom_gap);

        if shrink_top == 0 && shrink_bottom == 0 && expand_top == 0 && expand_bottom == 0 {
            continue;
        }

        let new_y = bounds
            .y
            .saturating_sub(expand_top)
            .saturating_add(shrink_top);
        let new_height = bounds
            .height
            .saturating_add(expand_top.saturating_add(expand_bottom))
            .saturating_sub(shrink_top.saturating_add(shrink_bottom));

        if new_height < 2 {
            continue;
        }

        if let Some(entry) = subgraph_bounds.get_mut(&sg_id) {
            entry.y = new_y;
            entry.height = new_height;
        }
    }

    expand_parent_subgraph_bounds(subgraphs, subgraph_bounds);
}

/// Ensure at least 1 row/column of space between a direction-override
/// subgraph border and external predecessor/successor nodes.
///
/// After sublayout reconciliation, the subgraph bounds are recomputed from
/// the sublayout dimensions.  This can leave the border flush against nodes
/// above (TD) or below (BT), making edge entry visually cluttered.
///
/// For each direction-override subgraph with external edges, this pushes the
/// border inward so there is a 1-cell gap on the entry side.
fn ensure_external_edge_spacing(
    diagram: &Diagram,
    draw_positions: &mut HashMap<String, (usize, usize)>,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    for (sg_id, sg) in &diagram.subgraphs {
        if sg.dir.is_none() {
            continue;
        }
        let Some(sb) = subgraph_bounds.get(sg_id).cloned() else {
            continue;
        };
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();

        // Classify each external predecessor/successor by its position
        // relative to the subgraph, not by the diagram's main direction.
        // This avoids false positives for nested subgraphs whose parent
        // has a different direction (e.g. inner BT inside outer LR).
        let mut max_pred_bottom: Option<usize> = None; // preds above
        let mut min_succ_top: Option<usize> = None; // succs below

        for edge in &diagram.edges {
            if sg_node_set.contains(edge.to.as_str())
                && !sg_node_set.contains(edge.from.as_str())
                && let Some(nb) = node_bounds.get(&edge.from)
            {
                let nb_cy = nb.y + nb.height / 2;
                // Only count predecessors whose center is above the border.
                if nb_cy < sb.y {
                    let bottom = nb.y + nb.height.saturating_sub(1);
                    max_pred_bottom = Some(max_pred_bottom.map_or(bottom, |c| c.max(bottom)));
                }
            }
            if sg_node_set.contains(edge.from.as_str())
                && !sg_node_set.contains(edge.to.as_str())
                && let Some(nb) = node_bounds.get(&edge.to)
            {
                let nb_cy = nb.y + nb.height / 2;
                let sg_bottom = sb.y + sb.height.saturating_sub(1);
                if nb_cy > sg_bottom {
                    min_succ_top = Some(min_succ_top.map_or(nb.y, |c| c.min(nb.y)));
                }
            }
        }

        // Top side: shift entire subgraph down if too close to predecessor bottom.
        // Use +4 to leave room for horizontal edge routing + 1 clear row
        // above the subgraph border.
        if let Some(pred_bottom) = max_pred_bottom {
            let min_y = pred_bottom + 4;
            let current_y = subgraph_bounds[sg_id].y;
            if current_y < min_y {
                let adjust = min_y - current_y;
                // Shift the subgraph bounds down (keep height, move y).
                subgraph_bounds.get_mut(sg_id).unwrap().y = min_y;
                // Shift all member nodes down by the same amount.
                for member_id in &sg.nodes {
                    if let Some(nb) = node_bounds.get_mut(member_id) {
                        nb.y += adjust;
                    }
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.1 += adjust;
                    }
                }
                // Shift nested child subgraph bounds down too.
                let children: Vec<String> = diagram
                    .subgraphs
                    .iter()
                    .filter(|(cid, _)| *cid != sg_id && sg_node_set.contains(cid.as_str()))
                    .map(|(cid, _)| cid.clone())
                    .collect();
                for child_id in &children {
                    if let Some(cb) = subgraph_bounds.get_mut(child_id) {
                        cb.y += adjust;
                    }
                }
            }
        }
        // Bottom side: shift entire subgraph up if border too close to successor top.
        if let Some(succ_top) = min_succ_top {
            let max_bottom = succ_top.saturating_sub(4);
            let sb = &subgraph_bounds[sg_id];
            let current_bottom = sb.y + sb.height.saturating_sub(1);
            if current_bottom > max_bottom {
                let adjust = current_bottom - max_bottom;
                subgraph_bounds.get_mut(sg_id).unwrap().y =
                    subgraph_bounds[sg_id].y.saturating_sub(adjust);
                for member_id in &sg.nodes {
                    if let Some(nb) = node_bounds.get_mut(member_id) {
                        nb.y = nb.y.saturating_sub(adjust);
                    }
                    if let Some(pos) = draw_positions.get_mut(member_id) {
                        pos.1 = pos.1.saturating_sub(adjust);
                    }
                }
                let children: Vec<String> = diagram
                    .subgraphs
                    .iter()
                    .filter(|(cid, _)| *cid != sg_id && sg_node_set.contains(cid.as_str()))
                    .map(|(cid, _)| cid.clone())
                    .collect();
                for child_id in &children {
                    if let Some(cb) = subgraph_bounds.get_mut(child_id) {
                        cb.y = cb.y.saturating_sub(adjust);
                    }
                }
            }
        }
    }
}

fn shrink_subgraph_horizontal_gaps(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
    node_bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    direction: Direction,
) {
    let parent_map = build_subgraph_parent_map(subgraphs);
    let incoming_map = build_subgraph_incoming_map(subgraphs, edges, &parent_map);

    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();

    for sg_id in ids {
        let Some(bounds) = subgraph_bounds.get(&sg_id).cloned() else {
            continue;
        };
        let Some(sg) = subgraphs.get(&sg_id) else {
            continue;
        };

        let mut min_x: Option<usize> = None;
        let mut max_x: Option<usize> = None;
        for member in &sg.nodes {
            if let Some(node) = node_bounds.get(member) {
                let node_right = node.x.saturating_add(node.width.saturating_sub(1));
                min_x = Some(min_x.map_or(node.x, |cur| cur.min(node.x)));
                max_x = Some(max_x.map_or(node_right, |cur| cur.max(node_right)));
                continue;
            }
            if let Some(child_bounds) = subgraph_bounds.get(member) {
                let child_right = child_bounds
                    .x
                    .saturating_add(child_bounds.width.saturating_sub(1));
                min_x = Some(min_x.map_or(child_bounds.x, |cur| cur.min(child_bounds.x)));
                max_x = Some(max_x.map_or(child_right, |cur| cur.max(child_right)));
            }
        }

        let (Some(min_x), Some(max_x)) = (min_x, max_x) else {
            continue;
        };

        let content_left = bounds.x.saturating_add(1);
        let content_right = bounds.x.saturating_add(bounds.width.saturating_sub(2));
        let left_gap = min_x.saturating_sub(content_left);
        let right_gap = content_right.saturating_sub(max_x);

        let has_incoming = incoming_map.get(&sg_id).copied().unwrap_or(false);
        let incoming_gap = if has_incoming { 1 } else { 0 };

        let (min_left_gap, min_right_gap) = match direction {
            Direction::LeftRight => (incoming_gap, 0),
            Direction::RightLeft => (0, incoming_gap),
            _ => (0, 0),
        };

        let base_target = left_gap.min(right_gap);
        let desired_left = base_target.max(min_left_gap);
        let desired_right = base_target.max(min_right_gap);
        let mut shrink_left = left_gap.saturating_sub(desired_left);
        let mut shrink_right = right_gap.saturating_sub(desired_right);
        let expand_left = desired_left.saturating_sub(left_gap);
        let expand_right = desired_right.saturating_sub(right_gap);

        let mut new_width = bounds
            .width
            .saturating_add(expand_left.saturating_add(expand_right))
            .saturating_sub(shrink_left.saturating_add(shrink_right));

        if new_width < 2 {
            continue;
        }

        let inner_width = bounds.width.saturating_sub(2);
        let visible_title_len = if !bounds.title.trim().is_empty() && inner_width >= 5 {
            let max_title_len = inner_width.saturating_sub(4);
            bounds.title.len().min(max_title_len)
        } else {
            0
        };
        let title_width = if visible_title_len > 0 {
            visible_title_len.saturating_add(6)
        } else {
            2
        };
        let max_width_without_shrink = bounds
            .width
            .saturating_add(expand_left.saturating_add(expand_right));
        let min_width = title_width.min(max_width_without_shrink);

        if new_width < min_width {
            let deficit = min_width.saturating_sub(new_width);
            let reduce_left = deficit.min(shrink_left);
            shrink_left = shrink_left.saturating_sub(reduce_left);
            let reduce_right = deficit.saturating_sub(reduce_left);
            shrink_right = shrink_right.saturating_sub(reduce_right);
            new_width = bounds
                .width
                .saturating_add(expand_left.saturating_add(expand_right))
                .saturating_sub(shrink_left.saturating_add(shrink_right));
        }

        if new_width < 2 {
            continue;
        }

        let new_x = bounds
            .x
            .saturating_sub(expand_left)
            .saturating_add(shrink_left);

        if let Some(entry) = subgraph_bounds.get_mut(&sg_id) {
            entry.x = new_x;
            entry.width = new_width;
        }
    }

    expand_parent_subgraph_bounds(subgraphs, subgraph_bounds);
}

fn build_subgraph_parent_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
) -> HashMap<String, String> {
    let mut parent_map = HashMap::new();

    let mut ids: Vec<&String> = subgraphs.keys().collect();
    ids.sort_by(|a, b| {
        let depth_a = subgraph_depth(subgraphs, a.as_str());
        let depth_b = subgraph_depth(subgraphs, b.as_str());
        depth_b.cmp(&depth_a).then_with(|| a.cmp(b))
    });

    for sg_id in ids {
        if let Some(sg) = subgraphs.get(sg_id) {
            for node_id in &sg.nodes {
                parent_map
                    .entry(node_id.clone())
                    .or_insert_with(|| sg.id.clone());
            }
        }
    }

    parent_map
}

fn subgraph_depth(subgraphs: &HashMap<String, crate::graph::Subgraph>, sg_id: &str) -> usize {
    let mut depth = 0usize;
    let mut cur = sg_id;
    while let Some(sg) = subgraphs.get(cur) {
        if let Some(ref parent) = sg.parent {
            depth += 1;
            cur = parent;
        } else {
            break;
        }
    }
    depth
}

fn build_subgraph_incoming_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
    parent_map: &HashMap<String, String>,
) -> HashMap<String, bool> {
    let mut incoming: HashMap<String, bool> = HashMap::new();
    for edge in edges {
        let dst_ancestors = collect_subgraph_ancestors(&edge.to, subgraphs, parent_map);
        if dst_ancestors.is_empty() {
            continue;
        }
        for sg_id in dst_ancestors {
            if !is_node_in_subgraph(&edge.from, &sg_id, subgraphs, parent_map) {
                incoming.insert(sg_id, true);
            }
        }
    }
    incoming
}

fn build_subgraph_outgoing_map(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    edges: &[crate::graph::Edge],
    parent_map: &HashMap<String, String>,
) -> HashMap<String, bool> {
    let mut outgoing: HashMap<String, bool> = HashMap::new();
    for edge in edges {
        let src_ancestors = collect_subgraph_ancestors(&edge.from, subgraphs, parent_map);
        if src_ancestors.is_empty() {
            continue;
        }
        for sg_id in src_ancestors {
            if !is_node_in_subgraph(&edge.to, &sg_id, subgraphs, parent_map) {
                outgoing.insert(sg_id, true);
            }
        }
    }
    outgoing
}

fn collect_subgraph_ancestors(
    node_id: &str,
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    parent_map: &HashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = parent_map.get(node_id).cloned();
    while let Some(parent_id) = cur {
        out.push(parent_id.clone());
        cur = subgraphs
            .get(&parent_id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
    }
    out
}

fn is_node_in_subgraph(
    node_id: &str,
    sg_id: &str,
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    parent_map: &HashMap<String, String>,
) -> bool {
    let mut cur = parent_map.get(node_id).cloned();
    while let Some(parent_id) = cur {
        if parent_id == sg_id {
            return true;
        }
        cur = subgraphs
            .get(&parent_id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
    }
    false
}

fn expand_parent_subgraph_bounds(
    subgraphs: &HashMap<String, crate::graph::Subgraph>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
) {
    // Expand parent bounds to contain child bounds (inside-out).
    let mut ids: Vec<String> = subgraph_bounds.keys().cloned().collect();
    ids.sort_by_key(|id| subgraph_bounds.get(id).map(|b| b.depth).unwrap_or(0));
    ids.reverse();
    for id in ids {
        let parent_id = subgraphs
            .get(&id)
            .and_then(|sg| sg.parent.as_ref())
            .cloned();
        let (Some(parent_id), Some(child_bounds)) = (parent_id, subgraph_bounds.get(&id).cloned())
        else {
            continue;
        };
        let Some(parent_bounds) = subgraph_bounds.get_mut(&parent_id) else {
            continue;
        };

        let pad = 1usize;
        let child_left = child_bounds.x.saturating_sub(pad);
        let child_top = child_bounds.y.saturating_sub(pad);
        let child_right = child_bounds.x + child_bounds.width + pad;
        let child_bottom = child_bounds.y + child_bounds.height + pad;
        let parent_right = parent_bounds.x + parent_bounds.width;
        let parent_bottom = parent_bounds.y + parent_bounds.height;

        let new_left = parent_bounds.x.min(child_left);
        let new_top = parent_bounds.y.min(child_top);
        let new_right = parent_right.max(child_right);
        let new_bottom = parent_bottom.max(child_bottom);

        parent_bounds.x = new_left;
        parent_bounds.y = new_top;
        parent_bounds.width = new_right.saturating_sub(new_left);
        parent_bounds.height = new_bottom.saturating_sub(new_top);
    }
}

/// Nudge waypoints that overlap with node bounding boxes.
///
/// If a waypoint falls inside a node, push it just past the node's edge along the
/// cross-axis (X for vertical layouts, Y for horizontal). The waypoint is then
/// clamped to stay within canvas bounds.
fn nudge_colliding_waypoints(
    edge_waypoints: &mut HashMap<usize, Vec<(usize, usize)>>,
    node_bounds: &HashMap<String, NodeBounds>,
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) {
    for waypoints in edge_waypoints.values_mut() {
        for wp in waypoints.iter_mut() {
            for bounds in node_bounds.values() {
                if bounds.contains(wp.0, wp.1) {
                    if is_vertical {
                        wp.0 = bounds.x + bounds.width + 1;
                    } else {
                        wp.1 = bounds.y + bounds.height + 1;
                    }
                    break;
                }
            }
            wp.0 = wp.0.min(canvas_width.saturating_sub(1));
            wp.1 = wp.1.min(canvas_height.saturating_sub(1));
        }
    }
}

/// Shared parameters for transforming layout coordinates to ASCII draw coordinates.
struct TransformContext {
    layout_min_x: f64,
    layout_min_y: f64,
    scale_x: f64,
    scale_y: f64,
    padding: usize,
    left_label_margin: usize,
    overhang_x: usize,
    overhang_y: usize,
}

impl TransformContext {
    /// Transform a layout top-left-based Rect to draw coordinates (x, y, width, height).
    #[allow(dead_code)]
    ///
    /// Transforms the top-left and bottom-right corners independently using
    /// `to_ascii()`, then computes the draw rect between them. This ensures
    /// the transformed rect faithfully represents the layout bounding box in
    /// draw space.
    fn to_ascii_rect(&self, rect: &Rect) -> (usize, usize, usize, usize) {
        let (x1, y1) = self.to_ascii(rect.x, rect.y);
        let (x2, y2) = self.to_ascii(rect.x + rect.width, rect.y + rect.height);
        let draw_x = x1.min(x2);
        let draw_y = y1.min(y2);
        let draw_w = x1.max(x2) - draw_x;
        let draw_h = y1.max(y2) - draw_y;
        (draw_x, draw_y, draw_w.max(1), draw_h.max(1))
    }

    /// Transform a layout (x, y) coordinate to ASCII draw coordinates.
    fn to_ascii(&self, layout_x: f64, layout_y: f64) -> (usize, usize) {
        let x = ((layout_x - self.layout_min_x) * self.scale_x).round() as usize
            + self.overhang_x
            + self.padding
            + self.left_label_margin;
        let y = ((layout_y - self.layout_min_y) * self.scale_y).round() as usize
            + self.overhang_y
            + self.padding;
        (x, y)
    }
}

/// Transform layout waypoints to ASCII draw coordinates using uniform scale factors.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses `layer_starts` to snap to
/// the correct rank position. The cross axis uses uniform scaling from layout
/// coordinates, ensuring consistency with node positions.
fn transform_waypoints_direct(
    edge_waypoints: &HashMap<usize, Vec<WaypointWithRank>>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, Vec<(usize, usize)>> {
    let mut converted = HashMap::new();

    for (edge_idx, waypoints) in edge_waypoints {
        if edges.get(*edge_idx).is_some() {
            let wps: Vec<(usize, usize)> = waypoints
                .iter()
                .map(|wp| {
                    let rank_idx = wp.rank as usize;
                    let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
                    let (scaled_x, scaled_y) = ctx.to_ascii(wp.point.x, wp.point.y);

                    if is_vertical {
                        (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
                    } else {
                        (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
                    }
                })
                .collect();

            converted.insert(*edge_idx, wps);
        }
    }

    converted
}

/// Transform layout label positions to ASCII draw coordinates.
///
/// The primary axis (Y for TD/BT, X for LR/RL) uses rank-based snapping via
/// `layer_starts[rank]`, matching how `transform_waypoints_direct()` works.
/// The cross axis uses uniform scaling from layout coordinates.
fn transform_label_positions_direct(
    label_positions: &HashMap<usize, WaypointWithRank>,
    edges: &[Edge],
    ctx: &TransformContext,
    layer_starts: &[usize],
    is_vertical: bool,
    canvas_width: usize,
    canvas_height: usize,
) -> HashMap<usize, (usize, usize)> {
    let mut converted = HashMap::new();

    for (edge_idx, wp) in label_positions {
        if edges.get(*edge_idx).is_some() {
            let rank_idx = wp.rank as usize;
            let layer_pos = layer_starts.get(rank_idx).copied().unwrap_or(0);
            let (scaled_x, scaled_y) = ctx.to_ascii(wp.point.x, wp.point.y);

            let pos = if is_vertical {
                (scaled_x.min(canvas_width.saturating_sub(1)), layer_pos)
            } else {
                (layer_pos, scaled_y.min(canvas_height.saturating_sub(1)))
            };
            converted.insert(*edge_idx, pos);
        }
    }

    converted
}

fn waypoint_inside_bounds(bounds: &SubgraphBounds, point: (usize, usize)) -> bool {
    let (x, y) = point;
    let max_x = bounds.x + bounds.width.saturating_sub(1);
    let max_y = bounds.y + bounds.height.saturating_sub(1);
    x > bounds.x && x < max_x && y > bounds.y && y < max_y
}

fn segment_bounds_intersection(
    start: (usize, usize),
    end: (usize, usize),
    bounds: &SubgraphBounds,
) -> Option<(usize, usize)> {
    let (x0, y0) = (start.0 as f64, start.1 as f64);
    let (x1, y1) = (end.0 as f64, end.1 as f64);
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return None;
    }

    let x_min = bounds.x as f64;
    let x_max = (bounds.x + bounds.width) as f64;
    let y_min = bounds.y as f64;
    let y_max = (bounds.y + bounds.height) as f64;

    let mut candidates: Vec<(f64, (usize, usize))> = Vec::new();

    if dx.abs() > f64::EPSILON {
        let t_left = (x_min - x0) / dx;
        if (0.0..=1.0).contains(&t_left) {
            let y = y0 + t_left * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_left, (x_min.round() as usize, y.round() as usize)));
            }
        }
        let t_right = (x_max - x0) / dx;
        if (0.0..=1.0).contains(&t_right) {
            let y = y0 + t_right * dy;
            if y >= y_min && y <= y_max {
                candidates.push((t_right, (x_max.round() as usize, y.round() as usize)));
            }
        }
    }

    if dy.abs() > f64::EPSILON {
        let t_top = (y_min - y0) / dy;
        if (0.0..=1.0).contains(&t_top) {
            let x = x0 + t_top * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_top, (x.round() as usize, y_min.round() as usize)));
            }
        }
        let t_bottom = (y_max - y0) / dy;
        if (0.0..=1.0).contains(&t_bottom) {
            let x = x0 + t_bottom * dx;
            if x >= x_min && x <= x_max {
                candidates.push((t_bottom, (x.round() as usize, y_max.round() as usize)));
            }
        }
    }

    candidates
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| point)
}

fn clip_waypoints_to_subgraph(
    waypoints: &[(usize, usize)],
    bounds: &SubgraphBounds,
    clip_start: bool,
    clip_end: bool,
) -> Vec<(usize, usize)> {
    if waypoints.len() < 2 {
        return waypoints.to_vec();
    }
    let mut out = waypoints.to_vec();

    if clip_start && waypoint_inside_bounds(bounds, out[0]) {
        let mut idx = 0usize;
        while idx + 1 < out.len() && waypoint_inside_bounds(bounds, out[idx]) {
            idx += 1;
        }
        if idx < out.len() {
            let inside = out[idx.saturating_sub(1)];
            let outside = out[idx];
            let intersection =
                segment_bounds_intersection(inside, outside, bounds).unwrap_or(inside);
            let mut new_points = Vec::new();
            new_points.push(intersection);
            new_points.extend_from_slice(&out[idx..]);
            out = new_points;
        }
    }

    if clip_end && out.len() >= 2 {
        let last_idx = out.len() - 1;
        if waypoint_inside_bounds(bounds, out[last_idx]) {
            let mut idx = last_idx;
            while idx > 0 && waypoint_inside_bounds(bounds, out[idx]) {
                idx -= 1;
            }
            if idx < last_idx {
                let outside = out[idx];
                let inside = out[idx + 1];
                let intersection =
                    segment_bounds_intersection(outside, inside, bounds).unwrap_or(inside);
                let mut new_points = out[..=idx].to_vec();
                new_points.push(intersection);
                out = new_points;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Scale Factor Tests (Phase 2)
    // =========================================================================

    #[test]
    fn scale_factors_td_typical() {
        // Typical TD: 3 nodes with widths 9,7,11 and heights all 3
        // avg_w = 9.0, max_h = 3
        // rank_sep = 50.0, node_sep = 50.0, v_spacing = 3, h_spacing = 4
        // scale_y (primary) = (3 + 3) / (3 + 50) = 6/53
        // scale_x (cross)   = (9 + 4) / (9 + 50) = 13/59
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (7, 3));
        dims.insert("C".into(), (11, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);

        let expected_sy = 6.0 / 53.0;
        let expected_sx = 13.0 / 59.0;
        assert!(
            (sx - expected_sx).abs() < 1e-6,
            "sx: got {sx}, expected {expected_sx}"
        );
        assert!(
            (sy - expected_sy).abs() < 1e-6,
            "sy: got {sy}, expected {expected_sy}"
        );
    }

    #[test]
    fn scale_factors_lr_direction_aware() {
        // LR: nodes widths 9,9, heights 3,3 → avg_h = 3, max_w = 9
        // scale_x (primary) = (9 + 4) / (9 + 50) = 13/59
        // scale_y (cross)   = (3 + 3) / (3 + 6) = 6/9
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (9, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 6.0, 3, 4, false, false);

        let expected_sx = 13.0 / 59.0;
        let expected_sy = 6.0 / 9.0;
        assert!(
            (sx - expected_sx).abs() < 1e-6,
            "sx: got {sx}, expected {expected_sx}"
        );
        assert!(
            (sy - expected_sy).abs() < 1e-6,
            "sy: got {sy}, expected {expected_sy}"
        );
    }

    #[test]
    fn scale_factors_single_node() {
        let mut dims = HashMap::new();
        dims.insert("X".into(), (5, 3));

        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        assert!(sx > 0.0, "sx should be positive, got {sx}");
        assert!(sy > 0.0, "sy should be positive, got {sy}");
        assert!(sx.is_finite());
        assert!(sy.is_finite());
    }

    // =========================================================================
    // Layered Layout Helper Tests
    // =========================================================================

    #[test]
    fn build_layered_layout_includes_label_positions() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA -- yes --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = build_layered_layout(
            &diagram,
            &LayoutConfig::default(),
            |node| (node.label.len() as f64 + 4.0, 3.0),
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| text_edge_label_dimensions(label))
            },
        );

        assert!(result.label_positions.contains_key(&0));
    }

    #[test]
    fn scale_factors_halved_for_doubled_ranks() {
        // With ranks_doubled=true, effective_rank_sep = max_h + 2*rank_sep = 3 + 100 = 103
        // scale_y = (max_h + v_spacing) / (max_h + eff_rs) = 6/106
        // This is exactly half of the non-doubled scale: 6/53 / 2 = 6/106
        let mut dims = HashMap::new();
        dims.insert("A".into(), (9, 3));
        dims.insert("B".into(), (7, 3));

        let (_, sy_normal) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        let (_, sy_doubled) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, true);

        // Doubled-rank scale should be exactly half of normal scale
        let expected_sy = sy_normal / 2.0;
        assert!(
            (sy_doubled - expected_sy).abs() < 1e-6,
            "sy_doubled: got {sy_doubled}, expected {expected_sy} (half of {sy_normal})"
        );

        // Verify: gap_new = 2*rank_sep*scale_doubled = gap_old = rank_sep*scale_normal
        let gap_normal = 50.0 * sy_normal;
        let gap_doubled = 100.0 * sy_doubled;
        assert!(
            (gap_normal - gap_doubled).abs() < 1e-6,
            "Gaps should match: normal={gap_normal}, doubled={gap_doubled}"
        );
    }

    #[test]
    fn scale_factors_empty_nodes() {
        let dims: HashMap<String, (usize, usize)> = HashMap::new();
        let (sx, sy) = compute_ascii_scale_factors(&dims, 50.0, 50.0, 3, 4, true, false);
        assert!(sx.is_finite());
        assert!(sy.is_finite());
    }

    // =========================================================================
    // Collision Repair Tests (Phase 3)
    // =========================================================================

    #[test]
    fn collision_repair_pushes_overlapping_nodes_apart() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (5, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (0, 0), "A should not move");
        assert_eq!(positions["B"], (12, 0), "B pushed to right edge of A + gap");
    }

    #[test]
    fn collision_repair_cascading() {
        let layers = vec![vec!["A".into(), "B".into(), "C".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (3, 0));
        positions.insert("C".into(), (8, 0));
        let dims: HashMap<String, (usize, usize)> = [
            ("A".into(), (6, 3)),
            ("B".into(), (6, 3)),
            ("C".into(), (6, 3)),
        ]
        .into();

        collision_repair(&layers, &mut positions, &dims, true, 2);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (8, 0));
        assert_eq!(positions["C"], (16, 0));
    }

    #[test]
    fn collision_repair_no_change_when_spaced() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (20, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (20, 0));
    }

    #[test]
    fn collision_repair_horizontal_layout() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (0, 0));
        positions.insert("B".into(), (0, 2));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, false, 3);

        assert_eq!(positions["A"], (0, 0));
        assert_eq!(positions["B"], (0, 6));
    }

    #[test]
    fn collision_repair_single_node_layer_noop() {
        let layers = vec![vec!["A".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (5, 5));
        let dims: HashMap<String, (usize, usize)> = [("A".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["A"], (5, 5));
    }

    #[test]
    fn collision_repair_sorts_by_cross_axis() {
        let layers = vec![vec!["A".into(), "B".into()]];
        let mut positions: HashMap<String, (usize, usize)> = HashMap::new();
        positions.insert("A".into(), (20, 0));
        positions.insert("B".into(), (0, 0));
        let dims: HashMap<String, (usize, usize)> =
            [("A".into(), (8, 3)), ("B".into(), (8, 3))].into();

        collision_repair(&layers, &mut positions, &dims, true, 4);

        assert_eq!(positions["B"], (0, 0));
        assert_eq!(positions["A"], (20, 0));
    }

    // =========================================================================
    // Waypoint Transform Tests (Phase 4)
    // =========================================================================

    #[test]
    fn waypoint_transform_vertical_basic() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "C")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(
            0usize,
            vec![WaypointWithRank {
                point: Point { x: 100.0, y: 75.0 },
                rank: 1,
            }],
        );

        let layer_starts = vec![1, 5, 9];
        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 25.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 80, 20);

        assert!(
            result.contains_key(&0),
            "should have waypoints for edge 0 (A→C)"
        );
        let wps = &result[&0];
        assert_eq!(wps.len(), 1);
        assert_eq!(wps[0].1, 5, "y should be layer_starts[1]");
        assert_eq!(wps[0].0, 12, "x should be scaled layout x + padding");
    }

    #[test]
    fn waypoint_transform_horizontal_basic() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "C")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(
            0usize,
            vec![WaypointWithRank {
                point: Point { x: 75.0, y: 100.0 },
                rank: 1,
            }],
        );

        let layer_starts = vec![1, 8, 15];
        let ctx = TransformContext {
            layout_min_x: 25.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.67,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, false, 40, 80);

        let wps = &result[&0];
        assert_eq!(wps[0].0, 8, "x should be layer_starts[1]");
        assert_eq!(wps[0].1, 35, "y should be scaled layout y + padding");
    }

    #[test]
    fn waypoint_transform_clamps_to_canvas() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut waypoints = HashMap::new();
        waypoints.insert(
            0usize,
            vec![WaypointWithRank {
                point: Point { x: 5000.0, y: 50.0 },
                rank: 0,
            }],
        );

        let layer_starts = vec![1];
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.5,
            scale_y: 0.5,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result =
            transform_waypoints_direct(&waypoints, &edges, &ctx, &layer_starts, true, 30, 20);

        let wps = &result[&0];
        assert!(wps[0].0 <= 29, "x clamped to canvas_width - 1");
    }

    #[test]
    fn waypoint_transform_empty_input() {
        let edges: Vec<Edge> = vec![];
        let waypoints: HashMap<usize, Vec<WaypointWithRank>> = HashMap::new();
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let result = transform_waypoints_direct(&waypoints, &edges, &ctx, &[], true, 80, 20);
        assert!(result.is_empty());
    }

    // =========================================================================
    // Label Transform Tests (Phase 5)
    // =========================================================================

    #[test]
    fn label_transform_basic_scaling() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("yes")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(
            0usize,
            WaypointWithRank {
                point: Point { x: 150.0, y: 100.0 },
                rank: 1,
            },
        );

        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        // layer_starts: rank 0 → y=0, rank 1 → y=8, rank 2 → y=16
        let layer_starts = vec![0, 8, 16];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        assert!(result.contains_key(&0));
        // x uses uniform scale: (150-50)*0.22 + 1 = 23
        // y = layer_starts[rank=1] = 8
        assert_eq!(result[&0], (23, 8));
    }

    #[test]
    fn label_transform_with_left_margin() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("yes")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(
            0usize,
            WaypointWithRank {
                point: Point { x: 150.0, y: 100.0 },
                rank: 1,
            },
        );

        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 50.0,
            scale_x: 0.22,
            scale_y: 0.11,
            padding: 1,
            left_label_margin: 3,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts = vec![0, 8, 16];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        // x = 23 + 3 (left_label_margin) = 26
        assert_eq!(result[&0].0, 26);
    }

    #[test]
    fn label_transform_empty_input() {
        let edges: Vec<Edge> = vec![];
        let labels: HashMap<usize, WaypointWithRank> = HashMap::new();
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts: Vec<usize> = vec![];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);
        assert!(result.is_empty());
    }

    // =========================================================================
    // Compound Graph Wiring Tests
    // =========================================================================

    #[test]
    fn test_layout_subgraph_bounds_present() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        assert!(
            layout.subgraph_bounds.contains_key("sg1"),
            "should have bounds for sg1"
        );
        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(bounds.width > 0, "width should be positive");
        assert!(bounds.height > 0, "height should be positive");
        assert_eq!(bounds.title, "Group");
    }

    #[test]
    fn test_nested_subgraph_layout_produces_both_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA[Node A]\nsubgraph inner[Inner]\nB[Node B]\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        assert!(
            layout.subgraph_bounds.contains_key("outer"),
            "should have outer bounds"
        );
        assert!(
            layout.subgraph_bounds.contains_key("inner"),
            "should have inner bounds"
        );
    }

    #[test]
    fn test_layout_no_subgraph_bounds_simple() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        assert!(layout.subgraph_bounds.is_empty());
    }

    #[test]
    fn test_layout_canvas_dimensions_include_borders() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(
            layout.width >= bounds.x + bounds.width,
            "canvas width {} should contain border x+w={}",
            layout.width,
            bounds.x + bounds.width
        );
        assert!(
            layout.height >= bounds.y + bounds.height,
            "canvas height {} should contain border y+h={}",
            layout.height,
            bounds.y + bounds.height
        );
    }

    #[test]
    fn test_compute_layout_subgraph_diagram_succeeds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\nC --> A\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // Should not panic
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        assert!(layout.draw_positions.contains_key("A"));
        assert!(layout.draw_positions.contains_key("B"));
        assert!(layout.draw_positions.contains_key("C"));
    }

    #[test]
    fn test_compute_layout_simple_diagram_no_compound() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        assert!(!diagram.has_subgraphs());

        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        assert!(layout.draw_positions.contains_key("A"));
    }

    #[test]
    fn label_position_within_canvas_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n    A -->|yes| B";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        // Label position should exist — edge A→B is at index 0
        let edge_idx = diagram
            .edges
            .iter()
            .find(|e| e.from == "A" && e.to == "B")
            .unwrap()
            .index;
        assert!(
            layout.edge_label_positions.contains_key(&edge_idx),
            "Should have precomputed label position for A->B, got keys: {:?}",
            layout.edge_label_positions.keys().collect::<Vec<_>>()
        );

        let (lx, ly) = layout.edge_label_positions[&edge_idx];
        // Should be within canvas bounds
        assert!(
            lx < layout.width && ly < layout.height,
            "Label position ({}, {}) should be within canvas ({}, {})",
            lx,
            ly,
            layout.width,
            layout.height
        );
    }

    #[test]
    fn label_transform_skips_missing_edge() {
        use crate::graph::{Arrow, Stroke};
        let edges = vec![
            Edge::new("A", "B")
                .with_label("x")
                .with_stroke(Stroke::Solid)
                .with_arrows(Arrow::None, Arrow::Normal),
        ];

        let mut labels = HashMap::new();
        labels.insert(
            5usize,
            WaypointWithRank {
                point: Point { x: 100.0, y: 100.0 },
                rank: 0,
            },
        );

        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            padding: 1,
            left_label_margin: 0,
            overhang_x: 0,
            overhang_y: 0,
        };
        let layer_starts = vec![0];
        let result =
            transform_label_positions_direct(&labels, &edges, &ctx, &layer_starts, true, 50, 20);

        assert!(
            result.is_empty(),
            "out-of-bounds edge index should be skipped"
        );
    }

    // =========================================================================
    // Nested Subgraph Tests (Plan 0032)
    // =========================================================================

    #[test]
    fn test_nested_borders_inner_visible() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;
        use crate::render::{RenderOptions, render};

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let output = render(&diagram, &RenderOptions::default());
        assert!(
            output.contains("Outer"),
            "Output should contain 'Outer' title"
        );
        assert!(
            output.contains("Inner"),
            "Output should contain 'Inner' title"
        );
    }

    #[test]
    fn test_nested_subgraph_depth_values() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        assert_eq!(layout.subgraph_bounds["outer"].depth, 0);
        assert_eq!(layout.subgraph_bounds["inner"].depth, 1);
    }

    #[test]
    fn test_nested_subgraph_parent_contains_child_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nA\nsubgraph inner[Inner]\nB --> C\nend\nend\nA --> B\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        let outer = &layout.subgraph_bounds["outer"];
        let inner = &layout.subgraph_bounds["inner"];
        // Parent must fully contain child
        assert!(
            outer.x <= inner.x,
            "outer.x ({}) should be <= inner.x ({})",
            outer.x,
            inner.x
        );
        assert!(
            outer.y <= inner.y,
            "outer.y ({}) should be <= inner.y ({})",
            outer.y,
            inner.y
        );
        assert!(
            outer.x + outer.width >= inner.x + inner.width,
            "outer right ({}) should be >= inner right ({})",
            outer.x + outer.width,
            inner.x + inner.width
        );
        assert!(
            outer.y + outer.height >= inner.y + inner.height,
            "outer bottom ({}) should be >= inner bottom ({})",
            outer.y + outer.height,
            inner.y + inner.height
        );
    }

    #[test]
    fn test_nested_outer_only_subgraph_gets_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());
        assert!(
            layout.subgraph_bounds.contains_key("outer"),
            "outer should have bounds"
        );
        let outer = &layout.subgraph_bounds["outer"];
        assert!(outer.width > 0, "width should be positive");
        assert!(outer.height > 0, "height should be positive");
    }

    #[test]
    fn test_build_children_map() {
        use crate::graph::Subgraph;
        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "inner".to_string(),
            Subgraph {
                id: "inner".to_string(),
                title: "Inner".to_string(),
                nodes: vec!["A".to_string()],
                parent: Some("outer".to_string()),
                dir: None,
            },
        );
        subgraphs.insert(
            "outer".to_string(),
            Subgraph {
                id: "outer".to_string(),
                title: "Outer".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );
        let children_map = build_children_map(&subgraphs);
        assert_eq!(children_map["outer"], vec!["inner".to_string()]);
        assert!(!children_map.contains_key("inner"));
    }

    // =========================================================================
    // Subgraph Bounds Tests (Layout-derived bounds)
    // =========================================================================

    #[test]
    fn test_subgraph_bounds_no_overlap_from_separated_rects() {
        use crate::graph::Subgraph;

        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "Left".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );
        subgraphs.insert(
            "sg2".to_string(),
            Subgraph {
                id: "sg2".to_string(),
                title: "Right".to_string(),
                nodes: vec!["B".to_string()],
                parent: None,
                dir: None,
            },
        );

        let mut layout_bounds = HashMap::new();
        layout_bounds.insert(
            "sg1".to_string(),
            Rect {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 5.0,
            },
        );
        layout_bounds.insert(
            "sg2".to_string(),
            Rect {
                x: 40.0,
                y: 10.0,
                width: 10.0,
                height: 5.0,
            },
        );

        let config = LayoutConfig {
            padding: 0,
            left_label_margin: 0,
            ..LayoutConfig::default()
        };

        let transform = CoordTransform {
            scale_x: 1.0,
            scale_y: 1.0,
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            max_overhang_x: 0,
            max_overhang_y: 0,
            config: &config,
        };

        let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

        let a = &result["sg1"];
        let b = &result["sg2"];

        // Separated member nodes should produce non-overlapping draw bounds
        let no_x_overlap = a.x + a.width <= b.x || b.x + b.width <= a.x;
        let no_y_overlap = a.y + a.height <= b.y || b.y + b.height <= a.y;
        assert!(
            no_x_overlap || no_y_overlap,
            "Bounds should not overlap: sg1=({},{} {}x{}) sg2=({},{} {}x{})",
            a.x,
            a.y,
            a.width,
            a.height,
            b.x,
            b.y,
            b.width,
            b.height
        );
    }

    #[test]
    fn test_subgraph_bounds_maps_rects() {
        use crate::graph::Subgraph;

        let mut subgraphs = HashMap::new();
        subgraphs.insert(
            "sg1".to_string(),
            Subgraph {
                id: "sg1".to_string(),
                title: "G".to_string(),
                nodes: vec!["A".to_string()],
                parent: None,
                dir: None,
            },
        );

        let mut layout_bounds = HashMap::new();
        layout_bounds.insert(
            "sg1".to_string(),
            Rect {
                x: 10.0,
                y: 10.0,
                width: 5.0,
                height: 3.0,
            },
        );

        let config = LayoutConfig {
            padding: 0,
            left_label_margin: 0,
            ..LayoutConfig::default()
        };

        let transform = CoordTransform {
            scale_x: 1.0,
            scale_y: 1.0,
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            max_overhang_x: 0,
            max_overhang_y: 0,
            config: &config,
        };

        let result = subgraph_bounds_to_draw(&subgraphs, &layout_bounds, &transform);

        let b = &result["sg1"];
        // Title "G" requires min width = len("G") + 6 = 7, which exceeds rect width 5.
        // Title-width enforcement expands by (7-5)=2 and shifts x left by 2/2=1.
        assert_eq!(b.x, 9, "x shifted left by 1 due to title-width expansion");
        assert_eq!(b.y, 10, "y should match layout rect y");
        assert_eq!(b.width, 7, "width expanded to fit title");
        assert_eq!(b.height, 3, "height should match layout rect height");
    }

    // =========================================================================
    // Title Width Enforcement Tests (Plan 0026, Task 2.3)
    // =========================================================================

    #[test]
    fn test_subgraph_bounds_expanded_for_title() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[This Is A Very Long Title]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        let bounds = layout
            .subgraph_bounds
            .values()
            .next()
            .expect("Expected subgraph bounds");

        // Border must be wide enough for: corners (2) + "─ " (2) + title + " ─" (2)
        let min_width = "This Is A Very Long Title".len() + 6;
        assert!(
            bounds.width >= min_width,
            "Border width {} too narrow for title (need >= {})",
            bounds.width,
            min_width
        );
    }

    #[test]
    fn test_titled_subgraph_creates_title_rank() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = r#"graph TD
    subgraph sg1[Processing]
        A[Step 1] --> B[Step 2]
    end"#;

        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        assert!(layout.subgraph_bounds.contains_key("sg1"));
        let bounds = &layout.subgraph_bounds["sg1"];
        assert!(bounds.height > 0);
    }

    // =========================================================================
    // to_ascii_rect() Tests (Plan 0028, Task 1.1)
    // =========================================================================

    #[test]
    fn to_ascii_rect_at_layout_minimum() {
        // A rect centered at the layout minimum should produce draw coords near origin + padding
        let ctx = TransformContext {
            layout_min_x: 50.0,
            layout_min_y: 30.0,
            scale_x: 0.2,
            scale_y: 0.1,
            overhang_x: 2,
            overhang_y: 1,
            padding: 1,
            left_label_margin: 0,
        };
        let rect = Rect {
            x: 50.0,
            y: 30.0,
            width: 40.0,
            height: 20.0,
        };
        let (_x, _y, w, h) = ctx.to_ascii_rect(&rect);
        assert!(w > 0, "width should be positive, got {w}");
        assert!(h > 0, "height should be positive, got {h}");
    }

    #[test]
    fn to_ascii_rect_offset_from_minimum() {
        // A rect offset from layout minimum should have proportionally offset draw coords
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.2,
            scale_y: 0.1,
            overhang_x: 0,
            overhang_y: 0,
            padding: 0,
            left_label_margin: 0,
        };
        let rect1 = Rect {
            x: 50.0,
            y: 50.0,
            width: 40.0,
            height: 20.0,
        };
        let rect2 = Rect {
            x: 100.0,
            y: 100.0,
            width: 40.0,
            height: 20.0,
        };
        let (x1, y1, _, _) = ctx.to_ascii_rect(&rect1);
        let (x2, y2, _, _) = ctx.to_ascii_rect(&rect2);
        assert!(x2 > x1, "rect2 should be further right: x2={x2} vs x1={x1}");
        assert!(y2 > y1, "rect2 should be further down: y2={y2} vs y1={y1}");
    }

    #[test]
    fn to_ascii_rect_dimensions_scale_with_layout_size() {
        let ctx = TransformContext {
            layout_min_x: 0.0,
            layout_min_y: 0.0,
            scale_x: 0.5,
            scale_y: 0.5,
            overhang_x: 0,
            overhang_y: 0,
            padding: 0,
            left_label_margin: 0,
        };
        let small = Rect {
            x: 50.0,
            y: 50.0,
            width: 20.0,
            height: 10.0,
        };
        let large = Rect {
            x: 50.0,
            y: 50.0,
            width: 60.0,
            height: 30.0,
        };
        let (_, _, w1, h1) = ctx.to_ascii_rect(&small);
        let (_, _, w2, h2) = ctx.to_ascii_rect(&large);
        assert!(
            w2 > w1,
            "larger rect should have larger width: w2={w2} vs w1={w1}"
        );
        assert!(
            h2 > h1,
            "larger rect should have larger height: h2={h2} vs h1={h1}"
        );
    }

    // =========================================================================
    // Non-overlap Tests (Plan 0028, Task 2.1)
    // =========================================================================

    #[test]
    fn stacked_subgraphs_do_not_overlap() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n\
            subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
            subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
            A --> C\nB --> D";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        let sg1 = &layout.subgraph_bounds["sg1"];
        let sg2 = &layout.subgraph_bounds["sg2"];

        let sg1_bottom = sg1.y + sg1.height;
        let sg2_bottom = sg2.y + sg2.height;

        // Determine which is "upper" and which is "lower"
        let (_upper, lower, upper_bottom) = if sg1.y < sg2.y {
            (sg1, sg2, sg1_bottom)
        } else {
            (sg2, sg1, sg2_bottom)
        };

        // Upper subgraph's bottom must be strictly above lower's top
        assert!(
            upper_bottom <= lower.y,
            "Subgraphs should not overlap vertically: upper bottom={upper_bottom}, lower top={}",
            lower.y
        );
    }

    // =========================================================================
    // Containment Tests (Plan 0028, Task 1.2)
    // =========================================================================

    #[test]
    fn subgraph_bounds_contain_member_node_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA[Node1]\nB[Node2]\nend\nA --> B";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
    }

    #[test]
    fn stacked_subgraph_bounds_contain_member_nodes_after_overlap_resolution() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\n\
            subgraph sg1[Input]\nA[Data]\nB[Config]\nend\n\
            subgraph sg2[Output]\nC[Result]\nD[Log]\nend\n\
            A --> C\nB --> D";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layout = compute_layout_direct(&diagram, &LayoutConfig::default());

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B"]);
        assert_subgraph_contains_members(&layout, "sg2", &["C", "D"]);
    }

    fn assert_subgraph_contains_members(layout: &Layout, sg_id: &str, members: &[&str]) {
        let sg = &layout.subgraph_bounds[sg_id];
        let sg_right = sg.x + sg.width;
        let sg_bottom = sg.y + sg.height;

        for member_id in members {
            let nb = &layout.node_bounds[*member_id];
            let nb_right = nb.x + nb.width;
            let nb_bottom = nb.y + nb.height;

            assert!(
                sg.x <= nb.x,
                "{sg_id} left ({}) should be <= {member_id} left ({})",
                sg.x,
                nb.x
            );
            assert!(
                sg.y <= nb.y,
                "{sg_id} top ({}) should be <= {member_id} top ({})",
                sg.y,
                nb.y
            );
            assert!(
                sg_right >= nb_right,
                "{sg_id} right ({sg_right}) should be >= {member_id} right ({nb_right})"
            );
            assert!(
                sg_bottom >= nb_bottom,
                "{sg_id} bottom ({sg_bottom}) should be >= {member_id} bottom ({nb_bottom})"
            );
        }
    }

    // =========================================================================
    // Direction Override: Field Plumbing (Phase 4, Task 4.1)
    // =========================================================================

    #[test]
    fn direction_override_field_available_at_layout() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // Direction override is present on the subgraph
        assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::LeftRight));

        // Layout computation succeeds without panic
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);
        assert!(!layout.node_bounds.is_empty());
    }

    #[test]
    fn direction_override_none_when_not_specified() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        // No direction override: field should be None
        assert_eq!(diagram.subgraphs["sg1"].dir, None);
    }

    // =========================================================================
    // Direction Override Sub-Layout Tests (Phase 4, Tasks 4.2-4.4)
    // =========================================================================

    /// Helper: compute a sub-layout for a direction-override subgraph.
    /// Returns the LayoutResult for just the subgraph's internal nodes/edges.
    fn run_sublayout_for_sg(diagram: &Diagram, sg_id: &str) -> layered::LayoutResult {
        let sg = &diagram.subgraphs[sg_id];
        let sub_dir = sg.dir.expect("subgraph should have direction override");

        let layered_direction = match sub_dir {
            Direction::TopDown => LayeredDirection::TopBottom,
            Direction::BottomTop => LayeredDirection::BottomTop,
            Direction::LeftRight => LayeredDirection::LeftRight,
            Direction::RightLeft => LayeredDirection::RightLeft,
        };

        let mut sub_graph: layered::DiGraph<(f64, f64)> = layered::DiGraph::new();

        // Add leaf nodes (not child subgraphs)
        for node_id in &sg.nodes {
            if !diagram.is_subgraph(node_id)
                && let Some(node) = diagram.nodes.get(node_id)
            {
                let (w, h) = node_dimensions(node, sub_dir);
                sub_graph.add_node(node_id.as_str(), (w as f64, h as f64));
            }
        }

        // Add internal edges
        let sg_node_set: HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
        for edge in &diagram.edges {
            if sg_node_set.contains(edge.from.as_str()) && sg_node_set.contains(edge.to.as_str()) {
                sub_graph.add_edge(edge.from.as_str(), edge.to.as_str());
            }
        }

        let sub_config = LayeredConfig {
            direction: layered_direction,
            ..LayeredConfig::default()
        };

        layered::layout(&sub_graph, &sub_config, |_, dims| *dims)
    }

    #[test]
    fn sublayout_lr_nodes_arranged_horizontally() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        // In LR layout, nodes should be arranged horizontally (increasing x, similar y)
        let a = &result.nodes[&layered::NodeId::from("A")];
        let b = &result.nodes[&layered::NodeId::from("B")];
        let c = &result.nodes[&layered::NodeId::from("C")];

        // Centers should have increasing x
        let a_cx = a.x + a.width / 2.0;
        let b_cx = b.x + b.width / 2.0;
        let c_cx = c.x + c.width / 2.0;

        assert!(
            a_cx < b_cx,
            "A center_x ({a_cx}) should be < B center_x ({b_cx})"
        );
        assert!(
            b_cx < c_cx,
            "B center_x ({b_cx}) should be < C center_x ({c_cx})"
        );

        // Centers should have similar y (within tolerance for same-rank nodes)
        let a_cy = a.y + a.height / 2.0;
        let b_cy = b.y + b.height / 2.0;
        let c_cy = c.y + c.height / 2.0;

        assert!(
            (a_cy - b_cy).abs() < 1.0,
            "A and B should be at similar y: {a_cy} vs {b_cy}"
        );
        assert!(
            (b_cy - c_cy).abs() < 1.0,
            "B and C should be at similar y: {b_cy} vs {c_cy}"
        );
    }

    #[test]
    fn sublayout_dimensions_wider_than_tall_for_lr() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        assert!(
            result.width > result.height,
            "LR sub-layout should be wider than tall: {}x{}",
            result.width,
            result.height
        );
    }

    #[test]
    fn sublayout_bt_nodes_arranged_bottom_to_top() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Start] --> B[End]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        let a = &result.nodes[&layered::NodeId::from("A")];
        let b = &result.nodes[&layered::NodeId::from("B")];

        // BT: A should be below B (higher y means lower on screen)
        let a_cy = a.y + a.height / 2.0;
        let b_cy = b.y + b.height / 2.0;

        assert!(
            a_cy > b_cy,
            "In BT layout, A (start) should be below B (end): A_cy={a_cy} B_cy={b_cy}"
        );
    }

    #[test]
    fn sublayout_rl_reverses_node_order() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Reverse]\ndirection RL\nA[Left] --> B[Right]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let a = layout.get_bounds("A").unwrap();
        let b = layout.get_bounds("B").unwrap();

        // RL: A (start) should be RIGHT of B (end) since flow goes right-to-left
        assert!(
            a.center_x() > b.center_x(),
            "In RL layout, A should be right of B: A_cx={} B_cx={}",
            a.center_x(),
            b.center_x()
        );

        // Both should be at similar y
        let y_tolerance = 2;
        assert!(
            (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
            "A and B should be at similar y in RL: {} vs {}",
            a.center_y(),
            b.center_y()
        );
    }

    #[test]
    fn direction_override_nodes_horizontal_in_final_layout() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal Section]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let a = layout.get_bounds("A").unwrap();
        let b = layout.get_bounds("B").unwrap();
        let c = layout.get_bounds("C").unwrap();

        // In an LR subgraph within a TD parent:
        // A, B, C should be arranged horizontally (increasing x, similar y)
        assert!(
            a.center_x() < b.center_x(),
            "A ({}) should be left of B ({})",
            a.center_x(),
            b.center_x()
        );
        assert!(
            b.center_x() < c.center_x(),
            "B ({}) should be left of C ({})",
            b.center_x(),
            c.center_x()
        );

        // All should be at similar y (within a small tolerance for rounding)
        let y_tolerance = 2;
        assert!(
            (a.center_y() as isize - b.center_y() as isize).abs() <= y_tolerance,
            "A and B should be at similar y: {} vs {}",
            a.center_y(),
            b.center_y()
        );
        assert!(
            (b.center_y() as isize - c.center_y() as isize).abs() <= y_tolerance,
            "B and C should be at similar y: {} vs {}",
            b.center_y(),
            c.center_y()
        );
    }

    #[test]
    fn direction_override_subgraph_wider_than_tall() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        assert!(
            sg.width > sg.height,
            "LR subgraph should be wider than tall: {}x{}",
            sg.width,
            sg.height
        );
    }

    #[test]
    fn direction_override_bt_subgraph_taller_than_wide() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // BT subgraph inside an LR parent: subgraph should be taller than wide
        let input =
            "graph LR\nsubgraph sg1[Vertical]\ndirection BT\nA[Top] --> B[Mid] --> C[Bot]\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        assert!(
            sg.height > sg.width,
            "BT subgraph should be taller than wide: {}w x {}h",
            sg.width,
            sg.height
        );
    }

    #[test]
    fn direction_override_subgraph_title_width_minimum() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // Subgraph with a long title should have bounds wide enough for the title
        let input =
            "graph TD\nsubgraph sg1[A Very Long Section Title]\ndirection LR\nA --> B\nend\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];
        let title = "A Very Long Section Title";
        // Title with padding characters on either side
        assert!(
            sg.width >= title.len(),
            "Subgraph width ({}) should accommodate title length ({})",
            sg.width,
            title.len()
        );
    }

    #[test]
    fn direction_override_nodes_inside_subgraph_bounds() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        assert_subgraph_contains_members(&layout, "sg1", &["A", "B", "C"]);
    }

    #[test]
    fn direction_override_no_node_overlap() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2] --> C[Step 3]\nend\nStart --> A\nC --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        // Verify no overlap between A, B, C
        let nodes = ["A", "B", "C"];
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = layout.get_bounds(nodes[i]).unwrap();
                let b = layout.get_bounds(nodes[j]).unwrap();
                let overlap_x = a.x < b.x + b.width && b.x < a.x + a.width;
                let overlap_y = a.y < b.y + b.height && b.y < a.y + a.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "Nodes {} and {} should not overlap: {:?} vs {:?}",
                    nodes[i],
                    nodes[j],
                    (a.x, a.y, a.width, a.height),
                    (b.x, b.y, b.width, b.height)
                );
            }
        }
    }

    #[test]
    fn direction_override_external_nodes_outside_subgraph() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA[Step 1] --> B[Step 2]\nend\nStart --> A\nB --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        let sg = &layout.subgraph_bounds["sg1"];

        // Start and End should NOT be inside the subgraph bounds
        // (they are external to sg1)
        for ext_node in &["Start", "End"] {
            let bounds = layout.get_bounds(ext_node).unwrap();
            let inside_x = bounds.x >= sg.x && bounds.x + bounds.width <= sg.x + sg.width;
            let inside_y = bounds.y >= sg.y && bounds.y + bounds.height <= sg.y + sg.height;
            // At least one dimension should be outside
            assert!(
                !(inside_x && inside_y),
                "External node {} should not be fully inside sg1 bounds",
                ext_node
            );
        }
    }

    // =========================================================================
    // Cross-Boundary Edge Routing (Phase 4, Task 4.5)
    // =========================================================================

    #[test]
    fn cross_boundary_edge_no_panic() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;
        use crate::render::{RenderOptions, render};

        let input =
            "graph TD\nsubgraph sg1[Horizontal]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let options = RenderOptions::default();

        // Full render pipeline should not panic
        let output = render(&diagram, &options);
        assert!(output.contains("A"));
        assert!(output.contains("B"));
        assert!(output.contains("C"));
        assert!(output.contains("D"));
        assert!(output.contains("Horizontal"));
    }

    #[test]
    fn node_effective_direction_populated() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A\nB --> D\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let config = LayoutConfig::default();
        let layout = compute_layout_direct(&diagram, &config);

        // Nodes inside the LR subgraph should have LR effective direction
        assert_eq!(
            layout.node_directions.get("A"),
            Some(&Direction::LeftRight),
            "A should have LR direction"
        );
        assert_eq!(
            layout.node_directions.get("B"),
            Some(&Direction::LeftRight),
            "B should have LR direction"
        );

        // Nodes outside the subgraph should have the parent direction (TD)
        assert_eq!(
            layout.node_directions.get("C"),
            Some(&Direction::TopDown),
            "C should have TD direction"
        );
        assert_eq!(
            layout.node_directions.get("D"),
            Some(&Direction::TopDown),
            "D should have TD direction"
        );
    }

    #[test]
    fn sublayout_excludes_cross_boundary_edges() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        let input =
            "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nStart --> A\nB --> End\n";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);

        let result = run_sublayout_for_sg(&diagram, "sg1");

        // Sub-layout should only have A and B, not Start or End
        assert!(result.nodes.contains_key(&layered::NodeId::from("A")));
        assert!(result.nodes.contains_key(&layered::NodeId::from("B")));
        assert!(!result.nodes.contains_key(&layered::NodeId::from("Start")));
        assert!(!result.nodes.contains_key(&layered::NodeId::from("End")));
    }

    #[test]
    fn compute_sublayouts_skips_non_isolated_when_flag_set() {
        use crate::graph::build_diagram;
        use crate::parser::parse_flowchart;

        // sg1 has direction LR but cross-boundary edge C --> A
        let input = "graph TD\nsubgraph sg1[Group]\ndirection LR\nA --> B\nend\nC --> A";
        let flowchart = parse_flowchart(input).unwrap();
        let diagram = build_diagram(&flowchart);
        let layered_config = LayeredConfig::default(); // direction = TopBottom

        // With flag false: sublayout uses override direction (LR)
        let subs_false = compute_sublayouts(
            &diagram,
            &layered_config,
            |_node| (40.0, 20.0),
            |_edge| None,
            false,
        );
        let lr_result = &subs_false["sg1"];
        let a_lr = lr_result.result.nodes[&layered::NodeId::from("A")];
        let b_lr = lr_result.result.nodes[&layered::NodeId::from("B")];
        // LR: A and B should be side-by-side (different x, similar y)
        assert!(
            (a_lr.y - b_lr.y).abs() < 1.0,
            "LR: A.y={} B.y={} should be similar",
            a_lr.y,
            b_lr.y
        );

        // With flag true: non-isolated override is skipped entirely.
        let subs_true = compute_sublayouts(
            &diagram,
            &layered_config,
            |_node| (40.0, 20.0),
            |_edge| None,
            true,
        );
        assert!(
            !subs_true.contains_key("sg1"),
            "non-isolated sublayout should be skipped"
        );
    }
}
