//! Runtime rendering from diagram payloads.

use super::graph_family;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::measure;
use crate::payload::Diagram;
use crate::render::text::CharSet;
use crate::render::timeline;
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
        Diagram::Flowchart(mut graph) => {
            graph_family::render_graph_family("flowchart", &mut graph, format, config)
        }
        Diagram::Class(mut graph) => {
            graph_family::render_graph_family("class", &mut graph, format, config)
        }
        Diagram::State(mut graph) => {
            graph_family::render_graph_family("state", &mut graph, format, config)
        }
        Diagram::Sequence(model) => match format {
            OutputFormat::Svg => {
                let metrics = measure::default_proportional_text_metrics();
                let font_family = "\"trebuchet ms\", verdana, arial, sans-serif";
                let theme = super::resolve_configured_svg_theme(config)?;
                Ok(timeline::render_svg(
                    &model,
                    &metrics,
                    font_family,
                    theme.as_ref(),
                ))
            }
            OutputFormat::Mmds => {
                let metrics = measure::default_proportional_text_metrics();
                Ok(super::timeline_family::to_json(&model, &metrics))
            }
            _ => {
                let seq_layout = layout::layout(&model);
                let charset = match format {
                    OutputFormat::Ascii => CharSet::ascii(),
                    _ => CharSet::unicode(),
                };
                Ok(timeline::render(&seq_layout, &charset))
            }
        },
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
        Diagram::State(mut graph) => {
            annotate_node_ids(&mut graph);
            Diagram::State(graph)
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
