//! Pest parser for Mermaid info diagrams.

use pest::Parser;
use pest_derive::Parser;

use super::error::ParseError;

#[derive(Parser)]
#[grammar = "mermaid/info_grammar.pest"]
pub struct InfoParser;

/// Parsed info diagram.
#[derive(Debug, Clone)]
pub struct Info {
    pub show_info: bool,
    pub title: Option<String>,
}

/// Parse an info diagram string.
pub fn parse_info(input: &str) -> Result<Info, ParseError> {
    let pairs =
        InfoParser::parse(Rule::info_diagram, input).map_err(ParseError::from_pest_error)?;

    let mut show_info = false;
    let mut title = None;

    for pair in pairs.filter(|p| p.as_rule() == Rule::info_diagram) {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::show_info => show_info = true,
                Rule::info_with_show_info => show_info = true,
                Rule::title_stmt => {
                    title = inner
                        .into_inner()
                        .find(|t| t.as_rule() == Rule::title_text)
                        .map(|t| t.as_str().to_string());
                }
                _ => {}
            }
        }
    }

    Ok(Info { show_info, title })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_info_minimal() {
        let result = parse_info("info\n").unwrap();
        assert!(!result.show_info);
        assert!(result.title.is_none());
    }

    #[test]
    fn test_parse_info_show_info() {
        let result = parse_info("info\nshowInfo\n").unwrap();
        assert!(result.show_info);
    }

    #[test]
    fn test_parse_info_show_info_same_line() {
        let result = parse_info("info showInfo\n").unwrap();
        assert!(result.show_info);
    }

    #[test]
    fn test_parse_info_with_title() {
        let result = parse_info("info\ntitle My Info\n").unwrap();
        assert_eq!(result.title.as_deref(), Some("My Info"));
    }

    #[test]
    fn test_parse_info_show_info_and_title() {
        let result = parse_info("info\nshowInfo\ntitle My Info\n").unwrap();
        assert!(result.show_info);
        assert_eq!(result.title.as_deref(), Some("My Info"));
    }

    #[test]
    fn test_parse_info_invalid() {
        let result = parse_info("not info\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_info_minimal_without_trailing_newline() {
        let result = parse_info("info").unwrap();
        assert!(!result.show_info);
        assert!(result.title.is_none());
    }

    #[test]
    fn test_parse_info_show_info_without_trailing_newline() {
        let result = parse_info("info\nshowInfo").unwrap();
        assert!(result.show_info);
        assert!(result.title.is_none());
    }

    #[test]
    fn test_parse_info_title_without_trailing_newline() {
        let result = parse_info("info\ntitle My Info").unwrap();
        assert_eq!(result.title.as_deref(), Some("My Info"));
    }

    #[test]
    fn test_parse_info_leading_whitespace() {
        let result = parse_info("\n  info\n");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_info_trailing_whitespace_on_keyword_line() {
        let result = parse_info("info   \n");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_info_unsupported_keyword() {
        let result = parse_info("info unsupported\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_info_empty_input() {
        let result = parse_info("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_info_whitespace_only_input() {
        let result = parse_info("   \n  \n");
        assert!(result.is_err());
    }
}
