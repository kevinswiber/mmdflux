//! Diagram registry for type detection and dispatch.
//!
//! The registry holds diagram definitions and provides:
//! - Type detection from input text
//! - Factory creation of unparsed diagram instances
//! - Format support queries

use std::collections::HashMap;

use crate::config::RenderConfig;
use crate::errors::{ParseDiagnostic, RenderError};
use crate::format::OutputFormat;
use crate::payload::Diagram;

/// Diagram family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramFamily {
    /// Node-edge graphs (flowchart, class).
    Graph,
    /// Timeline-based (sequence).
    Timeline,
}

/// Detector function type.
///
/// Returns `true` if the input text matches this diagram type.
pub type DiagramDetector = fn(&str) -> bool;

/// Factory for creating unparsed diagram instances.
pub type DiagramFactory = fn() -> Box<dyn DiagramInstance>;

/// Diagram definition for registration.
///
/// Each diagram type provides a definition that describes how to
/// detect, create, and parse that diagram type.
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

    /// Create a new unparsed diagram instance by ID.
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

/// Unparsed diagram instance.
///
/// Each diagram type implements this trait to provide parsing and
/// format support queries before a parsed handle exists.
pub trait DiagramInstance: Send + Sync {
    /// Parse input text into a typed parsed-diagram handle.
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>>;

    /// Check if this instance supports the given output format.
    fn supports_format(&self, format: OutputFormat) -> bool;

    /// Return validation warnings for the input.
    ///
    /// Called after successful parsing to collect diagram-type-specific
    /// warnings (e.g., unsupported keywords, strict-mode issues).
    /// Default: no warnings.
    fn validation_warnings(&self, _input: &str) -> Vec<ParseDiagnostic> {
        Vec::new()
    }
}

/// Parsed diagram handle produced by [`DiagramInstance::parse`].
///
/// Parsed diagrams can only advance forward into runtime payloads.
pub trait ParsedDiagram: Send + Sync {
    /// Consume the parsed diagram into a family-local payload for runtime dispatch.
    fn into_payload(self: Box<Self>, config: &RenderConfig) -> Result<Diagram, RenderError>;
}
