//! Diagram registry for type detection and dispatch.
//!
//! The registry holds diagram definitions and provides:
//! - Type detection from input text
//! - Factory creation of diagram instances
//! - Format support queries

use std::collections::HashMap;

use crate::engines::graph::{DiagramFamily, OutputFormat, RenderConfig, RenderError};

/// Detector function type.
///
/// Returns `true` if the input text matches this diagram type.
pub type DiagramDetector = fn(&str) -> bool;

/// Factory for creating diagram instances.
pub type DiagramFactory = fn() -> Box<dyn DiagramInstance>;

/// Diagram definition for registration.
///
/// Each diagram type provides a definition that describes how to
/// detect, create, and render that diagram type.
pub struct DiagramDefinition {
    /// Unique identifier (e.g., "flowchart", "pie").
    pub id: &'static str,
    /// Diagram family classification.
    pub family: DiagramFamily,
    /// Detection function that checks if input matches this type.
    pub detector: DiagramDetector,
    /// Factory for creating instances.
    pub factory: DiagramFactory,
    /// Supported output formats.
    pub supported_formats: &'static [OutputFormat],
}

/// Global diagram registry.
///
/// Holds all registered diagram types and provides detection/dispatch.
pub struct DiagramRegistry {
    diagrams: HashMap<&'static str, DiagramDefinition>,
    /// Detection order (priority) - first match wins.
    detection_order: Vec<&'static str>,
}

impl DiagramRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            diagrams: HashMap::new(),
            detection_order: Vec::new(),
        }
    }

    /// Register a diagram type.
    ///
    /// Diagrams are detected in registration order (first match wins).
    pub fn register(&mut self, definition: DiagramDefinition) {
        let id = definition.id;
        self.diagrams.insert(id, definition);
        self.detection_order.push(id);
    }

    /// Detect diagram type from input text.
    ///
    /// Returns the ID of the first registered diagram whose detector
    /// returns `true` for the input.
    #[must_use]
    pub fn detect(&self, input: &str) -> Option<&'static str> {
        for id in &self.detection_order {
            if let Some(def) = self.diagrams.get(id)
                && (def.detector)(input)
            {
                return Some(def.id);
            }
        }
        None
    }

    /// Get a diagram definition by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&DiagramDefinition> {
        self.diagrams.get(id)
    }

    /// List all registered diagram IDs.
    pub fn diagram_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.detection_order.iter().copied()
    }

    /// Create a new diagram instance by ID.
    ///
    /// Returns `None` if no diagram with the given ID is registered.
    #[must_use]
    pub fn create(&self, id: &str) -> Option<Box<dyn DiagramInstance>> {
        self.diagrams.get(id).map(|def| (def.factory)())
    }

    /// Resolve input text to a diagram handle with metadata.
    ///
    /// Detects the diagram type and returns a [`ResolvedDiagram`] that
    /// exposes the diagram ID, family, and supported formats without
    /// creating an instance.
    #[must_use]
    pub fn resolve(&self, input: &str) -> Option<ResolvedDiagram<'_>> {
        let id = self.detect(input)?;
        let definition = self.diagrams.get(id)?;
        Some(ResolvedDiagram { definition })
    }
}

impl Default for DiagramRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle returned by [`DiagramRegistry::resolve`].
///
/// Provides diagram metadata (ID, family, supported formats) without
/// creating an instance. Use the registry's `create()` to instantiate.
pub struct ResolvedDiagram<'a> {
    definition: &'a DiagramDefinition,
}

impl ResolvedDiagram<'_> {
    /// Diagram type identifier (e.g., "flowchart", "class").
    #[must_use]
    pub fn diagram_id(&self) -> &'static str {
        self.definition.id
    }

    /// Diagram family classification.
    #[must_use]
    pub fn family(&self) -> DiagramFamily {
        self.definition.family
    }

    /// Supported output formats for this diagram type.
    #[must_use]
    pub fn supported_formats(&self) -> &'static [OutputFormat] {
        self.definition.supported_formats
    }
}

/// Instance of a parsed diagram.
///
/// Each diagram type implements this trait to provide parsing and rendering.
pub trait DiagramInstance: Send + Sync {
    /// Parse input text into the diagram model.
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Render the diagram to the specified format.
    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError>;

    /// Check if this instance supports the given output format.
    fn supports_format(&self, format: OutputFormat) -> bool;
}

/// Create the default registry with all built-in diagram types.
///
/// Registration order determines detection priority. Flowchart is
/// registered first as the most common diagram type.
pub fn default_registry() -> DiagramRegistry {
    use crate::diagrams::{class, flowchart, info, packet, pie, sequence};

    let mut registry = DiagramRegistry::new();

    // Flowchart - most common, register first
    registry.register(flowchart::definition());

    // Graph-family diagrams
    registry.register(class::definition());

    // Timeline-family diagrams
    registry.register(sequence::definition());

    // Simple diagrams (shims)
    registry.register(pie::definition());
    registry.register(info::definition());
    registry.register(packet::definition());

    registry
}
