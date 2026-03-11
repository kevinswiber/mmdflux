//! Pest parser for Mermaid pie chart diagrams.

use pest::Parser;
use pest_derive::Parser;

use super::error::ParseError;

#[derive(Parser)]
#[grammar = "frontends/mermaid/pie_grammar.pest"]
pub struct PieParser;

/// A section in a pie chart.
#[derive(Debug, Clone)]
pub struct PieSection {
    pub label: String,
    pub value: f64,
}

/// Parsed pie chart diagram.
#[derive(Debug, Clone)]
pub struct Pie {
    pub show_data: bool,
    pub title: Option<String>,
    pub sections: Vec<PieSection>,
}

/// Strip surrounding quotes (double or single) from a string.
fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parse a pie chart diagram string.
pub fn parse_pie(input: &str) -> Result<Pie, ParseError> {
    let pairs = PieParser::parse(Rule::pie_diagram, input).map_err(ParseError::from_pest_error)?;

    let mut show_data = false;
    let mut title = None;
    let mut sections = Vec::new();

    for pair in pairs.filter(|p| p.as_rule() == Rule::pie_diagram) {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::show_data => show_data = true,
                Rule::title_stmt => {
                    title = inner
                        .into_inner()
                        .find(|t| t.as_rule() == Rule::title_text)
                        .map(|t| t.as_str().to_string());
                }
                Rule::pie_section => {
                    sections.push(parse_pie_section(inner));
                }
                _ => {}
            }
        }
    }

    Ok(Pie {
        show_data,
        title,
        sections,
    })
}

fn parse_pie_section(pair: pest::iterators::Pair<Rule>) -> PieSection {
    let mut label = String::new();
    let mut value = 0.0;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::string => {
                let raw = part.into_inner().next().unwrap().as_str();
                label = strip_quotes(raw).to_string();
            }
            Rule::number => {
                value = part.as_str().parse::<f64>().unwrap_or(0.0);
            }
            _ => {}
        }
    }

    PieSection { label, value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pie_single_section() {
        let result = parse_pie("pie\n\"ash\": 100\n").unwrap();
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].label, "ash");
        assert!((result.sections[0].value - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_pie_multiple_sections() {
        let result = parse_pie("pie\n\"ash\" : 60\n\"bat\" : 40\n").unwrap();
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].label, "ash");
        assert_eq!(result.sections[1].label, "bat");
    }

    #[test]
    fn test_parse_pie_show_data() {
        let result = parse_pie("pie showData\n\"ash\" : 60\n\"bat\" : 40\n").unwrap();
        assert!(result.show_data);
    }

    #[test]
    fn test_parse_pie_title() {
        let result = parse_pie("pie title A 60/40 pie\n\"ash\" : 60\n\"bat\" : 40\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("A 60/40 pie"));
    }

    #[test]
    fn test_parse_pie_float_values() {
        let result = parse_pie("pie\n\"a\": 10.5\n\"b\": 20.3\n").unwrap();
        assert!((result.sections[0].value - 10.5).abs() < f64::EPSILON);
        assert!((result.sections[1].value - 20.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_pie_with_comments() {
        let result = parse_pie("pie\n%% a comment\n\"ash\" : 60\n").unwrap();
        assert_eq!(result.sections.len(), 1);
    }

    #[test]
    fn test_parse_pie_single_quoted_labels() {
        let result = parse_pie("pie\n'ash': 100\n").unwrap();
        assert_eq!(result.sections[0].label, "ash");
    }
}
