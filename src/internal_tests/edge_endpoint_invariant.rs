//! Corpus-wide invariant: every compiled flowchart edge's `from`/`to` resolves
//! to an entry in `Graph.nodes` and never to a `Graph.subgraphs` entry.
//!
//! `resolve_subgraph_edges` already satisfies this structurally via
//! `find_subgraph_sink` and `find_non_cluster_child`; this fixture-walk pins
//! the property so a future regression to either helper fails loudly without
//! requiring a render-layer snapshot diff.
//!
//! The walker recurses into subdirectories so nested fixtures (e.g.
//! `dynamic/*.mmd`) are covered, not just the top-level fixture directory.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagrams::flowchart::compile_to_graph;
use crate::mermaid::parse_flowchart;

fn walk_flowchart_fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut out = Vec::new();
    collect_mmd_recursive(&dir, &mut out);
    out.sort();
    out
}

fn collect_mmd_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("failed to read entry in {}: {e}", dir.display()));
        let p = entry.path();
        if p.is_dir() {
            collect_mmd_recursive(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("mmd") {
            out.push(p);
        }
    }
}

#[test]
fn final_edges_never_reference_a_subgraph_id_as_endpoint() {
    let mut failures: Vec<String> = Vec::new();
    let paths = walk_flowchart_fixtures();
    let canary: PathBuf = ["dynamic", "multi_font_styles.mmd"].iter().collect();
    assert!(
        paths.iter().any(|p| p.ends_with(&canary)),
        "walker must recurse into subdirectories (e.g. flowchart/{})",
        canary.display(),
    );

    for path in paths {
        let input = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let flowchart = parse_flowchart(&input).unwrap_or_else(|err| {
            panic!(
                "fixture {} must parse permissively (the runtime uses permissive parsing); got {err}",
                path.display(),
            )
        });
        let graph = compile_to_graph(&flowchart);

        for (i, edge) in graph.edges.iter().enumerate() {
            if !graph.nodes.contains_key(&edge.from) {
                failures.push(format!(
                    "{}: edge[{i}].from = {:?} is not in Graph.nodes (subgraph id leak?)",
                    path.display(),
                    edge.from,
                ));
            }
            if !graph.nodes.contains_key(&edge.to) {
                failures.push(format!(
                    "{}: edge[{i}].to = {:?} is not in Graph.nodes (subgraph id leak?)",
                    path.display(),
                    edge.to,
                ));
            }
            if graph.subgraphs.contains_key(&edge.from) {
                failures.push(format!(
                    "{}: edge[{i}].from = {:?} matches a subgraph id; resolve_subgraph_edges should have rewritten it",
                    path.display(),
                    edge.from,
                ));
            }
            if graph.subgraphs.contains_key(&edge.to) {
                failures.push(format!(
                    "{}: edge[{i}].to = {:?} matches a subgraph id; resolve_subgraph_edges should have rewritten it",
                    path.display(),
                    edge.to,
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "edge-endpoint invariant violated:\n{}",
        failures.join("\n"),
    );
}
