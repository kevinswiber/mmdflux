//! Assign parents to dummy chains so they track compound hierarchy.
//!
//! Mirrors dagre's parent-dummy-chains.js. This ensures dummy nodes created
//! during normalization are associated with the correct compound ancestors,
//! which affects ordering and border placement.

use super::graph::LayoutGraph;

#[derive(Clone, Copy, Debug)]
struct PostorderRange {
    low: i32,
    lim: i32,
}

pub(crate) fn run(graph: &mut LayoutGraph) {
    if graph.dummy_chains.is_empty() {
        return;
    }

    let debug = std::env::var("MMDFLUX_DEBUG_DUMMY_PARENTS").is_ok_and(|v| v == "1");
    let postorder = compute_postorder(graph);

    for chain in &graph.dummy_chains {
        let Some((src, tgt)) = find_original_edge_endpoints(graph, chain.edge_index) else {
            continue;
        };

        let (path, lca) = find_path(graph, &postorder, src, tgt);
        if path.is_empty() {
            continue;
        }

        if debug {
            let src_id = &graph.node_ids[src].0;
            let tgt_id = &graph.node_ids[tgt].0;
            let lca_label = lca
                .map(|l| graph.node_ids[l].0.clone())
                .unwrap_or_else(|| "None".to_string());
            let path_ids: Vec<String> = path
                .iter()
                .map(|p| {
                    p.map(|idx| graph.node_ids[idx].0.clone())
                        .unwrap_or_else(|| "None".to_string())
                })
                .collect();
            eprintln!(
                "[dummy_parents] edge {} src={} tgt={} lca={} path={:?}",
                chain.edge_index, src_id, tgt_id, lca_label, path_ids
            );
        }

        let mut path_idx = 0usize;
        let mut path_v = path[path_idx];
        let mut ascending = true;

        for dummy_id in &chain.dummy_ids {
            let Some(&dummy_idx) = graph.node_index.get(dummy_id) else {
                continue;
            };
            let dummy_rank = graph.ranks[dummy_idx];

            if ascending {
                // Advance through ascending (source-side) path entries.
                // path_v is Option<usize>: Some(node) for compound ancestors,
                // None for the root sentinel. Mirrors dagre's while loop where
                // pathV !== lca is checked first (short-circuits when both are undefined).
                while path_v != lca && path_v.is_some_and(|pv| max_rank(graph, pv) < dummy_rank) {
                    path_idx += 1;
                    if path_idx >= path.len() {
                        break;
                    }
                    path_v = path[path_idx];
                }
                if path_v == lca {
                    ascending = false;
                }
            }

            if !ascending {
                while path_idx + 1 < path.len()
                    && path[path_idx + 1].is_some_and(|pv| min_rank(graph, pv) <= dummy_rank)
                {
                    path_idx += 1;
                }
                path_v = path[path_idx];
            }

            graph.parents[dummy_idx] = path_v;
            if debug {
                let dummy_name = &graph.node_ids[dummy_idx].0;
                let parent_label = path_v
                    .map(|pv| graph.node_ids[pv].0.clone())
                    .unwrap_or_else(|| "None".to_string());
                eprintln!(
                    "[dummy_parents]   dummy {} rank {} -> {}",
                    dummy_name, dummy_rank, parent_label
                );
            }
        }
    }
}

fn compute_postorder(graph: &LayoutGraph) -> Vec<PostorderRange> {
    let n = graph.node_ids.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (child, parent) in graph.parents.iter().enumerate() {
        if let Some(p) = parent {
            children[*p].push(child);
        }
    }
    for kids in &mut children {
        kids.sort();
    }

    let mut result = vec![PostorderRange { low: 0, lim: 0 }; n];
    let mut lim: i32 = 0;

    fn dfs(v: usize, children: &[Vec<usize>], result: &mut [PostorderRange], lim: &mut i32) {
        let low = *lim;
        for &child in &children[v] {
            dfs(child, children, result, lim);
        }
        result[v] = PostorderRange { low, lim: *lim };
        *lim += 1;
    }

    for v in 0..n {
        if graph.parents[v].is_none() {
            dfs(v, &children, &mut result, &mut lim);
        }
    }

    result
}

/// Find the path from v to w through their lowest common ancestor (LCA).
///
/// Returns `(path, lca)` where:
/// - `path` is a `Vec<Option<usize>>`: `Some(node)` for compound ancestors,
///   `None` for the root sentinel (matching dagre's `undefined` in the path array).
/// - `lca` is `Some(node)` if v and w share a compound ancestor, or `None` if
///   the LCA is the implicit root (no shared compound parent).
///
/// Mirrors dagre's `findPath()` exactly: the do-while traverses up from v,
/// pushing each parent (including `undefined`/`None`) until the LCA is found.
/// The w-side path is then appended in reverse.
fn find_path(
    graph: &LayoutGraph,
    postorder: &[PostorderRange],
    v: usize,
    w: usize,
) -> (Vec<Option<usize>>, Option<usize>) {
    let low = postorder[v].low.min(postorder[w].low);
    let lim = postorder[v].lim.max(postorder[w].lim);

    // Traverse up from v to find the LCA.
    // Mirrors dagre's do-while: push parent first, then check.
    // g.parent(v) can be undefined (None), which is a valid LCA.
    let mut v_path: Vec<Option<usize>> = Vec::new();
    let mut parent_opt = graph.parents[v];
    let lca: Option<usize> = loop {
        v_path.push(parent_opt);
        match parent_opt {
            Some(parent) => {
                let range = &postorder[parent];
                if !(range.low > low || lim > range.lim) {
                    break Some(parent);
                }
                parent_opt = graph.parents[parent];
            }
            None => {
                // Reached root — LCA is the implicit root (None).
                break None;
            }
        }
    };

    // Traverse from w up to the LCA.
    let mut w_path: Vec<Option<usize>> = Vec::new();
    let mut cur = w;
    loop {
        let p = graph.parents[cur];
        if p == lca {
            break;
        }
        match p {
            Some(parent) => {
                w_path.push(Some(parent));
                cur = parent;
            }
            None => {
                // Reached root; lca must also be None, so we're done.
                break;
            }
        }
    }

    w_path.reverse();
    v_path.extend(w_path);
    (v_path, lca)
}

fn min_rank(graph: &LayoutGraph, node: usize) -> i32 {
    graph
        .min_rank
        .get(&node)
        .copied()
        .unwrap_or(graph.ranks[node])
}

fn max_rank(graph: &LayoutGraph, node: usize) -> i32 {
    graph
        .max_rank
        .get(&node)
        .copied()
        .unwrap_or(graph.ranks[node])
}

fn find_original_edge_endpoints(graph: &LayoutGraph, orig_idx: usize) -> Option<(usize, usize)> {
    graph.original_edge_endpoints.get(orig_idx).copied()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::layered::graph::DiGraph;
    use crate::layered::{LayoutConfig, normalize, rank};

    /// Build a LayoutGraph matching the external_node_subgraph fixture and run
    /// the pipeline up through parent_dummy_chains.
    fn build_external_node_subgraph_after_parent_dummy_chains() -> LayoutGraph {
        // external_node_subgraph.mmd:
        //   graph TD
        //     subgraph Cloud
        //       subgraph us-east [US East Region]
        //         A[Web Server] --> B[App Server]
        //       end
        //       subgraph us-west [US West Region]
        //         C[Web Server] --> D[App Server]
        //       end
        //     end
        //     E[Load Balancer] --> A
        //     E --> C
        //
        // Node dimensions: "Web Server" = 14, "App Server" = 14, "Load Balancer" = 17
        // node_dimensions = label.len() + 4 => Web Server=14, App Server=14, Load Balancer=17
        // height = 3 for all
        let mut g: DiGraph<(usize, usize)> = DiGraph::new();

        // Add nodes in edge-first order (matching render/layout.rs behavior)
        g.add_node("A", (14, 3)); // Web Server
        g.add_node("B", (14, 3)); // App Server
        g.add_node("C", (14, 3)); // Web Server
        g.add_node("D", (14, 3)); // App Server
        g.add_node("E", (17, 3)); // Load Balancer (13 chars + 4 = 17)

        // Subgraph compound nodes (zero dimensions, sorted alphabetically like layout.rs)
        g.add_node("Cloud", (0, 0));
        g.add_node("us-east", (0, 0));
        g.add_node("us-west", (0, 0));

        // Titles
        g.set_has_title("Cloud");
        g.set_has_title("us-east");
        g.set_has_title("us-west");

        // Parent relationships for nodes
        g.set_parent("A", "us-east");
        g.set_parent("B", "us-east");
        g.set_parent("C", "us-west");
        g.set_parent("D", "us-west");

        // Parent relationships for nested subgraphs
        g.set_parent("us-east", "Cloud");
        g.set_parent("us-west", "Cloud");

        // Edges: A→B (0), C→D (1), E→A (2), E→C (3)
        g.add_edge("A", "B");
        g.add_edge("C", "D");
        g.add_edge("E", "A");
        g.add_edge("E", "C");

        let mut lg = LayoutGraph::from_digraph(&g, |_, dims| (dims.0 as f64, dims.1 as f64));

        // Run pipeline up through parent_dummy_chains (matching layout_with_labels)
        crate::layered::extract_self_edges(&mut lg);
        crate::layered::acyclic::run(&mut lg);
        crate::layered::make_space_for_edge_labels(&mut lg);
        crate::layered::nesting::run(&mut lg);
        rank::run(&mut lg, &LayoutConfig::default());
        rank::remove_empty_ranks(&mut lg);
        crate::layered::nesting::cleanup(&mut lg);
        rank::normalize(&mut lg);
        crate::layered::nesting::insert_title_nodes(&mut lg);
        rank::normalize(&mut lg);
        crate::layered::nesting::assign_rank_minmax(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        run(&mut lg);

        lg
    }

    #[test]
    fn assigns_parents_for_external_chains() {
        let lg = build_external_node_subgraph_after_parent_dummy_chains();

        // Collect dummy parents by (edge_index, rank) -> parent name
        let mut parents: HashMap<(usize, i32), Option<String>> = HashMap::new();
        for chain in &lg.dummy_chains {
            for dummy_id in &chain.dummy_ids {
                let &dummy_idx = lg.node_index.get(dummy_id).unwrap();
                let rank = lg.ranks[dummy_idx];
                let parent_name = lg.parents[dummy_idx].map(|p| lg.node_ids[p].0.clone());
                parents.insert((chain.edge_index, rank), parent_name);
            }
        }

        // Edge 2: E→A — dummies should get: rank 1 = None, rank 2 = Cloud, rank 3 = us-east
        // (rank numbers may differ from research due to title nodes and normalization,
        // so we check by collecting all dummies for this edge and verifying the parent sequence)
        let mut e_to_a_parents: Vec<(i32, Option<String>)> = parents
            .iter()
            .filter(|((edge, _), _)| *edge == 2)
            .map(|((_, rank), parent)| (*rank, parent.clone()))
            .collect();
        e_to_a_parents.sort_by_key(|(r, _)| *r);

        // The key assertion: at least some dummies should have compound parents (Cloud, us-east)
        let has_cloud_parent = e_to_a_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("Cloud"));
        let has_us_east_parent = e_to_a_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("us-east"));
        assert!(
            has_cloud_parent,
            "E→A chain should have a dummy parented to Cloud, got: {:?}",
            e_to_a_parents
        );
        assert!(
            has_us_east_parent,
            "E→A chain should have a dummy parented to us-east, got: {:?}",
            e_to_a_parents
        );

        // Edge 3: E→C — dummies should get: rank 1 = None, rank 2 = Cloud, rank 3 = us-west
        let mut e_to_c_parents: Vec<(i32, Option<String>)> = parents
            .iter()
            .filter(|((edge, _), _)| *edge == 3)
            .map(|((_, rank), parent)| (*rank, parent.clone()))
            .collect();
        e_to_c_parents.sort_by_key(|(r, _)| *r);

        let has_cloud_parent_c = e_to_c_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("Cloud"));
        let has_us_west_parent = e_to_c_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("us-west"));
        assert!(
            has_cloud_parent_c,
            "E→C chain should have a dummy parented to Cloud, got: {:?}",
            e_to_c_parents
        );
        assert!(
            has_us_west_parent,
            "E→C chain should have a dummy parented to us-west, got: {:?}",
            e_to_c_parents
        );
    }

    /// Build a LayoutGraph matching the multi_subgraph fixture and run
    /// the pipeline up through parent_dummy_chains.
    fn build_multi_subgraph_after_parent_dummy_chains() -> LayoutGraph {
        // multi_subgraph.mmd:
        //   graph LR
        //   subgraph sg1[Frontend]
        //   A[UI] --> B[API]
        //   end
        //   subgraph sg2[Backend]
        //   C[Server] --> D[DB]
        //   end
        //   B --> C
        let mut g: DiGraph<(usize, usize)> = DiGraph::new();

        // Nodes in edge-first order
        g.add_node("A", (6, 3)); // UI
        g.add_node("B", (7, 3)); // API
        g.add_node("C", (10, 3)); // Server
        g.add_node("D", (6, 3)); // DB

        // Subgraph compound nodes
        g.add_node("sg1", (0, 0));
        g.add_node("sg2", (0, 0));

        // Titles
        g.set_has_title("sg1");
        g.set_has_title("sg2");

        // Parent relationships
        g.set_parent("A", "sg1");
        g.set_parent("B", "sg1");
        g.set_parent("C", "sg2");
        g.set_parent("D", "sg2");

        // Edges: A→B (0), C→D (1), B→C (2)
        g.add_edge("A", "B");
        g.add_edge("C", "D");
        g.add_edge("B", "C");

        let config = LayoutConfig {
            direction: crate::layered::Direction::LeftRight,
            ..Default::default()
        };

        let mut lg = LayoutGraph::from_digraph(&g, |_, dims| (dims.0 as f64, dims.1 as f64));

        crate::layered::extract_self_edges(&mut lg);
        crate::layered::acyclic::run(&mut lg);
        crate::layered::make_space_for_edge_labels(&mut lg);
        crate::layered::nesting::run(&mut lg);
        rank::run(&mut lg, &config);
        rank::remove_empty_ranks(&mut lg);
        crate::layered::nesting::cleanup(&mut lg);
        rank::normalize(&mut lg);
        crate::layered::nesting::insert_title_nodes(&mut lg);
        rank::normalize(&mut lg);
        crate::layered::nesting::assign_rank_minmax(&mut lg);
        normalize::run(&mut lg, &HashMap::new(), false);
        run(&mut lg);

        lg
    }

    #[test]
    fn assigns_parents_for_cross_subgraph_chains() {
        let lg = build_multi_subgraph_after_parent_dummy_chains();

        // Edge 2: B→C crosses from sg1 to sg2 (top-level sibling subgraphs).
        // Dagre's path for this case is [sg1, undefined, sg2].
        // Source-side dummies within sg1's rank span should be parented to sg1.
        // Target-side dummies within sg2's rank span should be parented to sg2.
        let mut b_to_c_parents: Vec<(i32, Option<String>)> = Vec::new();
        for chain in &lg.dummy_chains {
            if chain.edge_index != 2 {
                continue;
            }
            for dummy_id in &chain.dummy_ids {
                let &dummy_idx = lg.node_index.get(dummy_id).unwrap();
                let rank = lg.ranks[dummy_idx];
                let parent_name = lg.parents[dummy_idx].map(|p| lg.node_ids[p].0.clone());
                b_to_c_parents.push((rank, parent_name));
            }
        }
        b_to_c_parents.sort_by_key(|(r, _)| *r);

        // There should be dummies for this edge (B→C spans multiple ranks after make_space)
        assert!(
            !b_to_c_parents.is_empty(),
            "B→C edge should have dummy nodes"
        );

        // At least one dummy should be parented to sg1 (source side) or sg2 (target side).
        // With the v_path bug, ALL dummies only get sg2 or None — sg1 never appears.
        let has_sg1 = b_to_c_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("sg1"));
        let has_sg2 = b_to_c_parents
            .iter()
            .any(|(_, p)| p.as_deref() == Some("sg2"));
        assert!(
            has_sg1 || has_sg2,
            "B→C dummies should be parented to sg1 or sg2, got: {:?}",
            b_to_c_parents
        );

        // The specific dagre parity check: source-side dummies should see sg1
        assert!(
            has_sg1,
            "B→C chain should have a dummy parented to sg1 (source subgraph), got: {:?}",
            b_to_c_parents
        );
    }
}
