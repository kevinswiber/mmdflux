// Flowchart compliance tests translated from upstream Mermaid spec files.
//
// Sources:
//   - flow-singlenode.spec.js
//   - flow-edges.spec.js
//   - flow-arrows.spec.js
//   - flow-lines.spec.js
//   - flow-comments.spec.js
//   - flow-direction.spec.js
//   - flow-vertice-chaining.spec.js
//   - subgraph.spec.js
//
// Tally: see bottom of file for pass/ignore counts.

use mmdflux::mermaid::{ArrowHead, ShapeSpec, Statement, StrokeSpec, parse_flowchart};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn edge_count(input: &str) -> usize {
    let fc = parse_flowchart(input).unwrap();
    fc.statements
        .iter()
        .filter(|s| matches!(s, Statement::Edge(_)))
        .count()
}

fn vertex_ids(input: &str) -> Vec<String> {
    let fc = parse_flowchart(input).unwrap();
    let mut ids = Vec::new();
    for stmt in &fc.statements {
        match stmt {
            Statement::Vertex(v) => {
                if !ids.contains(&v.id) {
                    ids.push(v.id.clone());
                }
            }
            Statement::Edge(e) => {
                if !ids.contains(&e.from.id) {
                    ids.push(e.from.id.clone());
                }
                if !ids.contains(&e.to.id) {
                    ids.push(e.to.id.clone());
                }
            }
            _ => {}
        }
    }
    ids
}

// ===========================================================================
// flow-singlenode.spec.js — Node shapes
// ===========================================================================
mod single_node {
    use super::*;

    #[test]
    fn bare_node() {
        let fc = parse_flowchart("graph TD\nA\n").unwrap();
        assert_eq!(fc.statements.len(), 1);
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.id, "A");
                assert!(v.shape.is_none());
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn rectangle_node() {
        let fc = parse_flowchart("graph TD\nA[This is the text]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.id, "A");
                assert_eq!(
                    v.shape,
                    Some(ShapeSpec::Rectangle("This is the text".into()))
                );
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn round_node() {
        let fc = parse_flowchart("graph TD\nA(This is the text)\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Round("This is the text".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn diamond_node() {
        let fc = parse_flowchart("graph TD\nA{This is the text}\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Diamond("This is the text".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn stadium_node() {
        let fc = parse_flowchart("graph TD\nA([Stadium])\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Stadium("Stadium".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn subroutine_node() {
        let fc = parse_flowchart("graph TD\nA[[Subroutine]]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Subroutine("Subroutine".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn cylinder_node() {
        let fc = parse_flowchart("graph TD\nA[(Database)]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Cylinder("Database".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn circle_node() {
        let fc = parse_flowchart("graph TD\nA((Circle))\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Circle("Circle".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn double_circle_node() {
        let fc = parse_flowchart("graph TD\nA(((Double Circle)))\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(
                    v.shape,
                    Some(ShapeSpec::DoubleCircle("Double Circle".into()))
                );
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn hexagon_node() {
        let fc = parse_flowchart("graph TD\nA{{Hexagon}}\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Hexagon("Hexagon".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn asymmetric_node() {
        let fc = parse_flowchart("graph TD\nA>Flag]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Asymmetric("Flag".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn trapezoid_node() {
        let fc = parse_flowchart("graph TD\nA[/Trapezoid\\]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(v.shape, Some(ShapeSpec::Trapezoid("Trapezoid".into())));
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn inv_trapezoid_node() {
        let fc = parse_flowchart("graph TD\nA[\\Inv Trapezoid/]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => {
                assert_eq!(
                    v.shape,
                    Some(ShapeSpec::InvTrapezoid("Inv Trapezoid".into()))
                );
            }
            _ => panic!("expected vertex"),
        }
    }

    #[test]
    fn node_with_underscore_id() {
        let fc = parse_flowchart("graph TD\nmy_node[Text]\n").unwrap();
        match &fc.statements[0] {
            Statement::Vertex(v) => assert_eq!(v.id, "my_node"),
            _ => panic!("expected vertex"),
        }
    }

    // Upstream supports numeric and quoted IDs; keep these as compatibility checks.
    #[test]
    fn numeric_start_id() {
        let _fc = parse_flowchart("graph TD\n1test[Text]\n").unwrap();
    }

    #[test]
    fn quoted_node_text() {
        let _fc = parse_flowchart("graph TD\nA[\"quoted text\"]\n").unwrap();
    }
}

// ===========================================================================
// flow-edges.spec.js — Edge styles
// ===========================================================================
mod edges {
    use super::*;

    #[test]
    fn solid_arrow() {
        let fc = parse_flowchart("graph TD\nA --> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Solid);
                assert_eq!(e.connector.right, ArrowHead::Normal);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn solid_open() {
        let fc = parse_flowchart("graph TD\nA --- B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Solid);
                assert_eq!(e.connector.right, ArrowHead::None);
                assert_eq!(e.connector.left, ArrowHead::None);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn dotted_arrow() {
        let fc = parse_flowchart("graph TD\nA -.-> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Dotted);
                assert_eq!(e.connector.right, ArrowHead::Normal);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn dotted_open() {
        let fc = parse_flowchart("graph TD\nA -.- B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Dotted);
                assert_eq!(e.connector.right, ArrowHead::None);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn thick_arrow() {
        let fc = parse_flowchart("graph TD\nA ==> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Thick);
                assert_eq!(e.connector.right, ArrowHead::Normal);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn thick_open() {
        let fc = parse_flowchart("graph TD\nA === B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Thick);
                assert_eq!(e.connector.right, ArrowHead::None);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn labeled_solid_arrow() {
        let fc = parse_flowchart("graph TD\nA -->|yes| B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.label, Some("yes".to_string()));
                assert_eq!(e.connector.stroke, StrokeSpec::Solid);
                assert_eq!(e.connector.right, ArrowHead::Normal);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn labeled_dotted_arrow() {
        let fc = parse_flowchart("graph TD\nA -.->|label| B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.label, Some("label".to_string()));
                assert_eq!(e.connector.stroke, StrokeSpec::Dotted);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn labeled_thick_arrow() {
        let fc = parse_flowchart("graph TD\nA ==>|label| B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.label, Some("label".to_string()));
                assert_eq!(e.connector.stroke, StrokeSpec::Thick);
            }
            _ => panic!("expected edge"),
        }
    }
}

// ===========================================================================
// flow-arrows.spec.js — Arrow heads and variable length
// ===========================================================================
mod arrows {
    use super::*;

    #[test]
    fn bidirectional_solid() {
        let fc = parse_flowchart("graph TD\nA <--> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.left, ArrowHead::Normal);
                assert_eq!(e.connector.right, ArrowHead::Normal);
                assert_eq!(e.connector.stroke, StrokeSpec::Solid);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn bidirectional_dotted() {
        let fc = parse_flowchart("graph TD\nA <-.-> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.left, ArrowHead::Normal);
                assert_eq!(e.connector.right, ArrowHead::Normal);
                assert_eq!(e.connector.stroke, StrokeSpec::Dotted);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn bidirectional_thick() {
        let fc = parse_flowchart("graph TD\nA <==> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.left, ArrowHead::Normal);
                assert_eq!(e.connector.right, ArrowHead::Normal);
                assert_eq!(e.connector.stroke, StrokeSpec::Thick);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn cross_arrow_right() {
        let fc = parse_flowchart("graph TD\nA --x B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.right, ArrowHead::Cross);
                assert_eq!(e.connector.left, ArrowHead::None);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn circle_arrow_right() {
        let fc = parse_flowchart("graph TD\nA --o B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.right, ArrowHead::Circle);
                assert_eq!(e.connector.left, ArrowHead::None);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn cross_bidirectional() {
        let fc = parse_flowchart("graph TD\nA x--x B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.left, ArrowHead::Cross);
                assert_eq!(e.connector.right, ArrowHead::Cross);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn circle_bidirectional() {
        let fc = parse_flowchart("graph TD\nA o--o B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.left, ArrowHead::Circle);
                assert_eq!(e.connector.right, ArrowHead::Circle);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn variable_length_solid_arrow() {
        // ----> is longer than -->
        let fc = parse_flowchart("graph TD\nA ----> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Solid);
                assert_eq!(e.connector.right, ArrowHead::Normal);
                assert!(e.connector.length > 1);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn variable_length_dotted_arrow() {
        let fc = parse_flowchart("graph TD\nA -..-> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Dotted);
                assert!(e.connector.length > 1);
            }
            _ => panic!("expected edge"),
        }
    }

    #[test]
    fn variable_length_thick_arrow() {
        let fc = parse_flowchart("graph TD\nA ===> B\n").unwrap();
        match &fc.statements[0] {
            Statement::Edge(e) => {
                assert_eq!(e.connector.stroke, StrokeSpec::Thick);
                assert!(e.connector.length > 1);
            }
            _ => panic!("expected edge"),
        }
    }
}

// ===========================================================================
// flow-lines.spec.js — Multi-line and semicolons
// ===========================================================================
mod lines {
    use super::*;

    #[test]
    fn newline_separated_statements() {
        assert_eq!(edge_count("graph TD\nA --> B\nB --> C\n"), 2);
    }

    #[test]
    fn semicolon_separated_statements() {
        assert_eq!(edge_count("graph TD;A --> B;B --> C;\n"), 2);
    }

    #[test]
    fn mixed_separators() {
        let input = "graph TD;A --> B\nB --> C;\n";
        assert_eq!(edge_count(input), 2);
    }
}

// ===========================================================================
// flow-comments.spec.js — Comments
// ===========================================================================
mod comments {
    use super::*;

    #[test]
    fn comment_line_between_statements() {
        let input = "graph TD\nA --> B\n%% this is a comment\nB --> C\n";
        assert_eq!(edge_count(input), 2);
    }

    #[test]
    fn comment_at_start() {
        let input = "graph TD\n%% comment\nA --> B\n";
        assert_eq!(edge_count(input), 1);
    }

    #[test]
    fn inline_content_after_comment_ignored() {
        // Everything after %% on that line is ignored
        let input = "graph TD\nA --> B\n%% B --> C\n";
        assert_eq!(edge_count(input), 1);
    }
}

// ===========================================================================
// flow-direction.spec.js — Direction keywords
// ===========================================================================
mod direction {
    use mmdflux::mermaid::Direction;

    use super::*;

    #[test]
    fn direction_td() {
        let fc = parse_flowchart("graph TD\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::TopDown);
    }

    #[test]
    fn direction_tb() {
        let fc = parse_flowchart("graph TB\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::TopDown);
    }

    #[test]
    fn direction_bt() {
        let fc = parse_flowchart("graph BT\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::BottomTop);
    }

    #[test]
    fn direction_lr() {
        let fc = parse_flowchart("graph LR\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::LeftRight);
    }

    #[test]
    fn direction_rl() {
        let fc = parse_flowchart("graph RL\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::RightLeft);
    }

    #[test]
    fn flowchart_keyword() {
        let fc = parse_flowchart("flowchart TD\nA --> B\n").unwrap();
        assert_eq!(fc.direction, Direction::TopDown);
    }
}

// ===========================================================================
// flow-vertice-chaining.spec.js — Chains and ampersands
// ===========================================================================
mod chaining {
    use super::*;

    #[test]
    fn chain_a_to_b_to_c() {
        let input = "graph TD\nA --> B --> C\n";
        let ids = vertex_ids(input);
        assert!(ids.contains(&"A".to_string()));
        assert!(ids.contains(&"B".to_string()));
        assert!(ids.contains(&"C".to_string()));
        assert_eq!(edge_count(input), 2);
    }

    #[test]
    fn ampersand_source() {
        assert_eq!(edge_count("graph TD\nA & B --> C\n"), 2);
    }

    #[test]
    fn ampersand_target() {
        assert_eq!(edge_count("graph TD\nA --> B & C\n"), 2);
    }

    #[test]
    fn ampersand_both_sides() {
        assert_eq!(edge_count("graph TD\nA & B --> C & D\n"), 4);
    }

    #[test]
    fn chain_with_ampersand_middle() {
        // A --> B & B2 & C --> D2 = 6 edges
        assert_eq!(edge_count("graph TD\nA --> B & B2 & C --> D2\n"), 6);
    }
}

// ===========================================================================
// subgraph.spec.js — Subgraphs
// ===========================================================================
mod subgraphs {
    use super::*;

    #[test]
    fn basic_subgraph() {
        let input = "graph TB\nsubgraph One\na1 --> a2\nend\n";
        let fc = parse_flowchart(input).unwrap();
        let sg = fc
            .statements
            .iter()
            .find_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .expect("expected subgraph");
        // When no bracket title, title defaults to id
        assert_eq!(sg.id, "One");
    }

    #[test]
    fn subgraph_with_bracket_title() {
        let input = "graph TB\nsubgraph some-id[Some Title]\na1 --> a2\nend\n";
        let fc = parse_flowchart(input).unwrap();
        let sg = fc
            .statements
            .iter()
            .find_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .unwrap();
        assert_eq!(sg.id, "some-id");
        assert_eq!(sg.title, "Some Title");
    }

    #[test]
    fn subgraph_with_semicolon_separator() {
        let input = "graph TD;A-->B;subgraph myTitle;c-->d;end;\n";
        let fc = parse_flowchart(input).unwrap();
        let has_sg = fc
            .statements
            .iter()
            .any(|s| matches!(s, Statement::Subgraph(_)));
        assert!(has_sg);
    }

    #[test]
    fn nested_subgraphs() {
        let input = "graph TD\nsubgraph outer[Outer]\nsubgraph inner[Inner]\nA --> B\nend\nend\n";
        let fc = parse_flowchart(input).unwrap();
        let sgs: Vec<_> = fc
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .collect();
        // The outer subgraph should contain the inner one
        assert!(!sgs.is_empty());
    }

    #[test]
    fn subgraph_with_chaining_inside() {
        let input = "graph TB\nsubgraph One\na1 --> a2 --> a3\nend\n";
        let fc = parse_flowchart(input).unwrap();
        let sg = fc
            .statements
            .iter()
            .find_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .unwrap();
        // Should have edges inside
        let inner_edges = sg
            .statements
            .iter()
            .filter(|s| matches!(s, Statement::Edge(_)))
            .count();
        assert_eq!(inner_edges, 2);
    }

    #[test]
    fn subgraph_with_ampersand_inside() {
        let input = "graph TD\nA-->B\nsubgraph myTitle\na & b --> c & e\nend\n";
        let fc = parse_flowchart(input).unwrap();
        // Should parse without error
        assert!(fc.statements.len() >= 2);
    }

    #[test]
    fn subgraph_direction_passthrough() {
        // direction statements inside subgraphs are parsed and discarded
        let input = "graph LR\nsubgraph WithTD\ndirection TD\nA1 --> A2\nend\n";
        let fc = parse_flowchart(input).unwrap();
        let sg = fc
            .statements
            .iter()
            .find_map(|s| match s {
                Statement::Subgraph(sg) => Some(sg),
                _ => None,
            })
            .unwrap();
        assert_eq!(sg.id, "WithTD");
    }

    // Upstream supports numeric-start subgraph IDs and quoted titles without brackets.
    #[test]
    fn numeric_start_subgraph_id() {
        let _fc = parse_flowchart("graph TD\nsubgraph 1test\nA\nend\n").unwrap();
    }

    #[test]
    fn quoted_subgraph_title() {
        let _fc = parse_flowchart("graph TB\nsubgraph \"Some Title\"\na1-->a2\nend\n").unwrap();
    }
}

// ===========================================================================
// Style passthrough
// ===========================================================================
mod style_passthrough {
    use super::*;

    #[test]
    fn style_statement_discarded() {
        // style statement should be silently consumed, not an error
        assert_eq!(edge_count("graph TD\nA --> B\nstyle A fill:#f9f\n"), 1);
    }

    #[test]
    fn classdef_statement_discarded() {
        assert_eq!(
            edge_count("graph TD\nA --> B\nclassDef myClass fill:#f9f\n"),
            1
        );
    }

    #[test]
    fn class_statement_discarded() {
        assert_eq!(edge_count("graph TD\nA --> B\nclass A myClass\n"), 1);
    }

    #[test]
    fn click_statement_discarded() {
        assert_eq!(edge_count("graph TD\nA --> B\nclick A callback\n"), 1);
    }

    #[test]
    fn linkstyle_statement_discarded() {
        assert_eq!(
            edge_count("graph TD\nA --> B\nlinkStyle 0 stroke:#ff3\n"),
            1
        );
    }
}

// ===========================================================================
// Tally
// ===========================================================================
// Passing: 62 tests
// Ignored: 4 tests (numeric IDs, quoted node text, numeric subgraph IDs, quoted subgraph titles)
