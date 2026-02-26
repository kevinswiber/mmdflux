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
fn sort_entries(entries: &[ResolvedEntry], bias_right: bool) -> SortResult {
    let (mut sortable, mut unsortable): (Vec<&ResolvedEntry>, Vec<&ResolvedEntry>) =
        entries.iter().partition(|e| e.barycenter.is_some());

    // Sort unsortable by descending i
    unsortable.sort_by(|a, b| b.i.cmp(&a.i));

    // Sort sortable by barycenter, bias-aware tie-break on i
    sortable.sort_by(|a, b| {
        a.barycenter
            .unwrap()
            .partial_cmp(&b.barycenter.unwrap())
            .unwrap_or(std::cmp::Ordering::Equal)
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
    let mut result = sort_entries(&resolved, bias_right);

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

fn effective_edges_weighted_filtered(graph: &LayoutGraph) -> Vec<(usize, usize, f64)> {
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

    // Build successor adjacency list
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(from, to, _) in &edges {
        successors[from].push(to);
    }

    // Get all nodes sorted by rank (ascending), matching Dagre's
    // `simpleNodes.sort((a, b) => g.node(a).rank - g.node(b).rank)`
    let mut start_nodes: Vec<usize> = layers.iter().flatten().copied().collect();
    start_nodes.sort_by_key(|&node| graph.ranks[node]);

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
pub fn run(graph: &mut LayoutGraph) {
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

    while last_best < 4 {
        let bias_right = (i % 4) >= 2;

        // Always use the compound ordering path (sort_subgraph + constraint
        // graph) even for flat graphs.  Dagre v0.8.5 always runs this pipeline
        // because Mermaid creates graphs with `compound: true`, and the
        // constraint graph propagates ordering decisions across layers within a
        // sweep, finding lower-crossing solutions that the simpler flat
        // reorder_layer path misses.
        let downward = !i.is_multiple_of(2); // odd = down, even = up
        sweep_compound(graph, &layers, &edges, bias_right, downward);

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

/// Reorder nodes in `free` layer based on barycenter of connections to `fixed` layer.
///
/// Uses dagre v0.8.5's partition-and-interleave algorithm: nodes with neighbors
/// in the fixed layer are "sortable" (sorted by barycenter), while nodes without
/// neighbors are "unsortable" (interleaved at their original positions).
#[cfg(test)]
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

    // Step 2: Sort sortable by barycenter with bias-aware tie-breaking
    sortable.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
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

/// Count total crossings between all adjacent layer pairs.
fn count_all_crossings(
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

        run(&mut lg);

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

        run(&mut lg);

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
        crate::layered::normalize::run(&mut lg, &std::collections::HashMap::new());

        run(&mut lg);

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

        // Left bias: B before C (smaller original_pos wins)
        assert!(
            left_order_b < left_order_c,
            "Left bias should put B before C"
        );
        // Right bias: C before B (larger original_pos wins)
        assert!(
            right_order_b > right_order_c,
            "Right bias should put C before B"
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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);
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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

        run(&mut lg);

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

    #[test]
    fn test_sort_entries_all_sortable() {
        let entries = vec![
            re(vec![10], 0, Some(2.0), Some(1.0)),
            re(vec![11], 1, Some(0.5), Some(1.0)),
            re(vec![12], 2, Some(1.5), Some(1.0)),
        ];
        let result = sort_entries(&entries, false);
        assert_eq!(result.vs, vec![11, 12, 10]);
    }

    #[test]
    fn test_sort_entries_interleave_unsortable() {
        // Sortable: A(bc=0.5, i=0), C(bc=1.5, i=2)
        // Unsortable: B(i=1)
        let entries = vec![
            re(vec![10], 0, Some(0.5), Some(1.0)),
            re(vec![11], 1, None, None),
            re(vec![12], 2, Some(1.5), Some(1.0)),
        ];
        let result = sort_entries(&entries, false);
        // A sorted first (bc=0.5), B interleaved at position 1, C at position 2
        assert_eq!(result.vs, vec![10, 11, 12]);
    }

    #[test]
    fn test_sort_entries_bias_right_tie_break() {
        let entries = vec![
            re(vec![10], 0, Some(1.0), Some(1.0)),
            re(vec![11], 1, Some(1.0), Some(1.0)),
        ];

        // bias_right=false: lower i first
        let result_left = sort_entries(&entries, false);
        assert_eq!(result_left.vs, vec![10, 11]);

        // bias_right=true: higher i first
        let result_right = sort_entries(&entries, true);
        assert_eq!(result_right.vs, vec![11, 10]);
    }

    #[test]
    fn test_sort_entries_aggregate_barycenter() {
        let entries = vec![
            re(vec![10], 0, Some(1.0), Some(2.0)),
            re(vec![11], 1, Some(3.0), Some(1.0)),
        ];
        let result = sort_entries(&entries, false);
        // sum = 1.0*2.0 + 3.0*1.0 = 5.0, weight = 3.0
        assert!((result.barycenter.unwrap() - 5.0 / 3.0).abs() < 1e-9);
        assert!((result.weight.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_sort_entries_multi_node_entries() {
        // Entry with vs=[A, B] from resolveConflicts merge at i=0
        // and a single entry C at i=2 (unsortable)
        let entries = vec![
            re(vec![10, 11], 0, Some(1.0), Some(1.0)),
            re(vec![12], 2, None, None),
        ];
        let result = sort_entries(&entries, false);
        // [10,11] placed first (sortable, vs_index advances by 2), then 12 at i=2
        assert_eq!(result.vs, vec![10, 11, 12]);
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
}
