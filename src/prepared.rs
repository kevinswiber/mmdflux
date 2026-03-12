//! Prepared-diagram contract returned by [`crate::registry::DiagramInstance::prepare`].
//!
//! Diagram modules stop at parsing, compilation, and config-sensitive
//! preparation. Runtime owns the final dispatch from these payloads to
//! family-specific renderers.

use std::borrow::Cow;

pub use crate::frontends::mermaid::info::Info;
pub use crate::frontends::mermaid::packet::Packet;
pub use crate::frontends::mermaid::pie::Pie;
use crate::graph::Diagram;
pub use crate::timeline::sequence::model::SequenceModel;

/// Prepared payload for graph-family diagrams.
#[derive(Debug, Clone)]
pub struct PreparedGraph<'a> {
    pub diagram_type: &'static str,
    pub diagram: Cow<'a, Diagram>,
}

/// Prepared payload for timeline-family diagrams.
#[derive(Debug, Clone, Copy)]
pub struct PreparedTimeline<'a> {
    pub model: &'a SequenceModel,
}

/// Prepared payload for pie diagrams.
#[derive(Debug, Clone, Copy)]
pub struct PreparedPie<'a> {
    pub pie: &'a Pie,
    pub source: &'a str,
}

/// Prepared payload for packet diagrams.
#[derive(Debug, Clone, Copy)]
pub struct PreparedPacket<'a> {
    pub packet: &'a Packet,
    pub source: &'a str,
}

/// Prepared diagram payload returned by the registry contract.
#[derive(Debug, Clone)]
pub enum PreparedDiagram<'a> {
    /// Pre-rendered text payload used by tests and fallback adapters.
    Text(Cow<'a, str>),
    /// Graph-family payload for flowchart and class diagrams.
    Graph(PreparedGraph<'a>),
    /// Timeline-family payload for sequence diagrams.
    Timeline(PreparedTimeline<'a>),
    /// Info diagrams render through their runtime-owned family renderer.
    Info,
    /// Pie diagrams carry parsed state plus source for current text rendering.
    Pie(PreparedPie<'a>),
    /// Packet diagrams carry parsed state plus source for current text rendering.
    Packet(PreparedPacket<'a>),
}
