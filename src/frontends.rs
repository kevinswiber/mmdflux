//! Source-format frontends.
//!
//! Frontends own source-format detection before runtime dispatch resolves a
//! logical diagram type and family pipeline.

/// Input source format detected from raw input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFrontend {
    Mermaid,
    Mmds,
}

/// Detect the source-format frontend from raw input.
#[must_use]
pub fn detect_input_frontend(input: &str) -> Option<InputFrontend> {
    if crate::mmds::is_mmds_input(input) {
        Some(InputFrontend::Mmds)
    } else if crate::mermaid::detect_diagram_type(input).is_some() {
        Some(InputFrontend::Mermaid)
    } else {
        None
    }
}
