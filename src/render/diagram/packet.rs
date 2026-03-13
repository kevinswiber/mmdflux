//! Packet-diagram renderer.

use crate::mermaid::packet::{Packet, PacketBlock};

pub fn render(packet: &Packet) -> String {
    let mut lines = vec![
        String::from("[Packet Diagram]"),
        String::from("packet-beta"),
    ];

    if let Some(title) = &packet.title {
        lines.push(format!("title {title}"));
    }

    for block in &packet.blocks {
        let line = match block {
            PacketBlock::Range { start, end, label } => match end {
                Some(end) => format!("{start}-{end}: \"{label}\""),
                None => format!("{start}: \"{label}\""),
            },
            PacketBlock::Relative { bits, label } => format!("+{bits}: \"{label}\""),
        };
        lines.push(line);
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::mermaid::packet::{Packet, PacketBlock};

    #[test]
    fn render_uses_typed_packet_blocks() {
        let packet = Packet {
            title: Some("IPv4".to_string()),
            blocks: vec![
                PacketBlock::Range {
                    start: 0,
                    end: Some(7),
                    label: "Version".to_string(),
                },
                PacketBlock::Relative {
                    bits: 8,
                    label: "Payload".to_string(),
                },
            ],
        };

        let output = super::render(&packet);
        assert!(output.contains("packet-beta"));
        assert!(output.contains("title IPv4"));
        assert!(output.contains("0-7: \"Version\""));
        assert!(output.contains("+8: \"Payload\""));
    }
}
