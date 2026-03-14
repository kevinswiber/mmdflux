//! Runtime payload contract returned by [`crate::registry::ParsedDiagram::into_payload`].
//!
//! Diagram modules stop at parsing, compilation, and config-sensitive payload
//! construction. Runtime owns the final dispatch from these payloads to
//! family-specific renderers.

/// Diagram payload returned by the registry contract.
#[derive(Debug, Clone)]
pub enum Diagram {
    /// Flowchart payload (graph-family).
    Flowchart(crate::graph::Graph),
    /// Class diagram payload (graph-family).
    Class(crate::graph::Graph),
    /// Sequence diagram payload.
    Sequence(crate::timeline::Sequence),
}
