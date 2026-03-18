//! Flowchart diagram implementation.
//!
//! Flowcharts are node-edge graphs rendered using hierarchical (Sugiyama) layout.
//! This is the original and most feature-complete diagram type in mmdflux.

pub mod compiler;
mod instance;
pub(crate) mod validation;

pub use compiler::compile_to_graph;
pub use instance::FlowchartInstance;

/// Detect if input is a flowchart diagram.
///
/// Delegates to the centralized parser detection to ensure consistent behavior:
/// - Skips `%%` comment lines
/// - Case-insensitive keyword matching
/// - Exact first-word matching (not prefix)
pub fn detect(input: &str) -> bool {
    crate::mermaid::detect_diagram_type(input) == Some(crate::mermaid::DiagramType::Flowchart)
}
