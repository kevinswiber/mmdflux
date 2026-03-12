//! Prepared-diagram contract returned by [`crate::registry::DiagramInstance::prepare`].
//!
//! Diagram modules stop at parsing, compilation, and config-sensitive
//! preparation. Runtime owns the final dispatch from these payloads to
//! family-specific renderers.

pub use crate::frontends::mermaid::info::Info;
pub use crate::frontends::mermaid::packet::Packet;
pub use crate::frontends::mermaid::pie::Pie;
use crate::graph::Diagram;
pub use crate::timeline::sequence::model::SequenceModel;

/// Prepared payload for graph-family diagrams.
#[derive(Debug, Clone)]
pub struct PreparedGraph {
    pub diagram_type: &'static str,
    pub diagram: Diagram,
}

/// Prepared payload for timeline-family diagrams.
#[derive(Debug, Clone)]
pub struct PreparedTimeline {
    pub model: SequenceModel,
}

/// Prepared payload for pie diagrams.
#[derive(Debug, Clone)]
pub struct PreparedPie {
    pub pie: Pie,
    pub source: String,
}

/// Prepared payload for packet diagrams.
#[derive(Debug, Clone)]
pub struct PreparedPacket {
    pub packet: Packet,
    pub source: String,
}

/// Prepared diagram payload returned by the registry contract.
#[derive(Debug, Clone)]
pub enum PreparedDiagram {
    /// Pre-rendered text payload used by tests and fallback adapters.
    Text(String),
    /// Graph-family payload for flowchart and class diagrams.
    Graph(PreparedGraph),
    /// Timeline-family payload for sequence diagrams.
    Timeline(PreparedTimeline),
    /// Info diagrams render through their runtime-owned family renderer.
    Info,
    /// Pie diagrams carry parsed state plus source for current text rendering.
    Pie(PreparedPie),
    /// Packet diagrams carry parsed state plus source for current text rendering.
    Packet(PreparedPacket),
}
