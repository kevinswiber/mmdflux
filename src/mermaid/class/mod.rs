//! Class diagram parser.
//!
//! Hand-written line-oriented parser for Mermaid class diagram syntax.
//! Supports MVP scope: class declarations (with optional body), and
//! relationships (association, inheritance, realization, composition, aggregation, dependency).

pub mod ast;

use ast::{ClassDecl, ClassModel, ClassNamespace, ClassRelation, ClassRelationType};

/// Ensure a class exists in the classes list, merging metadata if it already does.
///
/// Uses `class_index` to track position of each class in `classes` for O(1) lookup.
/// If the class already exists, annotations/members are appended. If not, a new entry is created.
fn ensure_class(
    classes: &mut Vec<ClassDecl>,
    class_index: &mut std::collections::HashMap<String, usize>,
    name: String,
    display_label: Option<String>,
    namespace: Option<String>,
    annotations: Vec<String>,
    members: Vec<String>,
) {
    if let Some(&idx) = class_index.get(&name) {
        if display_label.is_some() {
            classes[idx].display_label = display_label;
        }
        if namespace.is_some() && classes[idx].namespace.is_none() {
            classes[idx].namespace = namespace;
        }
        classes[idx].annotations.extend(annotations);
        classes[idx].members.extend(members);
    } else {
        class_index.insert(name.clone(), classes.len());
        classes.push(ClassDecl {
            name,
            display_label,
            namespace,
            annotations,
            members,
        });
    }
}

#[derive(Debug, Clone)]
struct OpenNamespace {
    id: String,
    name: String,
    parent: Option<String>,
}

fn namespace_id(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(parent) => format!("{parent}/{name}"),
        None => format!("namespace:{name}"),
    }
}

/// Parse a class diagram from Mermaid input text.
///
/// Expects the input to start with `classDiagram` (case-insensitive).
pub fn parse_class_diagram(
    input: &str,
) -> Result<ClassModel, Box<dyn std::error::Error + Send + Sync>> {
    let mut classes: Vec<ClassDecl> = Vec::new();
    let mut relations: Vec<ClassRelation> = Vec::new();
    let mut direction: Option<String> = None;
    let mut namespaces: Vec<ClassNamespace> = Vec::new();
    let mut namespace_stack: Vec<OpenNamespace> = Vec::new();
    let mut class_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    let mut lines = input.lines().peekable();

    // Skip frontmatter
    if let Some(first) = lines.peek()
        && first.trim() == "---"
    {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }

    // Skip leading comments and whitespace, then consume header
    let mut found_header = false;
    while let Some(line) = lines.peek() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            lines.next();
            continue;
        }
        if trimmed.to_lowercase().starts_with("classdiagram") {
            found_header = true;
            lines.next();
            break;
        }
        return Err(format!("Expected 'classDiagram' header, got: {trimmed}").into());
    }

    if !found_header {
        return Err("Missing 'classDiagram' header".into());
    }

    // Parse body lines
    let mut in_class_body: Option<String> = None;
    let mut current_display_label: Option<String> = None;
    let mut current_namespace: Option<String> = None;
    let mut current_annotations: Vec<String> = Vec::new();
    let mut current_members: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }

        // Handle class body / namespace close.
        if trimmed == "}" {
            if let Some(class_name) = in_class_body.take() {
                ensure_class(
                    &mut classes,
                    &mut class_index,
                    class_name,
                    std::mem::take(&mut current_display_label),
                    std::mem::take(&mut current_namespace),
                    std::mem::take(&mut current_annotations),
                    std::mem::take(&mut current_members),
                );
            } else if let Some(open_namespace) = namespace_stack.pop() {
                namespaces.push(ClassNamespace {
                    id: open_namespace.id,
                    name: open_namespace.name,
                    parent: open_namespace.parent,
                });
            }
            continue;
        }

        // Inside a class body — collect annotations/members
        if in_class_body.is_some() {
            if let Some(annotation) = parse_annotation(trimmed) {
                current_annotations.push(annotation);
            } else {
                current_members.push(trimmed.to_string());
            }
            continue;
        }

        // Try: `direction LR|RL|BT|TB`
        if let Some(rest) = strip_keyword(trimmed, "direction") {
            if let Some(token) = rest.split_whitespace().next()
                && let Some(parsed) = normalize_class_direction(token)
            {
                direction = Some(parsed);
            }
            continue;
        }

        // Try: `namespace Name {`
        if let Some(rest) = strip_keyword(trimmed, "namespace")
            && let Some(name_raw) = rest.trim().strip_suffix('{')
        {
            let name_raw = name_raw.trim();
            if let Some((name, consumed)) = parse_class_name_token(name_raw)
                && consumed == name_raw.len()
            {
                let parent = namespace_stack.last().map(|ns| ns.id.clone());
                let id = namespace_id(parent.as_deref(), &name);
                namespace_stack.push(OpenNamespace { id, name, parent });
            }
            continue;
        }

        // Try: `class ClassName {`  (body start)
        if let Some(rest) = strip_keyword(trimmed, "class") {
            let rest = rest.trim();
            if let Some(name) = rest.strip_suffix('{') {
                let (name, display_label, annotations) = parse_class_decl(name.trim());
                if name.is_empty() {
                    continue;
                }
                in_class_body = Some(name);
                current_display_label = display_label;
                current_namespace = namespace_stack.last().map(|ns| ns.id.clone());
                current_annotations = annotations;
                current_members.clear();
                continue;
            }

            // Try: `class ClassName`  (bare declaration)
            // May have optional generic/type and annotation like:
            //   `class ClassName~T~`
            //   `class ClassName <<interface>>`
            let (name, display_label, annotations) = parse_class_decl(rest);
            if !name.is_empty() {
                ensure_class(
                    &mut classes,
                    &mut class_index,
                    name,
                    display_label,
                    namespace_stack.last().map(|ns| ns.id.clone()),
                    annotations,
                    Vec::new(),
                );
            }
            continue;
        }

        // Try: `<<annotation>> ClassName`
        if let Some((annotation, class_name)) = parse_annotation_statement(trimmed) {
            ensure_class(
                &mut classes,
                &mut class_index,
                class_name,
                None,
                namespace_stack.last().map(|ns| ns.id.clone()),
                vec![annotation],
                Vec::new(),
            );
            continue;
        }

        // Try: relationship line
        if let Some(rel) = try_parse_relation(trimmed) {
            // Ensure both sides are tracked as classes
            for name in [&rel.from, &rel.to] {
                ensure_class(
                    &mut classes,
                    &mut class_index,
                    name.clone(),
                    None,
                    namespace_stack.last().map(|ns| ns.id.clone()),
                    Vec::new(),
                    Vec::new(),
                );
            }
            relations.push(rel);
            continue;
        }

        // Try: `ClassName : member` or `ClassName: member` (inline member)
        if let Some(colon_pos) = trimmed.find(':') {
            let left = trimmed[..colon_pos].trim();
            let member = trimmed[colon_pos + 1..].trim();
            if !left.is_empty()
                && !member.is_empty()
                && let Some((class_name, consumed)) = parse_class_name_token(left)
                && consumed == left.len()
            {
                ensure_class(
                    &mut classes,
                    &mut class_index,
                    class_name,
                    None,
                    namespace_stack.last().map(|ns| ns.id.clone()),
                    Vec::new(),
                    vec![member.to_string()],
                );
                continue;
            }
        }

        // Permissive: skip unrecognized lines (style, note, click, etc.)
    }

    // Handle unclosed class body
    if let Some(class_name) = in_class_body.take() {
        ensure_class(
            &mut classes,
            &mut class_index,
            class_name,
            std::mem::take(&mut current_display_label),
            std::mem::take(&mut current_namespace),
            std::mem::take(&mut current_annotations),
            std::mem::take(&mut current_members),
        );
    }

    while let Some(open_namespace) = namespace_stack.pop() {
        namespaces.push(ClassNamespace {
            id: open_namespace.id,
            name: open_namespace.name,
            parent: open_namespace.parent,
        });
    }

    Ok(ClassModel {
        classes,
        relations,
        direction,
        namespaces,
    })
}

/// Strip a case-insensitive keyword prefix followed by whitespace.
fn strip_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = line.to_lowercase();
    if lower.starts_with(keyword) {
        let rest = &line[keyword.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// Normalize a class-diagram direction token into Mermaid canonical uppercase form.
fn normalize_class_direction(token: &str) -> Option<String> {
    let upper = token.to_ascii_uppercase();
    match upper.as_str() {
        "LR" | "RL" | "BT" | "TB" => Some(upper),
        _ => None,
    }
}

/// Parse a class name token from the start of `s`.
///
/// Supports:
/// - Mermaid literal/backtick names: `` `A B` ``
/// - Regular identifiers including alnum + `_` + `.` + `-`.
fn parse_class_name_token(s: &str) -> Option<(String, usize)> {
    if let Some(rest) = s.strip_prefix('`') {
        let end = rest.find('`')?;
        if end == 0 {
            return None;
        }
        let name = rest[..end].to_string();
        return Some((name, end + 2));
    }

    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-') {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if end == 0 {
        None
    } else {
        Some((s[..end].to_string(), end))
    }
}

/// Parse Mermaid class display label syntax after an identifier:
/// `[\"Display Label\"]`.
fn parse_class_display_label(s: &str) -> (Option<String>, &str) {
    let trimmed = s.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return (None, trimmed);
    };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('"') else {
        return (None, trimmed);
    };
    let Some(end_quote) = rest.find('"') else {
        return (None, trimmed);
    };
    let label = rest[..end_quote].to_string();
    let rest = rest[end_quote + 1..].trim_start();
    let Some(rest) = rest.strip_prefix(']') else {
        return (None, trimmed);
    };

    (Some(label), rest)
}

/// Parse `class` declaration tail into class name, display label, and inline annotations.
fn parse_class_decl(s: &str) -> (String, Option<String>, Vec<String>) {
    let Some((name, consumed)) = parse_class_name_token(s) else {
        return (String::new(), None, Vec::new());
    };
    if name.is_empty() {
        return (String::new(), None, Vec::new());
    }

    let (display_label, rest) = parse_class_display_label(&s[consumed..]);
    let annotations = parse_annotations(rest);
    (name, display_label, annotations)
}

/// Parse a standalone annotation line like `<<interface>>`.
fn parse_annotation(line: &str) -> Option<String> {
    let inner = line.strip_prefix("<<")?.strip_suffix(">>")?.trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// Parse an annotation statement line like `<<interface>> ClassName`.
fn parse_annotation_statement(line: &str) -> Option<(String, String)> {
    let start = line.strip_prefix("<<")?;
    let end_idx = start.find(">>")?;
    let annotation = start[..end_idx].trim();
    if annotation.is_empty() {
        return None;
    }
    let class_part = start[end_idx + 2..].trim();
    let (class_name, consumed) = parse_class_name_token(class_part)?;
    if class_name.is_empty() || consumed != class_part.len() {
        return None;
    }
    Some((annotation.to_string(), class_name.to_string()))
}

/// Extract all annotation markers from text, returning marker content without brackets.
fn parse_annotations(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;

    while let Some(start_idx) = rest.find("<<") {
        let after_start = &rest[start_idx + 2..];
        if let Some(end_idx) = after_start.find(">>") {
            let annotation = after_start[..end_idx].trim();
            if !annotation.is_empty() {
                out.push(annotation.to_string());
            }
            rest = &after_start[end_idx + 2..];
        } else {
            break;
        }
    }

    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedRelationMarker {
    Arrow,
    Triangle,
    Diamond,
    OpenDiamond,
    Lollipop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedRelationLine {
    Solid,
    Dotted,
}

/// Parse `ClassName ...` from the start of a relation line.
fn parse_relation_endpoint(input: &str) -> Option<(String, &str)> {
    let trimmed = input.trim_start();
    let (name, consumed) = parse_class_name_token(trimmed)?;
    if name.is_empty() {
        return None;
    }
    Some((name, &trimmed[consumed..]))
}

/// Parse an optional quoted cardinality token (`"1"`, `"*"`, etc).
fn parse_optional_cardinality(input: &str) -> Option<(Option<String>, &str)> {
    let trimmed = input.trim_start();
    let Some(rest) = trimmed.strip_prefix('"') else {
        return Some((None, trimmed));
    };
    let end_quote = rest.find('"')?;
    Some((Some(rest[..end_quote].to_string()), &rest[end_quote + 1..]))
}

/// Split relation text from an optional edge label (`:label` or ` : label`).
/// Colons inside quoted cardinality tokens are ignored.
fn split_relation_label(line: &str) -> (&str, Option<String>) {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                let relation = line[..idx].trim_end();
                let label = line[idx + 1..].trim();
                return (relation, (!label.is_empty()).then(|| label.to_string()));
            }
            _ => {}
        }
    }
    (line.trim_end(), None)
}

fn parse_left_relation_marker(input: &str) -> (Option<ParsedRelationMarker>, &str) {
    if let Some(rest) = input.strip_prefix("<|") {
        (Some(ParsedRelationMarker::Triangle), rest)
    } else if let Some(rest) = input.strip_prefix("()") {
        (Some(ParsedRelationMarker::Lollipop), rest)
    } else if let Some(rest) = input.strip_prefix('*') {
        (Some(ParsedRelationMarker::Diamond), rest)
    } else if let Some(rest) = input.strip_prefix('o') {
        (Some(ParsedRelationMarker::OpenDiamond), rest)
    } else if let Some(rest) = input.strip_prefix('<') {
        (Some(ParsedRelationMarker::Arrow), rest)
    } else {
        (None, input)
    }
}

fn parse_right_relation_marker(input: &str) -> (Option<ParsedRelationMarker>, &str) {
    if let Some(rest) = input.strip_prefix("|>") {
        (Some(ParsedRelationMarker::Triangle), rest)
    } else if let Some(rest) = input.strip_prefix("()") {
        (Some(ParsedRelationMarker::Lollipop), rest)
    } else if let Some(rest) = input.strip_prefix('*') {
        (Some(ParsedRelationMarker::Diamond), rest)
    } else if let Some(rest) = input.strip_prefix('o') {
        (Some(ParsedRelationMarker::OpenDiamond), rest)
    } else if let Some(rest) = input.strip_prefix('>') {
        (Some(ParsedRelationMarker::Arrow), rest)
    } else {
        (None, input)
    }
}

fn relation_type_from_markers(
    start: Option<ParsedRelationMarker>,
    end: Option<ParsedRelationMarker>,
    line: ParsedRelationLine,
) -> ClassRelationType {
    let primary_marker = end.or(start);
    match primary_marker {
        Some(ParsedRelationMarker::Arrow) => match line {
            ParsedRelationLine::Solid => ClassRelationType::DirectedAssociation,
            ParsedRelationLine::Dotted => ClassRelationType::DirectedDependency,
        },
        Some(ParsedRelationMarker::Triangle) => match line {
            ParsedRelationLine::Solid => ClassRelationType::Inheritance,
            ParsedRelationLine::Dotted => ClassRelationType::Realization,
        },
        Some(ParsedRelationMarker::Diamond) => ClassRelationType::Composition,
        Some(ParsedRelationMarker::OpenDiamond) => ClassRelationType::Aggregation,
        Some(ParsedRelationMarker::Lollipop) => ClassRelationType::Lollipop,
        None => match line {
            ParsedRelationLine::Solid => ClassRelationType::Association,
            ParsedRelationLine::Dotted => ClassRelationType::Dependency,
        },
    }
}

fn parse_relation_operator(input: &str) -> Option<(ClassRelationType, bool, bool, &str)> {
    let (start_marker, rest) = parse_left_relation_marker(input);

    let (line, rest) = if let Some(rest) = rest.strip_prefix("--") {
        (ParsedRelationLine::Solid, rest)
    } else if let Some(rest) = rest.strip_prefix("..") {
        (ParsedRelationLine::Dotted, rest)
    } else {
        return None;
    };

    let (end_marker, rest) = parse_right_relation_marker(rest);
    let relation_type = relation_type_from_markers(start_marker, end_marker, line);

    Some((
        relation_type,
        start_marker.is_some(),
        end_marker.is_some(),
        rest,
    ))
}

/// Parse class relationships, including lollipop/two-way operators, cardinality,
/// and relaxed relation labels.
fn try_parse_relation(line: &str) -> Option<ClassRelation> {
    let (relation_text, label) = split_relation_label(line.trim());
    let (from_name, rest) = parse_relation_endpoint(relation_text)?;
    let (cardinality_from, rest) = parse_optional_cardinality(rest)?;
    let (relation_type, marker_start, marker_end, rest) =
        parse_relation_operator(rest.trim_start())?;
    let (cardinality_to, rest) = parse_optional_cardinality(rest)?;
    let to_text = rest.trim();

    let (to_name, consumed) = parse_class_name_token(to_text)?;
    if to_name.is_empty() || consumed != to_text.len() {
        return None;
    }

    Some(ClassRelation {
        from: from_name,
        to: to_name,
        relation_type,
        label,
        cardinality_from,
        cardinality_to,
        marker_start,
        marker_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_class_diagram() {
        let model = parse_class_diagram("classDiagram\n").unwrap();
        assert!(model.classes.is_empty());
        assert!(model.relations.is_empty());
    }

    #[test]
    fn parse_single_class() {
        let model = parse_class_diagram("classDiagram\nclass User").unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].name, "User");
        assert!(model.classes[0].annotations.is_empty());
    }

    #[test]
    fn parse_backtick_class_name() {
        let model = parse_class_diagram("classDiagram\nclass `A B`").unwrap();
        assert!(model.classes.iter().any(|c| c.name == "A B"));
    }

    #[test]
    fn parse_class_display_label() {
        let model = parse_class_diagram("classDiagram\nclass User[\"App User\"]").unwrap();
        let user = model.classes.iter().find(|c| c.name == "User").unwrap();
        assert_eq!(user.display_label.as_deref(), Some("App User"));
    }

    #[test]
    fn parse_dotted_and_hyphenated_class_name() {
        let model = parse_class_diagram("classDiagram\nclass HTTP.Client-Parser").unwrap();
        assert!(model.classes.iter().any(|c| c.name == "HTTP.Client-Parser"));
    }

    #[test]
    fn parse_multiple_classes() {
        let input = "classDiagram\nclass User\nclass Order\nclass Product";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 3);
        assert_eq!(model.classes[0].name, "User");
        assert_eq!(model.classes[1].name, "Order");
        assert_eq!(model.classes[2].name, "Product");
    }

    #[test]
    fn parse_class_with_body() {
        let input = "classDiagram\nclass User {\n  +String name\n  +login()\n}";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].name, "User");
        assert!(model.classes[0].annotations.is_empty());
        assert_eq!(model.classes[0].members.len(), 2);
        assert_eq!(model.classes[0].members[0], "+String name");
        assert_eq!(model.classes[0].members[1], "+login()");
    }

    #[test]
    fn parse_class_with_inline_annotation() {
        let input = "classDiagram\nclass Logger <<interface>>";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].name, "Logger");
        assert_eq!(model.classes[0].annotations, vec!["interface"]);
    }

    #[test]
    fn parse_annotation_statement() {
        let input = "classDiagram\nclass Logger\n<<interface>> Logger";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].annotations, vec!["interface"]);
    }

    #[test]
    fn parse_annotation_in_class_body() {
        let input = "classDiagram\nclass Logger {\n  <<interface>>\n  +log(message)\n}";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].annotations, vec!["interface"]);
        assert_eq!(model.classes[0].members, vec!["+log(message)"]);
    }

    #[test]
    fn parse_inheritance_relation() {
        let input = "classDiagram\nAnimal <|-- Dog";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(model.relations[0].from, "Animal");
        assert_eq!(model.relations[0].to, "Dog");
        assert!(model.relations[0].marker_start);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Inheritance
        );
    }

    #[test]
    fn parse_composition_relation() {
        let input = "classDiagram\nCar *-- Engine";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Composition
        );
    }

    #[test]
    fn parse_aggregation_relation() {
        let input = "classDiagram\nLibrary o-- Book";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Aggregation
        );
    }

    #[test]
    fn parse_dependency_relation() {
        let input = "classDiagram\nService ..> Repository";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(model.relations[0].from, "Service");
        assert_eq!(model.relations[0].to, "Repository");
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::DirectedDependency
        );
    }

    #[test]
    fn parse_realization_relation() {
        let input = "classDiagram\nLogger <|.. ConsoleLogger";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Realization
        );
        assert!(model.relations[0].marker_start);
    }

    #[test]
    fn parse_association_directed() {
        let input = "classDiagram\nA --> B";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::DirectedAssociation
        );
    }

    #[test]
    fn parse_association_undirected() {
        let input = "classDiagram\nA -- B";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Association
        );
    }

    #[test]
    fn parse_relation_with_label() {
        let input = "classDiagram\nA --> B : uses";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.relations[0].label, Some("uses".to_string()));
    }

    #[test]
    fn parse_lollipop_relations_do_not_drop_classes() {
        let input = "classDiagram\nClass01 --() bar\nClass02 --() bar\nfoo ()-- Class01";
        let model = parse_class_diagram(input).unwrap();

        assert!(model.classes.iter().any(|c| c.name == "Class02"));
        assert!(model.classes.iter().any(|c| c.name == "foo"));
        assert!(
            model
                .relations
                .iter()
                .all(|r| r.relation_type == ClassRelationType::Lollipop)
        );
        assert!(model.relations[0].marker_end);
        assert!(model.relations[2].marker_start);
    }

    #[test]
    fn parse_cardinality_relation_with_label_without_strict_spaces() {
        let input = "classDiagram\nA \"1\" --> \"*\" B:uses";
        let model = parse_class_diagram(input).unwrap();

        assert_eq!(model.relations.len(), 1);
        assert_eq!(model.relations[0].cardinality_from, Some("1".to_string()));
        assert_eq!(model.relations[0].cardinality_to, Some("*".to_string()));
        assert_eq!(model.relations[0].label, Some("uses".to_string()));
    }

    #[test]
    fn parse_two_way_relation_operator() {
        let input = "classDiagram\nA <|--|> B";
        let model = parse_class_diagram(input).unwrap();

        assert_eq!(model.relations.len(), 1);
        assert_eq!(
            model.relations[0].relation_type,
            ClassRelationType::Inheritance
        );
        assert!(model.relations[0].marker_start);
        assert!(model.relations[0].marker_end);
    }

    #[test]
    fn parse_relation_creates_implicit_classes() {
        let input = "classDiagram\nA --> B";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 2);
    }

    #[test]
    fn parse_class_declared_and_in_relation() {
        let input = "classDiagram\nclass A\nA --> B";
        let model = parse_class_diagram(input).unwrap();
        // A declared explicitly, B implicitly via relation
        assert_eq!(model.classes.len(), 2);
    }

    #[test]
    fn parse_skips_comments() {
        let input = "classDiagram\n%% comment\nclass User";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
    }

    #[test]
    fn parse_missing_header_errors() {
        let result = parse_class_diagram("class User\nA --> B");
        assert!(result.is_err());
    }

    #[test]
    fn parse_case_insensitive_header() {
        let model = parse_class_diagram("CLASSDIAGRAM\nclass User").unwrap();
        assert_eq!(model.classes.len(), 1);
    }

    #[test]
    fn parse_class_direction_lr() {
        let model = parse_class_diagram("classDiagram\ndirection LR\nA --> B").unwrap();
        assert_eq!(model.direction, Some("LR".into()));
    }

    #[test]
    fn parse_class_direction_defaults_when_absent() {
        let model = parse_class_diagram("classDiagram\nA --> B").unwrap();
        assert!(model.direction.is_none());
    }

    #[test]
    fn parse_class_direction_ignores_invalid_values() {
        let model = parse_class_diagram("classDiagram\ndirection DIAGONAL\nA --> B").unwrap();
        assert!(model.direction.is_none());
    }

    #[test]
    fn parse_namespace_with_nested_classes() {
        let input = "\
classDiagram
namespace BaseShapes {
  class Triangle
  namespace Primitives {
    class Rectangle
  }
}";
        let model = parse_class_diagram(input).unwrap();

        assert!(model.namespaces.iter().any(|ns| ns.name == "BaseShapes"));
        assert!(model.namespaces.iter().any(|ns| ns.name == "Primitives"));

        let triangle = model.classes.iter().find(|c| c.name == "Triangle").unwrap();
        assert!(triangle.namespace.is_some());

        let rectangle = model
            .classes
            .iter()
            .find(|c| c.name == "Rectangle")
            .unwrap();
        assert!(rectangle.namespace.is_some());
    }

    #[test]
    fn parse_class_with_generic_annotation() {
        let input = "classDiagram\nclass List~T~";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes[0].name, "List");
    }

    #[test]
    fn parse_inline_member_with_space() {
        let input = "classDiagram\nAnimal : +int age";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].name, "Animal");
        assert_eq!(model.classes[0].members, vec!["+int age"]);
    }

    #[test]
    fn parse_inline_member_without_space() {
        let input = "classDiagram\nAnimal: +isMammal()";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes[0].members, vec!["+isMammal()"]);
    }

    #[test]
    fn parse_multiple_inline_members() {
        let input = "classDiagram\nAnimal : +int age\nAnimal : +String gender\nAnimal: +mate()";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.classes[0].members.len(), 3);
    }

    #[test]
    fn parse_relation_before_class_body_preserves_members() {
        let input = "classDiagram\nAnimal <|-- Dog\nclass Dog {\n  +bark()\n}";
        let model = parse_class_diagram(input).unwrap();
        let dog = model.classes.iter().find(|c| c.name == "Dog").unwrap();
        assert_eq!(dog.members, vec!["+bark()"]);
    }

    #[test]
    fn parse_inline_members_with_relations() {
        let input = "\
classDiagram
    Animal <|-- Duck
    Animal : +int age
    class Duck{
      +swim()
    }";
        let model = parse_class_diagram(input).unwrap();
        let animal = model.classes.iter().find(|c| c.name == "Animal").unwrap();
        assert_eq!(animal.members, vec!["+int age"]);
        let duck = model.classes.iter().find(|c| c.name == "Duck").unwrap();
        assert_eq!(duck.members, vec!["+swim()"]);
    }

    #[test]
    fn parse_full_example() {
        let input = "\
classDiagram
    class Animal {
        +String name
        +makeSound()
    }
    class Dog
    Animal <|-- Dog
    Dog --> Bone : chews";
        let model = parse_class_diagram(input).unwrap();
        assert_eq!(model.classes.len(), 3); // Animal, Dog, Bone
        assert_eq!(model.relations.len(), 2);
        assert_eq!(model.classes[0].name, "Animal");
        assert_eq!(model.classes[0].members.len(), 2);
    }
}
