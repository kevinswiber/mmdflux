// Pie chart compliance tests translated from upstream Mermaid spec files.
//
// Sources:
//   - packages/mermaid/src/diagrams/pie/pie.spec.ts (Langium parser)
//   - cypress/integration/rendering/pie.spec.ts

use mmdflux::mermaid::parse_pie;

mod basic {
    use super::*;

    #[test]
    fn single_section() {
        let result = parse_pie("pie\n\"ash\": 100\n").unwrap();
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].label, "ash");
        assert!((result.sections[0].value - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn multiple_sections() {
        let result = parse_pie("pie\n\"GitHub\": 100\n\"GitLab\": 50\n").unwrap();
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].label, "GitHub");
        assert!((result.sections[0].value - 100.0).abs() < f64::EPSILON);
        assert_eq!(result.sections[1].label, "GitLab");
        assert!((result.sections[1].value - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn three_sections() {
        let result =
            parse_pie("pie\n\"Bandy\": 40\n\"Ice-Hockey\": 80\n\"Football\": 90\n").unwrap();
        assert_eq!(result.sections.len(), 3);
        assert_eq!(result.sections[0].label, "Bandy");
        assert_eq!(result.sections[1].label, "Ice-Hockey");
        assert_eq!(result.sections[2].label, "Football");
    }
}

mod title_and_flags {
    use super::*;

    #[test]
    fn with_title() {
        let result = parse_pie("pie title Sports in Sweden\n\"Bandy\": 40\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("Sports in Sweden"));
    }

    #[test]
    fn with_show_data() {
        let result = parse_pie("pie showData\n\"Dogs\": 50\n\"Cats\": 25\n").unwrap();
        assert!(result.show_data);
        assert_eq!(result.sections.len(), 2);
    }

    #[test]
    fn title_with_show_data() {
        let result =
            parse_pie("pie showData title A 60/40 pie\n\"ash\": 60\n\"bat\": 40\n").unwrap();
        assert!(result.show_data);
        assert_eq!(result.title.as_deref(), Some("A 60/40 pie"));
    }

    #[test]
    fn title_with_sections_and_values() {
        let result = parse_pie("pie title sample wow\n\"GitHub\": 100\n\"GitLab\": 50\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("sample wow"));
        assert_eq!(result.sections[0].label, "GitHub");
        assert_eq!(result.sections[1].label, "GitLab");
    }
}

mod values {
    use super::*;

    #[test]
    fn float_values() {
        let result = parse_pie("pie\n\"ash\": 60.67\n\"bat\": 40\n").unwrap();
        assert!((result.sections[0].value - 60.67).abs() < 0.001);
        assert!((result.sections[1].value - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn integer_values() {
        let result = parse_pie("pie\n\"a\": 10\n\"b\": 20\n").unwrap();
        assert!((result.sections[0].value - 10.0).abs() < f64::EPSILON);
        assert!((result.sections[1].value - 20.0).abs() < f64::EPSILON);
    }
}

mod comments {
    use super::*;

    #[test]
    fn comment_between_sections() {
        let result = parse_pie("pie\n%% a comment\n\"ash\": 60\n").unwrap();
        assert_eq!(result.sections.len(), 1);
    }
}

mod quotes {
    use super::*;

    #[test]
    fn single_quoted_labels() {
        let result = parse_pie("pie\n'ash': 100\n").unwrap();
        assert_eq!(result.sections[0].label, "ash");
    }

    #[test]
    fn double_quoted_labels() {
        let result = parse_pie("pie\n\"ash\": 100\n").unwrap();
        assert_eq!(result.sections[0].label, "ash");
    }

    #[test]
    fn long_labels() {
        let result = parse_pie(
            "pie\n\"Time spent looking for movie\": 90\n\"Time spent watching it\": 10\n",
        )
        .unwrap();
        assert_eq!(result.sections[0].label, "Time spent looking for movie");
        assert_eq!(result.sections[1].label, "Time spent watching it");
    }

    #[test]
    fn labels_with_capital_letters() {
        let result = parse_pie("pie\n\"FRIENDS\": 2\n\"FAMILY\": 3\n\"NOSE\": 45\n").unwrap();
        assert_eq!(result.sections.len(), 3);
        assert_eq!(result.sections[0].label, "FRIENDS");
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn invalid_input_rejected() {
        let result = parse_pie("not pie\n");
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_rejected() {
        let result = parse_pie("");
        assert!(result.is_err());
    }

    #[test]
    fn pie_with_no_sections() {
        let result = parse_pie("pie\n").unwrap();
        assert!(result.sections.is_empty());
    }
}

// Tally: 16 passing, 0 ignored
