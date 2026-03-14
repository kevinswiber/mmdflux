//! Error and diagnostic types for rendering and validation.

use std::error::Error;

pub use crate::mermaid::ParseDiagnostic;

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
