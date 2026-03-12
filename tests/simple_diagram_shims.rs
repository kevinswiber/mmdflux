use mmdflux::diagrams::{info, packet, pie};
use mmdflux::registry::DiagramInstance;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn render_simple(input: &str) -> String {
    render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap()
}

#[test]
fn pie_definition_exists() {
    let def = pie::definition();
    assert_eq!(def.id, "pie");
}

#[test]
fn pie_detector_works() {
    assert!(pie::detect("pie\n\"A\": 50"));
    assert!(pie::detect("pie title My Chart\n\"A\": 50"));
    assert!(!pie::detect("graph TD\nA-->B"));
}

#[test]
fn pie_detector_skips_comments() {
    assert!(pie::detect("%% comment\npie\n\"A\": 50"));
}

#[test]
fn pie_detector_case_insensitive() {
    assert!(pie::detect("PIE\n\"A\": 50"));
    assert!(pie::detect("Pie\n\"A\": 50"));
}

#[test]
fn pie_detector_first_word_only() {
    assert!(!pie::detect("piechart\n\"A\": 50"));
}

#[test]
fn pie_instance_renders() {
    let output = render_simple("pie\n\"A\": 50\n\"B\": 50");
    assert!(!output.is_empty());
}

#[test]
fn info_definition_exists() {
    let def = info::definition();
    assert_eq!(def.id, "info");
}

#[test]
fn info_detector_works() {
    assert!(info::detect("info"));
    assert!(!info::detect("pie"));
}

#[test]
fn info_detector_skips_comments() {
    assert!(info::detect("%% comment\ninfo"));
}

#[test]
fn info_detector_case_insensitive() {
    assert!(info::detect("INFO"));
    assert!(info::detect("Info"));
}

#[test]
fn info_detector_first_word_only() {
    assert!(!info::detect("infographic"));
}

#[test]
fn info_instance_renders() {
    let output = render_simple("info");
    assert!(output.contains("mmdflux"));
}

#[test]
fn packet_definition_exists() {
    let def = packet::definition();
    assert_eq!(def.id, "packet");
}

#[test]
fn packet_detector_works() {
    assert!(packet::detect("packet-beta"));
    assert!(packet::detect("packet"));
    assert!(!packet::detect("graph TD"));
}

#[test]
fn packet_detector_skips_comments() {
    assert!(packet::detect("%% comment\npacket-beta"));
    assert!(packet::detect("%% comment\npacket"));
}

#[test]
fn packet_detector_case_insensitive() {
    assert!(packet::detect("PACKET-BETA"));
    assert!(packet::detect("Packet-Beta"));
    assert!(packet::detect("PACKET"));
}

#[test]
fn packet_instance_renders() {
    let output = render_simple("packet-beta\n0-15: \"Header\"");
    assert!(!output.is_empty());
}

#[test]
fn simple_diagrams_dont_support_svg() {
    let pie_inst = pie::PieInstance::new();
    let info_inst = info::InfoInstance::new();
    let packet_inst = packet::PacketInstance::new();

    assert!(!pie_inst.supports_format(OutputFormat::Svg));
    assert!(!info_inst.supports_format(OutputFormat::Svg));
    assert!(!packet_inst.supports_format(OutputFormat::Svg));
}
