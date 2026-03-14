//! Mermaid source-ingestion boundary.
//!
//! Owns Mermaid source detection, diagnostics, and parse entrypoints before
//! diagram modules compile Mermaid syntax into family IR.

pub mod ast;
pub mod class;
pub mod error;
pub mod flowchart;
pub mod sequence;

pub use ast::*;
pub use error::*;
pub use flowchart::{
    Flowchart, ParseOptions, parse_flowchart, parse_flowchart_with_options, strip_frontmatter,
};

/// The type of Mermaid diagram detected from input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramType {
    Flowchart,
    Class,
    Sequence,
}

/// Detect the Mermaid logical diagram type from the first significant keyword.
#[must_use]
pub fn detect_diagram_type(input: &str) -> Option<DiagramType> {
    let input = strip_frontmatter(input);
    let first_word = input
        .lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .and_then(|line| line.split_whitespace().next())?;

    match first_word.to_ascii_lowercase().as_str() {
        "graph" | "flowchart" => Some(DiagramType::Flowchart),
        "classdiagram" => Some(DiagramType::Class),
        "sequencediagram" => Some(DiagramType::Sequence),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagramType, detect_diagram_type};

    #[test]
    fn detects_mermaid_logical_types() {
        assert_eq!(
            detect_diagram_type("graph TD\nA-->B\n"),
            Some(DiagramType::Flowchart)
        );
        assert_eq!(
            detect_diagram_type("classDiagram\nclass User"),
            Some(DiagramType::Class)
        );
        assert_eq!(
            detect_diagram_type("sequenceDiagram\nparticipant A"),
            Some(DiagramType::Sequence)
        );
    }
}
