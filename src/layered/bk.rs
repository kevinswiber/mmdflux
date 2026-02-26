//! Brandes-Köpf algorithm for horizontal coordinate assignment.
//!
//! This module implements the algorithm described in:
//! Brandes, U. and Köpf, B. (2001). Fast and Simple Horizontal Coordinate Assignment.
//!
//! The algorithm produces x-coordinates that minimize total edge length while
//! respecting node separation constraints.

use std::collections::{HashMap, HashSet, VecDeque};

use super::graph::LayoutGraph;
use super::types::Direction;

/// Index type for nodes in the layout graph
pub type NodeIndex = usize;

/// Set of conflicts indexed by node pairs for O(1) lookup.
///
/// Conflicts are stored by node indices (unordered)
/// node-based conflict checks.
pub type ConflictSet = HashSet<(NodeIndex, NodeIndex)>;

/// Represents a vertical alignment of nodes into blocks.
///
/// A "block" is a set of nodes that are vertically aligned (same x-coordinate).
/// The alignment is represented as a linked list through the `align` map,
/// with each block having a single root node.
#[derive(Debug, Clone)]
pub struct BlockAlignment {
    /// Maps each node to its block root (representative node).
    /// All nodes in the same block share the same root.
    pub root: HashMap<NodeIndex, NodeIndex>,

    /// Maps each node to the next node in its alignment chain.
    /// Forms a linked list within each block.
    pub align: HashMap<NodeIndex, NodeIndex>,
}

impl BlockAlignment {
    /// Create a new alignment where each node is its own singleton block.
    pub fn new(nodes: &[NodeIndex]) -> Self {
        let mut root = HashMap::new();
        let mut align = HashMap::new();

        // Initially, each node is its own root and aligns to itself
        for &node in nodes {
            root.insert(node, node);
            align.insert(node, node);
        }

        Self { root, align }
    }

    /// Get the root of the block containing `node`.
    pub fn get_root(&self, node: NodeIndex) -> NodeIndex {
        self.root.get(&node).copied().unwrap_or(node)
    }

    /// Get all nodes in the block containing `node`.
    #[cfg(test)]
    pub fn get_block_nodes(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let root = self.get_root(node);
        let mut nodes = Vec::new();
        let mut current = root;

        // Follow align pointers until we cycle back to root
        loop {
            nodes.push(current);
            let next = self.align.get(&current).copied().unwrap_or(current);
            if next == root || next == current {
                break;
            }
            current = next;
        }

        nodes
    }

    /// Get all unique block roots.
    pub fn get_all_roots(&self) -> Vec<NodeIndex> {
        let mut roots: Vec<NodeIndex> = self.root.values().copied().collect();
        roots.sort();
        roots.dedup();
        roots
    }
}

/// Result of horizontal compaction for one alignment.
#[derive(Debug, Clone, Default)]
pub struct CompactionResult {
    /// X coordinate for each node.
    pub x: HashMap<NodeIndex, f64>,
}

/// The four alignment directions used by Brandes-Köpf.
///
/// The algorithm computes four different alignments and takes the median
/// of all four to produce balanced coordinates. Each direction represents
/// a combination of:
/// - Sweep direction: top-to-bottom (downward) or bottom-to-top (upward)
/// - Neighbor preference: prefer left or right median neighbor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlignmentDirection {
    /// Up-Left: sweep top-to-bottom, prefer left neighbors
    UL,
    /// Up-Right: sweep top-to-bottom, prefer right neighbors
    UR,
    /// Down-Left: sweep bottom-to-top, prefer left neighbors
    DL,
    /// Down-Right: sweep bottom-to-top, prefer right neighbors
    DR,
}

impl AlignmentDirection {
    /// Returns all four alignment directions.
    pub fn all() -> [Self; 4] {
        [Self::UL, Self::UR, Self::DL, Self::DR]
    }

    /// Whether this direction sweeps from top to bottom (downward).
    ///
    /// UL and UR sweep downward (processing layers from top to bottom).
    /// DL and DR sweep upward (processing layers from bottom to top).
    pub fn is_downward(&self) -> bool {
        matches!(self, Self::UL | Self::UR)
    }

    /// Whether this direction prefers left neighbors when there are two medians.
    ///
    /// When a node has an even number of neighbors, there are two median values.
    /// UL and DL prefer the left (lower index) median.
    /// UR and DR prefer the right (higher index) median.
    pub fn prefers_left(&self) -> bool {
        matches!(self, Self::UL | Self::DL)
    }
}

/// Configuration for the Brandes-Köpf algorithm.
#[derive(Debug, Clone)]
pub struct BKConfig {
    /// Minimum separation between adjacent real nodes.
    pub node_sep: f64,

    /// Minimum separation between adjacent dummy nodes (edge segments).
    pub edge_sep: f64,

    /// Layout direction - determines whether to use node width or height
    /// for separation calculations. For TD/BT, uses width. For LR/RL, uses height.
    pub direction: Direction,
}

impl Default for BKConfig {
    fn default() -> Self {
        Self {
            node_sep: 50.0,
            edge_sep: 20.0,
            direction: Direction::TopBottom,
        }
    }
}

// =============================================================================
// Helper Functions for Layer/Neighbor Traversal
// =============================================================================

/// Get all nodes grouped by layer (rank), sorted by position within each layer.
///
/// Returns a vector where index is the layer number and value is a vector of
/// node indices in that layer, sorted by their order within the layer.
#[cfg(test)]
pub fn get_layers(graph: &LayoutGraph) -> Vec<Vec<NodeIndex>> {
    let max_rank = graph.ranks.iter().max().copied().unwrap_or(0) as usize;
    let mut layers: Vec<Vec<NodeIndex>> = vec![Vec::new(); max_rank + 1];

    for (node, &rank) in graph.ranks.iter().enumerate() {
        if !graph.is_position_node(node) {
            continue;
        }
        layers[rank as usize].push(node);
    }

    // Sort each layer by order (position within layer)
    for layer in &mut layers {
        layer.sort_by_key(|&node| graph.order[node]);
    }

    layers
}

/// Get layers indexed by order (sparse, with None for gaps).
///
/// This mirrors dagre's buildLayerMatrix, preserving order positions so
/// conflict detection can reason about border boundaries.
fn get_layers_with_order(graph: &LayoutGraph) -> Vec<Vec<Option<NodeIndex>>> {
    let max_rank = graph.ranks.iter().max().copied().unwrap_or(0) as usize;
    let mut layers: Vec<Vec<Option<NodeIndex>>> = vec![Vec::new(); max_rank + 1];

    for (node, &rank) in graph.ranks.iter().enumerate() {
        if !graph.is_position_node(node) {
            continue;
        }
        let order = graph.order[node];
        let layer = &mut layers[rank as usize];
        if layer.len() <= order {
            layer.resize(order + 1, None);
        }
        layer[order] = Some(node);
    }

    layers
}

fn adjusted_layers_with_order(
    layers: &[Vec<Option<NodeIndex>>],
    direction: AlignmentDirection,
) -> Vec<Vec<Option<NodeIndex>>> {
    let mut adjusted = layers.to_vec();
    if !direction.is_downward() {
        adjusted.reverse();
    }
    if !direction.prefers_left() {
        for layer in &mut adjusted {
            layer.reverse();
        }
    }
    adjusted
}

fn compact_layers(layers: &[Vec<Option<NodeIndex>>]) -> Vec<Vec<NodeIndex>> {
    layers
        .iter()
        .map(|layer| layer.iter().filter_map(|n| *n).collect())
        .collect()
}

/// Get the layer indices in sweep order.
///
/// For downward sweep (UL, UR): layers 0, 1, 2, ... (top to bottom)
/// For upward sweep (DL, DR): layers n, n-1, ... 0 (bottom to top)
#[cfg(test)]
pub fn get_layers_in_order(num_layers: usize, downward: bool) -> Vec<usize> {
    if downward {
        (0..num_layers).collect()
    } else {
        (0..num_layers).rev().collect()
    }
}

/// Get the predecessors of a node (nodes in the layer above that connect to this node).
///
/// Returns node indices sorted by their position in their layer.
pub fn get_predecessors(graph: &LayoutGraph, node: NodeIndex) -> Vec<NodeIndex> {
    let effective_edges = graph.effective_edges();
    let mut preds: Vec<NodeIndex> = effective_edges
        .iter()
        .enumerate()
        .filter(|&(idx, &(from, to))| {
            to == node && !graph.excluded_edges.contains(&idx) && graph.is_position_node(from)
        })
        .map(|(_, &(from, _))| from)
        .collect();

    preds.sort_by_key(|&n| graph.order[n]);
    preds
}

/// Get the successors of a node (nodes in the layer below that this node connects to).
///
/// Returns node indices sorted by their position in their layer.
pub fn get_successors(graph: &LayoutGraph, node: NodeIndex) -> Vec<NodeIndex> {
    let effective_edges = graph.effective_edges();
    let mut succs: Vec<NodeIndex> = effective_edges
        .iter()
        .enumerate()
        .filter(|&(idx, &(from, to))| {
            from == node && !graph.excluded_edges.contains(&idx) && graph.is_position_node(to)
        })
        .map(|(_, &(_, to))| to)
        .collect();

    succs.sort_by_key(|&n| graph.order[n]);
    succs
}

/// Get neighbors based on sweep direction.
///
/// - Downward sweep (UL, UR): use predecessors (upper neighbors)
/// - Upward sweep (DL, DR): use successors (lower neighbors)
///
/// Returns neighbors sorted by position in their layer.
#[cfg(test)]
pub fn get_neighbors(graph: &LayoutGraph, node: NodeIndex, downward: bool) -> Vec<NodeIndex> {
    if downward {
        get_predecessors(graph, node)
    } else {
        get_successors(graph, node)
    }
}

/// Get the position (order) of a node within its layer.
#[inline]
#[cfg(test)]
pub fn get_position(graph: &LayoutGraph, node: NodeIndex) -> usize {
    graph.order[node]
}

/// Get the layer (rank) of a node.
#[inline]
#[cfg(test)]
pub fn get_layer(graph: &LayoutGraph, node: NodeIndex) -> usize {
    graph.ranks[node] as usize
}

/// Get the "width" of a node in the coordinate axis being optimized.
///
/// For TD/BT layouts, this returns the actual width (x-axis separation).
/// For LR/RL layouts, this returns the height (y-axis separation).
#[inline]
pub fn get_width(graph: &LayoutGraph, node: NodeIndex, direction: Direction) -> f64 {
    let (w, h) = graph.dimensions[node];
    if direction.is_horizontal() {
        h // LR/RL: optimize y-axis, so "width" is height
    } else {
        w // TD/BT: optimize x-axis, so "width" is width
    }
}

// =============================================================================
// Conflict Detection
// =============================================================================

/// Check if two segments cross.
///
/// Segments are defined by (upper_position, lower_position) where positions
/// are the node's order within its layer.
///
/// Two segments cross if one starts left and ends right of the other, or vice versa.
#[inline]
#[cfg(test)]
fn segments_cross(u1: usize, l1: usize, u2: usize, l2: usize) -> bool {
    (u1 < u2 && l1 > l2) || (u1 > u2 && l1 < l2)
}

/// Check if an edge is an inner segment (both endpoints are dummy/border nodes).
///
/// Inner segments are part of long edges that span multiple layers.
#[inline]
#[cfg(test)]
fn is_inner_segment(graph: &LayoutGraph, from: NodeIndex, to: NodeIndex) -> bool {
    is_dummy_like(graph, from) && is_dummy_like(graph, to)
}

/// Find all inner segments (edges between dummy/border nodes) between two adjacent layers.
///
/// Returns a vector of (upper_position, lower_position) tuples.
#[cfg(test)]
fn find_inner_segments(
    graph: &LayoutGraph,
    upper_layer: usize,
    lower_layer: usize,
) -> Vec<(usize, usize)> {
    let effective_edges = effective_edges_for_position(graph);
    let mut segments = Vec::new();

    for &(from, to) in &effective_edges {
        let from_layer = get_layer(graph, from);
        let to_layer = get_layer(graph, to);

        // Check if edge spans from upper to lower layer
        if from_layer != upper_layer || to_layer != lower_layer {
            continue;
        }

        // Check if both endpoints are dummy/border nodes (inner segment)
        if is_inner_segment(graph, from, to) {
            let from_pos = get_position(graph, from);
            let to_pos = get_position(graph, to);
            segments.push((from_pos, to_pos));
        }
    }

    segments
}

#[cfg(test)]
fn effective_edges_for_position(graph: &LayoutGraph) -> Vec<(usize, usize)> {
    graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(idx, &(from, to, _))| {
            if graph.excluded_edges.contains(&idx) {
                return None;
            }
            let (from, to) = if graph.reversed_edges.contains(&idx) {
                (to, from)
            } else {
                (from, to)
            };
            if !graph.is_position_node(from) || !graph.is_position_node(to) {
                return None;
            }
            Some((from, to))
        })
        .collect()
}

/// Find all Type-1 conflicts in the graph.
///
/// A Type-1 conflict occurs when a non-inner segment crosses an inner segment.
/// Inner segments are edges between dummy/border nodes (part of long edge normalization).
///
/// These conflicts are used during vertical alignment to prevent alignments
/// that would cause edge crossings.
pub fn find_type1_conflicts(graph: &LayoutGraph) -> ConflictSet {
    let mut conflicts = ConflictSet::new();
    let layers = get_layers_with_order(graph);
    if layers.len() < 2 {
        return conflicts;
    }

    for layer_idx in 1..layers.len() {
        let prev_layer = &layers[layer_idx - 1];
        let layer = &layers[layer_idx];

        let mut k0: isize = 0;
        let mut scan_pos: usize = 0;
        let prev_layer_len = prev_layer.len();
        let last_node = layer.last().copied().flatten();

        for (i, v) in layer.iter().enumerate() {
            let Some(v) = *v else {
                continue;
            };
            let w = find_other_inner_segment_node(graph, v);
            let k1 = w
                .map(|node| graph.order[node] as isize)
                .unwrap_or(prev_layer_len as isize);

            if w.is_some() || Some(v) == last_node {
                for scan_node_opt in layer.iter().take(i + 1).skip(scan_pos) {
                    let Some(scan_node) = *scan_node_opt else {
                        continue;
                    };
                    for u in get_predecessors(graph, scan_node) {
                        let u_pos = graph.order[u] as isize;
                        if (u_pos < k0 || k1 < u_pos)
                            && !(is_dummy_like(graph, u) && is_dummy_like(graph, scan_node))
                        {
                            let (a, b) = if u < scan_node {
                                (u, scan_node)
                            } else {
                                (scan_node, u)
                            };
                            conflicts.insert((a, b));
                        }
                    }
                }
                scan_pos = i + 1;
                k0 = k1;
            }
        }
    }

    conflicts
}

fn find_other_inner_segment_node(graph: &LayoutGraph, node: NodeIndex) -> Option<NodeIndex> {
    if !is_dummy_like(graph, node) {
        return None;
    }
    get_predecessors(graph, node)
        .into_iter()
        .find(|&u| is_dummy_like(graph, u))
}

/// Find all Type-2 conflicts in the graph.
///
/// A Type-2 conflict occurs between inner segments of different long edges
/// when they cross each other.
pub fn find_type2_conflicts(graph: &LayoutGraph) -> ConflictSet {
    let mut conflicts = ConflictSet::new();
    let layers = get_layers_with_order(graph);
    if layers.len() < 2 {
        return conflicts;
    }

    if std::env::var("MMDFLUX_DEBUG_CONFLICTS").is_ok_and(|v| v == "1") {
        debug_dump_layer_matrix(graph, &layers);
    }

    for layer in 1..layers.len() {
        let north = &layers[layer - 1];
        let south = &layers[layer];

        let mut prev_north_pos: isize = -1;
        let mut next_north_pos: Option<isize> = None;
        let mut south_pos: usize = 0;

        for (south_lookahead, slot) in south.iter().enumerate() {
            if let Some(v) = *slot
                && is_border_node(graph, v)
            {
                let predecessors = get_predecessors(graph, v);
                if let Some(&first) = predecessors.first() {
                    next_north_pos = Some(graph.order[first] as isize);
                    scan_type2_conflicts(
                        graph,
                        &mut conflicts,
                        south,
                        south_pos,
                        south_lookahead,
                        prev_north_pos,
                        next_north_pos.unwrap_or(north.len() as isize),
                    );
                    south_pos = south_lookahead;
                    prev_north_pos = next_north_pos.unwrap_or(prev_north_pos);
                }
            }

            let scan_prev = next_north_pos.unwrap_or(prev_north_pos);
            scan_type2_conflicts(
                graph,
                &mut conflicts,
                south,
                south_pos,
                south.len(),
                scan_prev,
                north.len() as isize,
            );
        }
    }

    conflicts
}

fn scan_type2_conflicts(
    graph: &LayoutGraph,
    conflicts: &mut ConflictSet,
    south: &[Option<NodeIndex>],
    south_pos: usize,
    south_end: usize,
    prev_north_border: isize,
    next_north_border: isize,
) {
    for v_opt in south.iter().take(south_end).skip(south_pos) {
        let Some(v) = *v_opt else {
            continue;
        };
        if !is_dummy_like(graph, v) {
            continue;
        }
        for u in get_predecessors(graph, v) {
            if !is_dummy_like(graph, u) {
                continue;
            }
            let u_pos = graph.order[u] as isize;
            if u_pos < prev_north_border || u_pos > next_north_border {
                let (a, b) = if u < v { (u, v) } else { (v, u) };
                conflicts.insert((a, b));
            }
        }
    }
}

fn is_border_node(graph: &LayoutGraph, node: NodeIndex) -> bool {
    graph.border_type.contains_key(&node)
        || graph.border_top.values().any(|&idx| idx == node)
        || graph.border_bottom.values().any(|&idx| idx == node)
}

pub(crate) fn is_dummy_like(graph: &LayoutGraph, node: NodeIndex) -> bool {
    graph.is_dummy_index(node) || is_border_node(graph, node)
}

fn debug_dump_layer_matrix(graph: &LayoutGraph, layers: &[Vec<Option<NodeIndex>>]) {
    eprintln!("[layer_matrix] start");
    for (rank, layer) in layers.iter().enumerate() {
        eprintln!("[layer_matrix] rank {}", rank);
        for (pos, slot) in layer.iter().enumerate() {
            let Some(node) = *slot else {
                continue;
            };
            let node_id = &graph.node_ids[node].0;
            let order = graph.order[node];
            let is_border = is_border_node(graph, node);
            let is_dummy_like = is_dummy_like(graph, node);
            let preds = get_predecessors(graph, node);
            let pred_entries: Vec<String> = preds
                .iter()
                .map(|&p| format!("{}(order={})", graph.node_ids[p].0, graph.order[p]))
                .collect();
            eprintln!(
                "[layer_matrix]   pos {} node={} order={} border={} dummy_like={} preds=[{}]",
                pos,
                node_id,
                order,
                is_border,
                is_dummy_like,
                pred_entries.join(", ")
            );
        }
    }
    eprintln!("[layer_matrix] end");
}

/// Find all conflicts (Type-1 and Type-2) in the graph.
pub fn find_all_conflicts(graph: &LayoutGraph) -> ConflictSet {
    let type1 = find_type1_conflicts(graph);
    let type2 = find_type2_conflicts(graph);

    if std::env::var("MMDFLUX_DEBUG_CONFLICTS").is_ok_and(|v| v == "1") {
        debug_log_conflicts(graph, &type1, &type2);
    }

    let mut conflicts = type1;
    conflicts.extend(type2);
    conflicts
}

fn debug_log_conflicts(graph: &LayoutGraph, type1: &ConflictSet, type2: &ConflictSet) {
    let layers = get_layers_with_order(graph);
    let mut node_layer: Vec<(usize, usize)> = vec![(0, 0); graph.node_ids.len()];
    for (li, layer) in layers.iter().enumerate() {
        for (pos, slot) in layer.iter().enumerate() {
            if let Some(idx) = *slot {
                node_layer[idx] = (li, pos);
            }
        }
    }

    let log_conflict = |conflict_type: &str, a: usize, b: usize| {
        let (la, pa) = node_layer[a];
        let (lb, pb) = node_layer[b];
        let layer = la.max(lb);
        eprintln!(
            "[conflicts] {} layer {} pos {}({}) vs pos {}({})",
            conflict_type, layer, pa, &graph.node_ids[a], pb, &graph.node_ids[b]
        );
    };

    for &(a, b) in type1 {
        log_conflict("type1", a, b);
    }
    for &(a, b) in type2 {
        log_conflict("type2", a, b);
    }
}

/// Check if aligning two positions would violate a conflict.
///
/// Used during vertical alignment to skip alignments that would cause crossings.
pub fn has_conflict(conflicts: &ConflictSet, v: NodeIndex, w: NodeIndex) -> bool {
    let (a, b) = if v < w { (v, w) } else { (w, v) };
    conflicts.contains(&(a, b))
}

// =============================================================================
// Vertical Alignment
// =============================================================================

/// Compute vertical alignment for one direction/bias combination.
///
/// Vertical alignment groups nodes into "blocks" that will share the same
/// x-coordinate. Nodes are aligned with their median neighbor if no conflict
/// prevents it.
///
/// # Arguments
/// * `graph` - The layered graph
/// * `conflicts` - Detected conflicts to respect
/// * `direction` - Which of the 4 alignment directions to use
///
/// # Returns
/// A BlockAlignment with root and align mappings
pub fn vertical_alignment(
    graph: &LayoutGraph,
    conflicts: &ConflictSet,
    direction: AlignmentDirection,
) -> BlockAlignment {
    let base_layers = get_layers_with_order(graph);
    let adjusted_layers = adjusted_layers_with_order(&base_layers, direction);
    let downward = direction.is_downward();
    vertical_alignment_with_layering(graph, &adjusted_layers, conflicts, downward)
}

fn vertical_alignment_with_layering(
    graph: &LayoutGraph,
    layers: &[Vec<Option<NodeIndex>>],
    conflicts: &ConflictSet,
    downward: bool,
) -> BlockAlignment {
    let all_nodes: Vec<NodeIndex> = (0..graph.node_ids.len()).collect();
    let mut alignment = BlockAlignment::new(&all_nodes);

    if layers.len() < 2 {
        return alignment;
    }

    let bk_trace = std::env::var("MMDFLUX_DEBUG_BK_TRACE").is_ok_and(|v| v == "1");

    let mut pos: HashMap<NodeIndex, isize> = HashMap::new();
    for layer in layers {
        for (order, slot) in layer.iter().enumerate() {
            if let Some(node) = *slot {
                pos.insert(node, order as isize);
            }
        }
    }

    for layer in layers {
        let mut prev_idx: isize = -1;
        for slot in layer {
            let Some(v) = *slot else {
                continue;
            };

            let mut neighbors = if downward {
                get_predecessors(graph, v)
            } else {
                get_successors(graph, v)
            };
            if neighbors.is_empty() {
                continue;
            }
            neighbors.sort_by_key(|n| pos.get(n).copied().unwrap_or(graph.order[*n] as isize));

            let mp = (neighbors.len() - 1) as f64 / 2.0;
            let start = mp.floor() as usize;
            let end = mp.ceil() as usize;

            for i in start..=end {
                let m = neighbors[i];
                if alignment.align.get(&v) != Some(&v) {
                    continue;
                }

                let m_pos = pos.get(&m).copied().unwrap_or(graph.order[m] as isize);
                let order_ok = prev_idx < m_pos;
                let conflict_free = !has_conflict(conflicts, v, m);

                if bk_trace && graph.border_type.contains_key(&v) {
                    let v_name = &graph.node_ids[v].0;
                    let neighbor_names: Vec<&str> = neighbors
                        .iter()
                        .map(|n| graph.node_ids[*n].0.as_str())
                        .collect();
                    let m_name = &graph.node_ids[m].0;
                    eprintln!(
                        "[BK] border node={} neighbors={:?} prev_idx={} conflict_free={} order_ok={} median_candidate={}",
                        v_name, neighbor_names, prev_idx, conflict_free, order_ok, m_name
                    );
                }

                if conflict_free && order_ok {
                    alignment.align.insert(m, v);

                    let m_root = alignment.get_root(m);
                    alignment.root.insert(v, m_root);
                    alignment.align.insert(v, m_root);
                    prev_idx = m_pos;

                    if bk_trace && graph.border_type.contains_key(&v) {
                        let v_name = &graph.node_ids[v].0;
                        let m_name = &graph.node_ids[m].0;
                        let m_root_name = &graph.node_ids[alignment.get_root(m)].0;
                        eprintln!(
                            "[BK]   -> ALIGNED {}  to {} (root={})",
                            v_name, m_name, m_root_name
                        );
                    }
                }
            }
        }
    }

    alignment
}

/// Get median neighbor(s) from a sorted list of neighbors.
///
/// For odd count: returns single true median.
/// For even count: returns both middle elements, ordered by preference.
#[cfg(test)]
fn get_medians(neighbors: &[NodeIndex], prefer_left: bool) -> Vec<NodeIndex> {
    let len = neighbors.len();
    if len <= 1 {
        return neighbors.to_vec();
    }

    let mid = len / 2;
    if len % 2 == 1 {
        vec![neighbors[mid]]
    } else if prefer_left {
        vec![neighbors[mid - 1], neighbors[mid]]
    } else {
        vec![neighbors[mid], neighbors[mid - 1]]
    }
}

// =============================================================================
// Horizontal Compaction
// =============================================================================

/// Compute x-coordinates for a single alignment using horizontal compaction.
///
/// This assigns x-coordinates to blocks such that:
/// 1. Nodes in the same block have the same x-coordinate
/// 2. Adjacent nodes in the same layer have at least `node_sep` separation
/// 3. The layout is as compact as possible
///
/// # Arguments
/// * `graph` - The layered graph
/// * `alignment` - The vertical alignment (blocks)
/// * `config` - Configuration including node separation
///
/// # Returns
/// A CompactionResult with x-coordinates for all nodes
#[cfg(test)]
pub fn horizontal_compaction(
    graph: &LayoutGraph,
    alignment: &BlockAlignment,
    config: &BKConfig,
) -> CompactionResult {
    let layers = get_layers(graph);
    horizontal_compaction_with_direction(graph, alignment, config, &layers, None)
}

/// Horizontal compaction with optional alignment direction for border guard.
///
/// When `direction` is provided and the graph has border nodes, Pass 2
/// skips pull-right for left border nodes in left-biased alignments
/// and right border nodes in right-biased alignments. This prevents
/// border nodes from crossing their subgraph boundary.
fn horizontal_compaction_with_direction(
    graph: &LayoutGraph,
    alignment: &BlockAlignment,
    config: &BKConfig,
    layers: &[Vec<NodeIndex>],
    direction: Option<AlignmentDirection>,
) -> CompactionResult {
    let mut result = CompactionResult::default();
    let num_nodes = graph.node_ids.len();

    // Build block graph with separation constraints
    let block_graph = build_block_graph(graph, alignment, layers, config);

    // Two-pass compaction on the block graph
    let mut xs: HashMap<NodeIndex, f64> = HashMap::new();

    // Pass 1: Assign smallest valid coordinates (topological order from sources)
    // Each block root is placed at the max of (predecessor_x + edge_weight)
    let topo = block_graph.topological_order();
    for &root in &topo {
        let x = block_graph
            .predecessors(root)
            .iter()
            .map(|&(pred, weight)| xs.get(&pred).copied().unwrap_or(0.0) + weight)
            .fold(0.0_f64, f64::max);
        xs.insert(root, x);
    }

    // Pass 2: Assign greatest valid coordinates (reverse topological order)
    // Pull blocks rightward to consume unused slack.
    //
    // For simple flowcharts (DAGs without compound/subgraph nodes), this pass
    // is a no-op: Pass 1's longest-path placement already satisfies all
    // separation constraints, so pull_right <= current for every block.
    //
    // For compound graphs, this pass applies a borderType guard: border nodes
    // matching the current alignment direction are skipped to prevent them
    // from crossing their subgraph boundary. See the guard logic below.
    for &root in topo.iter().rev() {
        let successors = block_graph.successors(root);
        if !successors.is_empty() {
            // borderType guard: skip pull-right for border nodes that match
            // the current alignment direction. This prevents border nodes from
            // crossing their subgraph boundary.
            // - Left borders: skip in left-biased alignments (UL, DL)
            // - Right borders: skip in right-biased alignments (UR, DR)
            // Reference: dagre.js positionX → horizontalCompaction
            if let Some(dir) = direction
                && let Some(&bt) = graph.border_type.get(&root)
            {
                // Dagre: reverseSep (right align) protects left borders,
                // left align protects right borders.
                let skip = match bt {
                    super::graph::BorderType::Left => !dir.prefers_left(),
                    super::graph::BorderType::Right => dir.prefers_left(),
                };
                if skip {
                    continue;
                }
            }

            let pull_right = successors
                .iter()
                .filter_map(|&(succ, weight)| xs.get(&succ).map(|&x| x - weight))
                .fold(f64::INFINITY, f64::min);
            if pull_right.is_finite() {
                let current = xs[&root];
                if pull_right > current {
                    xs.insert(root, pull_right);
                }
            }
        }
    }

    // Propagate: all nodes in block get root's coordinate
    for node in 0..num_nodes {
        let root = alignment.get_root(node);
        if let Some(&root_x) = xs.get(&root) {
            result.x.insert(node, root_x);
        }
    }

    // Normalize: shift so minimum x is 0
    let min_x = result.x.values().copied().fold(f64::INFINITY, f64::min);
    if min_x.is_finite() {
        for x in result.x.values_mut() {
            *x -= min_x;
        }
    }

    result
}

// =============================================================================
// Block Graph
// =============================================================================

/// A directed graph of block separation constraints for two-pass compaction.
/// Nodes are block roots. Edges carry minimum separation weights.
#[derive(Debug)]
struct BlockGraph {
    /// All block root node indices.
    nodes: Vec<NodeIndex>,
    /// Adjacency: node -> [(successor, weight)].
    out_edges: HashMap<NodeIndex, Vec<(NodeIndex, f64)>>,
    /// Reverse adjacency: node -> [(predecessor, weight)].
    in_edges: HashMap<NodeIndex, Vec<(NodeIndex, f64)>>,
}

impl BlockGraph {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            out_edges: HashMap::new(),
            in_edges: HashMap::new(),
        }
    }

    fn add_node(&mut self, root: NodeIndex) {
        if !self.nodes.contains(&root) {
            self.nodes.push(root);
        }
    }

    /// Add an edge or update to max weight if edge already exists.
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: f64) {
        let out = self.out_edges.entry(from).or_default();
        if let Some(entry) = out.iter_mut().find(|(n, _)| *n == to) {
            entry.1 = entry.1.max(weight);
        } else {
            out.push((to, weight));
        }

        let inp = self.in_edges.entry(to).or_default();
        if let Some(entry) = inp.iter_mut().find(|(n, _)| *n == from) {
            entry.1 = entry.1.max(weight);
        } else {
            inp.push((from, weight));
        }
    }

    fn predecessors(&self, node: NodeIndex) -> &[(NodeIndex, f64)] {
        self.in_edges
            .get(&node)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn successors(&self, node: NodeIndex) -> &[(NodeIndex, f64)] {
        self.out_edges
            .get(&node)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Kahn's algorithm for topological sort.
    fn topological_order(&self) -> Vec<NodeIndex> {
        let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
        for &n in &self.nodes {
            in_degree.insert(n, self.predecessors(n).len());
        }

        // Collect sources, sorted for determinism
        let mut sources: Vec<NodeIndex> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&n, _)| n)
            .collect();
        sources.sort();

        let mut queue: VecDeque<NodeIndex> = sources.into_iter().collect();
        let mut result = Vec::with_capacity(self.nodes.len());

        while let Some(node) = queue.pop_front() {
            result.push(node);
            // Collect and sort successors for determinism
            let mut succs: Vec<NodeIndex> = self
                .successors(node)
                .iter()
                .filter_map(|&(succ, _)| {
                    let deg = in_degree.get_mut(&succ).unwrap();
                    *deg -= 1;
                    if *deg == 0 { Some(succ) } else { None }
                })
                .collect();
            succs.sort();
            queue.extend(succs);
        }

        result
    }
}

/// Compute the minimum center-to-center separation between two adjacent nodes.
/// Uses `node_sep` for real nodes and `edge_sep` for dummy nodes, averaged between the pair.
fn compute_sep(graph: &LayoutGraph, left: NodeIndex, right: NodeIndex, config: &BKConfig) -> f64 {
    let left_width = get_width(graph, left, config.direction);
    let right_width = get_width(graph, right, config.direction);
    let left_sep = separation_for(graph, left, config);
    let right_sep = separation_for(graph, right, config);
    left_width / 2.0 + (left_sep + right_sep) / 2.0 + right_width / 2.0
}

/// Build a block graph from a vertical alignment.
///
/// Each unique block root becomes a node. For each pair of adjacent nodes
/// in a layer with different block roots, adds an edge with the separation weight.
/// Duplicate edges are merged by taking the maximum weight.
///
/// Mirrors dagre.js `buildBlockGraph()` (bk.js lines 267-287).
fn build_block_graph(
    graph: &LayoutGraph,
    alignment: &BlockAlignment,
    layers: &[Vec<NodeIndex>],
    config: &BKConfig,
) -> BlockGraph {
    let mut bg = BlockGraph::new();

    for &root in &alignment.get_all_roots() {
        bg.add_node(root);
    }

    for layer in layers {
        for i in 1..layer.len() {
            let left = layer[i - 1];
            let right = layer[i];
            let left_root = alignment.get_root(left);
            let right_root = alignment.get_root(right);

            if left_root != right_root {
                let weight = compute_sep(graph, left, right, config);
                bg.add_edge(left_root, right_root, weight);
            }
        }
    }

    bg
}

/// Get the separation value for a node: `edge_sep` for dummy nodes, `node_sep` for real nodes.
#[inline]
fn separation_for(graph: &LayoutGraph, node: NodeIndex, config: &BKConfig) -> f64 {
    if is_dummy_like(graph, node) {
        config.edge_sep
    } else {
        config.node_sep
    }
}

/// Calculate the total width of a compaction result.
pub fn calculate_width(
    graph: &LayoutGraph,
    result: &CompactionResult,
    direction: Direction,
) -> f64 {
    let (min_x, max_x) = find_bounds(graph, result, direction);
    if !(min_x.is_finite() && max_x.is_finite()) {
        return f64::INFINITY;
    }
    (max_x - min_x).max(0.0)
}

// =============================================================================
// Balance and Final Coordinate Assignment
// =============================================================================

/// Compute all four alignment/compaction results.
fn compute_all_alignments(
    graph: &LayoutGraph,
    conflicts: &ConflictSet,
    config: &BKConfig,
) -> HashMap<AlignmentDirection, CompactionResult> {
    let mut results = HashMap::new();
    let base_layers = get_layers_with_order(graph);

    for direction in AlignmentDirection::all() {
        let adjusted_layers = adjusted_layers_with_order(&base_layers, direction);
        let compact_layers = compact_layers(&adjusted_layers);
        let downward = direction.is_downward();
        let alignment =
            vertical_alignment_with_layering(graph, &adjusted_layers, conflicts, downward);
        let mut compaction = horizontal_compaction_with_direction(
            graph,
            &alignment,
            config,
            &compact_layers,
            Some(direction),
        );
        if !direction.prefers_left() {
            for x in compaction.x.values_mut() {
                *x = -*x;
            }
        }
        results.insert(direction, compaction);
    }

    results
}

/// Find the alignment with smallest total width.
fn find_smallest_width(
    graph: &LayoutGraph,
    results: &HashMap<AlignmentDirection, CompactionResult>,
    direction: Direction,
) -> AlignmentDirection {
    let mut best_dir: Option<AlignmentDirection> = None;
    let mut best_width = f64::INFINITY;

    for dir in AlignmentDirection::all() {
        if let Some(result) = results.get(&dir) {
            let width = calculate_width(graph, result, direction);
            if width.is_finite() && width < best_width {
                best_width = width;
                best_dir = Some(dir);
            }
        }
    }

    best_dir.unwrap_or(AlignmentDirection::UL)
}

/// Find the bounding box (min_x, max_x) of a compaction result.
fn find_bounds(graph: &LayoutGraph, result: &CompactionResult, direction: Direction) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;

    for node in 0..graph.node_ids.len() {
        if let Some(&x) = result.x.get(&node) {
            let width = get_width(graph, node, direction);
            min_x = min_x.min(x - width / 2.0);
            max_x = max_x.max(x + width / 2.0);
        }
    }

    (min_x, max_x)
}

/// Align all results to the smallest width result's bounds.
///
/// This shifts each alignment so they share common boundaries,
/// making the median more meaningful.
fn align_to_smallest(
    results: &mut HashMap<AlignmentDirection, CompactionResult>,
    smallest: AlignmentDirection,
) {
    let Some(smallest_result) = results.get(&smallest) else {
        return;
    };
    let (target_min, target_max) = find_center_bounds(smallest_result);
    if !(target_min.is_finite() && target_max.is_finite()) {
        return;
    }

    for (dir, result) in results.iter_mut() {
        if *dir == smallest {
            continue;
        }

        let (result_min, result_max) = find_center_bounds(result);
        if !(result_min.is_finite() && result_max.is_finite()) {
            continue;
        }
        let shift = if dir.prefers_left() {
            target_min - result_min
        } else {
            target_max - result_max
        };
        if !shift.is_finite() {
            continue;
        }

        // Apply shift to all coordinates
        for x in result.x.values_mut() {
            *x += shift;
        }
    }
}

/// Find the min/max of center coordinates for a compaction result.
///
/// This matches dagre's alignCoordinates behavior (center-based alignment).
fn find_center_bounds(result: &CompactionResult) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for &x in result.x.values() {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
    }
    (min_x, max_x)
}

/// Compute final x-coordinates as median of all 4 alignments.
fn balance(
    graph: &LayoutGraph,
    results: &HashMap<AlignmentDirection, CompactionResult>,
) -> HashMap<NodeIndex, f64> {
    let mut final_x = HashMap::new();

    for node in 0..graph.node_ids.len() {
        let mut xs: Vec<f64> = results
            .values()
            .filter_map(|r| r.x.get(&node).copied())
            .collect();

        if xs.is_empty() {
            continue;
        }

        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let len = xs.len();
        let mid = len / 2;
        let median = if len.is_multiple_of(2) {
            (xs[mid - 1] + xs[mid]) / 2.0
        } else {
            xs[mid]
        };

        final_x.insert(node, median);
    }

    final_x
}

// =============================================================================
// Main Algorithm Entry Point
// =============================================================================

/// Main entry point for Brandes-Köpf coordinate assignment.
///
/// Returns x-coordinates (center of node) for all nodes in the graph that
/// minimize total edge length while respecting separation constraints.
///
/// # Algorithm
///
/// 1. Find Type-1 and Type-2 conflicts between edges
/// 2. For each of 4 alignment directions (UL, UR, DL, DR):
///    a. Compute vertical alignment (group nodes into blocks)
///    b. Compute horizontal compaction (assign x-coordinates)
/// 3. Find the alignment with smallest width
/// 4. Align all results to the smallest's bounds
/// 5. Return median of all 4 alignments for each node
pub fn position_x(graph: &LayoutGraph, config: &BKConfig) -> HashMap<NodeIndex, f64> {
    if graph.node_ids.is_empty() {
        return HashMap::new();
    }
    let bk_trace = std::env::var("MMDFLUX_DEBUG_BK_TRACE").is_ok_and(|v| v == "1");

    // Step 1: Find all conflicts
    let conflicts = find_all_conflicts(graph);
    debug_dump_border_blocks(graph, &conflicts);

    // Step 2: Compute all 4 alignments
    let mut results = compute_all_alignments(graph, &conflicts, config);
    if bk_trace {
        for dir in AlignmentDirection::all() {
            if let Some(result) = results.get(&dir) {
                let (center_min, center_max) = find_center_bounds(result);
                let (bounds_min, bounds_max) = find_bounds(graph, result, config.direction);
                let width = calculate_width(graph, result, config.direction);
                let finite = result.x.values().filter(|x| x.is_finite()).count();
                eprintln!(
                    "[BK] {:?}: nodes={} finite={} center=[{center_min}, {center_max}] bounds=[{bounds_min}, {bounds_max}] width={width}",
                    dir,
                    result.x.len(),
                    finite,
                );
            }
        }
    }

    // Step 3: Find smallest width
    let smallest = find_smallest_width(graph, &results, config.direction);

    // Step 4: Align others to smallest's bounds
    align_to_smallest(&mut results, smallest);

    // Step 5: Balance (median of all 4)
    balance(graph, &results)
}

fn debug_dump_border_blocks(graph: &LayoutGraph, conflicts: &ConflictSet) {
    if !std::env::var("MMDFLUX_DEBUG_BORDER_BLOCKS").is_ok_and(|v| v == "1") {
        return;
    }

    let mut compounds: Vec<usize> = graph.compound_nodes.iter().copied().collect();
    compounds.sort_by_key(|&idx| graph.node_ids[idx].0.clone());

    eprintln!("[border_blocks] block roots");
    for dir in AlignmentDirection::all() {
        let alignment = vertical_alignment(graph, conflicts, dir);
        eprintln!("[border_blocks] {:?}", dir);

        for compound_idx in &compounds {
            let name = &graph.node_ids[*compound_idx].0;
            eprintln!("[border_blocks]   {}", name);

            let mut nodes: Vec<NodeIndex> = Vec::new();
            if let Some(left) = graph.border_left.get(compound_idx) {
                nodes.extend(left.iter().copied());
            }
            if let Some(right) = graph.border_right.get(compound_idx) {
                nodes.extend(right.iter().copied());
            }
            if let Some(&top) = graph.border_top.get(compound_idx) {
                nodes.push(top);
            }
            if let Some(&bot) = graph.border_bottom.get(compound_idx) {
                nodes.push(bot);
            }

            nodes.sort_by_key(|&idx| (graph.ranks[idx], graph.order[idx], idx));
            nodes.dedup();

            for idx in nodes {
                let root = alignment.get_root(idx);
                let root_id = &graph.node_ids[root].0;
                eprintln!(
                    "[border_blocks]     {} rank={} order={} root={}",
                    graph.node_ids[idx].0, graph.ranks[idx], graph.order[idx], root_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layered::graph::{BorderType, DiGraph};
    use crate::layered::normalize::{DummyNode, DummyType, LabelPos, LabelSide};
    use crate::layered::{LayoutConfig, order, rank};

    /// Test helper: check if two nodes are in the same block
    fn same_block(alignment: &BlockAlignment, v: NodeIndex, w: NodeIndex) -> bool {
        alignment.get_root(v) == alignment.get_root(w)
    }

    /// Create a diamond-shaped test graph:
    /// ```text
    /// Layer 0:    [A]
    /// Layer 1:  [B] [C]
    /// Layer 2:    [D]
    /// ```
    /// Edges: A->B, A->C, B->D, C->D
    fn make_diamond_graph() -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_node("C", (100.0, 50.0));
        graph.add_node("D", (100.0, 50.0));
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Set order within layers: B before C
        // A is alone in layer 0 (order 0)
        // B at order 0, C at order 1 in layer 1
        // D is alone in layer 2 (order 0)
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        lg.order[a] = 0;
        lg.order[b] = 0;
        lg.order[c] = 1;
        lg.order[d] = 0;

        lg
    }

    #[test]
    fn test_block_alignment_new() {
        let nodes = vec![0, 1, 2, 3];
        let alignment = BlockAlignment::new(&nodes);

        // Each node should be its own root
        for &node in &nodes {
            assert_eq!(alignment.get_root(node), node);
            assert_eq!(alignment.align.get(&node), Some(&node));
        }
    }

    #[test]
    fn test_alignment_direction_properties() {
        // Downward sweep
        assert!(AlignmentDirection::UL.is_downward());
        assert!(AlignmentDirection::UR.is_downward());
        assert!(!AlignmentDirection::DL.is_downward());
        assert!(!AlignmentDirection::DR.is_downward());

        // Left preference
        assert!(AlignmentDirection::UL.prefers_left());
        assert!(!AlignmentDirection::UR.prefers_left());
        assert!(AlignmentDirection::DL.prefers_left());
        assert!(!AlignmentDirection::DR.prefers_left());
    }

    #[test]
    fn test_alignment_direction_all() {
        let all = AlignmentDirection::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&AlignmentDirection::UL));
        assert!(all.contains(&AlignmentDirection::UR));
        assert!(all.contains(&AlignmentDirection::DL));
        assert!(all.contains(&AlignmentDirection::DR));
    }

    #[test]
    fn test_compaction_result_default() {
        let result = CompactionResult::default();
        assert!(result.x.is_empty());
    }

    #[test]
    fn test_bk_config_default() {
        let config = BKConfig::default();
        assert_eq!(config.node_sep, 50.0);
        assert_eq!(config.direction, Direction::TopBottom);
    }

    // =========================================================================
    // Helper Function Tests
    // =========================================================================

    #[test]
    fn test_get_layers() {
        let lg = make_diamond_graph();
        let layers = get_layers(&lg);

        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].len(), 1); // A
        assert_eq!(layers[1].len(), 2); // B, C
        assert_eq!(layers[2].len(), 1); // D

        // Check that layer 1 is sorted by order (B before C)
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        assert_eq!(layers[1][0], b);
        assert_eq!(layers[1][1], c);
    }

    #[test]
    fn test_get_layers_excludes_compound_parents_and_root() {
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("sg", ());
        g.add_node("A", ());
        g.set_parent("A", "sg");

        let mut lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));

        let root_idx = lg.add_nesting_node("_nesting_root".into());
        lg.nesting_root = Some(root_idx);
        lg.position_excluded_nodes.insert(root_idx);

        let sg_idx = lg.node_index[&"sg".into()];
        let a_idx = lg.node_index[&"A".into()];

        lg.ranks[sg_idx] = 0;
        lg.ranks[a_idx] = 0;
        lg.ranks[root_idx] = 0;

        let layers = get_layers(&lg);

        assert!(layers[0].contains(&a_idx));
        assert!(
            !layers[0].contains(&sg_idx),
            "compound parent should be excluded"
        );
        assert!(
            !layers[0].contains(&root_idx),
            "nesting root should be excluded"
        );
    }

    #[test]
    fn test_get_neighbors_skips_excluded_nodes() {
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("A", ());
        g.add_node("B", ());
        g.add_node("sg", ());
        g.set_parent("B", "sg");
        g.add_edge("A", "B");

        let lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));
        let sg_idx = lg.node_index[&"sg".into()];
        let a_idx = lg.node_index[&"A".into()];
        let b_idx = lg.node_index[&"B".into()];

        assert!(!lg.is_position_node(sg_idx));

        let preds = get_predecessors(&lg, b_idx);
        assert_eq!(preds, vec![a_idx]);
    }

    #[test]
    fn test_get_layers_in_order_downward() {
        let order = get_layers_in_order(3, true);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn test_get_layers_in_order_upward() {
        let order = get_layers_in_order(3, false);
        assert_eq!(order, vec![2, 1, 0]);
    }

    #[test]
    fn test_get_predecessors() {
        let lg = make_diamond_graph();
        let d = lg.node_index[&"D".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        let preds = get_predecessors(&lg, d);
        // D has predecessors B and C, sorted by order (B=0, C=1)
        assert_eq!(preds.len(), 2);
        assert_eq!(preds[0], b);
        assert_eq!(preds[1], c);
    }

    #[test]
    fn test_get_successors() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        let succs = get_successors(&lg, a);
        // A has successors B and C, sorted by order (B=0, C=1)
        assert_eq!(succs.len(), 2);
        assert_eq!(succs[0], b);
        assert_eq!(succs[1], c);
    }

    #[test]
    fn test_get_neighbors_downward() {
        let lg = make_diamond_graph();
        let d = lg.node_index[&"D".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // Downward sweep: use predecessors
        let neighbors = get_neighbors(&lg, d, true);
        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0], b);
        assert_eq!(neighbors[1], c);
    }

    #[test]
    fn test_get_neighbors_upward() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // Upward sweep: use successors
        let neighbors = get_neighbors(&lg, a, false);
        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0], b);
        assert_eq!(neighbors[1], c);
    }

    #[test]
    fn test_get_position() {
        let lg = make_diamond_graph();
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        assert_eq!(get_position(&lg, b), 0);
        assert_eq!(get_position(&lg, c), 1);
    }

    #[test]
    fn test_get_layer() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let d = lg.node_index[&"D".into()];

        assert_eq!(get_layer(&lg, a), 0);
        assert_eq!(get_layer(&lg, b), 1);
        assert_eq!(get_layer(&lg, d), 2);
    }

    #[test]
    fn test_get_width() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];

        // For TD/BT layouts, get_width returns the actual width
        assert_eq!(get_width(&lg, a, Direction::TopBottom), 100.0);
        // For LR/RL layouts, get_width returns the height
        assert_eq!(get_width(&lg, a, Direction::LeftRight), 50.0);
    }

    // =========================================================================
    // Conflict Detection Tests
    // =========================================================================

    #[test]
    fn test_segments_cross_yes() {
        // Segment 1: upper=0, lower=2
        // Segment 2: upper=1, lower=0
        // These cross (one goes right, the other goes left)
        assert!(segments_cross(0, 2, 1, 0));
    }

    #[test]
    fn test_segments_cross_yes_reverse() {
        // Segment 1: upper=1, lower=0
        // Segment 2: upper=0, lower=2
        // These cross (one goes left, the other goes right)
        assert!(segments_cross(1, 0, 0, 2));
    }

    #[test]
    fn test_segments_cross_no_parallel() {
        // Segment 1: upper=0, lower=0
        // Segment 2: upper=1, lower=1
        // These don't cross (parallel/straight down)
        assert!(!segments_cross(0, 0, 1, 1));
    }

    #[test]
    fn test_segments_cross_no_diverging() {
        // Segment 1: upper=0, lower=0
        // Segment 2: upper=1, lower=2
        // These don't cross (diverging)
        assert!(!segments_cross(0, 0, 1, 2));
    }

    #[test]
    fn test_segments_cross_same_start() {
        // Segment 1: upper=0, lower=1
        // Segment 2: upper=0, lower=2
        // Same start, don't cross
        assert!(!segments_cross(0, 1, 0, 2));
    }

    #[test]
    fn test_segments_cross_same_end() {
        // Segment 1: upper=0, lower=2
        // Segment 2: upper=1, lower=2
        // Same end, don't cross
        assert!(!segments_cross(0, 2, 1, 2));
    }

    #[test]
    fn test_has_conflict_basic() {
        let mut conflicts = ConflictSet::new();
        conflicts.insert((1, 3));

        assert!(has_conflict(&conflicts, 1, 3));
        assert!(has_conflict(&conflicts, 3, 1));
        assert!(!has_conflict(&conflicts, 1, 2));
    }

    #[test]
    fn test_find_inner_segments_no_dummies() {
        let lg = make_diamond_graph();

        // Diamond graph has no dummy nodes, so no inner segments
        let segments = find_inner_segments(&lg, 0, 1);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_find_type1_conflicts_no_dummies() {
        let lg = make_diamond_graph();

        // No dummy nodes means no inner segments, so no Type-1 conflicts
        let conflicts = find_type1_conflicts(&lg);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_find_type2_conflicts_no_dummies() {
        let lg = make_diamond_graph();

        // No dummy nodes means no inner segments, so no Type-2 conflicts
        let conflicts = find_type2_conflicts(&lg);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_find_all_conflicts_no_dummies() {
        let lg = make_diamond_graph();

        // No dummy nodes means no conflicts
        let conflicts = find_all_conflicts(&lg);
        assert!(conflicts.is_empty());
    }

    // =========================================================================
    // Vertical Alignment Tests
    // =========================================================================

    /// Create a simple chain graph: A -> B -> C
    fn make_chain_graph() -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        graph.add_node("C", (100.0, 50.0));
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Set order within layers (all alone in their layers)
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        lg.order[a] = 0;
        lg.order[b] = 0;
        lg.order[c] = 0;

        lg
    }

    #[test]
    fn test_get_medians_single() {
        let neighbors = vec![5];
        let medians = get_medians(&neighbors, true);
        assert_eq!(medians, vec![5]);
    }

    #[test]
    fn test_get_medians_odd() {
        let neighbors = vec![1, 2, 3];
        let medians = get_medians(&neighbors, true);
        assert_eq!(medians, vec![2]);
    }

    #[test]
    fn test_get_medians_even_prefer_left() {
        let neighbors = vec![1, 2, 3, 4];
        let medians = get_medians(&neighbors, true);
        assert_eq!(medians, vec![2, 3]); // Left median first
    }

    #[test]
    fn test_get_medians_even_prefer_right() {
        let neighbors = vec![1, 2, 3, 4];
        let medians = get_medians(&neighbors, false);
        assert_eq!(medians, vec![3, 2]); // Right median first
    }

    #[test]
    fn test_vertical_alignment_chain_downward() {
        let lg = make_chain_graph();
        let conflicts = ConflictSet::new();

        // UL: sweep top-to-bottom, prefer left
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // All should be in the same block with A as root
        // (A is processed first, B aligns with A, C aligns with B)
        assert!(
            same_block(&alignment, a, b),
            "A and B should be in same block"
        );
        assert!(
            same_block(&alignment, b, c),
            "B and C should be in same block"
        );
        assert!(
            same_block(&alignment, a, c),
            "A and C should be in same block"
        );

        // A should be the root (it's at the top)
        assert_eq!(alignment.get_root(a), a);
        assert_eq!(alignment.get_root(b), a);
        assert_eq!(alignment.get_root(c), a);
    }

    #[test]
    fn test_vertical_alignment_chain_upward() {
        let lg = make_chain_graph();
        let conflicts = ConflictSet::new();

        // DL: sweep bottom-to-top, prefer left
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::DL);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // All should be in the same block with C as root
        // (C is processed first when sweeping bottom-to-top)
        assert!(same_block(&alignment, a, b));
        assert!(same_block(&alignment, b, c));

        // C should be the root (it's at the bottom, processed first in upward sweep)
        assert_eq!(alignment.get_root(c), c);
    }

    #[test]
    fn test_vertical_alignment_diamond() {
        let lg = make_diamond_graph();
        let conflicts = ConflictSet::new();

        // UL: sweep top-to-bottom, prefer left
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let _c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        // In a diamond A -> B,C -> D:
        // - B and C both have A as median (prefer_left, so B comes first)
        // - D has B and C as medians (prefer_left, so B comes first)
        //
        // Expected for UL:
        // - A is root of {A, B} (B aligns with A)
        // - C might form its own block (if order constraint prevents alignment)
        // - D aligns with B (left median of [B,C])

        // A and B should be in the same block
        assert!(
            same_block(&alignment, a, b),
            "A and B should be in same block"
        );

        // D should be in the same block as A/B (aligned through B)
        assert!(
            same_block(&alignment, a, d),
            "A and D should be in same block"
        );
    }

    #[test]
    fn test_vertical_alignment_empty_graph() {
        let graph: DiGraph<(f64, f64)> = DiGraph::new();
        let lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        let conflicts = ConflictSet::new();

        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);

        // Should handle empty graph gracefully
        assert!(alignment.root.is_empty());
    }

    #[test]
    fn test_vertical_alignment_single_node() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        let conflicts = ConflictSet::new();

        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);

        // Single node should be its own root
        assert_eq!(alignment.get_root(0), 0);
    }

    #[test]
    fn test_block_alignment_get_block_nodes() {
        let mut alignment = BlockAlignment::new(&[0, 1, 2, 3]);

        // Create block: 0 -> 1 -> 2 (root is 0)
        alignment.align.insert(0, 1);
        alignment.align.insert(1, 2);
        alignment.align.insert(2, 0); // cycle back to root
        alignment.root.insert(1, 0);
        alignment.root.insert(2, 0);

        let nodes = alignment.get_block_nodes(1);
        assert_eq!(nodes.len(), 3);
        assert!(nodes.contains(&0));
        assert!(nodes.contains(&1));
        assert!(nodes.contains(&2));
    }

    #[test]
    fn test_block_alignment_get_all_roots() {
        let mut alignment = BlockAlignment::new(&[0, 1, 2, 3]);

        // Create two blocks: {0, 1} and {2, 3}
        // Block 1: node 1 points to 0, root of 1 is 0
        alignment.align.insert(1, 0);
        alignment.root.insert(1, 0);
        // Block 2: node 3 points to 2, root of 3 is 2
        alignment.align.insert(3, 2);
        alignment.root.insert(3, 2);

        let roots = alignment.get_all_roots();
        assert_eq!(roots.len(), 2);
    }

    // =========================================================================
    // Horizontal Compaction Tests
    // =========================================================================

    /// Create a two-node layer graph:
    /// ```text
    /// Layer 0: [A] [B]
    /// ```
    fn make_two_node_layer() -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        // No edges - both in same layer

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        // Both at rank 0
        lg.ranks[0] = 0;
        lg.ranks[1] = 0;
        // A at position 0, B at position 1
        lg.order[0] = 0;
        lg.order[1] = 1;

        lg
    }

    #[test]
    fn test_horizontal_compaction_chain() {
        let lg = make_chain_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // Create alignment where all are in one block
        let mut alignment = BlockAlignment::new(&[a, b, c]);

        // Build proper block: a -> b -> c -> a (circular)
        alignment.root.insert(a, a);
        alignment.root.insert(b, a);
        alignment.root.insert(c, a);
        alignment.align.insert(a, b);
        alignment.align.insert(b, c);
        alignment.align.insert(c, a);

        let config = BKConfig::default();
        let result = horizontal_compaction(&lg, &alignment, &config);

        // All nodes should have the same x (they're in one block)
        let x_a = result.x.get(&a).unwrap();
        let x_b = result.x.get(&b).unwrap();
        let x_c = result.x.get(&c).unwrap();

        assert_eq!(*x_a, *x_b, "A and B should have same x");
        assert_eq!(*x_b, *x_c, "B and C should have same x");
    }

    #[test]
    fn test_horizontal_compaction_two_nodes_same_layer() {
        let lg = make_two_node_layer();

        // Separate blocks (each node is its own block)
        let alignment = BlockAlignment::new(&[0, 1]);

        let config = BKConfig {
            node_sep: 50.0,
            ..Default::default()
        };
        let result = horizontal_compaction(&lg, &alignment, &config);

        let x0 = result.x.get(&0).unwrap();
        let x1 = result.x.get(&1).unwrap();

        // B should be to the right of A
        assert!(
            x1 > x0,
            "B (x={}) should be to the right of A (x={})",
            x1,
            x0
        );

        // Check separation: distance between centers should be at least
        // (width_A/2 + width_B/2 + node_sep)
        let min_sep = 100.0 / 2.0 + 100.0 / 2.0 + 50.0; // 150.0
        let actual_sep = x1 - x0;
        assert!(
            actual_sep >= min_sep,
            "Separation {} should be >= {}",
            actual_sep,
            min_sep
        );
    }

    #[test]
    fn test_horizontal_compaction_diamond() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        // Separate blocks for all nodes
        let alignment = BlockAlignment::new(&[a, b, c, d]);

        let config = BKConfig::default();
        let result = horizontal_compaction(&lg, &alignment, &config);

        // B and C are in the same layer, B has order 0, C has order 1
        let x_b = result.x.get(&b).unwrap();
        let x_c = result.x.get(&c).unwrap();

        // C should be to the right of B
        assert!(x_c > x_b, "C should be to the right of B");
    }

    #[test]
    fn test_calculate_width() {
        let lg = make_diamond_graph();
        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        let alignment = BlockAlignment::new(&[a, b, c, d]);
        let config = BKConfig::default();
        let result = horizontal_compaction(&lg, &alignment, &config);

        let width = calculate_width(&lg, &result, config.direction);
        assert!(width > 0.0, "Width should be positive");
    }

    #[test]
    fn test_horizontal_compaction_empty() {
        let graph: DiGraph<(f64, f64)> = DiGraph::new();
        let lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        let alignment = BlockAlignment::new(&[]);
        let config = BKConfig::default();

        let result = horizontal_compaction(&lg, &alignment, &config);

        assert!(result.x.is_empty());
    }

    // =========================================================================
    // Balance and Full Algorithm Tests
    // =========================================================================

    #[test]
    fn test_balance_produces_median() {
        // Create mock results with known values
        let mut results: HashMap<AlignmentDirection, CompactionResult> = HashMap::new();

        let mut ul = CompactionResult::default();
        ul.x.insert(0, 10.0);
        results.insert(AlignmentDirection::UL, ul);

        let mut ur = CompactionResult::default();
        ur.x.insert(0, 20.0);
        results.insert(AlignmentDirection::UR, ur);

        let mut dl = CompactionResult::default();
        dl.x.insert(0, 30.0);
        results.insert(AlignmentDirection::DL, dl);

        let mut dr = CompactionResult::default();
        dr.x.insert(0, 40.0);
        results.insert(AlignmentDirection::DR, dr);

        // Create minimal graph with one node
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        let lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

        let final_x = balance(&lg, &results);

        // Median of [10, 20, 30, 40] = (20 + 30) / 2 = 25
        assert_eq!(final_x.get(&0), Some(&25.0));
    }

    #[test]
    fn test_position_x_chain() {
        let lg = make_chain_graph();
        let config = BKConfig::default();

        let x_coords = position_x(&lg, &config);

        // All nodes should have x-coordinates
        assert_eq!(x_coords.len(), 3);

        // All should have valid (finite) coordinates
        for &x in x_coords.values() {
            assert!(x.is_finite());
        }
    }

    #[test]
    fn test_position_x_diamond() {
        let lg = make_diamond_graph();
        let config = BKConfig::default();

        let x_coords = position_x(&lg, &config);

        // All nodes should have x-coordinates
        assert_eq!(x_coords.len(), 4);

        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        let x_b = x_coords.get(&b).unwrap();
        let x_c = x_coords.get(&c).unwrap();

        // B and C should have different x-coordinates (they're in the same layer)
        assert!(
            (x_b - x_c).abs() > 1.0,
            "B and C should be separated, got B={}, C={}",
            x_b,
            x_c
        );
    }

    #[test]
    fn test_position_x_empty() {
        let graph: DiGraph<(f64, f64)> = DiGraph::new();
        let lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        let config = BKConfig::default();

        let x_coords = position_x(&lg, &config);

        assert!(x_coords.is_empty());
    }

    #[test]
    fn test_position_x_single_node() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let config = BKConfig::default();
        let x_coords = position_x(&lg, &config);

        assert_eq!(x_coords.len(), 1);
        assert!(x_coords.get(&0).unwrap().is_finite());
    }

    #[test]
    fn test_find_bounds() {
        let mut result = CompactionResult::default();
        result.x.insert(0, 50.0); // center at 50, width 100 -> bounds [0, 100]
        result.x.insert(1, 200.0); // center at 200, width 100 -> bounds [150, 250]

        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("B", (100.0, 50.0));
        let lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);

        let (min_x, max_x) = find_bounds(&lg, &result, Direction::TopBottom);

        assert_eq!(min_x, 0.0);
        assert_eq!(max_x, 250.0);
    }

    #[test]
    fn test_compute_all_alignments() {
        let lg = make_diamond_graph();
        let conflicts = ConflictSet::new();
        let config = BKConfig::default();

        let results = compute_all_alignments(&lg, &conflicts, &config);

        // Should have all 4 alignments
        assert_eq!(results.len(), 4);
        assert!(results.contains_key(&AlignmentDirection::UL));
        assert!(results.contains_key(&AlignmentDirection::UR));
        assert!(results.contains_key(&AlignmentDirection::DL));
        assert!(results.contains_key(&AlignmentDirection::DR));

        // Each should have coordinates for all 4 nodes
        for result in results.values() {
            assert_eq!(result.x.len(), 4);
        }
    }

    /// Helper to create a two-node same-layer LayoutGraph, optionally marking nodes as dummy.
    fn make_two_node_graph(dims: [(f64, f64); 2], dummy_flags: [bool; 2]) -> LayoutGraph {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("N0", dims[0]);
        graph.add_node("N1", dims[1]);

        let mut lg = LayoutGraph::from_digraph(&graph, |_, d| *d);
        lg.ranks = vec![0, 0];
        lg.order = vec![0, 1];

        for (i, &is_dummy) in dummy_flags.iter().enumerate() {
            if is_dummy {
                let id = lg.node_ids[i].clone();
                lg.dummy_nodes.insert(
                    id,
                    DummyNode {
                        dummy_type: DummyType::Edge,
                        edge_index: 0,
                        rank: 0,
                        width: dims[i].0,
                        height: dims[i].1,
                        label_pos: LabelPos::Center,
                        label_side: LabelSide::Center,
                    },
                );
            }
        }

        lg
    }

    #[test]
    fn test_edge_sep_for_dummy_nodes() {
        let lg = make_two_node_graph([(1.0, 1.0), (1.0, 1.0)], [true, true]);
        let alignment = BlockAlignment::new(&[0, 1]);
        let config = BKConfig {
            node_sep: 50.0,
            edge_sep: 10.0,
            direction: Direction::TopBottom,
        };
        let result = horizontal_compaction(&lg, &alignment, &config);

        let x0 = result.x.get(&0).unwrap();
        let x1 = result.x.get(&1).unwrap();
        let actual_sep = x1 - x0;

        // Two dummy nodes: separation = width/2 + width/2 + edge_sep = 0.5 + 0.5 + 10.0 = 11.0
        let expected_min = 1.0 / 2.0 + 1.0 / 2.0 + 10.0;
        assert!(
            actual_sep >= expected_min - 0.01,
            "Dummy separation {} should be >= {} (edge_sep=10)",
            actual_sep,
            expected_min
        );
        // Must NOT use node_sep (would give 51.0)
        let node_sep_min = 1.0 / 2.0 + 1.0 / 2.0 + 50.0;
        assert!(
            actual_sep < node_sep_min,
            "Dummy separation {} should be < {} (should NOT use node_sep)",
            actual_sep,
            node_sep_min
        );
    }

    #[test]
    fn test_real_nodes_still_use_node_sep() {
        let lg = make_two_node_graph([(100.0, 50.0), (100.0, 50.0)], [false, false]);
        let alignment = BlockAlignment::new(&[0, 1]);
        let config = BKConfig {
            node_sep: 50.0,
            edge_sep: 10.0,
            direction: Direction::TopBottom,
        };
        let result = horizontal_compaction(&lg, &alignment, &config);

        let x0 = result.x.get(&0).unwrap();
        let x1 = result.x.get(&1).unwrap();
        let actual_sep = x1 - x0;

        // Two real nodes: separation = 50 + 50 + 50 = 150
        let expected_min = 100.0 / 2.0 + 100.0 / 2.0 + 50.0;
        assert!(
            actual_sep >= expected_min - 0.01,
            "Real node separation {} should be >= {} (node_sep=50)",
            actual_sep,
            expected_min
        );
    }

    #[test]
    fn test_mixed_dummy_real_separation() {
        let lg = make_two_node_graph([(100.0, 50.0), (1.0, 1.0)], [false, true]);
        let alignment = BlockAlignment::new(&[0, 1]);
        let config = BKConfig {
            node_sep: 50.0,
            edge_sep: 10.0,
            direction: Direction::TopBottom,
        };
        let result = horizontal_compaction(&lg, &alignment, &config);

        let x_real = result.x.get(&0).unwrap();
        let x_dummy = result.x.get(&1).unwrap();
        let actual_sep = x_dummy - x_real;

        // Mixed: sep = (node_sep + edge_sep) / 2 = (50+10)/2 = 30
        // min_separation = 50 + 0.5 + 30 = 80.5
        let expected_min = 100.0 / 2.0 + 1.0 / 2.0 + (50.0 + 10.0) / 2.0;
        assert!(
            actual_sep >= expected_min - 0.01,
            "Mixed separation {} should be >= {} (avg of node_sep and edge_sep)",
            actual_sep,
            expected_min
        );
    }

    // =========================================================================
    // BlockGraph Tests (Task 1.1)
    // =========================================================================

    #[test]
    fn test_block_graph_empty() {
        let bg = BlockGraph::new();
        assert!(bg.nodes.is_empty());
        assert_eq!(bg.topological_order().len(), 0);
    }

    #[test]
    fn test_block_graph_single_node() {
        let mut bg = BlockGraph::new();
        bg.add_node(0);
        assert_eq!(bg.nodes.len(), 1);
        assert_eq!(bg.predecessors(0).len(), 0);
        assert_eq!(bg.successors(0).len(), 0);
        assert_eq!(bg.topological_order(), vec![0]);
    }

    #[test]
    fn test_block_graph_chain() {
        let mut bg = BlockGraph::new();
        bg.add_node(0);
        bg.add_node(1);
        bg.add_node(2);
        bg.add_edge(0, 1, 50.0);
        bg.add_edge(1, 2, 50.0);

        assert_eq!(bg.predecessors(1).len(), 1);
        assert_eq!(bg.predecessors(1)[0], (0, 50.0));
        assert_eq!(bg.successors(1).len(), 1);
        assert_eq!(bg.successors(1)[0], (2, 50.0));

        let topo = bg.topological_order();
        let pos_0 = topo.iter().position(|&n| n == 0).unwrap();
        let pos_1 = topo.iter().position(|&n| n == 1).unwrap();
        let pos_2 = topo.iter().position(|&n| n == 2).unwrap();
        assert!(pos_0 < pos_1);
        assert!(pos_1 < pos_2);
    }

    #[test]
    fn test_block_graph_max_weight_edge() {
        let mut bg = BlockGraph::new();
        bg.add_node(0);
        bg.add_node(1);
        bg.add_edge(0, 1, 30.0);
        bg.add_edge(0, 1, 50.0);

        assert_eq!(bg.successors(0).len(), 1);
        assert_eq!(bg.successors(0)[0], (1, 50.0));
        assert_eq!(bg.predecessors(1)[0], (0, 50.0));
    }

    // =========================================================================
    // compute_sep Tests (Task 1.2)
    // =========================================================================

    #[test]
    fn test_compute_sep_real_nodes() {
        let lg = make_diamond_graph();
        let config = BKConfig::default(); // node_sep=50, edge_sep=20
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // Both real: b_width/2 + (node_sep+node_sep)/2 + c_width/2 = 50 + 50 + 50 = 150
        let sep = compute_sep(&lg, b, c, &config);
        assert_eq!(sep, 150.0);
    }

    #[test]
    fn test_compute_sep_dummy_nodes() {
        let lg = make_two_node_graph([(1.0, 1.0), (1.0, 1.0)], [true, true]);
        let config = BKConfig {
            node_sep: 50.0,
            edge_sep: 10.0,
            direction: Direction::TopBottom,
        };

        // Both dummy: 0.5 + (10+10)/2 + 0.5 = 11.0
        let sep = compute_sep(&lg, 0, 1, &config);
        assert_eq!(sep, 11.0);
    }

    #[test]
    fn test_compute_sep_mixed() {
        let lg = make_two_node_graph([(100.0, 50.0), (1.0, 1.0)], [false, true]);
        let config = BKConfig {
            node_sep: 50.0,
            edge_sep: 10.0,
            direction: Direction::TopBottom,
        };

        // Mixed: 50 + (50+10)/2 + 0.5 = 80.5
        let sep = compute_sep(&lg, 0, 1, &config);
        assert_eq!(sep, 80.5);
    }

    #[test]
    fn test_block_graph_diamond() {
        let mut bg = BlockGraph::new();
        bg.add_node(0);
        bg.add_node(1);
        bg.add_node(2);
        bg.add_edge(0, 1, 40.0);
        bg.add_edge(0, 2, 40.0);

        assert_eq!(bg.successors(0).len(), 2);
        assert_eq!(bg.predecessors(1).len(), 1);
        assert_eq!(bg.predecessors(2).len(), 1);

        let topo = bg.topological_order();
        let pos_0 = topo.iter().position(|&n| n == 0).unwrap();
        let pos_1 = topo.iter().position(|&n| n == 1).unwrap();
        let pos_2 = topo.iter().position(|&n| n == 2).unwrap();
        assert!(pos_0 < pos_1);
        assert!(pos_0 < pos_2);
    }

    // =========================================================================
    // Two-Pass Compaction Tests (Task 2.1)
    // =========================================================================

    #[test]
    fn test_compaction_two_pass_separation_constraints() {
        // Build a graph with skip edges to exercise the two-pass algorithm
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("Start", (50.0, 20.0));
        graph.add_node("Step1", (50.0, 20.0));
        graph.add_node("Step2", (50.0, 20.0));
        graph.add_node("End", (50.0, 20.0));
        graph.add_edge("Start", "Step1");
        graph.add_edge("Start", "Step2");
        graph.add_edge("Start", "End");
        graph.add_edge("Step1", "Step2");
        graph.add_edge("Step2", "End");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        order::run(&mut lg, false);

        let config = BKConfig::default();
        let conflicts = find_all_conflicts(&lg);
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);
        let result = horizontal_compaction(&lg, &alignment, &config);

        // Verify all separation constraints are met
        let layers = get_layers(&lg);
        for layer in &layers {
            for i in 1..layer.len() {
                let left = layer[i - 1];
                let right = layer[i];
                let left_x = result.x[&left];
                let right_x = result.x[&right];
                let min_sep = compute_sep(&lg, left, right, &config);
                assert!(
                    right_x - left_x >= min_sep - 0.001,
                    "Separation constraint violated: {} - {} = {} < {}",
                    right_x,
                    left_x,
                    right_x - left_x,
                    min_sep
                );
            }
        }
    }

    #[test]
    fn test_compaction_chain_no_regression() {
        let lg = make_chain_graph();
        let config = BKConfig::default();
        let conflicts = find_all_conflicts(&lg);
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);
        let result = horizontal_compaction(&lg, &alignment, &config);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // All in same block → same x coordinate
        assert_eq!(result.x[&a], result.x[&b]);
        assert_eq!(result.x[&b], result.x[&c]);
    }

    #[test]
    fn test_compaction_diamond_separation() {
        let lg = make_diamond_graph();
        let config = BKConfig::default();
        let conflicts = find_all_conflicts(&lg);
        let alignment = vertical_alignment(&lg, &conflicts, AlignmentDirection::UL);
        let result = horizontal_compaction(&lg, &alignment, &config);

        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];

        // B and C must be separated by at least compute_sep
        let sep = compute_sep(&lg, b, c, &config);
        assert!(
            result.x[&c] - result.x[&b] >= sep - 0.001,
            "Diamond separation: {} - {} = {} < {}",
            result.x[&c],
            result.x[&b],
            result.x[&c] - result.x[&b],
            sep
        );
    }

    // =========================================================================
    // build_block_graph Tests (Task 1.3)
    // =========================================================================

    #[test]
    fn test_build_block_graph_single_block() {
        let lg = make_diamond_graph();
        let layers = get_layers(&lg);
        let config = BKConfig::default();

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        let mut alignment = BlockAlignment::new(&[a, b, c, d]);
        alignment.root.insert(b, a);
        alignment.root.insert(c, a);
        alignment.root.insert(d, a);

        let bg = build_block_graph(&lg, &alignment, &layers, &config);
        assert_eq!(bg.nodes.len(), 1);
    }

    #[test]
    fn test_build_block_graph_diamond_separate_blocks() {
        let lg = make_diamond_graph();
        let layers = get_layers(&lg);
        let config = BKConfig::default();

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        let alignment = BlockAlignment::new(&[a, b, c, d]);

        let bg = build_block_graph(&lg, &alignment, &layers, &config);

        // Layer 1 has B (order 0) and C (order 1), different roots
        assert_eq!(bg.successors(b).len(), 1);
        assert_eq!(bg.successors(b)[0].0, c);

        // Layer 0 has only A, layer 2 has only D → no edges from those
        assert_eq!(bg.successors(a).len(), 0);
        assert_eq!(bg.successors(d).len(), 0);
    }

    #[test]
    fn test_block_graph_includes_border_child_separation() {
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("sg", (0.0, 0.0));
        graph.add_node("border", (0.0, 0.0));
        graph.add_node("child", (10.0, 10.0));
        graph.set_parent("border", "sg");
        graph.set_parent("child", "sg");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        let sg_idx = lg.node_index[&"sg".into()];
        let border_idx = lg.node_index[&"border".into()];
        let child_idx = lg.node_index[&"child".into()];

        lg.border_type.insert(border_idx, BorderType::Left);
        lg.parents[border_idx] = Some(sg_idx);
        lg.parents[child_idx] = Some(sg_idx);

        lg.ranks[border_idx] = 0;
        lg.ranks[child_idx] = 0;
        lg.order[border_idx] = 0;
        lg.order[child_idx] = 1;

        let layers = vec![vec![border_idx, child_idx]];
        let alignment = BlockAlignment::new(&[border_idx, child_idx]);
        let config = BKConfig::default();

        let bg = build_block_graph(&lg, &alignment, &layers, &config);
        let left_root = alignment.get_root(border_idx);
        let right_root = alignment.get_root(child_idx);
        assert!(
            bg.successors(left_root)
                .iter()
                .any(|(n, _)| *n == right_root)
        );
    }

    // =========================================================================
    // BK borderType guard tests (Task 3.2)
    // =========================================================================

    #[test]
    fn test_bk_border_guard_left_border_not_pulled_right() {
        // Test that a compound graph with external nodes produces valid layout
        // through the full pipeline. The BK algorithm positions nodes, and the
        // full pipeline ensures borders contain their children.
        let mut g: DiGraph<(f64, f64)> = DiGraph::new();
        g.add_node("X", (100.0, 50.0));
        g.add_node("Y", (100.0, 50.0));
        g.add_node("A", (100.0, 50.0));
        g.add_node("B", (100.0, 50.0));
        g.add_node("sg1", (0.0, 0.0));
        g.add_edge("X", "A");
        g.add_edge("X", "Y");
        g.add_edge("A", "B");
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");

        let config = LayoutConfig::default();
        let result = crate::layered::layout(&g, &config, |_, dims| *dims);

        // Verify the layout produces valid subgraph bounds
        assert!(
            result.subgraph_bounds.contains_key("sg1"),
            "Should have subgraph bounds"
        );
        let bounds = &result.subgraph_bounds["sg1"];
        assert!(bounds.width > 0.0, "Subgraph width should be positive");

        // A and B should be within the subgraph bounds
        let a_rect = result.nodes.get(&"A".into()).unwrap();
        let b_rect = result.nodes.get(&"B".into()).unwrap();
        assert!(
            a_rect.x >= bounds.x && a_rect.x + a_rect.width <= bounds.x + bounds.width,
            "A (x={}, w={}) should be within sg1 bounds (x={}, w={})",
            a_rect.x,
            a_rect.width,
            bounds.x,
            bounds.width
        );
        assert!(
            b_rect.x >= bounds.x && b_rect.x + b_rect.width <= bounds.x + bounds.width,
            "B (x={}, w={}) should be within sg1 bounds (x={}, w={})",
            b_rect.x,
            b_rect.width,
            bounds.x,
            bounds.width
        );
    }

    #[test]
    fn test_bk_simple_graph_unchanged_with_border_guard() {
        // Simple graph (no compound nodes) should produce valid positions
        // unaffected by the border guard logic.
        let lg = make_diamond_graph();
        let config = BKConfig::default();
        let xs = position_x(&lg, &config);

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        // All nodes should have valid x-coordinates
        assert!(xs.contains_key(&a));
        assert!(xs.contains_key(&b));
        assert!(xs.contains_key(&c));
        assert!(xs.contains_key(&d));

        // B and C should be separated
        assert!((xs[&b] - xs[&c]).abs() > 1.0, "B and C should be separated");
    }

    #[test]
    fn test_separation_for_border_node() {
        // Build a minimal LayoutGraph with a compound structure including border nodes.
        // Border nodes are tracked in border_type but NOT in dummy_nodes.
        let mut graph: DiGraph<(f64, f64)> = DiGraph::new();
        graph.add_node("A", (100.0, 50.0));
        graph.add_node("child", (100.0, 50.0));

        let mut lg = LayoutGraph::from_digraph(&graph, |_, dims| *dims);
        rank::run(&mut lg, &LayoutConfig::default());

        // Add a border node via add_nesting_node (does NOT add to dummy_nodes)
        let border_idx = lg.add_nesting_node("border_left".into());
        lg.border_type.insert(border_idx, BorderType::Left);

        let child_idx = lg.node_index[&"child".into()];

        let config = BKConfig {
            edge_sep: 20.0,
            node_sep: 50.0,
            ..Default::default()
        };

        // Border node should use edge_sep (20.0), not node_sep (50.0)
        let border_sep = separation_for(&lg, border_idx, &config);
        assert_eq!(
            border_sep, 20.0,
            "border nodes should use edge_sep, not node_sep"
        );

        // Regular child node should still use node_sep
        let child_sep = separation_for(&lg, child_idx, &config);
        assert_eq!(child_sep, 50.0, "regular nodes should use node_sep");
    }

    #[test]
    fn test_build_block_graph_adjacent_same_root_no_edge() {
        let lg = make_diamond_graph();
        let layers = get_layers(&lg);
        let config = BKConfig::default();

        let a = lg.node_index[&"A".into()];
        let b = lg.node_index[&"B".into()];
        let c = lg.node_index[&"C".into()];
        let d = lg.node_index[&"D".into()];

        // B and C share root B
        let mut alignment = BlockAlignment::new(&[a, b, c, d]);
        alignment.root.insert(c, b);

        let bg = build_block_graph(&lg, &alignment, &layers, &config);

        // B and C are adjacent in layer 1 but same root → no edge
        assert_eq!(bg.successors(b).len(), 0);
    }
}
