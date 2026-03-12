//! Prepared-diagram dispatch owned by runtime.

use super::graph_dispatch;
use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::prepared::PreparedDiagram;
use crate::render::diagram::{info, packet, pie, sequence};
use crate::render::text::CharSet;
use crate::timeline::sequence::layout;

pub(in crate::runtime) fn dispatch_prepared(
    prepared: PreparedDiagram<'_>,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    match prepared {
        PreparedDiagram::Text(text) => Ok(text.into_owned()),
        PreparedDiagram::Graph(graph) => {
            graph_dispatch::render_graph(graph.diagram_type, graph.diagram.as_ref(), format, config)
        }
        PreparedDiagram::Timeline(timeline) => {
            let seq_layout = layout::layout(timeline.model);
            let charset = match format {
                OutputFormat::Ascii => CharSet::ascii(),
                _ => CharSet::unicode(),
            };
            Ok(sequence::render(&seq_layout, &charset))
        }
        PreparedDiagram::Info => Ok(info::render()),
        PreparedDiagram::Pie(prepared_pie) => Ok(pie::render(prepared_pie.source)),
        PreparedDiagram::Packet(prepared_packet) => Ok(packet::render(prepared_packet.source)),
    }
}
