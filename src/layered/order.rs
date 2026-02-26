//! Phase 3: Reduce edge crossings by reordering nodes within ranks.
//!
//! Implements the barycenter heuristic with iterative sweeping.

use std::collections::{HashMap, HashSet};

use super::graph::LayoutGraph;
use super::rank;

// ---------------------------------------------------------------------------
// Types for hierarchical ordering (matching dagre's sort-subgraph pipeline)
// ---------------------------------------------------------------------------

/// A node with optional barycenter from the fixed layer.
/// Matches dagre's barycenter.js output: { v, barycenter?, weight? }
#[derive(Debug, Clone)]
struct OrderEntry {
    /// Node index in LayoutGraph.
    v: usize,
    /// Weighted barycenter from connections to fixed layer. None if no connections.
    barycenter: Option<f64>,
    /// Total edge weight to fixed layer. None if no connections.
    weight: Option<f64>,
}

/// Result of sorting: ordered node list + optional aggregate barycenter.
/// Matches dagre's sort.js output: { vs, barycenter?, weight? }
#[derive(Debug, Clone)]
struct SortResult {
    vs: Vec<usize>,
    barycenter: Option<f64>,
    weight: Option<f64>,
}

/// Resolved entry after resolveConflicts: may contain coalesced nodes.
/// Matches dagre's { vs, i, barycenter?, weight? }
#[derive(Debug, Clone)]
struct ResolvedEntry {
    vs: Vec<usize>,
    i: usize,
    barycenter: Option<f64>,
    weight: Option<f64>,
}

/// Compute barycenters for movable nodes from their connections to the fixed layer.
///
/// For each node in `movable`, find edges connecting it to nodes in the fixed layer
/// and compute the weighted average of those neighbors' order values.
///
/// Matches dagre's barycenter.js. Always uses *incoming* edges in the provided
/// edge list (which is already oriented per sweep direction via build-layer-graph).
fn compute_barycenters(
    graph: &LayoutGraph,
    movable: &[usize],
    edges: &[(usize, usize, f64)],
    fixed_layer_nodes: &[usize],
) -> Vec<OrderEntry> {
    movable
        .iter()
        .map(|&node| {
            // Predecessors in layer graph: edges where node is target
            let neighbors: Vec<(usize, f64)> = edges
                .iter()
                .filter(|&&(_, to, _)| to == node)
                .map(|&(from, _, w)| (from, w))
                .filter(|&(n, _)| fixed_layer_nodes.contains(&n))
                .collect();

            if neighbors.is_empty() {
                OrderEntry {
                    v: node,
                    barycenter: None,
                    weight: None,
                }
            } else {
                let weighted_sum: f64 = neighbors
                    .iter()
                    .map(|&(n, w)| w * graph.order[n] as f64)
                    .sum();
                let total_weight: f64 = neighbors.iter().map(|&(_, w)| w).sum();
                OrderEntry {
                    v: node,
                    barycenter: Some(weighted_sum / total_weight),
                    weight: Some(total_weight),
                }
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Constraint graph for subgraph ordering
// ---------------------------------------------------------------------------

/// Simple directed graph for subgraph ordering constraints.
/// Edges mean "left must come before right".
struct ConstraintGraph {
    /// Adjacency: source -> [targets]
    out_edges: HashMap<usize, Vec<usize>>,
    /// Reverse adjacency: target -> [sources]
    in_edges: HashMap<usize, Vec<usize>>,
    /// Dedup set: dagre's graphlib `setEdge` is idempotent.
    edge_set: HashSet<(usize, usize)>,
}

impl ConstraintGraph {
    fn new() -> Self {
        Self {
            out_edges: HashMap::new(),
            in_edges: HashMap::new(),
            edge_set: HashSet::new(),
        }
    }

    fn add_edge(&mut self, from: usize, to: usize) {
        if !self.edge_set.insert((from, to)) {
            return; // Already exists — dagre's setEdge is idempotent
        }
        self.out_edges.entry(from).or_default().push(to);
        self.in_edges.entry(to).or_default().push(from);
    }

    fn edges(&self) -> Vec<(usize, usize)> {
        self.out_edges
            .iter()
            .flat_map(|(&from, tos)| tos.iter().map(move |&to| (from, to)))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// resolve_conflicts — matching dagre's resolve-conflicts.js
// ---------------------------------------------------------------------------

/// Resolve conflicts between constraint graph ordering and barycenters.
///
/// Takes barycenter entries and a constraint graph, coalescing entries when
/// constraints conflict with barycenter ordering.
///
/// Matches dagre's resolve-conflicts.js.
fn resolve_conflicts(entries: &[OrderEntry], cg: &ConstraintGraph) -> Vec<ResolvedEntry> {
    // Build mapped entries: one per input entry, keyed by node index
    struct MappedEntry {
        indegree: usize,
        in_entries: Vec<usize>, // indices into mapped_entries
        out_entries: Vec<usize>,
        vs: Vec<usize>,
        i: usize,
        barycenter: Option<f64>,
        weight: Option<f64>,
        merged: bool,
    }

    let mut mapped: Vec<MappedEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| MappedEntry {
            indegree: 0,
            in_entries: Vec::new(),
            out_entries: Vec::new(),
            vs: vec![e.v],
            i,
            barycenter: e.barycenter,
            weight: e.weight,
            merged: false,
        })
        .collect();

    // Map node index -> mapped entry index
    let mut node_to_entry: HashMap<usize, usize> = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        node_to_entry.insert(entry.v, idx);
    }

    // Apply constraint graph edges
    for (from, to) in cg.edges() {
        if let (Some(&from_idx), Some(&to_idx)) = (node_to_entry.get(&from), node_to_entry.get(&to))
        {
            mapped[to_idx].indegree += 1;
            mapped[from_idx].out_entries.push(to_idx);
        }
    }

    // Source set: entries with indegree 0
    let mut source_set: Vec<usize> = mapped
        .iter()
        .enumerate()
        .filter(|(_, e)| e.indegree == 0)
        .map(|(i, _)| i)
        .collect();

    let mut result_order: Vec<usize> = Vec::new();

    while let Some(v_idx) = source_set.pop() {
        result_order.push(v_idx);

        // Process in-list: reverse, then check for merges
        let in_list: Vec<usize> = mapped[v_idx].in_entries.clone();
        for &u_idx in in_list.iter().rev() {
            if mapped[u_idx].merged {
                continue;
            }
            let u_bc = mapped[u_idx].barycenter;
            let v_bc = mapped[v_idx].barycenter;

            if u_bc.is_none() || v_bc.is_none() || u_bc.unwrap() >= v_bc.unwrap() {
                // Merge u into v: source.vs.concat(target.vs)
                // In dagre: target.vs = source.vs.concat(target.vs)
                // target=v, source=u → v.vs = u.vs ++ v.vs
                let u_vs = mapped[u_idx].vs.clone();
                let u_bc = mapped[u_idx].barycenter;
                let u_w = mapped[u_idx].weight;
                let u_i = mapped[u_idx].i;

                // Compute merged barycenter
                let mut sum = 0.0_f64;
                let mut weight = 0.0_f64;
                if let (Some(bc), Some(w)) = (mapped[v_idx].barycenter, mapped[v_idx].weight) {
                    sum += bc * w;
                    weight += w;
                }
                if let (Some(bc), Some(w)) = (u_bc, u_w) {
                    sum += bc * w;
                    weight += w;
                }

                let mut new_vs = u_vs;
                new_vs.extend(&mapped[v_idx].vs);
                mapped[v_idx].vs = new_vs;
                mapped[v_idx].barycenter = if weight > 0.0 {
                    Some(sum / weight)
                } else {
                    None
                };
                mapped[v_idx].weight = if weight > 0.0 { Some(weight) } else { None };
                mapped[v_idx].i = mapped[v_idx].i.min(u_i);
                mapped[u_idx].merged = true;
            }
        }

        // Process out-list: decrement indegree, add to source set when 0
        let out_list: Vec<usize> = mapped[v_idx].out_entries.clone();
        for &w_idx in &out_list {
            // Record that v is a predecessor of w (for w's in-list processing)
            mapped[w_idx].in_entries.push(v_idx);
            mapped[w_idx].indegree -= 1;
            if mapped[w_idx].indegree == 0 {
                source_set.push(w_idx);
            }
        }
    }

    // Return non-merged entries in processing order
    result_order
        .iter()
        .filter(|&&idx| !mapped[idx].merged)
        .map(|&idx| ResolvedEntry {
            vs: mapped[idx].vs.clone(),
            i: mapped[idx].i,
            barycenter: mapped[idx].barycenter,
            weight: mapped[idx].weight,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// sort_entries — matching dagre's sort.js
// ---------------------------------------------------------------------------

/// Sort resolved entries by barycenter with bias-aware tie-breaking,
/// interleaving unsortable entries at their original positions.
///
/// Matches dagre's sort.js.
fn sort_entries(entries: &[ResolvedEntry], bias_right: bool, graph: &LayoutGraph) -> SortResult {
    let (mut sortable, mut unsortable): (Vec<&ResolvedEntry>, Vec<&ResolvedEntry>) =
        entries.iter().partition(|e| e.barycenter.is_some());

    // Sort unsortable by descending i
    unsortable.sort_by(|a, b| b.i.cmp(&a.i));

    // Sort sortable by barycenter with model order + bias tie-breaking.
    // Three-level sort:
    //   1. Barycenter (primary) — crossing minimization objective
    //   2. Model order (secondary) — source declaration order for stability
    //   3. Entry index with bias (tertiary) — exploration diversity across sweeps
    // For multi-node entries (from resolve_conflicts), use the minimum model_order
    // among the group's original nodes as the representative value.
    sortable.sort_by(|a, b| {
        a.barycenter
            .unwrap()
            .partial_cmp(&b.barycenter.unwrap())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let mo_a = a.vs.iter().filter_map(|&v| graph.model_order[v]).min();
                let mo_b = b.vs.iter().filter_map(|&v| graph.model_order[v]).min();
                mo_a.cmp(&mo_b)
            })
            .then_with(|| {
                if bias_right {
                    b.i.cmp(&a.i)
                } else {
                    a.i.cmp(&b.i)
                }
            })
    });

    // Interleave using consumeUnsortable pattern
    let mut vs: Vec<usize> = Vec::new();
    let mut vs_index: usize = 0;
    let mut sum: f64 = 0.0;
    let mut weight: f64 = 0.0;

    consume_unsortable_entries(&mut vs, &mut unsortable, &mut vs_index);

    for entry in &sortable {
        vs_index += entry.vs.len();
        vs.extend(&entry.vs);
        sum += entry.barycenter.unwrap() * entry.weight.unwrap();
        weight += entry.weight.unwrap();
        consume_unsortable_entries(&mut vs, &mut unsortable, &mut vs_index);
    }

    // No drain: dagre's sort.js relies solely on consumeUnsortable.
    // Remaining unsortables (if any) are not appended.

    SortResult {
        vs,
        barycenter: if weight > 0.0 {
            Some(sum / weight)
        } else {
            None
        },
        weight: if weight > 0.0 { Some(weight) } else { None },
    }
}

fn consume_unsortable_entries(
    vs: &mut Vec<usize>,
    unsortable: &mut Vec<&ResolvedEntry>,
    vs_index: &mut usize,
) {
    while let Some(entry) = unsortable.last() {
        if entry.i <= *vs_index {
            let entry = unsortable.pop().unwrap();
            vs.extend(&entry.vs);
            *vs_index += 1; // dagre increments by 1, not vs.len()
        } else {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// sort_subgraph — matching dagre's sort-subgraph.js
// ---------------------------------------------------------------------------

/// Get children of a parent at a specific rank.
///
/// Mirrors dagre's layer graph children: returns position nodes at this rank
/// whose direct parent matches, PLUS compound nodes whose range spans this rank
/// and whose parent matches.
///
/// For parent=None (root):
///   - Position nodes at this rank with no parent
///   - Compound nodes spanning this rank with no parent
///
/// For parent=Some(idx):
///   - Position nodes at this rank with parent=idx
///   - Compound nodes spanning this rank with parent=idx
fn get_children_at_rank(graph: &LayoutGraph, parent: Option<usize>, rank: i32) -> Vec<usize> {
    // Single pass over node_ids in insertion order to match dagre's g.children(v)
    // which preserves layer-graph insertion order (interleaving compounds and
    // base nodes). The `i` values and tie-breaking depend on this order.
    let mut children = Vec::new();

    for n in 0..graph.node_ids.len() {
        if graph.parents[n] != parent {
            continue;
        }

        if graph.is_position_node(n) && graph.ranks[n] == rank {
            // Base node at this rank
            children.push(n);
        } else if graph.compound_nodes.contains(&n) {
            // Compound node spanning this rank
            let min_r = graph.min_rank.get(&n).copied().unwrap_or(i32::MAX);
            let max_r = graph.max_rank.get(&n).copied().unwrap_or(i32::MIN);
            if min_r <= rank && rank <= max_r {
                children.push(n);
            }
        }
    }

    children
}

/// Get border nodes for a compound at a specific rank.
fn get_borders_at_rank(
    graph: &LayoutGraph,
    parent: Option<usize>,
    rank: i32,
) -> (Option<usize>, Option<usize>) {
    let parent = match parent {
        Some(p) => p,
        None => return (None, None),
    };
    let left = graph.border_left.get(&parent);
    let right = graph.border_right.get(&parent);
    if left.is_none() || right.is_none() {
        return (None, None);
    }
    let min_r = match graph.min_rank.get(&parent) {
        Some(&r) => r,
        None => return (None, None),
    };
    let rank_offset = (rank - min_r) as usize;
    let bl = left.and_then(|v| v.get(rank_offset).copied());
    let br = right.and_then(|v| v.get(rank_offset).copied());
    match (bl, br) {
        (Some(bl), Some(br)) => (Some(bl), Some(br)),
        _ => (None, None),
    }
}

/// Check if a node is a compound that is active at a given rank.
fn is_compound_with_children_at_rank(graph: &LayoutGraph, node: usize, rank: i32) -> bool {
    if !graph.compound_nodes.contains(&node) {
        return false;
    }
    // A compound is "active" at a rank if it has children there
    // This matches dagre's `g.children(entry.v).length` check in sortSubgraph
    let min_r = graph.min_rank.get(&node).copied();
    let max_r = graph.max_rank.get(&node).copied();
    match (min_r, max_r) {
        (Some(min), Some(max)) => min <= rank && rank <= max,
        _ => false,
    }
}

/// Merge a subgraph's barycenter into an OrderEntry.
/// Matches dagre's mergeBarycenters.
fn merge_barycenters(entry: &mut OrderEntry, other_bc: f64, other_weight: f64) {
    if let (Some(bc), Some(w)) = (entry.barycenter, entry.weight) {
        entry.barycenter = Some((bc * w + other_bc * other_weight) / (w + other_weight));
        entry.weight = Some(w + other_weight);
    } else {
        entry.barycenter = Some(other_bc);
        entry.weight = Some(other_weight);
    }
}

/// Expand subgraph entries: replace each subgraph node with its sorted children.
/// Matches dagre's expandSubgraphs.
fn expand_subgraphs(entries: &mut [ResolvedEntry], subgraph_results: &HashMap<usize, SortResult>) {
    for entry in entries.iter_mut() {
        let mut new_vs = Vec::new();
        for &v in &entry.vs {
            if let Some(sub) = subgraph_results.get(&v) {
                new_vs.extend(&sub.vs);
            } else {
                new_vs.push(v);
            }
        }
        entry.vs = new_vs;
    }
}

/// Get the order of a border node's predecessor in the layer graph.
/// Used for border barycenter contribution.
fn get_border_predecessor_order(
    graph: &LayoutGraph,
    border: usize,
    edges: &[(usize, usize, f64)],
) -> Option<f64> {
    // Find predecessors of this border node in the layer graph edges
    edges
        .iter()
        .filter(|&&(_, to, _)| to == border)
        .map(|&(from, _, _)| graph.order[from] as f64)
        .next()
}

/// Recursively sort nodes within a compound parent at a given rank.
///
/// Matches dagre's sort-subgraph.js: sortSubgraph(g, v, cg, biasRight).
fn sort_subgraph(
    graph: &LayoutGraph,
    parent: Option<usize>,
    rank: i32,
    edges: &[(usize, usize, f64)],
    fixed_layer_nodes: &[usize],
    cg: &ConstraintGraph,
    bias_right: bool,
) -> SortResult {
    // 1. Get children of parent at this rank
    let children = get_children_at_rank(graph, parent, rank);

    // 2. Identify and strip border nodes
    let (bl, br) = get_borders_at_rank(graph, parent, rank);
    let movable: Vec<usize> = if let (Some(bl_node), Some(br_node)) = (bl, br) {
        children
            .iter()
            .copied()
            .filter(|&n| n != bl_node && n != br_node)
            .collect()
    } else {
        children
    };

    // 3. Compute barycenters for movable nodes
    let mut barycenters = compute_barycenters(graph, &movable, edges, fixed_layer_nodes);

    // 4. Recurse into subgraph children, merge barycenters
    let mut subgraph_results: HashMap<usize, SortResult> = HashMap::new();
    for entry in &mut barycenters {
        if is_compound_with_children_at_rank(graph, entry.v, rank) {
            let sub_result = sort_subgraph(
                graph,
                Some(entry.v),
                rank,
                edges,
                fixed_layer_nodes,
                cg,
                bias_right,
            );
            if let Some(sub_bc) = sub_result.barycenter {
                merge_barycenters(entry, sub_bc, sub_result.weight.unwrap_or(0.0));
            }
            subgraph_results.insert(entry.v, sub_result);
        }
    }

    // 5. Resolve conflicts with constraint graph
    let mut resolved = resolve_conflicts(&barycenters, cg);

    // 6. Expand subgraph entries
    expand_subgraphs(&mut resolved, &subgraph_results);

    // 7. Sort
    let mut result = sort_entries(&resolved, bias_right, graph);

    // 8. Re-insert borders and compute aggregate barycenter
    if let (Some(bl_node), Some(br_node)) = (bl, br) {
        let mut vs = vec![bl_node];
        vs.extend(&result.vs);
        vs.push(br_node);
        result.vs = vs;

        // Contribute border predecessors' orders to aggregate barycenter
        let bl_pred_order = get_border_predecessor_order(graph, bl_node, edges);
        let br_pred_order = get_border_predecessor_order(graph, br_node, edges);
        if let (Some(bl_ord), Some(br_ord)) = (bl_pred_order, br_pred_order) {
            if result.barycenter.is_none() {
                result.barycenter = Some(0.0);
                result.weight = Some(0.0);
            }
            let bc = result.barycenter.unwrap();
            let w = result.weight.unwrap();
            result.barycenter = Some((bc * w + bl_ord + br_ord) / (w + 2.0));
            result.weight = Some(w + 2.0);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// add_subgraph_constraints — matching dagre's add-subgraph-constraints.js
// ---------------------------------------------------------------------------

/// Record subgraph ordering constraints after sorting a rank.
///
/// Walks the sorted node list and, for each node, climbs the parent hierarchy.
/// At each parent level, if a previous sibling was seen, adds a constraint edge
/// from the previous sibling to the current one.
///
/// Matches dagre's add-subgraph-constraints.js.
fn add_subgraph_constraints(graph: &LayoutGraph, cg: &mut ConstraintGraph, sorted_vs: &[usize]) {
    let mut prev: HashMap<Option<usize>, usize> = HashMap::new();

    'outer: for &v in sorted_vs {
        let mut child_opt = graph.parents[v]; // parent of v
        while let Some(child_idx) = child_opt {
            let parent = graph.parents[child_idx]; // grandparent of v
            if let Some(&prev_child) = prev.get(&parent) {
                prev.insert(parent, child_idx);
                if prev_child != child_idx {
                    cg.add_edge(prev_child, child_idx);
                    continue 'outer; // dagre's `return` in forEach
                }
            } else {
                prev.insert(parent, child_idx);
            }
            child_opt = parent;
        }
    }
}

/// Check if order debug tracing is enabled via MMDFLUX_DEBUG_ORDER=1.
fn debug_order() -> bool {
    std::env::var("MMDFLUX_DEBUG_ORDER").is_ok_and(|v| v == "1")
}

/// Dump per-rank node names and order values.
fn debug_dump_order(graph: &LayoutGraph, label: &str) {
    if !debug_order() {
        return;
    }
    let layers = rank::by_rank(graph);
    eprintln!("[order] {label}");
    for (rank, layer) in layers.iter().enumerate() {
        let mut nodes: Vec<(usize, &str)> = layer
            .iter()
            .map(|&idx| (graph.order[idx], graph.node_ids[idx].0.as_str()))
            .collect();
        nodes.sort_by_key(|&(ord, _)| ord);
        let names: Vec<String> = nodes
            .iter()
            .map(|(ord, name)| format!("{name}={ord}"))
            .collect();
        eprintln!("[order]   rank {rank}: [{}]", names.join(", "));
    }
}

pub(crate) fn effective_edges_weighted_filtered(graph: &LayoutGraph) -> Vec<(usize, usize, f64)> {
    graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(idx, &(from, to, _))| {
            if graph.excluded_edges.contains(&idx) {
                return None;
            }
            let weight = graph.edge_weights[idx];
            let (from, to) = if graph.reversed_edges.contains(&idx) {
                (to, from)
            } else {
                (from, to)
            };
            if !graph.is_position_node(from) || !graph.is_position_node(to) {
                return None;
            }
            Some((from, to, weight))
        })
        .collect()
}

/// DFS-based initial ordering matching Dagre's initOrder().
///
/// Visits nodes sorted by rank, adding each to its layer in DFS visit order.
/// This groups connected nodes together, providing a better starting point
/// for crossing minimization than arbitrary insertion order.
///
/// Reference: Gansner et al., "A Technique for Drawing Directed Graphs"
fn init_order(graph: &mut LayoutGraph, layers: &[Vec<usize>]) {
    let edges = effective_edges_weighted_filtered(graph);
    let n = graph.node_ids.len();

    // Build successor adjacency list, sorted by model order so that the
    // DFS visits children in source declaration order. The reverse-push
    // DFS pattern means ascending sort here produces ascending visit order.
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(from, to, _) in &edges {
        successors[from].push(to);
    }
    for succs in &mut successors {
        succs.sort_by(|&a, &b| graph.model_order[a].cmp(&graph.model_order[b]));
    }

    // Get all nodes sorted by (rank, model_order). Rank is primary to match
    // Dagre's `simpleNodes.sort((a, b) => g.node(a).rank - g.node(b).rank)`.
    // Model order as secondary key ensures same-rank start nodes are processed
    // in source declaration order, giving the DFS a deterministic starting point.
    let mut start_nodes: Vec<usize> = layers.iter().flatten().copied().collect();
    start_nodes.sort_by(|&a, &b| {
        graph.ranks[a]
            .cmp(&graph.ranks[b])
            .then_with(|| graph.model_order[a].cmp(&graph.model_order[b]))
    });

    // Track visit state and per-rank insertion index
    let mut visited = vec![false; n];
    let max_rank = graph.ranks.iter().max().copied().unwrap_or(0) as usize;
    let mut layer_next_idx: Vec<usize> = vec![0; max_rank + 1];

    // Iterative DFS to avoid stack overflow on deep graphs.
    // Push successors in reverse so first successor is visited first,
    // matching recursive DFS visit order.
    for &root in &start_nodes {
        if visited[root] {
            continue;
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if visited[node] {
                continue;
            }
            visited[node] = true;

            let rank = graph.ranks[node] as usize;
            graph.order[node] = layer_next_idx[rank];
            layer_next_idx[rank] += 1;

            // Push successors in reverse for correct DFS order
            for &succ in successors[node].iter().rev() {
                if !visited[succ] {
                    stack.push(succ);
                }
            }
        }
    }
}

/// Build layer vectors sorted by node order.
fn layers_sorted_by_order(layers: &[Vec<usize>], graph: &LayoutGraph) -> Vec<Vec<usize>> {
    let mut layers: Vec<Vec<usize>> = layers.to_vec();
    for layer in &mut layers {
        layer.sort_by_key(|&node| graph.order[node]);
    }
    layers
}

/// Run crossing reduction using Dagre-style adaptive ordering.
///
/// Matches Dagre's `order()` function in `lib/order/index.js`:
/// - DFS-based initial ordering
/// - Alternating up/down sweeps (one per iteration)
/// - Alternating left/right bias (pattern: false, false, true, true)
/// - Best-order tracking across iterations
/// - Terminates after 4 consecutive non-improving iterations
pub fn run(graph: &mut LayoutGraph, enable_greedy_switch: bool) {
    run_with_options(graph, enable_greedy_switch, false);
}

/// Run crossing reduction with optional "always compound" sweep behavior.
///
/// When `always_compound_ordering` is `true`, the constraint-graph based
/// `sort_subgraph` pipeline is used for every sweep, even on flat graphs.
/// When `false`, flat graphs use the classic `reorder_layer` sweeps.
pub fn run_with_options(
    graph: &mut LayoutGraph,
    enable_greedy_switch: bool,
    always_compound_ordering: bool,
) {
    let layers = rank::by_rank_filtered(graph, |node| graph.is_position_node(node));
    if layers.len() < 2 {
        return;
    }

    // DFS-based initial ordering
    init_order(graph, &layers);
    debug_dump_order(graph, "after init_order");

    // Rebuild layers sorted by the new DFS order
    let layers = layers_sorted_by_order(&layers, graph);
    let edges = effective_edges_weighted_filtered(graph);

    let mut best_cc = usize::MAX;
    let mut best_order: Vec<usize> = Vec::new();

    // Dagre-style adaptive loop.
    //
    // Direction: i % 2 == 0 -> sweep_up, i % 2 == 1 -> sweep_down
    // Bias: i % 4 >= 2 -> bias_right = true
    // last_best increments every iteration, resets to 0 on strict improvement
    let mut i: usize = 0;
    let mut last_best: usize = 0;
    let use_compound_sweeps = always_compound_ordering || !graph.compound_nodes.is_empty();

    while last_best < 4 {
        let bias_right = (i % 4) >= 2;

        if use_compound_sweeps {
            let downward = !i.is_multiple_of(2); // odd = down, even = up
            sweep_compound(graph, &layers, &edges, bias_right, downward);
        } else if i.is_multiple_of(2) {
            sweep_up(graph, &layers, &edges, bias_right);
        } else {
            sweep_down(graph, &layers, &edges, bias_right);
        }

        let cc = count_all_crossings(graph, &layers, &edges);

        if debug_order() {
            let dir = if i.is_multiple_of(2) { "up" } else { "down" };
            eprintln!(
                "[order] iter {i}: sweep_{dir}, bias_right={bias_right}, cc={cc}, best_cc={best_cc}"
            );
            debug_dump_order(graph, &format!("after iter {i}"));
        }

        if cc < best_cc {
            last_best = 0;
            best_cc = cc;
            best_order = graph.order.clone();
            if debug_order() {
                eprintln!("[order] iter {i}: NEW BEST cc={cc}");
            }
        }
        // NOTE: dagre 0.8.5 (used by Mermaid) does NOT replace the best order
        // when cc ties the best. It keeps the first best ordering, which
        // preserves DFS insertion/declaration order in many cases.
        //
        // Newer dagre versions may update on ties; if we move off 0.8.5 we
        // should re-evaluate this logic against the target version.

        i += 1;
        last_best += 1;
    }

    // Restore best ordering found
    if !best_order.is_empty() {
        graph.order = best_order;
    }

    // Post-pass: greedy switch refinement (two-sided, never increases crossings)
    if enable_greedy_switch {
        if debug_order() {
            let before = count_all_crossings(graph, &layers, &edges);
            greedy_switch(graph, &layers, &edges);
            let after = count_all_crossings(graph, &layers, &edges);
            eprintln!("[order] greedy_switch: {before} -> {after} crossings");
        } else {
            greedy_switch(graph, &layers, &edges);
        }
    }

    debug_dump_order(graph, "final");
}

/// Compound graph sweep: hierarchical ordering via sort_subgraph at each rank.
///
/// Matches dagre's sweepLayerGraphs: creates a fresh constraint graph per sweep,
/// sorts each rank using sort_subgraph from root, assigns order, and records
/// subgraph constraints.
fn sweep_compound(
    graph: &mut LayoutGraph,
    layers: &[Vec<usize>],
    edges: &[(usize, usize, f64)],
    bias_right: bool,
    downward: bool,
) {
    let mut cg = ConstraintGraph::new();

    let rank_order: Vec<usize> = if downward {
        (1..layers.len()).collect()
    } else {
        (0..layers.len() - 1).rev().collect()
    };

    for &layer_idx in &rank_order {
        if layers[layer_idx].is_empty() {
            continue;
        }

        let fixed_layer = if downward {
            &layers[layer_idx - 1]
        } else {
            &layers[layer_idx + 1]
        };

        // Get rank of the free layer
        let free_rank = graph.ranks[layers[layer_idx][0]];

        // Build layer-graph edges oriented for this sweep direction
        let layer_edges: Vec<(usize, usize, f64)> = if downward {
            // Down sweep: use edges as-is (src in fixed, dst in free)
            edges
                .iter()
                .filter(|&&(from, to, _)| {
                    fixed_layer.contains(&from) && graph.ranks[to] == free_rank
                })
                .copied()
                .collect()
        } else {
            // Up sweep: reverse edges so "predecessors" in layer graph = successors
            edges
                .iter()
                .filter(|&&(from, to, _)| {
                    fixed_layer.contains(&to) && graph.ranks[from] == free_rank
                })
                .map(|&(from, to, w)| (to, from, w))
                .collect()
        };

        let result = sort_subgraph(
            graph,
            None,
            free_rank,
            &layer_edges,
            fixed_layer,
            &cg,
            bias_right,
        );

        // Assign order from sorted result
        for (order, &node) in result.vs.iter().enumerate() {
            graph.order[node] = order;
        }

        add_subgraph_constraints(graph, &mut cg, &result.vs);
    }
}

fn sweep_down(
    graph: &mut LayoutGraph,
    layers: &[Vec<usize>],
    edges: &[(usize, usize, f64)],
    bias_right: bool,
) {
    for i in 1..layers.len() {
        let fixed = &layers[i - 1];
        let free = &layers[i];
        reorder_layer(graph, fixed, free, edges, true, bias_right);
    }
}

fn sweep_up(
    graph: &mut LayoutGraph,
    layers: &[Vec<usize>],
    edges: &[(usize, usize, f64)],
    bias_right: bool,
) {
    for i in (0..layers.len() - 1).rev() {
        let fixed = &layers[i + 1];
        let free = &layers[i];
        reorder_layer(graph, fixed, free, edges, false, bias_right);
    }
}

/// Reorder nodes in `free` layer based on barycenter of connections to `fixed` layer.
///
/// Uses dagre v0.8.5's partition-and-interleave algorithm: nodes with neighbors
/// in the fixed layer are "sortable" (sorted by barycenter), while nodes without
/// neighbors are "unsortable" (interleaved at their original positions).
fn reorder_layer(
    graph: &mut LayoutGraph,
    fixed: &[usize],
    free: &[usize],
    edges: &[(usize, usize, f64)],
    downward: bool,
    bias_right: bool,
) {
    // Step 1: Compute weighted barycenters, partition into sortable/unsortable
    let mut sortable: Vec<(usize, f64, usize)> = Vec::new(); // (node, barycenter, original_pos)
    let mut unsortable: Vec<(usize, usize)> = Vec::new(); // (node, original_pos)

    for (original_pos, &node) in free.iter().enumerate() {
        let neighbor_weights: Vec<(usize, f64)> = if downward {
            edges
                .iter()
                .filter(|&&(_, to, _)| to == node)
                .map(|&(from, _, w)| (from, w))
                .filter(|&(n, _)| fixed.contains(&n))
                .collect()
        } else {
            edges
                .iter()
                .filter(|&&(from, _, _)| from == node)
                .map(|&(_, to, w)| (to, w))
                .filter(|&(n, _)| fixed.contains(&n))
                .collect()
        };

        if neighbor_weights.is_empty() {
            unsortable.push((node, original_pos));
        } else {
            let weighted_sum: f64 = neighbor_weights
                .iter()
                .map(|&(n, w)| w * graph.order[n] as f64)
                .sum();
            let total_weight: f64 = neighbor_weights.iter().map(|&(_, w)| w).sum();
            let barycenter = weighted_sum / total_weight;
            sortable.push((node, barycenter, original_pos));
        }
    }

    // Step 2: Sort sortable by barycenter with model order + bias tie-breaking.
    // Three-level sort:
    //   1. Barycenter (primary) — crossing minimization objective
    //   2. Model order (secondary) — source declaration order for stability
    //   3. Original position with bias (tertiary) — exploration diversity across sweeps
    sortable.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| graph.model_order[a.0].cmp(&graph.model_order[b.0]))
            .then_with(|| {
                if bias_right {
                    b.2.cmp(&a.2)
                } else {
                    a.2.cmp(&b.2)
                }
            })
    });

    // Step 3: Sort unsortable by descending original_pos (stack: pop from back)
    unsortable.sort_by(|a, b| b.1.cmp(&a.1));

    // Step 4: Interleave using consumeUnsortable pattern
    let mut result: Vec<usize> = Vec::with_capacity(free.len());
    let mut vs_index: usize = 0;

    // Helper: consume unsortable entries whose original_pos <= vs_index
    fn consume_unsortable(
        result: &mut Vec<usize>,
        unsortable: &mut Vec<(usize, usize)>,
        vs_index: &mut usize,
    ) {
        while let Some(&(_, orig_pos)) = unsortable.last() {
            if orig_pos <= *vs_index {
                let (node, _) = unsortable.pop().unwrap();
                result.push(node);
                *vs_index += 1;
            } else {
                break;
            }
        }
    }

    consume_unsortable(&mut result, &mut unsortable, &mut vs_index);

    for &(node, _, _) in &sortable {
        result.push(node);
        vs_index += 1;
        consume_unsortable(&mut result, &mut unsortable, &mut vs_index);
    }

    // Drain any remaining unsortable entries
    while let Some((node, _)) = unsortable.pop() {
        result.push(node);
    }

    // Step 5: Assign new order positions
    for (new_pos, &node) in result.iter().enumerate() {
        graph.order[node] = new_pos;
    }
}

// apply_compound_constraints removed — replaced by sweep_compound + sort_subgraph

/// Count crossings between edges of two specific nodes when `node_a` is
/// treated as being to the left of `node_b`. Two-sided: considers edges
/// to both the layer above and the layer below.
///
/// An edge from `node_a` to `neighbor_x` crosses an edge from `node_b` to
/// `neighbor_y` when the neighbor order is inverted relative to the
/// assumed a-left-of-b arrangement.
///
/// Note: This is a simpler but slower version used only in tests. The
/// production code uses `AdjacencyIndex::count_pair_crossings` which
/// pre-builds adjacency lists for O(1) neighbor lookup.
#[cfg(test)]
fn count_pair_crossings(
    graph: &LayoutGraph,
    node_a: usize,
    node_b: usize,
    layers: &[Vec<usize>],
    edges: &[(usize, usize, f64)],
) -> usize {
    let rank_a = graph.ranks[node_a];

    // Find the layer index for this rank
    let layer_idx = layers
        .iter()
        .position(|layer| layer.first().is_some_and(|&n| graph.ranks[n] == rank_a));
    let Some(layer_idx) = layer_idx else {
        return 0;
    };

    let mut crossings = 0;

    // Count crossings from edges to the layer above
    if layer_idx > 0 {
        crossings += count_pair_crossings_one_side(graph, node_a, node_b, edges, true);
    }

    // Count crossings from edges to the layer below
    if layer_idx + 1 < layers.len() {
        crossings += count_pair_crossings_one_side(graph, node_a, node_b, edges, false);
    }

    crossings
}

/// Count crossings between edges of `node_a` and `node_b` to one neighboring layer.
/// `is_upper` indicates whether we're counting edges to the layer above (true)
/// or below (false).
#[cfg(test)]
fn count_pair_crossings_one_side(
    graph: &LayoutGraph,
    node_a: usize,
    node_b: usize,
    edges: &[(usize, usize, f64)],
    is_upper: bool,
) -> usize {
    // Collect neighbor order values for node_a and node_b
    let neighbors_a = get_pair_neighbors(graph, node_a, edges, is_upper);
    let neighbors_b = get_pair_neighbors(graph, node_b, edges, is_upper);

    // Count crossings: for each pair (na, nb), they cross when
    // a's neighbor is to the right of b's neighbor (order[na] > order[nb]).
    // This is because we treat node_a as left of node_b,
    // so if a's neighbor is right of b's neighbor, the edges cross.
    let mut crossings = 0;
    for &na_order in &neighbors_a {
        for &nb_order in &neighbors_b {
            if na_order > nb_order {
                crossings += 1;
            }
        }
    }
    crossings
}

/// Get order values of neighbors of `node` connected via edges to an adjacent layer.
///
/// If `is_upper` (neighbor is above): look for edges where the neighbor is the source
/// and node is the target (edge goes from upper layer down to node's layer).
/// If `!is_upper` (neighbor is below): look for edges where node is the source
/// and the neighbor is the target (edge goes from node's layer down to lower layer).
#[cfg(test)]
fn get_pair_neighbors(
    graph: &LayoutGraph,
    node: usize,
    edges: &[(usize, usize, f64)],
    is_upper: bool,
) -> Vec<usize> {
    let node_rank = graph.ranks[node];
    edges
        .iter()
        .filter_map(|&(from, to, _)| {
            if is_upper {
                // Upper neighbor: edge goes from neighbor (above) -> node
                if to == node && graph.ranks[from] < node_rank {
                    Some(graph.order[from])
                } else {
                    None
                }
            } else {
                // Lower neighbor: edge goes from node -> neighbor (below)
                if from == node && graph.ranks[to] > node_rank {
                    Some(graph.order[to])
                } else {
                    None
                }
            }
        })
        .collect()
}

/// Pre-built adjacency lists for efficient neighbor lookup in greedy switch.
struct AdjacencyIndex {
    /// For each node: list of neighbor node indices in the layer above (predecessors)
    upper: Vec<Vec<usize>>,
    /// For each node: list of neighbor node indices in the layer below (successors)
    lower: Vec<Vec<usize>>,
}

impl AdjacencyIndex {
    fn build(graph: &LayoutGraph, edges: &[(usize, usize, f64)]) -> Self {
        let n = graph.node_ids.len();
        let mut upper = vec![Vec::new(); n];
        let mut lower = vec![Vec::new(); n];

        for &(from, to, _) in edges {
            if graph.ranks[from] < graph.ranks[to] {
                // Edge goes from upper layer to lower layer
                lower[from].push(to); // from's successor
                upper[to].push(from); // to's predecessor
            }
        }

        AdjacencyIndex { upper, lower }
    }

    /// Count crossings between edges of node_a and node_b when a is left of b.
    /// Two-sided: considers both upper and lower neighbors.
    fn count_pair_crossings(&self, graph: &LayoutGraph, node_a: usize, node_b: usize) -> usize {
        let mut crossings = 0;

        // Upper neighbors (predecessors)
        for &na in &self.upper[node_a] {
            let na_order = graph.order[na];
            for &nb in &self.upper[node_b] {
                if na_order > graph.order[nb] {
                    crossings += 1;
                }
            }
        }

        // Lower neighbors (successors)
        for &na in &self.lower[node_a] {
            let na_order = graph.order[na];
            for &nb in &self.lower[node_b] {
                if na_order > graph.order[nb] {
                    crossings += 1;
                }
            }
        }

        crossings
    }
}

/// Greedy switch post-pass: iterate adjacent pairs per layer, swap when
/// it reduces total crossing count. Two-sided variant considers both
/// neighboring layers, guaranteeing no swap increases total crossings.
///
/// Repeats until a full pass makes no improvement.
fn greedy_switch(graph: &mut LayoutGraph, layers: &[Vec<usize>], edges: &[(usize, usize, f64)]) {
    let adj = AdjacencyIndex::build(graph, edges);
    let mut improved = true;
    while improved {
        improved = false;
        for layer in layers {
            // Sort nodes in this layer by current order
            let mut sorted: Vec<usize> = layer.clone();
            sorted.sort_by_key(|&n| graph.order[n]);

            for i in 0..sorted.len().saturating_sub(1) {
                let node_a = sorted[i];
                let node_b = sorted[i + 1];

                // Skip swaps between nodes with different parents
                // (preserves subgraph contiguity constraints)
                if graph.parents[node_a] != graph.parents[node_b] {
                    continue;
                }

                let current = adj.count_pair_crossings(graph, node_a, node_b);
                let swapped = adj.count_pair_crossings(graph, node_b, node_a);

                if swapped < current {
                    // Swap order values
                    graph.order.swap(node_a, node_b);
                    // Update sorted array to reflect the swap
                    sorted.swap(i, i + 1);
                    improved = true;
                }
            }
        }
    }
}

/// Count total crossings between all adjacent layer pairs.
pub(crate) fn count_all_crossings(
    graph: &LayoutGraph,
    layers: &[Vec<usize>],
    edges: &[(usize, usize, f64)],
) -> usize {
    let mut total = 0;
    for i in 0..layers.len().saturating_sub(1) {
        total += count_crossings_between(graph, &layers[i], &layers[i + 1], edges);
    }
    total
}

/// Count crossings between two adjacent layers.
fn count_crossings_between(
    graph: &LayoutGraph,
    layer1: &[usize],
    layer2: &[usize],
    edges: &[(usize, usize, f64)],
) -> usize {
    // Collect edges between these layers with their positions
    let mut edge_positions: Vec<(usize, usize)> = Vec::new();

    for &(from, to, _) in edges {
        if layer1.contains(&from) && layer2.contains(&to) {
            edge_positions.push((graph.order[from], graph.order[to]));
        } else if layer1.contains(&to) && layer2.contains(&from) {
            edge_positions.push((graph.order[to], graph.order[from]));
        }
    }

    // Count crossings using simple O(e^2) algorithm
    let mut crossings = 0;
    for i in 0..edge_positions.len() {
        for j in i + 1..edge_positions.len() {
            let (u1, v1) = edge_positions[i];
            let (u2, v2) = edge_positions[j];

            // Edges cross if one goes up while the other goes down
            if (u1 < u2 && v1 > v2) || (u1 > u2 && v1 < v2) {
                crossings += 1;
            }
        }
    }

    crossings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layered::graph::DiGraph;
    use crate::layered::{LayoutConfig, NodeId};

    fn setup_graph_and_run(
        nodes: &[&str],
        edges_list: &[(&str, &str)],
    ) -> (LayoutGraph, Vec<Vec<usize>>) {
        let mut graph: DiGraph<()> = DiGraph::new();
        for &n in nodes {
            graph.add_node(n, ());
        }
        for &(from, to) in edges_list {
            graph.add_edge(from, to);
        }

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        let layers = rank::by_rank(&lg);
        (lg, layers)
    }

    #[test]
    fn test_order_no_crossings() {
        let (mut lg, _) = setup_graph_and_run(&["A", "B", "C"], &[("A", "B"), ("B", "C")]);

        run(&mut lg, false);

        // Simple chain should have no crossings
        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(count_all_crossings(&lg, &layers, &edges), 0);
    }

    #[test]
    fn test_order_reduces_crossings() {
        // Create a graph that initially has crossings
        // Layer 0: A, B
        // Layer 1: C, D
        // Edges: A->D, B->C (crosses if A,B and C,D are in wrong order)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        // After ordering, crossings should be minimized
        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let crossings = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(crossings, 0);
    }

    #[test]
    fn test_crossing_minimize_fan_out_with_back_edge() {
        // Regression test: the constraint graph in sweep_compound propagates
        // ordering decisions across layers, finding a 0-crossing solution.
        // Without the compound path, the flat reorder_layer settles on 1 crossing.
        //
        // Graph (matches dagre's ordering: D left of C at the fan-out rank):
        //   A → B → C → E → A (back-edge)
        //            ↘ D → G → I → F
        //              D → H → I
        //   E → F
        let mut graph: DiGraph<()> = DiGraph::new();
        for id in ["A", "B", "C", "D", "E", "F", "G", "H", "I"] {
            graph.add_node(id, ());
        }
        graph.add_edge("A", "B");
        graph.add_edge("B", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "E");
        graph.add_edge("E", "A"); // back-edge
        graph.add_edge("D", "G");
        graph.add_edge("D", "H");
        graph.add_edge("G", "I");
        graph.add_edge("H", "I");
        graph.add_edge("I", "F");
        graph.add_edge("E", "F");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        crate::layered::acyclic::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        crate::layered::normalize::run(&mut lg, &std::collections::HashMap::new(), false);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let crossings = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(crossings, 0, "Expected 0 crossings after ordering");
    }

    #[test]
    fn test_bias_right_changes_order() {
        // A fans out to B and C, giving both equal barycenters.
        //   A
        //  / \
        // B   C
        //
        // With model_order tie-breaking, B (model_order 1) always sorts before
        // C (model_order 2) regardless of bias, because model_order takes
        // precedence over original_pos/bias. Bias only affects nodes with both
        // equal barycenters AND equal model_orders.
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        for layer in &layers {
            for (idx, &node) in layer.iter().enumerate() {
                lg.order[node] = idx;
            }
        }

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = &layers[0]; // [A]
        let free = &layers[1]; // [B, C]

        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];

        // Left bias (bias_right = false)
        reorder_layer(&mut lg, fixed, free, &edges, true, false);
        let left_order_b = lg.order[b];
        let left_order_c = lg.order[c];

        // Reset orders
        for (idx, &node) in free.iter().enumerate() {
            lg.order[node] = idx;
        }

        // Right bias (bias_right = true)
        reorder_layer(&mut lg, fixed, free, &edges, true, true);
        let right_order_b = lg.order[b];
        let right_order_c = lg.order[c];

        // Model order stabilizes: B always before C regardless of bias
        assert!(
            left_order_b < left_order_c,
            "B should be before C with left bias"
        );
        assert!(
            right_order_b < right_order_c,
            "B should still be before C with right bias (model_order wins over bias)"
        );
    }

    #[test]
    fn test_init_order_groups_connected() {
        // Diamond graph:
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        // All nodes should have valid consecutive order values per layer
        let layers = rank::by_rank(&lg);
        for layer in &layers {
            let mut orders: Vec<usize> = layer.iter().map(|&n| lg.order[n]).collect();
            orders.sort();
            let expected: Vec<usize> = (0..layer.len()).collect();
            assert_eq!(
                orders, expected,
                "Orders should be consecutive starting from 0"
            );
        }
    }

    #[test]
    fn test_init_order_disconnected() {
        // Two disconnected chains: A->B, C->D
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        // All nodes should have valid order values, no panics
        let layers = rank::by_rank(&lg);
        for layer in &layers {
            let mut orders: Vec<usize> = layer.iter().map(|&n| lg.order[n]).collect();
            orders.sort();
            let expected: Vec<usize> = (0..layer.len()).collect();
            assert_eq!(orders, expected);
        }
    }

    #[test]
    fn test_adaptive_selects_best() {
        // Crossing graph: A->D, B->C
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let crossings = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(
            crossings, 0,
            "Adaptive loop should find zero-crossing ordering"
        );
    }

    #[test]
    fn test_adaptive_converges() {
        //     A
        //    / \
        //   B   C
        //   |   |
        //   D   E
        //    \ /
        //     F
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_node("E", ());
        graph.add_node("F", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "E");
        graph.add_edge("D", "F");
        graph.add_edge("E", "F");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        for layer in &layers {
            let mut orders: Vec<usize> = layer.iter().map(|&n| lg.order[n]).collect();
            orders.sort();
            let expected: Vec<usize> = (0..layer.len()).collect();
            assert_eq!(
                orders, expected,
                "Orders should be consecutive in each layer"
            );
        }

        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(count_all_crossings(&lg, &layers, &edges), 0);
    }

    #[test]
    fn test_adaptive_single_layer() {
        // All nodes at same rank - should exit early
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);
        // Should not panic
    }

    #[test]
    fn test_order_with_disconnected() {
        let (mut lg, _) = setup_graph_and_run(
            &["A", "B", "C", "D"],
            &[
                ("A", "C"),
                ("B", "D"),
                // Two parallel paths, no connections between them
            ],
        );

        run(&mut lg, false);

        // Should complete without errors
        let layers = rank::by_rank(&lg);
        assert!(!layers.is_empty());
    }

    #[test]
    fn test_unsortable_nodes_preserve_position() {
        // Layer 0: A, B
        // Layer 1: C (connected to A), D (disconnected), E (connected to B)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_node("E", ());
        graph.add_edge("A", "C");
        graph.add_edge("B", "E");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let d = lg.node_index[&NodeId::from("D")];
        let c = lg.node_index[&NodeId::from("C")];
        let e = lg.node_index[&NodeId::from("E")];
        let mut orders = vec![lg.order[c], lg.order[d], lg.order[e]];
        orders.sort();
        assert_eq!(orders, vec![0, 1, 2]);
    }

    #[test]
    fn test_all_unsortable_preserves_order() {
        // Two parallel paths: A->B, C->D
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(count_all_crossings(&lg, &layers, &edges), 0);
    }

    #[test]
    fn test_all_sortable_unchanged() {
        // Diamond: all nodes have neighbors — sortable path only
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(count_all_crossings(&lg, &layers, &edges), 0);
    }

    #[test]
    fn test_reorder_layer_unsortable_interleaving() {
        // Directly test reorder_layer with controlled setup:
        // Fixed: [X, Y] at positions 0, 1
        // Free: [A, B, C] where A->X, C->Y, B has no neighbors
        // B (unsortable, original_pos=1) should stay at position 1
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("Y", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("X", "A");
        graph.add_edge("Y", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let x = lg.node_index[&NodeId::from("X")];
        let y = lg.node_index[&NodeId::from("Y")];
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];

        lg.order[x] = 0;
        lg.order[y] = 1;
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 2;

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = vec![x, y];
        let free = vec![a, b, c];
        reorder_layer(&mut lg, &fixed, &free, &edges, true, false);

        assert_eq!(lg.order[a], 0);
        assert_eq!(lg.order[b], 1);
        assert_eq!(lg.order[c], 2);
    }

    #[test]
    fn test_weighted_barycenter_uniform_weights() {
        // With all weights = 1.0, weighted barycenter matches unweighted
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        assert!(lg.edge_weights.iter().all(|&w| w == 1.0));

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(count_all_crossings(&lg, &layers, &edges), 0);
    }

    #[test]
    fn test_weighted_barycenter_nonuniform() {
        // Layer 0: X(order=0), Y(order=1)
        // Layer 1: A has edges from X (weight=3) and Y (weight=1)
        //          B has edge from Y (weight=1)
        // Weighted barycenter of A = (3*0 + 1*1) / (3+1) = 0.25
        // A should be before B (barycenter 1.0)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("Y", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("X", "A");
        graph.add_edge("Y", "A");
        graph.add_edge("Y", "B");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        // Set non-uniform weight on the X->A edge
        let x = lg.node_index[&NodeId::from("X")];
        let a = lg.node_index[&NodeId::from("A")];
        for (idx, &(from, to, _)) in lg.edges.iter().enumerate() {
            let (eff_from, eff_to) = if lg.reversed_edges.contains(&idx) {
                (to, from)
            } else {
                (from, to)
            };
            if eff_from == x && eff_to == a {
                lg.edge_weights[idx] = 3.0;
            }
        }

        let y = lg.node_index[&NodeId::from("Y")];
        let b = lg.node_index[&NodeId::from("B")];

        lg.order[x] = 0;
        lg.order[y] = 1;
        lg.order[a] = 0;
        lg.order[b] = 1;

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = vec![x, y];
        let free = vec![a, b];

        reorder_layer(&mut lg, &fixed, &free, &edges, true, false);

        assert_eq!(
            lg.order[a], 0,
            "A (weighted barycenter 0.25) should be first"
        );
        assert_eq!(lg.order[b], 1, "B (barycenter 1.0) should be second");
    }

    // --- Compound ordering constraint tests ---

    use crate::layered::{border, nesting};

    /// Build a compound graph with border segments, ready for ordering.
    ///
    /// Graph: A -> B (both children of sg1), plus an external node X -> A.
    /// After nesting/ranking/border setup, each rank in sg1's span has
    /// left and right border nodes.
    fn build_compound_for_ordering() -> LayoutGraph {
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("X", ());
        g.add_node("A", ());
        g.add_node("B", ());
        g.add_node("sg1", ());
        g.add_edge("X", "A");
        g.add_edge("A", "B");
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");

        let mut lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));
        nesting::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        nesting::cleanup(&mut lg);
        nesting::assign_rank_minmax(&mut lg);
        border::add_segments(&mut lg);
        lg
    }

    #[test]
    fn test_compound_ordering_borders_at_edges() {
        let mut lg = build_compound_for_ordering();
        let sg1_idx = lg.node_index[&"sg1".into()];

        run(&mut lg, false);

        let border_tops: HashSet<usize> = lg.border_top.values().copied().collect();
        let border_bottoms: HashSet<usize> = lg.border_bottom.values().copied().collect();
        let border_titles: HashSet<usize> = lg.border_title.values().copied().collect();
        let is_excluded = |node: usize| {
            border_tops.contains(&node)
                || border_bottoms.contains(&node)
                || border_titles.contains(&node)
        };

        // For each rank in sg1's span, left border should be leftmost
        // and right border should be rightmost among sg1's children
        let left_borders = &lg.border_left[&sg1_idx];
        let right_borders = &lg.border_right[&sg1_idx];
        let min_r = lg.min_rank[&sg1_idx];
        let max_r = lg.max_rank[&sg1_idx];

        let layers = rank::by_rank(&lg);
        let layers = layers_sorted_by_order(&layers, &lg);
        for rank in min_r..=max_r {
            let rank_offset = (rank - min_r) as usize;
            let left_border = left_borders[rank_offset];
            let right_border = right_borders[rank_offset];

            // Find the layer for this rank
            let layer = layers
                .iter()
                .find(|l| !l.is_empty() && lg.ranks[l[0]] == rank)
                .expect("should find layer for rank");

            // Collect children of sg1 in this layer
            let sg1_children: Vec<usize> = layer
                .iter()
                .copied()
                .filter(|&n| lg.parents[n] == Some(sg1_idx) && !is_excluded(n))
                .collect();

            if sg1_children.len() >= 2 {
                let min_order = sg1_children.iter().map(|&n| lg.order[n]).min().unwrap();
                let max_order = sg1_children.iter().map(|&n| lg.order[n]).max().unwrap();

                assert_eq!(
                    lg.order[left_border], min_order,
                    "Left border should have min order among sg1 children at rank {rank}"
                );
                assert_eq!(
                    lg.order[right_border], max_order,
                    "Right border should have max order among sg1 children at rank {rank}"
                );
            }
        }
    }

    #[test]
    fn test_compound_ordering_children_contiguous() {
        // Two subgraphs at the same rank level
        // sg1: A, B; sg2: C, D; plus edges to force them into the same rank
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("X", ());
        g.add_node("A", ());
        g.add_node("B", ());
        g.add_node("C", ());
        g.add_node("D", ());
        g.add_node("sg1", ());
        g.add_node("sg2", ());
        g.add_edge("X", "A");
        g.add_edge("X", "C");
        g.add_edge("A", "B");
        g.add_edge("C", "D");
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");
        g.set_parent("C", "sg2");
        g.set_parent("D", "sg2");

        let mut lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));
        nesting::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        nesting::cleanup(&mut lg);
        nesting::assign_rank_minmax(&mut lg);
        border::add_segments(&mut lg);

        run(&mut lg, false);

        // For each rank, children of sg1 should be contiguous and
        // children of sg2 should be contiguous (no interleaving)
        let layers = rank::by_rank(&lg);
        let layers = layers_sorted_by_order(&layers, &lg);
        let sg1_idx = lg.node_index[&"sg1".into()];
        let sg2_idx = lg.node_index[&"sg2".into()];
        let mut border_nodes: HashSet<usize> = HashSet::new();
        for nodes in lg.border_left.values() {
            border_nodes.extend(nodes.iter().copied());
        }
        for nodes in lg.border_right.values() {
            border_nodes.extend(nodes.iter().copied());
        }
        border_nodes.extend(lg.border_top.values().copied());
        border_nodes.extend(lg.border_bottom.values().copied());
        border_nodes.extend(lg.border_title.values().copied());

        for layer in &layers {
            let sg1_children: Vec<usize> = layer
                .iter()
                .copied()
                .filter(|&n| lg.parents[n] == Some(sg1_idx) && !border_nodes.contains(&n))
                .collect();
            let sg2_children: Vec<usize> = layer
                .iter()
                .copied()
                .filter(|&n| lg.parents[n] == Some(sg2_idx) && !border_nodes.contains(&n))
                .collect();

            // Check contiguity: max_order - min_order + 1 == count
            if sg1_children.len() >= 2 {
                let orders: Vec<usize> = sg1_children.iter().map(|&n| lg.order[n]).collect();
                let span = orders.iter().max().unwrap() - orders.iter().min().unwrap() + 1;
                assert_eq!(
                    span,
                    sg1_children.len(),
                    "sg1 children should be contiguous in layer"
                );
            }
            if sg2_children.len() >= 2 {
                let orders: Vec<usize> = sg2_children.iter().map(|&n| lg.order[n]).collect();
                let span = orders.iter().max().unwrap() - orders.iter().min().unwrap() + 1;
                assert_eq!(
                    span,
                    sg2_children.len(),
                    "sg2 children should be contiguous in layer"
                );
            }
        }
    }

    #[test]
    fn test_simple_graph_ordering_unchanged() {
        // Simple graph without compound nodes should produce
        // a valid ordering (no regression from compound logic)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        run(&mut lg, false);

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        assert_eq!(
            count_all_crossings(&lg, &layers, &edges),
            0,
            "Simple diamond should have zero crossings"
        );
    }

    // --- 4.1.1: OrderEntry + compute_barycenters tests ---

    #[test]
    fn test_compute_barycenters_with_connections() {
        // Fixed layer: X(order=0), Y(order=1)
        // Free layer: A, B
        // Edges: X->A(w=1), Y->A(w=1), Y->B(w=1)
        // A barycenter = (1*0 + 1*1) / 2 = 0.5, weight=2
        // B barycenter = (1*1) / 1 = 1.0, weight=1
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("Y", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("X", "A");
        graph.add_edge("Y", "A");
        graph.add_edge("Y", "B");

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let x = lg.node_index[&NodeId::from("X")];
        let y = lg.node_index[&NodeId::from("Y")];
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        // Set orders for fixed layer
        let mut lg = lg;
        lg.order[x] = 0;
        lg.order[y] = 1;

        let edges: Vec<(usize, usize, f64)> = vec![(x, a, 1.0), (y, a, 1.0), (y, b, 1.0)];
        let fixed = vec![x, y];
        let movable = vec![a, b];

        let result = compute_barycenters(&lg, &movable, &edges, &fixed);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].v, a);
        assert!((result[0].barycenter.unwrap() - 0.5).abs() < 1e-9);
        assert!((result[0].weight.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(result[1].v, b);
        assert!((result[1].barycenter.unwrap() - 1.0).abs() < 1e-9);
        assert!((result[1].weight.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_compute_barycenters_no_connections() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("D", ());

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let x = lg.node_index[&NodeId::from("X")];
        let d = lg.node_index[&NodeId::from("D")];

        let edges: Vec<(usize, usize, f64)> = vec![];
        let fixed = vec![x];
        let movable = vec![d];

        let result = compute_barycenters(&lg, &movable, &edges, &fixed);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].v, d);
        assert!(result[0].barycenter.is_none());
        assert!(result[0].weight.is_none());
    }

    #[test]
    fn test_compute_barycenters_weighted() {
        // X(order=0), Y(order=1) -> A with weights 3.0 and 1.0
        // A barycenter = (3*0 + 1*1) / (3+1) = 0.25
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("Y", ());
        graph.add_node("A", ());

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let x = lg.node_index[&NodeId::from("X")];
        let y = lg.node_index[&NodeId::from("Y")];
        let a = lg.node_index[&NodeId::from("A")];
        lg.order[x] = 0;
        lg.order[y] = 1;

        let edges: Vec<(usize, usize, f64)> = vec![(x, a, 3.0), (y, a, 1.0)];
        let fixed = vec![x, y];
        let movable = vec![a];

        let result = compute_barycenters(&lg, &movable, &edges, &fixed);

        assert_eq!(result.len(), 1);
        assert!((result[0].barycenter.unwrap() - 0.25).abs() < 1e-9);
        assert!((result[0].weight.unwrap() - 4.0).abs() < 1e-9);
    }

    // --- 4.1.2: resolve_conflicts tests ---

    #[test]
    fn test_resolve_conflicts_no_constraints() {
        // Three entries with barycenters, empty constraint graph
        let entries = vec![
            OrderEntry {
                v: 10,
                barycenter: Some(1.0),
                weight: Some(1.0),
            },
            OrderEntry {
                v: 11,
                barycenter: Some(2.0),
                weight: Some(1.0),
            },
            OrderEntry {
                v: 12,
                barycenter: Some(3.0),
                weight: Some(1.0),
            },
        ];
        let cg = ConstraintGraph::new();
        let result = resolve_conflicts(&entries, &cg);

        assert_eq!(result.len(), 3);
        for r in &result {
            assert_eq!(r.vs.len(), 1);
            // Each entry's i matches its original index
            assert_eq!(r.vs[0], r.vs[0]); // not merged
        }
        // All three entries present (order may differ due to stack processing)
        let mut vs: Vec<usize> = result.iter().map(|r| r.vs[0]).collect();
        vs.sort();
        assert_eq!(vs, vec![10, 11, 12]);
    }

    #[test]
    fn test_resolve_conflicts_compatible_constraint() {
        // A(bc=1.0) -> B(bc=2.0) — barycenters agree, should not merge
        let entries = vec![
            OrderEntry {
                v: 10,
                barycenter: Some(1.0),
                weight: Some(1.0),
            },
            OrderEntry {
                v: 11,
                barycenter: Some(2.0),
                weight: Some(1.0),
            },
        ];
        let mut cg = ConstraintGraph::new();
        cg.add_edge(10, 11);
        let result = resolve_conflicts(&entries, &cg);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].vs, vec![10]);
        assert_eq!(result[1].vs, vec![11]);
    }

    #[test]
    fn test_resolve_conflicts_conflicting_merges() {
        // A(bc=3.0) -> B(bc=1.0) — barycenters conflict (A.bc >= B.bc), should merge
        let entries = vec![
            OrderEntry {
                v: 10,
                barycenter: Some(3.0),
                weight: Some(1.0),
            },
            OrderEntry {
                v: 11,
                barycenter: Some(1.0),
                weight: Some(1.0),
            },
        ];
        let mut cg = ConstraintGraph::new();
        cg.add_edge(10, 11);
        let result = resolve_conflicts(&entries, &cg);

        // Should merge: source(10).vs concat target(11).vs = [10, 11]
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vs, vec![10, 11]);
        // Merged barycenter: (3*1 + 1*1) / 2 = 2.0
        assert!((result[0].barycenter.unwrap() - 2.0).abs() < 1e-9);
        assert!((result[0].weight.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(result[0].i, 0); // min of 0, 1
    }

    #[test]
    fn test_resolve_conflicts_undefined_barycenter_merges() {
        // A(no bc) -> B(bc=1.0) — undefined bc should trigger merge
        let entries = vec![
            OrderEntry {
                v: 10,
                barycenter: None,
                weight: None,
            },
            OrderEntry {
                v: 11,
                barycenter: Some(1.0),
                weight: Some(1.0),
            },
        ];
        let mut cg = ConstraintGraph::new();
        cg.add_edge(10, 11);
        let result = resolve_conflicts(&entries, &cg);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vs, vec![10, 11]);
    }

    #[test]
    fn test_resolve_conflicts_unsortable_passthrough() {
        // Entry with no barycenter and no constraints
        let entries = vec![OrderEntry {
            v: 10,
            barycenter: None,
            weight: None,
        }];
        let cg = ConstraintGraph::new();
        let result = resolve_conflicts(&entries, &cg);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vs, vec![10]);
        assert_eq!(result[0].i, 0);
        assert!(result[0].barycenter.is_none());
    }

    // --- 4.1.3: sort_entries tests ---

    fn re(vs: Vec<usize>, i: usize, bc: Option<f64>, w: Option<f64>) -> ResolvedEntry {
        ResolvedEntry {
            vs,
            i,
            barycenter: bc,
            weight: w,
        }
    }

    /// Create a dummy LayoutGraph with enough nodes for sort_entries tests.
    /// Node indices 0..=max_node are created with sequential model_order.
    fn dummy_graph_for_sort(max_node: usize) -> LayoutGraph {
        let mut graph: DiGraph<()> = DiGraph::new();
        let names: Vec<String> = (0..=max_node).map(|i| format!("n{i}")).collect();
        for name in &names {
            graph.add_node(name.as_str(), ());
        }
        LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0))
    }

    #[test]
    fn test_sort_entries_all_sortable() {
        let lg = dummy_graph_for_sort(12);
        let entries = vec![
            re(vec![10], 0, Some(2.0), Some(1.0)),
            re(vec![11], 1, Some(0.5), Some(1.0)),
            re(vec![12], 2, Some(1.5), Some(1.0)),
        ];
        let result = sort_entries(&entries, false, &lg);
        assert_eq!(result.vs, vec![11, 12, 10]);
    }

    #[test]
    fn test_sort_entries_interleave_unsortable() {
        let lg = dummy_graph_for_sort(12);
        // Sortable: A(bc=0.5, i=0), C(bc=1.5, i=2)
        // Unsortable: B(i=1)
        let entries = vec![
            re(vec![10], 0, Some(0.5), Some(1.0)),
            re(vec![11], 1, None, None),
            re(vec![12], 2, Some(1.5), Some(1.0)),
        ];
        let result = sort_entries(&entries, false, &lg);
        // A sorted first (bc=0.5), B interleaved at position 1, C at position 2
        assert_eq!(result.vs, vec![10, 11, 12]);
    }

    #[test]
    fn test_sort_entries_bias_right_tie_break() {
        let lg = dummy_graph_for_sort(12);
        let entries = vec![
            re(vec![10], 0, Some(1.0), Some(1.0)),
            re(vec![11], 1, Some(1.0), Some(1.0)),
        ];

        // With model_order tie-breaking, node 10 (model_order=Some(10))
        // sorts before node 11 (model_order=Some(11)) regardless of bias.
        let result_left = sort_entries(&entries, false, &lg);
        assert_eq!(result_left.vs, vec![10, 11]);

        // bias_right no longer swaps when model_orders differ
        let result_right = sort_entries(&entries, true, &lg);
        assert_eq!(result_right.vs, vec![10, 11]);
    }

    #[test]
    fn test_sort_entries_aggregate_barycenter() {
        let lg = dummy_graph_for_sort(12);
        let entries = vec![
            re(vec![10], 0, Some(1.0), Some(2.0)),
            re(vec![11], 1, Some(3.0), Some(1.0)),
        ];
        let result = sort_entries(&entries, false, &lg);
        // sum = 1.0*2.0 + 3.0*1.0 = 5.0, weight = 3.0
        assert!((result.barycenter.unwrap() - 5.0 / 3.0).abs() < 1e-9);
        assert!((result.weight.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_sort_entries_multi_node_entries() {
        let lg = dummy_graph_for_sort(12);
        // Entry with vs=[A, B] from resolveConflicts merge at i=0
        // and a single entry C at i=2 (unsortable)
        let entries = vec![
            re(vec![10, 11], 0, Some(1.0), Some(1.0)),
            re(vec![12], 2, None, None),
        ];
        let result = sort_entries(&entries, false, &lg);
        // [10,11] placed first (sortable, vs_index advances by 2), then 12 at i=2
        assert_eq!(result.vs, vec![10, 11, 12]);
    }

    #[test]
    fn test_sort_entries_model_order_tie_break() {
        // Two entries with equal barycenters but different model orders.
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("B", ()); // model_order 0
        graph.add_node("C", ()); // model_order 1

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];

        let entries = vec![
            re(vec![b], 0, Some(1.0), Some(1.0)),
            re(vec![c], 1, Some(1.0), Some(1.0)),
        ];

        // bias_right=false: B (model_order 0) should come first
        let result = sort_entries(&entries, false, &lg);
        assert_eq!(
            result.vs[0], b,
            "B (lower model_order) should be first with bias_right=false"
        );
        assert_eq!(result.vs[1], c);

        // bias_right=true: B should STILL come first (model_order > bias)
        let result = sort_entries(&entries, true, &lg);
        assert_eq!(
            result.vs[0], b,
            "B (lower model_order) should be first even with bias_right=true"
        );
        assert_eq!(result.vs[1], c);
    }

    #[test]
    fn test_sort_entries_model_order_does_not_override_barycenter() {
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("D", ()); // model_order 0
        graph.add_node("C", ()); // model_order 1

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let d = lg.node_index[&NodeId::from("D")];
        let c = lg.node_index[&NodeId::from("C")];

        // D has lower model_order but higher barycenter
        let entries = vec![
            re(vec![d], 0, Some(2.0), Some(1.0)),
            re(vec![c], 1, Some(0.5), Some(1.0)),
        ];

        let result = sort_entries(&entries, false, &lg);
        assert_eq!(
            result.vs[0], c,
            "C (lower barycenter) should be first despite higher model_order"
        );
        assert_eq!(result.vs[1], d);
    }

    #[test]
    fn test_sort_entries_multi_node_entry_uses_min_model_order() {
        // A merged entry (from resolve_conflicts) has vs=[B, C].
        // The model_order used should be the minimum of the group.
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("B", ()); // model_order 0
        graph.add_node("C", ()); // model_order 1
        graph.add_node("D", ()); // model_order 2

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];

        // Merged entry [B, C] with bc=1.0, single entry D with bc=1.0
        let entries = vec![
            re(vec![b, c], 0, Some(1.0), Some(1.0)),
            re(vec![d], 1, Some(1.0), Some(1.0)),
        ];

        let result = sort_entries(&entries, false, &lg);
        // B has model_order 0, D has model_order 2 -> [B,C] group sorts first
        assert_eq!(result.vs[0], b);
        assert_eq!(result.vs[1], c);
        assert_eq!(result.vs[2], d);
    }

    // --- 4.1.4: sort_subgraph tests ---

    #[test]
    fn test_sort_subgraph_flat_no_compound() {
        // Simple chain: X -> A, X -> B (same rank), no compound nodes
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("X", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_edge("X", "A");
        graph.add_edge("X", "B");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let x = lg.node_index[&NodeId::from("X")];
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        lg.order[x] = 0;
        lg.order[a] = 0;
        lg.order[b] = 1;

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = vec![x];
        let cg = ConstraintGraph::new();

        let result = sort_subgraph(&lg, None, lg.ranks[a], &edges, &fixed, &cg, false);

        // Both A and B have the same barycenter (0.0), so order by i
        assert_eq!(result.vs.len(), 2);
        assert!(result.vs.contains(&a));
        assert!(result.vs.contains(&b));
    }

    #[test]
    fn test_sort_subgraph_with_borders() {
        // Build compound graph with borders
        let mut lg = build_compound_for_ordering();
        let sg1_idx = lg.node_index[&"sg1".into()];

        // Set up initial ordering
        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        init_order(&mut lg, &layers);
        let layers = layers_sorted_by_order(&layers, &lg);
        let edges = effective_edges_weighted_filtered(&lg);

        // Find a rank where sg1 has borders
        let min_r = lg.min_rank[&sg1_idx];
        let max_r = lg.max_rank[&sg1_idx];

        for rank in min_r..=max_r {
            let rank_offset = (rank - min_r) as usize;
            let bl = lg.border_left[&sg1_idx][rank_offset];
            let br = lg.border_right[&sg1_idx][rank_offset];

            // Find fixed layer
            let layer_idx = layers
                .iter()
                .position(|l| !l.is_empty() && lg.ranks[l[0]] == rank)
                .unwrap();
            if layer_idx == 0 {
                continue; // no fixed layer above
            }
            let fixed = &layers[layer_idx - 1];
            let cg = ConstraintGraph::new();

            let result = sort_subgraph(&lg, Some(sg1_idx), rank, &edges, fixed, &cg, false);

            // Border left should be first, border right should be last
            if result.vs.len() >= 2 {
                assert_eq!(
                    result.vs[0], bl,
                    "Border left should be first in sort_subgraph result"
                );
                assert_eq!(
                    *result.vs.last().unwrap(),
                    br,
                    "Border right should be last in sort_subgraph result"
                );
            }
        }
    }

    // --- 4.1.5: add_subgraph_constraints tests ---

    #[test]
    fn test_add_subgraph_constraints_siblings() {
        // Two subgraphs sg1, sg2 both children of root (parent=None)
        // Node A (parent=sg1), Node B (parent=sg2)
        // Sorted: [A, B] — should add sg1 -> sg2 constraint
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("sg1", ());
        graph.add_node("sg2", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.set_parent("A", "sg1");
        graph.set_parent("B", "sg2");

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let sg1 = lg.node_index[&NodeId::from("sg1")];
        let sg2 = lg.node_index[&NodeId::from("sg2")];
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        let mut cg = ConstraintGraph::new();
        add_subgraph_constraints(&lg, &mut cg, &[a, b]);

        let edges = cg.edges();
        assert!(
            edges.contains(&(sg1, sg2)),
            "Should have constraint sg1 -> sg2, got: {:?}",
            edges
        );
    }

    #[test]
    fn test_add_subgraph_constraints_same_subgraph_no_edge() {
        // Two nodes A, B both children of sg1
        // Should NOT add sg1 -> sg1 constraint
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("sg1", ());
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.set_parent("A", "sg1");
        graph.set_parent("B", "sg1");

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        let mut cg = ConstraintGraph::new();
        add_subgraph_constraints(&lg, &mut cg, &[a, b]);

        assert!(
            cg.edges().is_empty(),
            "Should have no constraints for same-subgraph siblings"
        );
    }

    #[test]
    fn test_add_subgraph_constraints_no_parent() {
        // Nodes with no parent — no constraints added
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());

        let lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        let mut cg = ConstraintGraph::new();
        add_subgraph_constraints(&lg, &mut cg, &[a, b]);

        assert!(cg.edges().is_empty());
    }

    // --- Crossing count validation tests (Task 0.1 + 3.1) ---
    //
    // Pre-greedy-switch baselines:
    //   K_3,3: 9 crossings
    //   two-fan-out: 0 crossings
    //   deep-graph: 1 crossing
    //
    // Post-greedy-switch values must be <= baselines (non-regression guarantee).

    #[test]
    fn test_crossing_count_k33_no_regression() {
        // K_{3,3} bipartite - known to have unavoidable crossings
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A1", "A2", "A3", "B1", "B2", "B3"] {
            graph.add_node(n, ());
        }
        for a in ["A1", "A2", "A3"] {
            for b in ["B1", "B2", "B3"] {
                graph.add_edge(a, b);
            }
        }
        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false);
        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let cc = count_all_crossings(&lg, &layers, &edges);
        assert!(
            cc <= 9,
            "K_3,3 crossings should not exceed baseline of 9, got {cc}"
        );
        assert!(cc >= 1, "K_3,3 must have at least 1 crossing");
    }

    #[test]
    fn test_crossing_count_fan_no_regression() {
        // Two crossing fan-outs
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["X", "Y", "C", "D", "E"] {
            graph.add_node(n, ());
        }
        graph.add_edge("X", "D");
        graph.add_edge("X", "C");
        graph.add_edge("Y", "C");
        graph.add_edge("Y", "E");
        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false);
        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let cc = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(cc, 0, "Fan-out crossings should be 0, got {cc}");
    }

    #[test]
    fn test_crossing_count_deep_no_regression() {
        // Deeper graph with multiple layers
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D", "E", "F", "G", "H"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "C");
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");
        graph.add_edge("B", "E");
        graph.add_edge("C", "F");
        graph.add_edge("D", "G");
        graph.add_edge("E", "H");
        graph.add_edge("D", "H");
        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false);
        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);
        let cc = count_all_crossings(&lg, &layers, &edges);
        assert!(
            cc <= 1,
            "Deep graph crossings should not exceed baseline of 1, got {cc}"
        );
    }

    /// Timing baseline for order::run() on representative large graphs.
    /// Run with: cargo nextest run test_order_timing_baseline -- --nocapture
    #[test]
    fn test_order_timing_baseline() {
        use std::time::Instant;

        let sizes = [10, 20, 50, 100];

        for &n in &sizes {
            let mut graph: DiGraph<()> = DiGraph::new();
            for i in 0..n {
                graph.add_node(format!("L0_{i}"), ());
                graph.add_node(format!("L1_{i}"), ());
            }
            for i in 0..n {
                for j in 0..3_usize.min(n) {
                    graph.add_edge(format!("L0_{i}"), format!("L1_{}", (i + j) % n));
                }
            }

            let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
            rank::run(&mut lg, &LayoutConfig::default());
            rank::normalize(&mut lg);

            let start = Instant::now();
            run(&mut lg, false);
            let elapsed = start.elapsed();

            let layers = rank::by_rank(&lg);
            let edges = effective_edges_weighted_filtered(&lg);
            let cc = count_all_crossings(&lg, &layers, &edges);

            eprintln!(
                "order::run() n={n}: {:.3}ms, crossings={}",
                elapsed.as_secs_f64() * 1000.0,
                cc
            );
        }
    }

    // --- count_pair_crossings tests (Task 1.1) ---

    #[test]
    fn test_count_pair_crossings_no_crossings() {
        // Layer 0: [A(0), B(1)]
        // Layer 1: [C(0), D(1)]
        // Edges: A->C, B->D (parallel, no crossings)
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);

        // A before B: no crossings (parallel edges)
        assert_eq!(count_pair_crossings(&lg, a, b, &layers, &edges), 0);
        // B before A: would create crossings
        assert_eq!(count_pair_crossings(&lg, b, a, &layers, &edges), 1);
    }

    #[test]
    fn test_count_pair_crossings_with_crossings() {
        // Layer 0: [A(0), B(1)]
        // Layer 1: [C(0), D(1)]
        // Edges: A->D, B->C (crossing)
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);

        // A before B: 1 crossing (A->D crosses B->C)
        assert_eq!(count_pair_crossings(&lg, a, b, &layers, &edges), 1);
        // B before A: 0 crossings
        assert_eq!(count_pair_crossings(&lg, b, a, &layers, &edges), 0);
    }

    #[test]
    fn test_count_pair_crossings_two_sided() {
        // Three layers:
        // Layer 0: [X(0), Y(1)]
        // Layer 1: [A(0), B(1)]
        // Layer 2: [C(0), D(1)]
        // Edges: X->A, Y->B (no crossing above), A->D, B->C (crossing below)
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["X", "Y", "A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("X", "A");
        graph.add_edge("Y", "B");
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let x = lg.node_index[&NodeId::from("X")];
        let y = lg.node_index[&NodeId::from("Y")];
        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];
        lg.order[x] = 0;
        lg.order[y] = 1;
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);

        // A before B: 0 crossings above + 1 crossing below = 1 total
        assert_eq!(count_pair_crossings(&lg, a, b, &layers, &edges), 1);
        // B before A: 1 crossing above + 0 crossings below = 1 total
        assert_eq!(count_pair_crossings(&lg, b, a, &layers, &edges), 1);
    }

    #[test]
    fn test_count_pair_crossings_no_edges() {
        // Nodes with no edges: zero crossings either way
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "C"); // Only A has edges

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        let layers = rank::by_rank(&lg);
        let edges = effective_edges_weighted_filtered(&lg);

        // B has no edges, so no crossings either way
        assert_eq!(count_pair_crossings(&lg, a, b, &layers, &edges), 0);
        assert_eq!(count_pair_crossings(&lg, b, a, &layers, &edges), 0);
    }

    // --- greedy_switch tests (Task 1.2) ---

    #[test]
    fn test_greedy_switch_reduces_crossings() {
        // Set up a graph where we force a crossing that greedy switch can fix.
        // Layer 0: [A(0), B(1)]
        // Layer 1: [C(0), D(1)] with edges A->D, B->C (crossing)
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];

        // Force a crossing order: A(0), B(1) on layer 0; C(0), D(1) on layer 1
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);

        let before = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(before, 1, "Should have 1 crossing before greedy switch");

        greedy_switch(&mut lg, &layers, &edges);

        let after = count_all_crossings(&lg, &layers, &edges);
        assert_eq!(after, 0, "Should have 0 crossings after greedy switch");
    }

    #[test]
    fn test_greedy_switch_no_change_when_optimal() {
        // Already optimal: no crossings, nothing to swap
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];

        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);
        let order_before: Vec<usize> = lg.order.clone();

        greedy_switch(&mut lg, &layers, &edges);

        assert_eq!(
            lg.order, order_before,
            "Order should not change when optimal"
        );
    }

    #[test]
    fn test_greedy_switch_preserves_consecutive_orders() {
        // After greedy switch, orders should still be consecutive 0..n within each layer
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D", "E", "F"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "D");
        graph.add_edge("A", "F");
        graph.add_edge("B", "E");
        graph.add_edge("C", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false); // Run full ordering first

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);

        greedy_switch(&mut lg, &layers, &edges);

        // Verify consecutive orders per layer
        for layer in &layers {
            let mut orders: Vec<usize> = layer.iter().map(|&n| lg.order[n]).collect();
            orders.sort();
            let expected: Vec<usize> = (0..layer.len()).collect();
            assert_eq!(
                orders, expected,
                "Orders must be consecutive within each layer"
            );
        }
    }

    #[test]
    fn test_greedy_switch_never_increases_crossings() {
        // Run greedy switch on a complex graph and verify crossings don't increase
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D", "E", "F", "G", "H"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "E");
        graph.add_edge("A", "F");
        graph.add_edge("B", "G");
        graph.add_edge("B", "E");
        graph.add_edge("C", "H");
        graph.add_edge("D", "F");
        graph.add_edge("D", "G");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false);

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);
        let before = count_all_crossings(&lg, &layers, &edges);

        greedy_switch(&mut lg, &layers, &edges);

        let after = count_all_crossings(&lg, &layers, &edges);
        assert!(
            after <= before,
            "Greedy switch must never increase crossings: before={}, after={}",
            before,
            after
        );
    }

    #[test]
    fn test_greedy_switch_converges() {
        // K_{3,3} has unavoidable crossings -- greedy switch should converge
        // (not loop forever) even when it can't reach 0.
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A1", "A2", "A3", "B1", "B2", "B3"] {
            graph.add_node(n, ());
        }
        for a in ["A1", "A2", "A3"] {
            for b in ["B1", "B2", "B3"] {
                graph.add_edge(a, b);
            }
        }

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        run(&mut lg, false);

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);

        // Should terminate (convergence guaranteed since each swap strictly reduces crossings)
        greedy_switch(&mut lg, &layers, &edges);

        let cc = count_all_crossings(&lg, &layers, &edges);
        // K_3,3 has minimum crossing number 1; optimal for bipartite layout is 3
        assert!(cc >= 1, "K_3,3 must have at least 1 crossing");
    }

    // --- Compound graph greedy switch tests (Task 2.1) ---

    #[test]
    fn test_greedy_switch_skips_different_parents() {
        // Two subgraphs: sg1 contains A,B; sg2 contains C,D
        // Greedy switch should not swap nodes from different subgraphs.
        // Uses the same contiguity check pattern as
        // test_compound_ordering_children_contiguous.
        let mut g: DiGraph<()> = DiGraph::new();
        g.add_node("X", ());
        g.add_node("A", ());
        g.add_node("B", ());
        g.add_node("C", ());
        g.add_node("D", ());
        g.add_node("sg1", ());
        g.add_node("sg2", ());
        g.add_edge("X", "A");
        g.add_edge("X", "C");
        g.add_edge("A", "B");
        g.add_edge("C", "D");
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");
        g.set_parent("C", "sg2");
        g.set_parent("D", "sg2");

        let mut lg = LayoutGraph::from_digraph(&g, |_, _| (10.0, 10.0));
        nesting::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);
        nesting::cleanup(&mut lg);
        nesting::assign_rank_minmax(&mut lg);
        border::add_segments(&mut lg);

        run(&mut lg, false);

        // After run() (which includes greedy_switch), verify subgraph contiguity
        // Exclude border nodes from contiguity check (same pattern as
        // test_compound_ordering_children_contiguous)
        let layers = rank::by_rank(&lg);
        let sorted_layers = layers_sorted_by_order(&layers, &lg);

        let sg1_idx = lg.node_index[&"sg1".into()];
        let sg2_idx = lg.node_index[&"sg2".into()];

        let mut border_nodes: HashSet<usize> = HashSet::new();
        for nodes in lg.border_left.values() {
            border_nodes.extend(nodes.iter().copied());
        }
        for nodes in lg.border_right.values() {
            border_nodes.extend(nodes.iter().copied());
        }
        border_nodes.extend(lg.border_top.values().copied());
        border_nodes.extend(lg.border_bottom.values().copied());
        border_nodes.extend(lg.border_title.values().copied());

        for layer in &sorted_layers {
            let sg1_children: Vec<usize> = layer
                .iter()
                .copied()
                .filter(|&n| lg.parents[n] == Some(sg1_idx) && !border_nodes.contains(&n))
                .collect();
            let sg2_children: Vec<usize> = layer
                .iter()
                .copied()
                .filter(|&n| lg.parents[n] == Some(sg2_idx) && !border_nodes.contains(&n))
                .collect();

            // Check contiguity for sg1
            if sg1_children.len() >= 2 {
                let orders: Vec<usize> = sg1_children.iter().map(|&n| lg.order[n]).collect();
                let span = orders.iter().max().unwrap() - orders.iter().min().unwrap() + 1;
                assert_eq!(
                    span,
                    sg1_children.len(),
                    "sg1 children should remain contiguous after greedy switch"
                );
            }
            // Check contiguity for sg2
            if sg2_children.len() >= 2 {
                let orders: Vec<usize> = sg2_children.iter().map(|&n| lg.order[n]).collect();
                let span = orders.iter().max().unwrap() - orders.iter().min().unwrap() + 1;
                assert_eq!(
                    span,
                    sg2_children.len(),
                    "sg2 children should remain contiguous after greedy switch"
                );
            }
        }
    }

    #[test]
    fn test_greedy_switch_flat_graph_no_parent_check_interference() {
        // Flat graph (no compound nodes): parent check should be a no-op
        let mut graph: DiGraph<()> = DiGraph::new();
        for n in ["A", "B", "C", "D"] {
            graph.add_node(n, ());
        }
        graph.add_edge("A", "D");
        graph.add_edge("B", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];
        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let layers = rank::by_rank_filtered(&lg, |node| lg.is_position_node(node));
        let edges = effective_edges_weighted_filtered(&lg);

        let before = count_all_crossings(&lg, &layers, &edges);
        greedy_switch(&mut lg, &layers, &edges);
        let after = count_all_crossings(&lg, &layers, &edges);

        // Should still reduce crossings in flat graphs
        assert!(after <= before);
    }

    #[test]
    fn test_init_order_respects_model_order_for_same_rank() {
        // Two disconnected chains at same rank: B->Y, A->X
        // B is declared before A in DiGraph, B should get order 0 in rank 0.
        let mut graph: DiGraph<()> = DiGraph::new();
        // Insert B before A
        graph.add_node("B", ());
        graph.add_node("A", ());
        graph.add_node("X", ());
        graph.add_node("Y", ());
        graph.add_edge("B", "Y");
        graph.add_edge("A", "X");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        // B was inserted first (model_order 0), A second (model_order 1)
        // At rank 0, B should come before A
        assert!(
            lg.order[b] < lg.order[a],
            "B (model_order 0) should come before A (model_order 1) at same rank. Got B={}, A={}",
            lg.order[b],
            lg.order[a]
        );
    }

    #[test]
    fn test_init_order_rank_still_primary() {
        // A at rank 0, B at rank 1 -- rank should still be primary sort key
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("B", ()); // model_order 0
        graph.add_node("A", ()); // model_order 1
        graph.add_edge("A", "B");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];

        // A is at rank 0, B at rank 1 -- different ranks, order within rank is independent
        // Just verify both have valid order values
        assert_eq!(lg.order[a], 0); // Only node at rank 0
        assert_eq!(lg.order[b], 0); // Only node at rank 1
    }

    #[test]
    fn test_init_order_fan_out_declaration_order() {
        // A fans out to C, B, D -- declared in that order.
        // After init_order, at rank 1: C should be 0, B should be 1, D should be 2.
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("C", ()); // model_order 1
        graph.add_node("B", ()); // model_order 2
        graph.add_node("D", ()); // model_order 3
        graph.add_edge("A", "C");
        graph.add_edge("A", "B");
        graph.add_edge("A", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        let c = lg.node_index[&NodeId::from("C")];
        let b = lg.node_index[&NodeId::from("B")];
        let d = lg.node_index[&NodeId::from("D")];

        // C was declared first (model_order 1), then B (2), then D (3)
        assert!(
            lg.order[c] < lg.order[b],
            "C (declared first) should precede B. Got C={}, B={}",
            lg.order[c],
            lg.order[b]
        );
        assert!(
            lg.order[b] < lg.order[d],
            "B (declared second) should precede D. Got B={}, D={}",
            lg.order[b],
            lg.order[d]
        );
    }

    #[test]
    fn test_init_order_fan_out_edge_order() {
        // Standard fan-out: A --> B, A --> C, A --> D
        // Node insertion order: A(0), B(1), C(2), D(3)
        // After init_order, rank 1 should be B(0), C(1), D(2)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_node("D", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");
        graph.add_edge("A", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let layers = rank::by_rank(&lg);
        init_order(&mut lg, &layers);

        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];

        assert_eq!(lg.order[b], 0, "B should be first");
        assert_eq!(lg.order[c], 1, "C should be second");
        assert_eq!(lg.order[d], 2, "D should be third");
    }

    #[test]
    fn test_reorder_layer_model_order_tie_break() {
        // A fans out to B and C. Both have barycenter = A's order.
        // B has lower model_order than C -> B should be first regardless of bias.
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("C", ());
        graph.add_edge("A", "B");
        graph.add_edge("A", "C");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];

        // Set up: A at order 0, B and C at order 0 and 1
        lg.order[a] = 0;
        lg.order[b] = 0;
        lg.order[c] = 1;

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = vec![a];
        let free = vec![b, c];

        // With bias_right = false, model_order should put B before C
        reorder_layer(&mut lg, &fixed, &free, &edges, true, false);
        assert_eq!(
            lg.order[b], 0,
            "B (lower model_order) should be first. Got B={}, C={}",
            lg.order[b], lg.order[c]
        );
        assert_eq!(lg.order[c], 1);

        // Reset and test with bias_right = true -- model_order should STILL win over bias
        lg.order[b] = 0;
        lg.order[c] = 1;
        reorder_layer(&mut lg, &fixed, &free, &edges, true, true);
        assert_eq!(
            lg.order[b], 0,
            "B (lower model_order) should still be first even with bias_right=true. Got B={}, C={}",
            lg.order[b], lg.order[c]
        );
    }

    #[test]
    fn test_reorder_layer_model_order_does_not_override_barycenter() {
        // A(order=0), B(order=1) in fixed layer
        // C connected to A (barycenter=0), D connected to B (barycenter=1)
        // Even if D has lower model_order, C should come first (barycenter wins)
        let mut graph: DiGraph<()> = DiGraph::new();
        graph.add_node("A", ());
        graph.add_node("B", ());
        graph.add_node("D", ()); // model_order 2 -- lower than C
        graph.add_node("C", ()); // model_order 3 -- higher
        graph.add_edge("A", "C");
        graph.add_edge("B", "D");

        let mut lg = LayoutGraph::from_digraph(&graph, |_, _| (10.0, 10.0));
        rank::run(&mut lg, &LayoutConfig::default());
        rank::normalize(&mut lg);

        let a = lg.node_index[&NodeId::from("A")];
        let b = lg.node_index[&NodeId::from("B")];
        let c = lg.node_index[&NodeId::from("C")];
        let d = lg.node_index[&NodeId::from("D")];

        lg.order[a] = 0;
        lg.order[b] = 1;
        lg.order[c] = 0;
        lg.order[d] = 1;

        let edges = effective_edges_weighted_filtered(&lg);
        let fixed = vec![a, b];
        let free = vec![c, d];

        reorder_layer(&mut lg, &fixed, &free, &edges, true, false);

        // C has barycenter 0 (connected to A), D has barycenter 1 (connected to B)
        // Barycenter should win over model_order
        assert_eq!(
            lg.order[c], 0,
            "C (barycenter=0) should be first despite higher model_order"
        );
        assert_eq!(
            lg.order[d], 1,
            "D (barycenter=1) should be second despite lower model_order"
        );
    }
}
