//! Packet diagram shim.
//!
//! Packet diagrams display network packet layouts.
//! Currently renders as a simple text table.

use crate::config::RenderConfig;
use crate::errors::RenderError;
use crate::family::DiagramFamily;
use crate::format::OutputFormat;
use crate::frontends::mermaid::packet::Packet;
use crate::prepared::{PreparedDiagram, PreparedPacket};
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance, ParsedDiagram};

pub const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Text, OutputFormat::Ascii];

/// Detect if input is a packet diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Accepts both `packet` and `packet-beta`
pub fn detect(input: &str) -> bool {
    crate::frontends::mermaid::detect_diagram_type(input)
        == Some(crate::frontends::mermaid::DiagramType::Packet)
}

/// Packet diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "packet",
        family: DiagramFamily::Table,
        detector: detect as DiagramDetector,
        factory: || Box::new(PacketInstance::new()),
        supported_formats: SUPPORTED_FORMATS,
    }
}

/// Packet diagram instance.
#[derive(Default)]
pub struct PacketInstance;

impl PacketInstance {
    /// Create a new packet diagram instance.
    pub fn new() -> Self {
        Self
    }
}

impl DiagramInstance for PacketInstance {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(ParsedPacket {
            packet: crate::frontends::mermaid::parse_packet(input)?,
            source: input.to_string(),
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}

struct ParsedPacket {
    packet: Packet,
    source: String,
}

impl ParsedDiagram for ParsedPacket {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Packet(PreparedPacket {
            packet: self.packet,
            source: self.source,
        }))
    }
}
