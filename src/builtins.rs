//! Built-in diagram registry assembly.
//!
//! This module wires the crate's built-in diagram definitions into a concrete
//! [`crate::registry::DiagramRegistry`]. Reusable registry contracts live in
//! [`crate::registry`].

use crate::diagrams::{class, flowchart, sequence};
use crate::registry::DiagramRegistry;

/// Create the default registry with all built-in diagram types.
///
/// Registration order determines detection priority. Flowchart is registered
/// first as the most common diagram type.
pub fn default_registry() -> DiagramRegistry {
    let mut registry = DiagramRegistry::new();

    // Flowchart - most common, register first.
    registry.register(flowchart::definition());

    // Graph-family diagrams.
    registry.register(class::definition());

    // Timeline-family diagrams.
    registry.register(sequence::definition());

    registry
}
