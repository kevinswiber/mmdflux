//! Flowchart-specific validation warnings.
//!
//! Produces `ParseDiagnostic` warnings for unsupported keywords,
//! missing subgraph `end` keywords, and strict-mode parse failures.

use crate::errors::ParseDiagnostic;
use crate::graph::style::{
    parse_classdef_statement, parse_linkstyle_statement, parse_node_style_statement,
};
use crate::mermaid::{
    ParseOptions, Statement, Vertex, parse_flowchart, parse_flowchart_with_options,
    strip_theme_only_compat_syntax,
};

const STRICT_PARSE_WARNING_PREFIX: &str = "Strict parsing would reject this input:";

const UNSUPPORTED_KEYWORDS: &[(&str, &str)] = &[(
    "click ",
    "click statements are not applicable in text/ASCII output",
)];

/// Collect all flowchart-specific validation warnings.
pub(crate) fn collect_all_warnings(input: &str) -> Vec<ParseDiagnostic> {
    let mut warnings = collect_unsupported_warnings(input);
    warnings.extend(collect_subgraph_warnings(input));
    warnings.extend(collect_id_collision_warnings(input));

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

        // classDef: warn on unsupported CSS properties
        if ci_starts_with(trimmed, "classDef ") {
            warnings.extend(collect_classdef_warnings(trimmed, line_num + 1));
            continue;
        }

        // class: now supported, skip
        if ci_starts_with(trimmed, "class ") && !ci_starts_with(trimmed, "classDef") {
            continue;
        }

        // linkStyle: warn on unsupported properties or invalid indices
        if ci_starts_with(trimmed, "linkStyle ") {
            warnings.extend(collect_linkstyle_warnings(trimmed, line_num + 1));
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
    }

    warnings
}

fn collect_classdef_warnings(line: &str, line_num: usize) -> Vec<ParseDiagnostic> {
    match parse_classdef_statement(line) {
        Some(parsed) => parsed
            .issues
            .into_iter()
            .map(|issue| ParseDiagnostic::warning(Some(line_num), Some(1), issue.message()))
            .collect(),
        None => vec![ParseDiagnostic::warning(
            Some(line_num),
            Some(1),
            "classDef statements must use the form `classDef className key:value,...`".to_string(),
        )],
    }
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

fn collect_linkstyle_warnings(line: &str, line_num: usize) -> Vec<ParseDiagnostic> {
    match parse_linkstyle_statement(line) {
        Some(parsed) => parsed
            .issues
            .into_iter()
            .map(|issue| ParseDiagnostic::warning(Some(line_num), Some(1), issue.message()))
            .collect(),
        None => vec![ParseDiagnostic::warning(
            Some(line_num),
            Some(1),
            "linkStyle statements must use the form `linkStyle <target> key:value,...`".to_string(),
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
    let original_error = match parse_flowchart_with_options(input, &strict) {
        Ok(_) => return None,
        Err(error) => error,
    };

    if let Some(stripped) = strip_theme_only_compat_syntax(input)
        && parse_flowchart_with_options(&stripped, &strict).is_ok()
    {
        return None;
    }

    let mut diagnostic = ParseDiagnostic::from(&original_error);
    diagnostic.severity = "warning".to_string();
    diagnostic.message = format!("{STRICT_PARSE_WARNING_PREFIX} {}", diagnostic.message);
    Some(diagnostic)
}

fn ci_starts_with(line: &str, prefix: &str) -> bool {
    line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.as_bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Detect subgraph/node id collisions via the parser.
///
/// We parse the input, walk the AST to find vertices with an explicit shape
/// whose id matches a declared subgraph, then look up a source line for each
/// occurrence. Sourcing from the parser (rather than a private text scan)
/// guarantees the warning's coverage matches the compiler's collapse rule —
/// numeric ids, dotted ids, and the `@{shape: …}` form all flow through.
/// Bare references like `A --> B` are NOT collisions (no `vertex.shape`).
/// Text inside edge labels never becomes a `Vertex`, so the false-positive
/// case `B -->|A[NotNode]| C` is naturally excluded.
fn collect_id_collision_warnings(input: &str) -> Vec<ParseDiagnostic> {
    let Ok(flowchart) = parse_flowchart(input) else {
        return Vec::new();
    };

    let mut subgraph_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_subgraph_ids_recursive(&flowchart.statements, &mut subgraph_ids);
    if subgraph_ids.is_empty() {
        return Vec::new();
    }

    let mut collisions: Vec<String> = Vec::new();
    walk_collision_references(&flowchart.statements, &subgraph_ids, &mut collisions);
    if collisions.is_empty() {
        return Vec::new();
    }

    let lines: Vec<String> = input.lines().map(censor_edge_label_text).collect();
    let mut used: Vec<(usize, usize)> = Vec::new();
    let mut warnings = Vec::new();
    for id in &collisions {
        let position = find_collision_position(&lines, id, &used);
        if let Some(pos) = position {
            used.push(pos);
        }
        let (line_num, column) = match position {
            Some((row, col)) => (Some(row + 1), Some(col + 1)),
            None => (None, None),
        };
        let message = format!(
            "Identifier '{id}' is declared as a subgraph; the explicit \
             node reference is collapsed into the subgraph.",
        );
        warnings.push(ParseDiagnostic::warning(line_num, column, message));
    }

    warnings
}

fn collect_subgraph_ids_recursive(
    statements: &[Statement],
    out: &mut std::collections::HashSet<String>,
) {
    for stmt in statements {
        if let Statement::Subgraph(sg) = stmt {
            out.insert(sg.id.clone());
            collect_subgraph_ids_recursive(&sg.statements, out);
        }
    }
}

fn walk_collision_references(
    statements: &[Statement],
    subgraph_ids: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    for stmt in statements {
        match stmt {
            Statement::Vertex(v) => record_collision(v, subgraph_ids, out),
            Statement::Edge(e) => {
                record_collision(&e.from, subgraph_ids, out);
                record_collision(&e.to, subgraph_ids, out);
            }
            Statement::Subgraph(sg) => {
                walk_collision_references(&sg.statements, subgraph_ids, out);
            }
            Statement::NodeStyle(_)
            | Statement::ClassDef(_)
            | Statement::ClassApply(_)
            | Statement::LinkStyle(_) => {}
        }
    }
}

fn record_collision(
    vertex: &Vertex,
    subgraph_ids: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    if vertex.shape.is_some() && subgraph_ids.contains(&vertex.id) {
        out.push(vertex.id.clone());
    }
}

/// Replace bytes inside `|...|` segments with spaces so a collision-position
/// scan doesn't latch onto an edge-label substring like `B -->|A[NotNode]| C`.
fn censor_edge_label_text(line: &str) -> String {
    let mut censored = line.as_bytes().to_vec();
    let mut i = 0;
    while i < censored.len() {
        if censored[i] == b'|' {
            let start = i + 1;
            let mut j = start;
            while j < censored.len() && censored[j] != b'|' {
                j += 1;
            }
            if j < censored.len() {
                for byte in &mut censored[start..j] {
                    *byte = b' ';
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    String::from_utf8(censored).unwrap_or_else(|_| line.to_string())
}

/// Find the first un-used line/column where `id` appears as a token followed
/// by a vertex-shape opener (`[`, `(`, `{`, `>`, or `@{`). Skips
/// `subgraph <id>…` declaration lines so the diagnostic points at the
/// colliding reference.
fn find_collision_position(
    lines: &[String],
    id: &str,
    used: &[(usize, usize)],
) -> Option<(usize, usize)> {
    let id_bytes = id.as_bytes();
    for (row, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if ci_starts_with(trimmed, "subgraph ") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut start = 0;
        while let Some(rel) = find_subsequence(&bytes[start..], id_bytes) {
            let pos = start + rel;
            start = pos + 1;

            if used.contains(&(row, pos)) {
                continue;
            }

            let prev_is_ident_continue = pos > 0 && is_ident_continue(bytes[pos - 1]);
            if prev_is_ident_continue {
                continue;
            }

            let after = pos + id_bytes.len();
            let next = bytes.get(after).copied();
            // Shape openers: `[`, `(`, `{`, `>` (asymmetric `>…]`), and
            // `@{` (Mermaid v11 `@{shape: …}` form).
            let matches_shape = matches!(next, Some(b'[' | b'(' | b'{' | b'>'))
                || (next == Some(b'@') && bytes.get(after + 1) == Some(&b'{'));
            if !matches_shape {
                continue;
            }

            return Some((row, pos));
        }
    }
    None
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
    fn strict_warning_ignores_theme_only_frontmatter() {
        let input = "---\nconfig:\n  theme: dark\n---\ngraph TD\nA --> B\n";
        let warning = collect_strict_parse_warning(input);
        assert!(warning.is_none());
    }

    #[test]
    fn strict_warning_ignores_theme_only_init_directive() {
        let input = "%%{init: {\"theme\": \"dark\"}}%%\ngraph TD\nA --> B\n";
        let warning = collect_strict_parse_warning(input);
        assert!(warning.is_none());
    }

    #[test]
    fn strict_warning_keeps_non_theme_init_keys() {
        let input = "%%{init: {\"theme\": \"dark\", \"flowchart\": {\"curve\": \"basis\"}}}%%\ngraph TD\nA --> B\n";
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
    fn classdef_no_longer_warned_as_unsupported() {
        let input = "graph TD\n  classDef foo fill:#f00\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            !warnings.iter().any(|w| w.message.contains("classDef")),
            "classDef with valid properties should not produce warnings: {:?}",
            warnings
        );
    }

    #[test]
    fn classdef_unsupported_property_warned() {
        let input = "graph TD\n  classDef foo fill:#f00,shape-padding:14px\n  A:::foo\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            warnings.iter().any(|w| w.message.contains("shape-padding")),
            "unsupported property in classDef should warn: {:?}",
            warnings
        );
    }

    #[test]
    fn unsupported_keyword_click() {
        let input = "graph TD\n  click A callback\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("click"));
    }

    #[test]
    fn class_statement_no_longer_warned() {
        let input = "graph TD\n  class A foo\n  A --> B\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            !warnings.iter().any(|w| w.message.contains("class")),
            "class statements should not produce warnings: {:?}",
            warnings
        );
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
        let input = "%%{init: {}}%%\ngraph TD\n  click A callback\n  A --> B\n";
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

    #[test]
    fn linkstyle_valid_no_warning() {
        let input = "graph TD\n  A --> B\n  linkStyle 0 stroke:#f00\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            warnings.is_empty(),
            "valid linkStyle should not produce warnings: {:?}",
            warnings
        );
    }

    #[test]
    fn linkstyle_invalid_index_warned() {
        let input = "graph TD\n  A --> B\n  B --> C\n  linkStyle nope,1 stroke:#f00\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            warnings.iter().any(|w| w.message.contains("nope")),
            "invalid linkStyle index should warn: {:?}",
            warnings
        );
    }

    #[test]
    fn linkstyle_unsupported_property_warned() {
        let input = "graph TD\n  A --> B\n  linkStyle 0 opacity:0.5\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            warnings.iter().any(|w| w.message.contains("opacity")),
            "unsupported linkStyle property should warn: {:?}",
            warnings
        );
    }

    #[test]
    fn linkstyle_malformed_warned() {
        let input = "graph TD\n  A --> B\n  linkStyle\n";
        let warnings = collect_unsupported_warnings(input);
        assert!(
            warnings.is_empty(),
            "bare linkStyle keyword (no target) should not match: {:?}",
            warnings
        );
    }

    #[test]
    fn id_collision_emits_warning_with_line() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";

        let warnings = collect_all_warnings(input);

        let collision = warnings
            .iter()
            .find(|w| w.message.contains("declared as a subgraph"))
            .expect("expected a collision warning");

        assert!(
            collision.message.contains("'A'"),
            "warning should single-quote the colliding id; got {:?}",
            collision.message,
        );
        assert_eq!(collision.severity, "warning");
        assert_eq!(
            collision.line,
            Some(8),
            "warning should point at the A[NodeBox] --> B[NodeBox2] line",
        );
    }

    #[test]
    fn no_collision_yields_no_warning() {
        let input = "flowchart LR\nsubgraph A\na1\nend\n";
        let warnings = collect_all_warnings(input);
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph")),
            "no collision in input but warning emitted: {warnings:?}",
        );
    }

    #[test]
    fn bare_reference_to_subgraph_id_does_not_emit_collision_warning() {
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A --> B
end
";
        let warnings = collect_all_warnings(input);
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph")),
            "bare reference must not trigger a collision warning: {warnings:?}",
        );
    }

    #[test]
    fn strict_parse_does_not_yet_escalate_id_collision() {
        // Phase 1 contract: id collisions emit a permissive warning but do
        // not escalate under strict parsing. A follow-up to issue #352 will
        // flip this assertion to pin the escalated error path.
        let input = "\
flowchart LR

subgraph A
a1
end

subgraph C
A[NodeBox] --> B[NodeBox2]
end
";

        let warning = collect_strict_parse_warning(input);

        match warning {
            None => {}
            Some(diagnostic) => {
                assert!(
                    !diagnostic.message.contains("declared as a subgraph"),
                    "strict-mode escalation of id collisions is a separate \
                     follow-up; got premature escalation: {diagnostic:?}",
                );
            }
        }
    }

    #[test]
    fn numeric_subgraph_id_collision_emits_warning() {
        let input = "\
flowchart LR

subgraph 123[Numbers]
a1
end

subgraph C
123[NodeBox] --> B
end
";
        let warnings = collect_all_warnings(input);
        assert!(
            warnings.iter().any(
                |w| w.message.contains("declared as a subgraph") && w.message.contains("'123'")
            ),
            "numeric id collision must warn: {warnings:?}",
        );
    }

    #[test]
    fn dotted_subgraph_id_collision_emits_warning() {
        let input = "\
flowchart LR

subgraph my.node[Group]
a1
end

subgraph C
my.node[NodeBox] --> B
end
";
        let warnings = collect_all_warnings(input);
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph")
                    && w.message.contains("'my.node'")),
            "dotted id collision must warn: {warnings:?}",
        );
    }

    #[test]
    fn asymmetric_shape_collision_emits_warning_with_line() {
        // Mermaid's asymmetric/flag shape `>...]`. The parser recognizes
        // `A>NodeBox]` as a Vertex with shape, so the warning must
        // line-locate it like the other shape forms.
        let input = "\
flowchart LR

subgraph A
a1
end

A>NodeBox]
";
        let warnings = collect_all_warnings(input);
        let collision = warnings
            .iter()
            .find(|w| w.message.contains("declared as a subgraph"))
            .expect("expected a collision warning");
        assert_eq!(
            collision.line,
            Some(7),
            "asymmetric shape collision must line-locate; got {:?}",
            collision,
        );
    }

    #[test]
    fn at_brace_shape_collision_emits_warning() {
        // Mermaid v11 `@{shape: ...}` shape syntax: the compiler collapses
        // `A@{shape: rect, label: "NodeBox"}` against `subgraph A`, so the
        // diagnostic path must too.
        let input = "\
flowchart LR

subgraph A
a1
end

A@{shape: rect, label: \"NodeBox\"}
";
        let warnings = collect_all_warnings(input);
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph") && w.message.contains("'A'")),
            "@{{shape}} collision must warn: {warnings:?}",
        );
    }

    #[test]
    fn bare_id_in_edge_label_does_not_emit_collision_warning() {
        // `B -->|A[NotNode]| C` writes the literal `A[NotNode]` as an edge
        // label between pipes. The parser sees no Vertex for `A` here, so
        // the warning path must not surface a false collision.
        let input = "\
flowchart LR

subgraph A
a1
end

B -->|A[NotNode]| C
";
        let warnings = collect_all_warnings(input);
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph")),
            "edge-label text must not trigger a collision warning: {warnings:?}",
        );
    }

    #[test]
    fn top_level_explicit_shape_collision_still_emits_warning() {
        let input = "\
flowchart LR

subgraph A
a1
end

A[NodeBox]
";
        let warnings = collect_all_warnings(input);
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("declared as a subgraph")),
            "top-level explicit-shape collision must still warn: {warnings:?}",
        );
    }
}
