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
use crate::registry::{DiagramDefinition, DiagramDetector, DiagramInstance};

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
        factory: || Box::new(PacketInstance::default()),
        supported_formats: SUPPORTED_FORMATS,
    }
}

/// Packet diagram instance.
pub struct PacketInstance {
    input: Option<String>,
    packet: Option<Packet>,
}

impl PacketInstance {
    /// Create a new packet diagram instance.
    pub fn new() -> Self {
        Self {
            input: None,
            packet: None,
        }
    }
}

impl Default for PacketInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for PacketInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.packet = Some(crate::frontends::mermaid::parse_packet(input)?);
        self.input = Some(input.to_string());
        Ok(())
    }

    fn prepare(&self, _config: &RenderConfig) -> Result<PreparedDiagram<'_>, RenderError> {
        let input = self.input.as_ref().ok_or("Not parsed")?;
        let packet = self.packet.as_ref().ok_or("Not parsed")?;

        Ok(PreparedDiagram::Packet(PreparedPacket {
            packet,
            source: input,
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        SUPPORTED_FORMATS.contains(&format)
    }
}
