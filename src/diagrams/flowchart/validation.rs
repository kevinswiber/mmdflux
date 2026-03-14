//! Flowchart-specific validation warnings.
//!
//! Produces `ParseDiagnostic` warnings for unsupported keywords,
//! missing subgraph `end` keywords, and strict-mode parse failures.

use crate::errors::ParseDiagnostic;
use crate::graph::style::parse_node_style_statement;
use crate::mermaid::{ParseOptions, parse_flowchart_with_options};

const STRICT_PARSE_WARNING_PREFIX: &str = "Strict parsing would reject this input:";

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

/// Collect all flowchart-specific validation warnings.
pub(crate) fn collect_all_warnings(input: &str) -> Vec<ParseDiagnostic> {
    let mut warnings = collect_unsupported_warnings(input);
    warnings.extend(collect_subgraph_warnings(input));

    if let Some(strict_warning) = collect_strict_parse_warning(input) {
        warnings.push(strict_warning);
    }

    warnings
}

fn collect_unsupported_warnings(input: &str) -> Vec<ParseDiagnostic> {
    let mut warnings = Vec::new();

    for (line_num, line) in input.lines().enumerate() {
        let trimmed = line.trim();

        if ci_starts_with(trimmed, "style ") {
            warnings.extend(collect_style_warnings(trimmed, line_num + 1));
            continue;
        }

        for &(prefix, message) in UNSUPPORTED_KEYWORDS {
            if ci_starts_with(trimmed, prefix) {
                warnings.push(ParseDiagnostic::warning(
                    Some(line_num + 1),
                    Some(1),
                    message.to_string(),
                ));
                break;
            }
        }

        // "class " needs special handling to avoid matching "classDef"
        if ci_starts_with(trimmed, "class ") && !ci_starts_with(trimmed, "classDef") {
            warnings.push(ParseDiagnostic::warning(
                Some(line_num + 1),
                Some(1),
                "class statements are parsed but ignored in rendering".to_string(),
            ));
        }
    }

    warnings
}

fn collect_style_warnings(line: &str, line_num: usize) -> Vec<ParseDiagnostic> {
    match parse_node_style_statement(line) {
        Some(parsed) => parsed
            .issues
            .into_iter()
            .map(|issue| ParseDiagnostic::warning(Some(line_num), Some(1), issue.message()))
            .collect(),
        None => vec![ParseDiagnostic::warning(
            Some(line_num),
            Some(1),
            "style statements must use the form `style NODE key:value,...`".to_string(),
        )],
    }
}

fn collect_subgraph_warnings(input: &str) -> Vec<ParseDiagnostic> {
    let mut subgraph_lines: Vec<usize> = Vec::new();
    let mut end_count: usize = 0;

    for (line_num, line) in input.lines().enumerate() {
        let trimmed = line.trim();

        if ci_starts_with(trimmed, "subgraph ") || trimmed.eq_ignore_ascii_case("subgraph") {
            subgraph_lines.push(line_num + 1);
        }

        if trimmed.eq_ignore_ascii_case("end")
            || ci_starts_with(trimmed, "end ")
            || ci_starts_with(trimmed, "end;")
        {
            end_count += 1;
        }
    }

    let unmatched = subgraph_lines.len().saturating_sub(end_count);
    if unmatched == 0 {
        return Vec::new();
    }

    subgraph_lines
        .into_iter()
        .rev()
        .take(unmatched)
        .map(|line_num| {
            ParseDiagnostic::warning(
                Some(line_num),
                Some(1),
                "Subgraph may be missing an 'end' keyword. \
                 Without 'end', the subgraph keyword is treated as a node identifier."
                    .to_string(),
            )
        })
        .collect()
}

fn collect_strict_parse_warning(input: &str) -> Option<ParseDiagnostic> {
    let strict = ParseOptions { strict: true };
    parse_flowchart_with_options(input, &strict)
        .err()
        .map(|error| {
            let mut diagnostic = ParseDiagnostic::from(&error);
            diagnostic.severity = "warning".to_string();
            diagnostic.message = format!("{STRICT_PARSE_WARNING_PREFIX} {}", diagnostic.message);
            diagnostic
        })
}

fn ci_starts_with(line: &str, prefix: &str) -> bool {
    line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_warning_for_permissive_input() {
        let input = "%%{init: {}}%%\ngraph TD\nA --> B\n";
        let warning = collect_strict_parse_warning(input);
        assert!(warning.is_some());
        assert!(
            warning
                .unwrap()
                .message
                .contains("Strict parsing would reject")
        );
    }

    #[test]
    fn no_strict_warning_for_strict_valid_input() {
        let input = "graph TD\n  A --> B\n";
        let warning = collect_strict_parse_warning(input);
        assert!(warning.is_none());
    }

    #[test]
    fn unsupported_keyword_classdef() {
        let input = "graph TD\n  classDef foo fill:#f00\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("classDef"));
    }

    #[test]
    fn unsupported_keyword_click() {
        let input = "graph TD\n  click A callback\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("click"));
    }

    #[test]
    fn class_statement_warned() {
        let input = "graph TD\n  class A foo\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("class statements"));
    }

    #[test]
    fn missing_subgraph_end_warned() {
        let input = "graph TD\n  subgraph sg1\n  A --> B\n";
        let warnings = collect_subgraph_warnings(input);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("missing an 'end'"));
    }

    #[test]
    fn matched_subgraph_no_warning() {
        let input = "graph TD\n  subgraph sg1\n  A --> B\n  end\n";
        let warnings = collect_subgraph_warnings(input);
        assert!(warnings.is_empty());
    }

    #[test]
    fn collect_all_includes_strict_and_unsupported() {
        let input = "%%{init: {}}%%\ngraph TD\n  classDef foo fill:#f00\n  A --> B\n";
        let all = collect_all_warnings(input);
        assert!(
            all.len() >= 2,
            "expected strict + unsupported warnings, got {}: {:?}",
            all.len(),
            all.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn clean_input_no_warnings() {
        let input = "graph TD\n  A --> B\n";
        let all = collect_all_warnings(input);
        assert!(all.is_empty());
    }
}
