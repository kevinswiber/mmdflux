//! Runtime rendering from diagram payloads.

use super::graph_family;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::payload::Diagram;
use crate::render::diagram::sequence;
use crate::render::text::CharSet;
use crate::timeline::sequence::layout;

pub(in crate::runtime) fn render_payload(
    payload: Diagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
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
