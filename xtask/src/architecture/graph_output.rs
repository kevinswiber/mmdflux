use std::fmt::Write;

use crate::architecture::boundaries::BoundaryGraph;

/// Render the boundary graph as a Mermaid flowchart.
pub(crate) fn render_mermaid(graph: &BoundaryGraph) -> String {
    let mut out = String::new();
    writeln!(out, "graph LR").unwrap();

    // Emit nodes (boundaries)
    for boundary in &graph.boundaries {
        writeln!(out, "    {boundary}[\"{boundary}\"]").unwrap();
    }

    // Emit edges
    for (source, target) in graph.edges.keys() {
        writeln!(out, "    {source} --> {target}").unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::architecture::boundaries::{DependencySample, EdgeProvenance};

    fn sample_graph() -> BoundaryGraph {
        let boundaries = BTreeSet::from([
            "errors".to_string(),
            "format".to_string(),
            "graph".to_string(),
        ]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            "graph".to_string(),
            "errors".to_string(),
            DependencySample {
                source: "crate::graph".to_string(),
                symbol: "crate::errors::E".to_string(),
                target: "crate::errors".to_string(),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        graph.insert_edge(
            "graph".to_string(),
            "format".to_string(),
            DependencySample {
                source: "crate::graph".to_string(),
                symbol: "crate::format::F".to_string(),
                target: "crate::format".to_string(),
                location: None,
            },
            EdgeProvenance::QualifiedPath,
        );
        graph
    }

    #[test]
    fn mermaid_output_contains_graph_header() {
        let output = render_mermaid(&sample_graph());
        assert!(output.starts_with("graph LR"), "got:\n{output}");
    }

    #[test]
    fn mermaid_output_contains_all_boundaries_as_nodes() {
        let output = render_mermaid(&sample_graph());
        assert!(output.contains("errors[\"errors\"]"), "got:\n{output}");
        assert!(output.contains("format[\"format\"]"), "got:\n{output}");
        assert!(output.contains("graph[\"graph\"]"), "got:\n{output}");
    }

    #[test]
    fn mermaid_output_contains_edges() {
        let output = render_mermaid(&sample_graph());
        assert!(output.contains("graph --> errors"), "got:\n{output}");
        assert!(output.contains("graph --> format"), "got:\n{output}");
    }

    #[test]
    fn mermaid_output_for_empty_graph() {
        let graph = BoundaryGraph::new(BTreeSet::from(["a".to_string()]));
        let output = render_mermaid(&graph);
        assert!(output.contains("graph LR"));
        assert!(output.contains("a[\"a\"]"));
        assert!(!output.contains("-->"));
    }
}
