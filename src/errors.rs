//! Error and diagnostic types for rendering and validation.

use std::error::Error;

use serde::Serialize;

/// Serializable diagnostic extracted from a parse error.
///
/// Used at the Wasm boundary to return structured parse error information
/// as JSON rather than a flat error string.
#[derive(Debug, Serialize)]
pub struct ParseDiagnostic {
    /// Severity level: `"error"` or `"warning"`.
    pub severity: String,
    /// Start line (1-indexed), if known.
    pub line: Option<usize>,
    /// Start column (1-indexed), if known.
    pub column: Option<usize>,
    /// End line (1-indexed), if the parser provided a span.
    pub end_line: Option<usize>,
    /// End column (1-indexed), if the parser provided a span.
    pub end_column: Option<usize>,
    /// Human-readable error message.
    pub message: String,
}

impl ParseDiagnostic {
    /// Create a warning diagnostic with the given position and message.
    pub fn warning(line: Option<usize>, column: Option<usize>, message: String) -> Self {
        ParseDiagnostic {
            severity: "warning".to_string(),
            line,
            column,
            end_line: None,
            end_column: None,
            message,
        }
    }
}

/// Error type for rendering failures.
#[derive(Debug, Clone)]
pub struct RenderError {
    pub message: String,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for RenderError {}

impl From<String> for RenderError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for RenderError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}
