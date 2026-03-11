// Packet diagram compliance tests translated from upstream Mermaid spec files.
//
// Sources:
//   - packages/parser/tests/packet.test.ts
//   - cypress/integration/rendering/packet.spec.ts

use mmdflux::frontends::mermaid::packet::PacketBlock;
use mmdflux::frontends::mermaid::parse_packet;

mod keywords {
    use super::*;

    #[test]
    fn packet_beta_keyword() {
        let result = parse_packet("packet-beta\n0-7: \"Header\"\n").unwrap();
        assert_eq!(result.blocks.len(), 1);
    }

    #[test]
    fn packet_keyword() {
        let result = parse_packet("packet\n0-7: \"Header\"\n").unwrap();
        assert_eq!(result.blocks.len(), 1);
    }
}

mod blocks {
    use super::*;

    #[test]
    fn range_block() {
        let result = parse_packet("packet-beta\n0-7: \"Header\"\n").unwrap();
        match &result.blocks[0] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, Some(7));
                assert_eq!(label, "Header");
            }
            _ => panic!("expected range block"),
        }
    }

    #[test]
    fn single_bit_block() {
        let result = parse_packet("packet\n0: \"h\"\n1: \"i\"\n").unwrap();
        assert_eq!(result.blocks.len(), 2);
        match &result.blocks[0] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, None);
                assert_eq!(label, "h");
            }
            _ => panic!("expected range block"),
        }
    }

    #[test]
    fn relative_block() {
        let result = parse_packet("packet-beta\n+8: \"Data\"\n").unwrap();
        match &result.blocks[0] {
            PacketBlock::Relative { bits, label } => {
                assert_eq!(*bits, 8);
                assert_eq!(label, "Data");
            }
            _ => panic!("expected relative block"),
        }
    }

    #[test]
    fn multiple_blocks() {
        let input = "packet-beta\n0-7: \"Header\"\n8-15: \"Payload\"\n+16: \"Padding\"\n";
        let result = parse_packet(input).unwrap();
        assert_eq!(result.blocks.len(), 3);
    }
}

mod title {
    use super::*;

    #[test]
    fn with_title() {
        let result = parse_packet("packet-beta\ntitle Hello world\n0-10: \"hello\"\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("Hello world"));
    }

    #[test]
    fn packet_with_title() {
        let result = parse_packet("packet\ntitle Hello world\n0-10: \"hello\"\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("Hello world"));
    }
}

mod complex {
    use super::*;

    #[test]
    fn tcp_header_style() {
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

        // Verify first block
        match &result.blocks[0] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 0);
                assert_eq!(*end, Some(15));
                assert_eq!(label, "Source Port");
            }
            _ => panic!("expected range block"),
        }

        // Verify single-bit block
        match &result.blocks[6] {
            PacketBlock::Range { start, end, label } => {
                assert_eq!(*start, 106);
                assert_eq!(*end, None);
                assert_eq!(label, "URG");
            }
            _ => panic!("expected range block"),
        }
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn invalid_input_rejected() {
        let result = parse_packet("not packet\n");
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_rejected() {
        let result = parse_packet("");
        assert!(result.is_err());
    }

    #[test]
    fn packet_with_no_blocks() {
        let result = parse_packet("packet-beta\n").unwrap();
        assert!(result.blocks.is_empty());
    }
}

// Tally: 13 passing, 0 ignored
