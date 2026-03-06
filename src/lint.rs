//! Validation/linting for Mermaid input.
//!
//! Provides structured diagnostic output suitable for LLM repair loops
//! and CI/CD integration.

use std::fmt;

use serde::Serialize;

use crate::parser::{
    DiagramType, ParseError, ParseOptions, detect_diagram_type, parse_flowchart_with_options,
};
use crate::style::parse_node_style_statement;

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Fatal error - diagram cannot be parsed.
    Error,
    /// Warning - diagram is valid but has issues.
    Warning,
}

/// A single diagnostic message.
#[derive(Debug, Clone, Serialize)]
pub struct LintDiagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// Source line number (1-based), if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Source column number (1-based), if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// Human-readable message.
    pub message: String,
}

impl fmt::Display for LintDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match (self.line, self.column) {
            (Some(line), Some(col)) => {
                write!(
                    f,
                    "{}: line {}, column {}: {}",
                    severity, line, col, self.message
                )
            }
            (Some(line), None) => {
                write!(f, "{}: line {}: {}", severity, line, self.message)
            }
            _ => {
                write!(f, "{}: {}", severity, self.message)
            }
        }
    }
}

/// Result of linting an input.
#[derive(Debug, Clone, Serialize)]
pub struct LintResult {
    /// Whether the input is valid (no errors).
    pub valid: bool,
    /// Parse errors.
    pub errors: Vec<LintDiagnostic>,
    /// Warnings (valid but has issues).
    pub warnings: Vec<LintDiagnostic>,
}

impl LintResult {
    /// Check if the input is valid (no errors).
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Check if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Suggested exit code for CLI.
    /// 0 = valid (with or without warnings)
    /// 1 = invalid (parse errors)
    pub fn exit_code(&self) -> i32 {
        if self.valid { 0 } else { 1 }
    }

    /// Serialize the lint result to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("LintResult serialization should not fail")
    }
}

/// Lint/validate Mermaid input, returning structured diagnostics.
///
/// Attempts to parse the input and reports any errors or warnings.
/// This is designed for LLM repair loops: the output provides enough
/// context for an LLM to fix syntax errors.
pub fn lint(input: &str) -> LintResult {
    match detect_diagram_type(input) {
        None => {
            return LintResult {
                valid: false,
                errors: vec![LintDiagnostic {
                    severity: Severity::Error,
                    line: Some(1),
                    column: Some(1),
                    message:
                        "Unknown or missing diagram type. Expected 'graph' or 'flowchart' header."
                            .to_string(),
                }],
                warnings: vec![],
            };
        }
        Some(dt) if dt != DiagramType::Flowchart => {
            let keyword = first_keyword(input);
            return LintResult {
                valid: false,
                errors: vec![LintDiagnostic {
                    severity: Severity::Error,
                    line: Some(1),
                    column: Some(1),
                    message: format!(
                        "Unsupported diagram type '{}'. Only flowchart/graph diagrams are supported.",
                        keyword
                    ),
                }],
                warnings: vec![],
            };
        }
        Some(_) => {} // Flowchart — proceed to parse
    }

    let strict = ParseOptions { strict: true };
    match parse_flowchart_with_options(input, &strict) {
        Ok(_) => {
            let warnings = collect_unsupported_warnings(input);
            LintResult {
                valid: true,
                errors: vec![],
                warnings,
            }
        }
        Err(parse_error) => {
            let diagnostic = parse_error_to_diagnostic(&parse_error);
            LintResult {
                valid: false,
                errors: vec![diagnostic],
                warnings: vec![],
            }
        }
    }
}

/// Extract the first non-comment keyword from the input (the diagram type identifier).
fn first_keyword(input: &str) -> &str {
    input
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("unknown")
}

/// Keyword prefixes and their corresponding warning messages.
const UNSUPPORTED_KEYWORDS: &[(&str, &str)] = &[
    (
        "classDef ",
        "classDef statements are parsed but ignored in rendering",
    ),
    (
        "click ",
        "click statements are not applicable in text/ASCII output",
    ),
    (
        "linkStyle ",
        "linkStyle statements are parsed but ignored in rendering",
    ),
];

/// Collect warnings for unsupported or partially supported keywords.
///
/// Returns warnings for `classDef`, `click`, `linkStyle`, and `class`
/// statements plus property-level diagnostics for flowchart node `style`.
pub fn collect_unsupported_warnings(input: &str) -> Vec<LintDiagnostic> {
    let mut warnings = Vec::new();

    for (line_num, line) in input.lines().enumerate() {
        let trimmed = line.trim();

        if ci_starts_with(trimmed, "style ") {
            warnings.extend(collect_style_warnings(trimmed, line_num + 1));
            continue;
        }

        for &(prefix, message) in UNSUPPORTED_KEYWORDS {
            if ci_starts_with(trimmed, prefix) {
                warnings.push(LintDiagnostic {
                    severity: Severity::Warning,
                    line: Some(line_num + 1),
                    column: Some(1),
                    message: message.to_string(),
                });
                break;
            }
        }

        // "class " needs special handling to avoid matching "classDef"
        if ci_starts_with(trimmed, "class ") && !ci_starts_with(trimmed, "classDef") {
            warnings.push(LintDiagnostic {
                severity: Severity::Warning,
                line: Some(line_num + 1),
                column: Some(1),
                message: "class statements are parsed but ignored in rendering".to_string(),
            });
        }
    }

    warnings
}

fn collect_style_warnings(line: &str, line_num: usize) -> Vec<LintDiagnostic> {
    match parse_node_style_statement(line) {
        Some(parsed) => parsed
            .issues
            .into_iter()
            .map(|issue| LintDiagnostic {
                severity: Severity::Warning,
                line: Some(line_num),
                column: Some(1),
                message: issue.message(),
            })
            .collect(),
        None => vec![LintDiagnostic {
            severity: Severity::Warning,
            line: Some(line_num),
            column: Some(1),
            message: "style statements must use the form `style NODE key:value,...`".to_string(),
        }],
    }
}

fn ci_starts_with(line: &str, prefix: &str) -> bool {
    line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Collect warnings for subgraph blocks that appear to be missing an `end` keyword.
///
/// Scans the input for lines that look like `subgraph` declarations and counts
/// matching `end` keywords. When there are more `subgraph` openings than `end`
/// closings, the unmatched subgraph lines are reported as warnings — these
/// typically get silently reinterpreted as regular nodes by the parser.
pub fn collect_subgraph_warnings(input: &str) -> Vec<LintDiagnostic> {
    let mut subgraph_lines: Vec<usize> = Vec::new();
    let mut end_count: usize = 0;

    for (line_num, line) in input.lines().enumerate() {
        let trimmed = line.trim();

        // Count subgraph openings
        if ci_starts_with(trimmed, "subgraph ") || trimmed.eq_ignore_ascii_case("subgraph") {
            subgraph_lines.push(line_num + 1);
        }

        // Count end closings
        if trimmed.eq_ignore_ascii_case("end")
            || ci_starts_with(trimmed, "end ")
            || ci_starts_with(trimmed, "end;")
        {
            end_count += 1;
        }
    }

    // If there are more subgraph lines than end lines, some are unmatched.
    // Warn starting from the last unmatched subgraph (LIFO matching).
    let unmatched = subgraph_lines.len().saturating_sub(end_count);
    if unmatched == 0 {
        return Vec::new();
    }

    // The last `unmatched` subgraph lines are the ones without a matching end.
    subgraph_lines
        .into_iter()
        .rev()
        .take(unmatched)
        .map(|line_num| LintDiagnostic {
            severity: Severity::Warning,
            line: Some(line_num),
            column: Some(1),
            message: "Subgraph may be missing an 'end' keyword. \
                      Without 'end', the subgraph keyword is treated as a node identifier."
                .to_string(),
        })
        .collect()
}

fn parse_error_to_diagnostic(err: &ParseError) -> LintDiagnostic {
    match err {
        ParseError::Syntax {
            line,
            column,
            message,
            ..
        } => LintDiagnostic {
            severity: Severity::Error,
            line: Some(*line),
            column: Some(*column),
            message: message.clone(),
        },
        ParseError::UnexpectedEof => LintDiagnostic {
            severity: Severity::Error,
            line: None,
            column: None,
            message: "Unexpected end of input".to_string(),
        },
        ParseError::Other(msg) => LintDiagnostic {
            severity: Severity::Error,
            line: None,
            column: None,
            message: msg.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_result_valid() {
        let result = LintResult {
            valid: true,
            errors: vec![],
            warnings: vec![],
        };
        assert!(result.is_valid());
        assert!(!result.has_warnings());
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_lint_result_with_errors() {
        let result = LintResult {
            valid: false,
            errors: vec![LintDiagnostic {
                severity: Severity::Error,
                line: Some(3),
                column: Some(5),
                message: "expected identifier".to_string(),
            }],
            warnings: vec![],
        };
        assert!(!result.is_valid());
        assert!(!result.has_warnings());
        assert_eq!(result.exit_code(), 1);
    }

    #[test]
    fn test_lint_result_with_warnings() {
        let result = LintResult {
            valid: true,
            errors: vec![],
            warnings: vec![LintDiagnostic {
                severity: Severity::Warning,
                line: Some(5),
                column: Some(1),
                message: "classDef statements are parsed but ignored".to_string(),
            }],
        };
        assert!(result.is_valid());
        assert!(result.has_warnings());
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_lint_diagnostic_display() {
        let d = LintDiagnostic {
            severity: Severity::Error,
            line: Some(3),
            column: Some(5),
            message: "expected identifier".to_string(),
        };
        let display = format!("{}", d);
        assert!(display.contains("error"));
        assert!(display.contains("line 3"));
        assert!(display.contains("column 5"));
    }

    #[test]
    fn test_lint_diagnostic_display_no_position() {
        let d = LintDiagnostic {
            severity: Severity::Warning,
            line: None,
            column: None,
            message: "something".to_string(),
        };
        let display = format!("{}", d);
        assert!(display.contains("warning"));
        assert!(display.contains("something"));
    }

    #[test]
    fn test_lint_result_json_serialization() {
        let result = LintResult {
            valid: false,
            errors: vec![LintDiagnostic {
                severity: Severity::Error,
                line: Some(3),
                column: Some(5),
                message: "expected identifier".to_string(),
            }],
            warnings: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"valid\":false"));
        assert!(json.contains("\"severity\":\"error\""));
    }

    #[test]
    fn test_lint_valid_input() {
        let result = lint("graph TD\nA --> B\n");
        assert!(result.is_valid());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_lint_invalid_syntax() {
        let result = lint("graph TD\nA --> --> B\n");
        assert!(!result.is_valid());
        assert!(!result.errors.is_empty());
        assert_eq!(result.errors[0].severity, Severity::Error);
        assert!(result.errors[0].line.is_some());
    }

    #[test]
    fn test_lint_empty_input() {
        let result = lint("");
        assert!(!result.is_valid());
    }

    #[test]
    fn test_lint_no_header() {
        let result = lint("A --> B\n");
        assert!(!result.is_valid());
    }

    #[test]
    fn test_lint_valid_complex() {
        let result = lint("graph LR\nA[Start] -->|yes| B(Process)\nB -.-> C{Decision}\n");
        assert!(result.is_valid());
    }

    #[test]
    fn test_lint_warns_on_classdef() {
        let result = lint("graph TD\nA --> B\nclassDef warning fill:#ff0\n");
        assert!(result.is_valid());
        assert!(result.has_warnings());
        let w = &result.warnings[0];
        assert_eq!(w.severity, Severity::Warning);
        assert!(w.message.contains("classDef"));
    }

    #[test]
    fn test_lint_allows_supported_style_properties_without_blanket_warning() {
        let result = lint("graph TD\nA --> B\nstyle A fill:#f9f,stroke:#333,color:#111\n");
        assert!(result.is_valid());
        assert!(!result.has_warnings());
    }

    #[test]
    fn test_lint_warns_on_unsupported_style_properties_not_on_supported_style_statements() {
        let result = lint("graph TD\nA --> B\nstyle A fill:#fff,stroke-width:4px\n");
        assert!(result.is_valid());
        assert!(result.has_warnings());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.message.contains("stroke-width"))
        );
        assert!(!result.warnings.iter().any(|warning| {
            warning
                .message
                .contains("style statements are parsed but ignored")
        }));
    }

    #[test]
    fn test_lint_reports_unsupported_style_keys_with_line_numbers() {
        let input = "graph TD\nA\nstyle A fill:#fff,stroke-width:4px,rx:4px\n";
        let result = lint(input);

        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.line == Some(3) && warning.message.contains("stroke-width"))
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.line == Some(3) && warning.message.contains("rx"))
        );
    }

    #[test]
    fn test_lint_warns_on_click() {
        let result = lint("graph TD\nA --> B\nclick A callback\n");
        assert!(result.is_valid());
        assert!(result.has_warnings());
        assert!(result.warnings[0].message.contains("click"));
    }

    #[test]
    fn test_lint_warns_on_linkstyle() {
        let result = lint("graph TD\nA --> B\nlinkStyle 0 stroke:#ff3\n");
        assert!(result.is_valid());
        assert!(result.has_warnings());
        assert!(result.warnings[0].message.contains("linkStyle"));
    }

    #[test]
    fn test_lint_non_flowchart_returns_unsupported_error() {
        let result = lint("pie\n  title Pets\n  \"Dogs\" : 50\n");
        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0]
                .message
                .contains("Unsupported diagram type"),
            "Expected 'Unsupported diagram type' but got: {}",
            result.errors[0].message
        );
        assert!(result.errors[0].message.contains("pie"));
    }

    #[test]
    fn test_lint_no_warnings_for_clean_input() {
        let result = lint("graph TD\nA[Start] --> B[End]\n");
        assert!(result.is_valid());
        assert!(!result.has_warnings());
    }

    #[test]
    fn test_lint_multiple_warnings() {
        let result =
            lint("graph TD\nA --> B\nstyle A fill:#f9f\nclassDef x fill:#0f0\nclass A x\n");
        assert!(result.is_valid());
        assert!(result.warnings.len() >= 2);
    }

    #[test]
    fn test_subgraph_warning_missing_end() {
        let input = "graph TD\n    subgraph test\n        A --> B\n    en";
        let warnings = collect_subgraph_warnings(input);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, Severity::Warning);
        assert_eq!(warnings[0].line, Some(2));
        assert!(warnings[0].message.contains("end"));
    }

    #[test]
    fn test_subgraph_warning_no_false_positive_with_end() {
        let input = "graph TD\n    subgraph test\n        A --> B\n    end";
        let warnings = collect_subgraph_warnings(input);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_subgraph_warning_multiple_missing_ends() {
        let input = "graph TD\n  subgraph one\n    A --> B\n  subgraph two\n    C --> D";
        let warnings = collect_subgraph_warnings(input);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn test_subgraph_warning_partial_match() {
        // One subgraph has end, one doesn't
        let input = "graph TD\n  subgraph one\n    A --> B\n  end\n  subgraph two\n    C --> D";
        let warnings = collect_subgraph_warnings(input);
        assert_eq!(warnings.len(), 1);
        // The unmatched one is "subgraph two" (line 5)
        assert_eq!(warnings[0].line, Some(5));
    }

    #[test]
    fn test_subgraph_warning_case_insensitive() {
        let input = "graph TD\n    Subgraph Test\n        A --> B\n    en";
        let warnings = collect_subgraph_warnings(input);
        assert_eq!(warnings.len(), 1);
    }
}
