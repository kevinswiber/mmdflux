//! Runtime rendering from prepared diagram payloads.

use super::graph_family;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::prepared::PreparedDiagram;
use crate::render::diagram::{info, packet, pie, sequence};
use crate::render::text::CharSet;
use crate::timeline::sequence::layout;

pub(in crate::runtime) fn render_prepared(
    prepared: PreparedDiagram,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    match prepared {
        PreparedDiagram::Text(text) => Ok(text),
        PreparedDiagram::Graph(graph) => {
            graph_family::render_graph_family(graph.diagram_type, &graph.diagram, format, config)
        }
        PreparedDiagram::Timeline(timeline) => {
            let seq_layout = layout::layout(&timeline.model);
            let charset = match format {
                OutputFormat::Ascii => CharSet::ascii(),
                _ => CharSet::unicode(),
            };
            Ok(sequence::render(&seq_layout, &charset))
        }
        PreparedDiagram::Info => Ok(info::render()),
        PreparedDiagram::Pie(prepared_pie) => Ok(pie::render(&prepared_pie.source)),
        PreparedDiagram::Packet(prepared_packet) => Ok(packet::render(&prepared_packet.source)),
    }
}
