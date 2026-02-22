//! Parser error types with line/column information.

use pest::RuleType;
use pest::error::Error as PestError;
use serde::Serialize;
use thiserror::Error;

/// Error that occurred during parsing.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Syntax error from the pest parser.
    #[error("Parse error at line {line}, column {column}: {message}")]
    Syntax {
        line: usize,
        column: usize,
        end_line: Option<usize>,
        end_column: Option<usize>,
        message: String,
    },

    /// Unexpected end of input.
    #[error("Unexpected end of input")]
    UnexpectedEof,

    /// Other parser error.
    #[error("Parse error: {0}")]
    Other(String),
}

/// Serializable diagnostic extracted from a [`ParseError`].
///
/// Used at the Wasm boundary to return structured parse error information
/// as JSON rather than a flat error string.
#[derive(Debug, Serialize)]
pub struct ParseDiagnostic {
    /// Severity level: `"error"` or `"warning"`.
    pub severity: String,
    /// Start line (1-indexed), if known.
    pub line: Option<usize>,
    /// Start column (1-indexed), if known.
    pub column: Option<usize>,
    /// End line (1-indexed), if the parser provided a span.
    pub end_line: Option<usize>,
    /// End column (1-indexed), if the parser provided a span.
    pub end_column: Option<usize>,
    /// Human-readable error message.
    pub message: String,
}

impl ParseDiagnostic {
    /// Create a warning diagnostic with the given position and message.
    pub fn warning(line: Option<usize>, column: Option<usize>, message: String) -> Self {
        ParseDiagnostic {
            severity: "warning".to_string(),
            line,
            column,
            end_line: None,
            end_column: None,
            message,
        }
    }
}

impl From<&ParseError> for ParseDiagnostic {
    fn from(err: &ParseError) -> Self {
        match err {
            ParseError::Syntax {
                line,
                column,
                end_line,
                end_column,
                message,
            } => ParseDiagnostic {
                severity: "error".to_string(),
                line: Some(*line),
                column: Some(*column),
                end_line: *end_line,
                end_column: *end_column,
                message: message.clone(),
            },
            other => ParseDiagnostic {
                severity: "error".to_string(),
                line: None,
                column: None,
                end_line: None,
                end_column: None,
                message: other.to_string(),
            },
        }
    }
}

impl ParseError {
    /// Create a ParseError from a pest error.
    pub fn from_pest_error<R: RuleType>(err: PestError<R>) -> Self {
        let (line, column, end_line, end_column) = match err.line_col {
            pest::error::LineColLocation::Pos((l, c)) => (l, c, None, None),
            pest::error::LineColLocation::Span((l, c), (el, ec)) => (l, c, Some(el), Some(ec)),
        };

        ParseError::Syntax {
            line,
            column,
            end_line,
            end_column,
            message: err.variant.message().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diagnostic_from_syntax_error_serializes_to_json() {
        let err = ParseError::Syntax {
            line: 5,
            column: 12,
            end_line: Some(5),
            end_column: Some(20),
            message: "expected node".to_string(),
        };
        let diag = ParseDiagnostic::from(&err);
        let json = serde_json::to_string(&diag).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["severity"], "error");
        assert_eq!(value["line"], 5);
        assert_eq!(value["column"], 12);
        assert_eq!(value["end_line"], 5);
        assert_eq!(value["end_column"], 20);
        assert_eq!(value["message"], "expected node");
    }

    #[test]
    fn parse_diagnostic_from_syntax_error_without_end_position() {
        let err = ParseError::Syntax {
            line: 3,
            column: 1,
            end_line: None,
            end_column: None,
            message: "unexpected token".to_string(),
        };
        let diag = ParseDiagnostic::from(&err);
        let json = serde_json::to_string(&diag).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["line"], 3);
        assert_eq!(value["column"], 1);
        assert!(value["end_line"].is_null());
        assert!(value["end_column"].is_null());
    }

    #[test]
    fn parse_diagnostic_from_other_error_has_no_position() {
        let err = ParseError::Other("something went wrong".to_string());
        let diag = ParseDiagnostic::from(&err);
        let json = serde_json::to_string(&diag).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["line"].is_null());
        assert!(value["column"].is_null());
        assert_eq!(value["message"], "Parse error: something went wrong");
    }

    #[test]
    fn parse_diagnostic_from_unexpected_eof() {
        let err = ParseError::UnexpectedEof;
        let diag = ParseDiagnostic::from(&err);
        let json = serde_json::to_string(&diag).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["line"].is_null());
        assert_eq!(value["message"], "Unexpected end of input");
    }

    #[test]
    fn syntax_error_display_includes_line_and_column() {
        let err = ParseError::Syntax {
            line: 5,
            column: 12,
            end_line: Some(5),
            end_column: Some(15),
            message: "expected node".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Parse error at line 5, column 12: expected node"
        );
    }

    #[test]
    fn syntax_error_display_unchanged_without_end_position() {
        let err = ParseError::Syntax {
            line: 3,
            column: 1,
            end_line: None,
            end_column: None,
            message: "unexpected token".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Parse error at line 3, column 1: unexpected token"
        );
    }

    #[test]
    fn warning_diagnostic_has_warning_severity() {
        let diag = ParseDiagnostic::warning(
            Some(5),
            Some(1),
            "style statements are parsed but ignored".to_string(),
        );
        let json = serde_json::to_string(&diag).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["line"], 5);
        assert_eq!(value["column"], 1);
        assert!(value["end_line"].is_null());
        assert_eq!(value["message"], "style statements are parsed but ignored");
    }
}
