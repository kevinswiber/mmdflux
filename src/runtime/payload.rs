//! Runtime rendering from diagram payloads.

use super::graph_family;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::payload::Diagram;
use crate::render::diagram::sequence;
use crate::render::text::CharSet;
use crate::runtime::config::RenderConfig;
use crate::timeline::sequence::layout;

pub(in crate::runtime) fn render_payload(
    payload: Diagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    // Apply show_ids annotation to graph-family payloads before rendering.
    // This is a presentation concern owned by runtime, not diagrams.
    let payload = if config.show_ids {
        annotate_graph_payload_ids(payload)
    } else {
        payload
    };

    match payload {
        Diagram::Flowchart(graph) => {
            graph_family::render_graph_family("flowchart", &graph, format, config)
        }
        Diagram::Class(graph) => graph_family::render_graph_family("class", &graph, format, config),
        Diagram::Sequence(model) => {
            let seq_layout = layout::layout(&model);
            let charset = match format {
                OutputFormat::Ascii => CharSet::ascii(),
                _ => CharSet::unicode(),
            };
            Ok(sequence::render(&seq_layout, &charset))
        }
    }
}

/// Annotate node labels as "ID: Label" for graph-family payloads.
/// Skips bare nodes where label == id (no useful annotation).
fn annotate_graph_payload_ids(payload: Diagram) -> Diagram {
    match payload {
        Diagram::Flowchart(mut graph) => {
            annotate_node_ids(&mut graph);
            Diagram::Flowchart(graph)
        }
        Diagram::Class(mut graph) => {
            annotate_node_ids(&mut graph);
            Diagram::Class(graph)
        }
        other => other,
    }
}

fn annotate_node_ids(graph: &mut crate::graph::Graph) {
    for node in graph.nodes.values_mut() {
        if node.label != node.id {
            node.label = format!("{}: {}", node.id, node.label);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::format::OutputFormat;
    use crate::runtime::config::RenderConfig;

    #[test]
    fn render_payload_annotates_node_ids_when_show_ids_is_set() {
        let config = RenderConfig {
            show_ids: true,
            ..RenderConfig::default()
        };
        let output = crate::runtime::render_diagram(
            "graph TD\nA[Start] --> B[End]",
            OutputFormat::Text,
            &config,
        )
        .unwrap();
        assert!(
            output.contains("A: Start"),
            "output should contain annotated ID: {output}"
        );
        assert!(
            output.contains("B: End"),
            "output should contain annotated ID: {output}"
        );
    }

    #[test]
    fn render_payload_skips_annotation_when_show_ids_is_false() {
        let config = RenderConfig::default();
        let output = crate::runtime::render_diagram(
            "graph TD\nA[Start] --> B[End]",
            OutputFormat::Text,
            &config,
        )
        .unwrap();
        assert!(
            !output.contains("A:"),
            "output should not have ID prefix: {output}"
        );
    }
}
