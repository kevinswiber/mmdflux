use std::error::Error;

use super::{OutputFormat, RenderConfig, RenderError};

/// Diagram family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramFamily {
    /// Node-edge graphs (flowchart, state, class, ER).
    Graph,
    /// Timeline-based (sequence, gantt, gitgraph).
    Timeline,
    /// Chart/visualization (pie, radar, xy).
    Chart,
    /// Tabular layout (packet, kanban).
    Table,
}

/// Metadata common to all diagram types.
pub trait DiagramModel: Send + Sync {
    /// Clear/reset the model state.
    fn clear(&mut self);

    /// Get the diagram title, if set.
    fn title(&self) -> Option<&str>;

    /// Get the accessibility title, if set.
    fn acc_title(&self) -> Option<&str>;

    /// Get the accessibility description, if set.
    fn acc_description(&self) -> Option<&str>;
}

/// Parser that converts text input into a diagram model.
pub trait DiagramParser: Send + Sync {
    /// The model type this parser produces.
    type Model: DiagramModel;
    /// Error type for parse failures.
    type Error: Error + Send + Sync + 'static;

    /// Parse input text into a diagram model.
    fn parse(&self, input: &str) -> Result<Self::Model, Self::Error>;
}

/// Renderer that produces output from a diagram model.
pub trait DiagramRenderer: Send + Sync {
    /// The model type this renderer consumes.
    type Model: DiagramModel;

    /// Render the model to a string in the specified format.
    fn render(
        &self,
        model: &Self::Model,
        format: OutputFormat,
        config: &RenderConfig,
    ) -> Result<String, RenderError>;

    /// Check if this renderer supports the given output format.
    fn supports_format(&self, format: OutputFormat) -> bool;
}
