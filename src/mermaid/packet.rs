//! Pest parser for Mermaid packet diagrams.

use pest::Parser;
use pest_derive::Parser;

use super::error::ParseError;

#[derive(Parser)]
#[grammar = "mermaid/packet_grammar.pest"]
pub struct PacketParser;

/// A block in a packet diagram.
#[derive(Debug, Clone)]
pub enum PacketBlock {
    /// Absolute bit range: `start-end: "label"` or single bit `start: "label"`
    Range {
        start: u32,
        end: Option<u32>,
        label: String,
    },
    /// Relative bits: `+bits: "label"`
    Relative { bits: u32, label: String },
}

/// Parsed packet diagram.
#[derive(Debug, Clone)]
pub struct Packet {
    pub title: Option<String>,
    pub blocks: Vec<PacketBlock>,
}

/// Strip surrounding quotes (double or single) from a string.
fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parse a packet diagram string.
pub fn parse_packet(input: &str) -> Result<Packet, ParseError> {
    let pairs =
        PacketParser::parse(Rule::packet_diagram, input).map_err(ParseError::from_pest_error)?;

    let mut title = None;
    let mut blocks = Vec::new();

    for pair in pairs.filter(|p| p.as_rule() == Rule::packet_diagram) {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::title_stmt => {
                    title = inner
                        .into_inner()
                        .find(|t| t.as_rule() == Rule::title_text)
                        .map(|t| t.as_str().to_string());
                }
                Rule::packet_block => blocks.push(parse_block(inner)),
                _ => {}
            }
        }
    }

    Ok(Packet { title, blocks })
}

fn parse_block(pair: pest::iterators::Pair<Rule>) -> PacketBlock {
    let mut label = String::new();
    let mut block = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::bit_spec => {
                block = Some(parse_bit_spec(part));
            }
            Rule::string => {
                let raw = part.into_inner().next().unwrap().as_str();
                label = strip_quotes(raw).to_string();
            }
            _ => {}
        }
    }

    match block {
        Some(BitSpec::Range(start, end)) => PacketBlock::Range { start, end, label },
        Some(BitSpec::Relative(bits)) => PacketBlock::Relative { bits, label },
        None => PacketBlock::Range {
            start: 0,
            end: None,
            label,
        },
    }
}

enum BitSpec {
    Range(u32, Option<u32>),
    Relative(u32),
}

fn parse_bit_spec(pair: pest::iterators::Pair<Rule>) -> BitSpec {
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::bit_range => {
                let mut ints = inner.into_inner();
                let start = parse_u32(ints.next());
                let end = parse_u32(ints.next());
                return BitSpec::Range(start, Some(end));
            }
            Rule::bit_relative => {
                return BitSpec::Relative(parse_u32(inner.into_inner().next()));
            }
            Rule::bit_single => {
                return BitSpec::Range(parse_u32(inner.into_inner().next()), None);
            }
            _ => {}
        }
    }
    BitSpec::Range(0, None)
}

fn parse_u32(pair: Option<pest::iterators::Pair<Rule>>) -> u32 {
    pair.and_then(|p| p.as_str().parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_packet_range_block() {
        let result = parse_packet("packet-beta\n0-7: \"Header\"\n").unwrap();
        assert_eq!(result.blocks.len(), 1);
        match &result.blocks[0] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, Some(7));
                assert_eq!(label, "Header");
            }
            _ => panic!("Expected Range block"),
        }
    }

    #[test]
    fn test_parse_packet_single_bit() {
        let result = parse_packet("packet-beta\n0: \"Flag\"\n").unwrap();
        match &result.blocks[0] {
            PacketBlock::Range {
                start, end, label, ..
            } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, None);
                assert_eq!(label, "Flag");
            }
            _ => panic!("Expected Range block"),
        }
    }

    #[test]
    fn test_parse_packet_relative_bits() {
        let result = parse_packet("packet-beta\n+8: \"Data\"\n").unwrap();
        match &result.blocks[0] {
            PacketBlock::Relative { bits, label } => {
                assert_eq!(*bits, 8);
                assert_eq!(label, "Data");
            }
            _ => panic!("Expected Relative block"),
        }
    }

    #[test]
    fn test_parse_packet_multiple_blocks() {
        let input = "packet-beta\n0-7: \"Header\"\n8-15: \"Payload\"\n+16: \"Padding\"\n";
        let result = parse_packet(input).unwrap();
        assert_eq!(result.blocks.len(), 3);
    }

    #[test]
    fn test_parse_packet_with_title() {
        let result = parse_packet("packet-beta\ntitle My Packet\n0-7: \"Header\"\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("My Packet"));
    }

    #[test]
    fn test_parse_packet_short_keyword() {
        let result = parse_packet("packet\n0-7: \"Header\"\n").unwrap();
        assert_eq!(result.blocks.len(), 1);
    }

    #[test]
    fn test_parse_packet_title_with_short_keyword() {
        let result = parse_packet("packet\ntitle Hello world\n0-10: \"hello\"\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("Hello world"));
    }

    #[test]
    fn test_parse_packet_tcp_header_style() {
        let input = concat!(
            "packet\n",
            "0-15: \"Source Port\"\n",
            "16-31: \"Destination Port\"\n",
            "32-63: \"Sequence Number\"\n",
            "64-95: \"Acknowledgment Number\"\n",
            "96-99: \"Data Offset\"\n",
            "100-105: \"Reserved\"\n",
            "106: \"URG\"\n",
            "107: \"ACK\"\n",
            "108: \"PSH\"\n",
            "109: \"RST\"\n",
            "110: \"SYN\"\n",
            "111: \"FIN\"\n",
            "112-127: \"Window\"\n",
            "128-143: \"Checksum\"\n",
            "144-159: \"Urgent Pointer\"\n",
            "160-191: \"(Options and Padding)\"\n",
            "192-223: \"data\"\n",
        );
        let result = parse_packet(input).unwrap();
        assert_eq!(result.blocks.len(), 17);

        match &result.blocks[0] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, Some(15));
                assert_eq!(label, "Source Port");
            }
            _ => panic!("Expected Range block"),
        }

        match &result.blocks[6] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 106);
                assert_eq!(*end, None);
                assert_eq!(label, "URG");
            }
            _ => panic!("Expected Range block"),
        }
    }

    #[test]
    fn test_parse_packet_invalid_input() {
        let result = parse_packet("not packet\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_packet_empty_input() {
        let result = parse_packet("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_packet_with_no_blocks() {
        let result = parse_packet("packet-beta\n").unwrap();
        assert!(result.blocks.is_empty());
    }
}
